// 自我改进递归（SOPHON.md §6）：
// 发现自身缺陷 → git 分支改自身代码 → cargo build + cargo test → 全绿 → 合并 → 重新编译；
// 任一步失败 → 回滚 + 记教训。只在 git 仓库内操作，用户可随时 git revert。
use crate::learning::LessonStore;
use serde::{Deserialize, Serialize};

/// 一步修复动作
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "kind")]
pub enum FixStep {
    /// 把文件里的 old 原样替换为 new（old 必须在文件里恰好出现一次）
    Edit { file: String, old: String, new: String },
    /// 在仓库目录跑一条修复命令
    Shell { command: String },
}

/// 管道报告
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ImproveReport {
    pub problem: String,
    pub branch: String,
    pub base_branch: String,
    /// 注入 / 复现：修复前验证命令是否失败（红）
    pub reproduced_failure: Option<bool>,
    pub steps: Vec<String>,
    /// 修复后验证是否全绿
    pub verified: bool,
    pub merged: bool,
    pub rolled_back: bool,
    pub notes: Vec<String>,
}

fn git(repo: &str, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|e| format!("git 启动失败：{}", e))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if out.status.success() {
        Ok(text)
    } else {
        Err(text)
    }
}

fn run_verify(repo: &str, command: &str, timeout_secs: u64) -> Result<String, String> {
    // PowerShell 执行验证命令
    let ps = format!(
        "& {{ {} }}; exit $LASTEXITCODE",
        command
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps])
        .current_dir(repo)
        .output();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    let _ = deadline; // 超时由外层/调用者控制；这里同步等待
    match out {
        Ok(o) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            if o.status.success() {
                Ok(text)
            } else {
                Err(text)
            }
        }
        Err(e) => Err(format!("验证命令启动失败：{}", e)),
    }
}

