// 自主执行策略（SOPHON.md §4）：
// 三级 autonomous（默认）/ safe / yolo；自动审批、破坏性检测、可逆替代、硬拒绝列表。
// 由 agent/runner.rs 在每个工具调用前调用 check_shell_command / approve_tool。
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 自主级别
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutonomyMode {
    /// 全自主（默认）：只读/安全写入/非破坏命令自动放行；破坏性先试可逆替代，不行才确认
    Autonomous,
    /// 谨慎：写操作与命令都要确认
    Safe,
    /// 全自动：破坏性也自动（硬拒绝列表仍然拒绝）
    Yolo,
}

impl Default for AutonomyMode {
    fn default() -> Self {
        AutonomyMode::Autonomous
    }
}

impl AutonomyMode {
    pub fn label(self) -> &'static str {
        match self {
            AutonomyMode::Autonomous => "自主",
            AutonomyMode::Safe => "谨慎",
            AutonomyMode::Yolo => "全自动",
        }
    }
    pub fn all() -> [AutonomyMode; 3] {
        [AutonomyMode::Autonomous, AutonomyMode::Safe, AutonomyMode::Yolo]
    }
    pub fn cycle(self) -> Self {
        match self {
            AutonomyMode::Autonomous => AutonomyMode::Safe,
            AutonomyMode::Safe => AutonomyMode::Yolo,
            AutonomyMode::Yolo => AutonomyMode::Autonomous,
        }
    }
}

/// 对一次工具调用的决策
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// 放行
    Allow,
    /// 需要用户确认（带原因）
    Confirm(String),
    /// 直接拒绝（硬拒绝列表 / 只读模式等，带原因）
    Deny(String),
    /// 已自动改写为可逆替代命令（携带新命令与说明）
    Transform(String, String),
}

// ── 硬拒绝列表 ─────────────────────────────────────────

/// 绝对拒绝（无论什么级别）：格式化 / 全盘删除 / 关机重启 / 结束自身等
pub fn is_hard_denied(cmd: &str) -> Option<String> {
    let lower = cmd.to_lowercase();
    let compact = lower.replace(char::is_whitespace, "");
    // 全盘删除
    let whole_disk = [
        "rm-rf/", "rm-fr/", "rm-rf/*", "rm-rf~", "rm-rf/~",
        "del/f/s/qc:\\", "del/f/s/qc:/",
        "remove-itemc:\\-recurse", "remove-itemc:/-recurse",
        "rd/s/qc:\\", "rd/s/qc:/",
    ];
    for p in whole_disk.iter() {
        if compact.contains(p) {
            return Some("拒绝：全盘删除命令".into());
        }
    }
    // 格式化 / 磁盘
    if compact.contains("mkfs") {
        return Some("拒绝：格式化命令".into());
    }
    for kw in ["format", "diskpart"] {
        if lower.split(|c: char| c == '|' || c == '&' || c == ';').any(|part| {
            let p = part.trim();
            p == kw
                || p.starts_with(&format!("{} ", kw))
                || p.starts_with(&format!("{}:", kw))
                || p.starts_with(&format!("{}c:", kw))
                || p.starts_with(&format!("{} c:", kw))
        }) {
            return Some("拒绝：格式化 / 磁盘操作命令".into());
        }
    }
    // 关机 / 重启 / 注销
    for kw in ["shutdown", "restart-computer", "stop-computer", "halt", "reboot", "poweroff", "logoff"] {
        if lower.split(|c: char| c == '|' || c == '&' || c == ';').any(|part| {
            let p = part.trim();
            p == kw || p.starts_with(&format!("{} ", kw)) || p.starts_with(&format!("{}/", kw))
        }) {
            return Some("拒绝：关机 / 重启命令".into());
        }
    }
    // 结束 Sophon 自身进程
    for kw in ["taskkill/f/im sophon", "kill-9$sophon", "pkill sop"] {
        if compact.contains(kw) {
            return Some("拒绝：结束 Sophon 自身进程".into());
        }
    }
    None
}

// ── 破坏性检测 ─────────────────────────────────────────