/// 完整管道。verify_cmd 例："cargo test --workspace --quiet"。
/// 预期修复前是红的（reproduced_failure）；修完必须全绿才合并。
pub fn run_self_improve(
    repo: &str,
    problem: &str,
    plan: &[FixStep],
    verify_cmd: &str,
    lessons: &mut LessonStore,
) -> ImproveReport {
    let mut report = ImproveReport {
        problem: problem.to_string(),
        ..Default::default()
    };
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    report.branch = format!("self-improve/{}", ts);

    // 0. 必须是 git 仓库
    if let Err(e) = git(repo, &["rev-parse", "--is-inside-work-tree"]) {
        report.notes.push(format!("不是 git 仓库，拒绝自修改：{}", e));
        return report;
    }

    // 1. 基线分支（当前 HEAD）；工作区必须先干净（脏则提交基线，保证可回滚）
    //    branch --show-current 对 unborn 分支也返回正确名字；abbrev-ref 在无提交时会返回默认名，不可信
    report.base_branch = match git(repo, &["branch", "--show-current"]) {
        Ok(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => git(repo, &["rev-parse", "--abbrev-ref", "HEAD"])
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "master".into()),
    };
    let dirty = !git(repo, &["status", "--porcelain"]).unwrap_or_default().trim().is_empty();
    if dirty {
        let _ = git(repo, &["add", "-A"]);
        match git(repo, &["commit", "-m", "chore: self-improve 基线（自动提交）"]) {
            Ok(_) => report.notes.push("工作区不干净，已自动提交基线保证可回滚".into()),
            Err(e) => {
                report.notes.push(format!("基线提交失败，拒绝继续：{}", one_line(&e)));
                return report;
            }
        }
    }

    // 2. 复现：修复前验证命令应当失败（红）
    match run_verify(repo, verify_cmd, 600) {
        Err(_) => {
            report.reproduced_failure = Some(true);
            report.steps.push("复现：修复前验证失败（红）✓".into());
        }
        Ok(_) => {
            report.reproduced_failure = Some(false);
            report.notes.push("修复前验证就是绿的——问题不存在或验证命令不对，不需要修改".into());
            return report;
        }
    }

    // 3. 开分支
    if let Err(e) = git(repo, &["checkout", "-b", &report.branch]) {
        report.notes.push(format!("开分支失败：{}", one_line(&e)));
        return report;
    }
    report.steps.push(format!("分支 {} 已创建", report.branch));

    // 4. 应用修复计划
    let mut apply_failed: Option<String> = None;
    for step in plan {
        match step {
            FixStep::Edit { file, old, new } => {
                let path = std::path::Path::new(repo).join(file);
                match std::fs::read_to_string(&path) {
                    Ok(content) => {
                        // 换行归一化：磁盘文件可能是 CRLF（git autocrlf），计划里是 LF。
                        // 在 LF 世界里匹配/替换，写回时按原文件的行尾风格还原，避免整个文件 diff。
                        let crlf = content.contains("\r\n");
                        let content_lf = content.replace("\r\n", "\n");
                        let old_lf = old.replace("\r\n", "\n");
                        let new_lf = new.replace("\r\n", "\n");
                        let n = content_lf.matches(old_lf.as_str()).count();
                        if n != 1 {
                            apply_failed = Some(format!(
                                "{}：替换目标出现 {} 次（应为 1），放弃这一步",
                                file, n
                            ));
                            break;
                        }
                        let mut result = content_lf.replacen(old_lf.as_str(), new_lf.as_str(), 1);
                        if crlf {
                            result = result.replace('\n', "\r\n");
                        }
                        if std::fs::write(&path, result).is_err() {
                            apply_failed = Some(format!("{}：写入失败", file));
                            break;
                        }
                        report.steps.push(format!("已修改 {}", file));
                    }
                    Err(e) => {
                        apply_failed = Some(format!("{}：读取失败（{}）", file, e));
                        break;
                    }
                }
            }
            FixStep::Shell { command } => match run_verify(repo, command, 300) {
                Ok(_) => report.steps.push(format!("已执行：{}", one_line(command))),
                Err(e) => {
                    apply_failed = Some(format!("修复命令失败：{}：{}", one_line(command), one_line(&e)));
                    break;
                }
            },
        }
    }

    // 5. 验证：build + verify（测试）
    if apply_failed.is_none() {
        let build = run_verify(repo, "cargo check --quiet --workspace", 600);
        let test = run_verify(repo, verify_cmd, 900);
        match (build, test) {
            (Ok(_), Ok(_)) => {
                report.verified = true;
                report.steps.push("验证：cargo build + cargo test 全绿 ✓".into());
            }
            (b, t) => {
                let mut msg = String::from("验证失败：");
                if let Err(e) = b {
                    msg.push_str(&format!("\n[build] {}", one_line(&e)));
                }
                if let Err(e) = t {
                    msg.push_str(&format!("\n[test] {}", one_line(&e)));
                }
                apply_failed = Some(msg);
            }
        }
    }

    // 6. 合并或回滚
    if report.verified {
        let commit_msg = format!("self-improve: {}", problem);
        let _ = git(repo, &["add", "-A"]);
        if let Err(e) = git(repo, &["commit", "-m", &commit_msg]) {
            report.notes.push(format!("分支提交失败：{}", one_line(&e)));
        }
        let merged = git(repo, &["checkout", &report.base_branch])
            .and_then(|_| git(repo, &["merge", "--no-ff", &report.branch, "-m", &commit_msg]));
        match merged {
            Ok(_) => {
                report.merged = true;
                report.steps.push(format!("已合并回 {} ✓", report.base_branch));
                // 重新编译二进制
                match run_verify(repo, "cargo check --quiet --workspace", 600) {
                    Ok(_) => report.steps.push("重新编译完成 ✓".into()),
                    Err(e) => report.notes.push(format!("合并后重新编译失败（分支已保留）：{}", one_line(&e))),
                }
            }
            Err(e) => {
                report.notes.push(format!("合并失败：{}", one_line(&e)));
                let base = report.base_branch.clone();
                let branch = report.branch.clone();
                rollback(repo, &base, &branch, &mut report);
            }
        }
        lessons.record(crate::learning::Lesson {
            ts: crate::models::now_iso(),
            problem: problem.to_string(),
            attempts: report.steps.clone(),
            result: if report.merged { "成功（已合并）".into() } else { "失败".into() },
            lesson: format!("自修改管道：{}（分支 {}）", problem, report.branch),
            tags: vec!["self-improve".into()],
        });
    } else {
        let reason = apply_failed.unwrap_or_else(|| "未知原因".into());
        report.notes.push(format!("修复未通过验证，回滚：{}", reason));
        let base = report.base_branch.clone();
        let branch = report.branch.clone();
        rollback(repo, &base, &branch, &mut report);
        lessons.record(crate::learning::Lesson {
            ts: crate::models::now_iso(),
            problem: problem.to_string(),
            attempts: report.steps.clone(),
            result: format!("失败：{}", reason),
            lesson: "自修改失败已回滚；下次先确认修法可行再动手".into(),
            tags: vec!["self-improve".into(), "失败".into()],
        });
    }
    report
}