/// 是否破坏性命令（可能不可逆地删 / 覆盖 / 回退）
pub fn is_destructive(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    // PowerShell / cmd / Unix 的删除
    let del_head = ["remove-item", "del ", "rd ", "rmdir", "rm "];
    for h in del_head.iter() {
        if lower.trim_start().starts_with(h)
            || lower.split('|').any(|p| p.trim().starts_with(h))
            || lower.contains(&format!("; {}", h))
        {
            return true;
        }
    }
    // git 破坏性
    for kw in [
        "git reset --hard",
        "git clean -f",
        "git clean -df",
        "git checkout -- ",
        "git restore .",
    ] {
        if lower.contains(kw) {
            return true;
        }
    }
    // 强制覆盖移动 / 覆盖写整盘文件（PowerShell Set-Content 到系统路径之类不判；只判明显的强制移动/复制覆盖）
    if (lower.contains("move-item") && lower.contains("-force"))
        || (lower.contains("copy-item") && lower.contains("-force"))
        || (lower.contains("mv ") && lower.contains("-f "))
    {
        return true;
    }
    false
}

/// 从删除类命令里提取目标路径（供可逆替代）
fn extract_delete_target(cmd: &str) -> Option<String> {
    let t = cmd.trim();
    let lower = t.to_lowercase();
    for head in ["remove-item", "rm", "del", "rd", "rmdir"] {
        if lower.starts_with(head) {
            let rest = &t[head.len()..];
            // 去掉 -rf / -Recurse -Force / /s /q 等旗标，取第一个非旗标 token
            for tok in rest.split_whitespace() {
                if tok.starts_with('-') || tok.starts_with('/') {
                    // -rf、-Recurse、/s、/q 等
                    // 但 Windows 路径不会以 / 开头出现在 del 后面（C:\ 开头），单 '/' 或 '-xxx' 都是旗标
                    if tok == "/" || tok == "-" || tok.len() <= 3 && (tok.starts_with('-') || tok.starts_with('/')) {
                        continue;
                    }
                }
                let cleaned = tok.trim_matches('"').trim_matches('\'').to_string();
                if !cleaned.is_empty() {
                    return Some(cleaned);
                }
            }
        }
    }
    None
}

/// 可逆替代（SOPHON.md §4）：删除 → 移到回收站目录；git reset --hard → stash。
/// 成功返回替代后的命令。工作目录用于放回收站。
pub fn reversible_alternative(cmd: &str, working_directory: &str) -> Option<(String, String)> {
    let lower = cmd.to_lowercase().trim().to_string();
    // git reset --hard → 先 stash（保住未提交改动）
    if lower.contains("git reset --hard") {
        return Some((
            cmd.replace("git reset --hard", "git stash push --include-untracked && git reset --hard"),
            "git reset --hard 前先把未提交改动 stash 到了 stash@{0}（可用 git stash pop 找回）".into(),
        ));
    }
    // 删除类 → Move-Item 到回收站
    if let Some(target) = extract_delete_target(cmd) {
        // 不替代通配符与根级路径（太危险 / 无意义），交给确认
        if target.contains('*') || target.contains('?') {
            return None;
        }
        let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let trash = std::path::Path::new(working_directory)
            .join(".sophon-trash")
            .join(ts.to_string());
        let file_name = std::path::Path::new(&target)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "item".into());
        let dest = trash.join(&file_name);
        let new_cmd = format!(
            "New-Item -ItemType Directory -Force -Path \"{}\" | Out-Null; if (Test-Path \"{}\") {{ Move-Item -LiteralPath \"{}\" -Destination \"{}\" -Force }}",
            trash.to_string_lossy().replace('\\', "/"),
            target.replace('\\', "/"),
            target.replace('\\', "/"),
            dest.to_string_lossy().replace('\\', "/"),
        );
        return Some((
            new_cmd,
            format!(
                "删除已改为可逆替代：目标被移到回收站 {}（确认无误后可手动清掉）",
                trash.display()
            ),
        ));
    }
    None
}

// ── 决策入口 ───────────────────────────────────────────

/// 对 run_shell 的一条命令做决策
pub fn check_shell_command(cmd: &str, mode: AutonomyMode, working_directory: &str) -> Decision {
    if cmd.trim().is_empty() {
        return Decision::Deny("空命令".into());
    }
    if let Some(reason) = is_hard_denied(cmd) {
        return Decision::Deny(reason);
    }
    let destructive = is_destructive(cmd);
    match mode {
        AutonomyMode::Yolo => {
            if destructive {
                // yolo 也走一遍可逆替代（有替代就用，没有就硬闯）
                match reversible_alternative(cmd, working_directory) {
                    Some((new_cmd, note)) => Decision::Transform(new_cmd, note),
                    None => Decision::Allow,
                }
            } else {
                Decision::Allow
            }
        }
        AutonomyMode::Autonomous => {
            if destructive {
                match reversible_alternative(cmd, working_directory) {
                    Some((new_cmd, note)) => Decision::Transform(new_cmd, note),
                    None => Decision::Confirm(format!(
                        "破坏性命令且无可逆替代：{}",
                        one_line(cmd)
                    )),
                }
            } else {
                Decision::Allow
            }
        }
        AutonomyMode::Safe => {
            if destructive {
                Decision::Confirm(format!("谨慎模式：破坏性命令需要确认：{}", one_line(cmd)))
            } else {
                Decision::Confirm("谨慎模式：命令执行需要确认".into())
            }
        }
    }
}