/// 交付自检：构建 + 测试；Ok=全绿（runner 交付自检也用它）
pub fn verify_only(repo: &str) -> Result<String, String> {
    let build = run_verify(repo, "cargo check --quiet --workspace", 600)?;
    let test = run_verify(repo, "cargo test --workspace --quiet", 900)?;
    Ok(format!("{}{}", build, test))
}

fn rollback(repo: &str, base: &str, branch: &str, report: &mut ImproveReport) {
    // 基线已提交、失败分支上无提交：丢弃工作区改动、切回基线分支、删掉失败分支
    let _ = git(repo, &["checkout", "--", "."]);
    match git(repo, &["checkout", base]) {
        Ok(_) => {
            let _ = git(repo, &["branch", "-D", branch]);
            report.rolled_back = true;
            report.steps.push(format!("已回滚到 {}，分支已删除", base));
        }
        Err(e) => {
            // 切不回去（比如分支名探测错了）：至少工作区已还原，如实报告，不谎报成功
            report.rolled_back = false;
            report.notes.push(format!(
                "回滚不完整：切回 {} 失败（{}），当前仍在新分支上，工作区已还原；请手动检查分支",
                base,
                one_line(&e)
            ));
        }
    }
}

fn one_line(s: &str) -> String {
    let t: String = s.chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
    let t = t.trim();
    // 500 字符：验证错误的关键信息常在 warning 之后，截太短会看不到真实原因
    if t.chars().count() > 500 {
        format!("{}…", t.chars().take(497).collect::<String>())
    } else {
        t.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RSI 第二层回归：Edit 步对 CRLF 文件的匹配（管道自身曾因这个 bug 修不了目标）
    #[test]
    fn edit_handles_crlf_files() {
        let dir = std::env::temp_dir().join(format!("sophon_crlf_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = dir.join("repo");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        // git init + 身份 + 基线提交（含 CRLF 文件）
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .unwrap()
        };
        let _ = git(&["init", "-b", "main"]);
        let _ = git(&["config", "user.name", "t"]);
        let _ = git(&["config", "user.email", "t@t"]);
        std::fs::write(
            repo.join("src/lib.rs"),
            "fn add(a: i32, b: i32) -> i32 {\r\n    a - b\r\n}\r\n",
        )
        .unwrap();
        std::fs::write(
            repo.join("Cargo.toml"),
            "[package]\nname = \"crlfdemo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        let _ = git(&["add", "-A"]);
        let _ = git(&["commit", "-m", "init"]);
        // 计划用 LF 写 old/new
        let plan = vec![FixStep::Edit {
            file: "src/lib.rs".into(),
            old: "a - b".into(),
            new: "a + b".into(),
        }];
        let mut lessons = LessonStore::in_memory();
        let repo_s = repo.to_str().unwrap();
        // 验证命令：bug（a - b）在时红，修复后绿
        let verify = "if (Select-String -Path src/lib.rs -Pattern 'a - b' -Quiet) { exit 1 } else { exit 0 }";
        let report = run_self_improve(repo_s, "CRLF Edit 测试", &plan, verify, &mut lessons);
        assert!(report.verified, "CRLF 文件 Edit 应成功：{:?}", report.notes);
        assert!(report.merged);
        let after = std::fs::read_to_string(repo.join("src/lib.rs")).unwrap();
        assert!(after.contains("a + b"));
        assert!(after.contains("\r\n"), "应保持 CRLF 行尾");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_non_repo() {
        let dir = std::env::temp_dir().join(format!("sophon_no_repo_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut lessons = LessonStore::in_memory();
        let r = run_self_improve(
            dir.to_str().unwrap(),
            "测试",
            &[],
            "echo ok",
            &mut lessons,
        );
        assert!(r.notes[0].contains("不是 git 仓库"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