/// 对非 shell 工具的写操作做决策（read/search 等只读工具一律 Allow）
pub fn approve_tool(tool_name: &str, is_write: bool, mode: AutonomyMode) -> Decision {
    if !is_write {
        return Decision::Allow;
    }
    match mode {
        AutonomyMode::Yolo | AutonomyMode::Autonomous => Decision::Allow,
        AutonomyMode::Safe => Decision::Confirm(format!("谨慎模式：{} 是写操作，需要确认", tool_name)),
    }
}

// ── 辅助 ─────────────────────────────────────────────

fn one_line(s: &str) -> String {
    let t: String = s.chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
    if t.chars().count() > 60 {
        let cut: String = t.chars().take(57).collect();
        format!("{}…", cut)
    } else {
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_denied() {
        assert!(is_hard_denied("rm -rf /").is_some());
        assert!(is_hard_denied("rm -fr /").is_some());
        assert!(is_hard_denied("shutdown /s /t 0").is_some());
        assert!(is_hard_denied("Restart-Computer").is_some());
        assert!(is_hard_denied("del /f /s /q C:\\").is_some());
        assert!(is_hard_denied("mkfs.ext4 /dev/sda1").is_some());
        assert!(is_hard_denied("git reset --hard").is_none());
        assert!(is_hard_denied("cargo build").is_none());
    }

    #[test]
    fn destructive_detected() {
        assert!(is_destructive("Remove-Item build -Recurse -Force"));
        assert!(is_destructive("rm -rf target"));
        assert!(is_destructive("del foo.txt"));
        assert!(is_destructive("git reset --hard HEAD~1"));
        assert!(!is_destructive("cargo build"));
        assert!(!is_destructive("git status"));
        assert!(!is_destructive("echo hello"));
    }

    #[test]
    fn trash_alternative() {
        let (new_cmd, note) = reversible_alternative("Remove-Item old_dir -Recurse -Force", "C:/tmp/proj").unwrap();
        assert!(new_cmd.contains(".sophon-trash"));
        assert!(new_cmd.contains("Move-Item"));
        assert!(note.contains("回收站"));
        // 通配符不替代
        assert!(reversible_alternative("del *.tmp", "C:/tmp").is_none());
        // git reset --hard → stash
        let (c2, _) = reversible_alternative("git reset --hard", "C:/tmp").unwrap();
        assert!(c2.contains("git stash push"));
    }

    #[test]
    fn decisions_by_mode() {
        let wd = ".";
        // 硬拒绝永远拒绝
        assert!(matches!(
            check_shell_command("shutdown /s", AutonomyMode::Yolo, wd),
            Decision::Deny(_)
        ));
        // 自主模式：非破坏放行；破坏性有替代 → Transform
        assert!(matches!(
            check_shell_command("cargo build", AutonomyMode::Autonomous, wd),
            Decision::Allow
        ));
        assert!(matches!(
            check_shell_command("Remove-Item old -Recurse -Force", AutonomyMode::Autonomous, wd),
            Decision::Transform(_, _)
        ));
        // 谨慎模式：都要确认
        assert!(matches!(
            check_shell_command("cargo build", AutonomyMode::Safe, wd),
            Decision::Confirm(_)
        ));
        // 全自动：破坏性无替代也放行
        assert!(matches!(
            check_shell_command("del *.tmp", AutonomyMode::Yolo, wd),
            Decision::Allow
        ));
    }

    #[test]
    fn tool_approval() {
        assert!(matches!(approve_tool("run_shell", true, AutonomyMode::Autonomous), Decision::Allow));
        assert!(matches!(approve_tool("write_file", true, AutonomyMode::Safe), Decision::Confirm(_)));
        assert!(matches!(approve_tool("read_file", false, AutonomyMode::Safe), Decision::Allow));
    }
}
