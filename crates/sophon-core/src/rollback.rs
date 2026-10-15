// 回滚（九条准则之一）：任务级 git 快照。
// - 每次任务开始前自动 checkpoint（记录沙盒当前状态）
// - 「回滚」恢复到上一个 checkpoint，undo 一整个任务的所有改动
// - 快照记录在沙盒内 .sophon/checkpoints.jsonl，回滚可连续多次逐步后退
// - 沙盒根就是 git 仓库（ensure_repo），快照轻量、用户也可直接用 git 审阅
use anyhow::{anyhow, Result};
use serde_json::json;
use std::path::Path;

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(anyhow!("git {} 失败: {}", args.join(" "), err.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn git_available() -> bool {
    std::process::Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 确保沙盒根是 git 仓库（本地仓库，不设远程；自带身份避免 commit 失败）
pub fn ensure_repo(root: &Path) -> Result<()> {
    if !root.join(".git").exists() {
        git(root, &["init", "-q"])?;
        let _ = git(root, &["config", "user.email", "sophon@localhost"]);
        let _ = git(root, &["config", "user.name", "sophon"]);
        let _ = git(root, &["config", "commit.gpgsign", "false"]);
    }
    Ok(())
}

fn record_checkpoint(root: &Path, hash: &str, label: &str) -> Result<()> {
    let file = root.join(".sophon").join("checkpoints.jsonl");
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut content = std::fs::read_to_string(&file).unwrap_or_default();
    content.push_str(&format!(
        "{}\n",
        serde_json::to_string(&json!({"hash": hash, "label": label})).unwrap_or_default()
    ));
    std::fs::write(&file, content)?;
    Ok(())
}

/// 打快照：git add -A + commit。没有变更时返回 Ok(false)。
pub fn checkpoint(root: &Path, label: &str) -> Result<bool> {
    ensure_repo(root)?;
    git(root, &["add", "-A"])?;
    let commit = git(root, &["commit", "-q", "-m", label]);
    match commit {
        Ok(_) => {
            let hash = git(root, &["rev-parse", "HEAD"])?.trim().to_string();
            record_checkpoint(root, &hash, label)?;
            Ok(true)
        }
        Err(_) => Ok(false), // nothing to commit
    }
}

/// 列出快照（旧→新），返回 (hash, label)
pub fn list_checkpoints(root: &Path) -> Vec<(String, String)> {
    let file = root.join(".sophon").join("checkpoints.jsonl");
    let content = std::fs::read_to_string(file).unwrap_or_default();
    content
        .lines()
        .filter_map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).ok()?;
            Some((v["hash"].as_str()?.to_string(), v["label"].as_str().unwrap_or("").to_string()))
        })
        .collect()
}

/// 回滚到上一个快照（硬重置 + 清掉未跟踪产物），返回恢复到的快照说明。
pub fn rollback_last(root: &Path) -> Result<String> {
    let mut cps = list_checkpoints(root);
    if cps.len() < 2 {
        return Err(anyhow!("没有更早的快照可回滚(快照数: {})", cps.len()));
    }
    cps.pop(); // 当前状态
    let (hash, label) = cps.last().unwrap().clone();
    git(root, &["reset", "--hard", &hash])?;
    let _ = git(root, &["clean", "-fd", "-e", ".sophon"]); // 保留快照记录目录
    // 快照列表同步回退（保持连续回滚可用）
    let file = root.join(".sophon").join("checkpoints.jsonl");
    let kept: Vec<String> = cps.iter().map(|(h, l)| {
        serde_json::to_string(&json!({"hash": h, "label": l})).unwrap_or_default()
    }).collect();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&file, format!("{}\n", kept.join("\n")))?;
    Ok(format!("已回滚到快照 {}: {}", &hash[..8.min(hash.len())], label))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_available_check() {
        // 开发机必有 git；没有时应返回 false 而不是 panic
        assert!(git_available() || !git_available());
    }

    #[test]
    fn checkpoint_and_rollback() {
        if !git_available() {
            return;
        }
        let tmp = std::env::temp_dir().join(format!("sophon_rb_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("a.txt"), "v1").unwrap();
        checkpoint(&tmp, "第一次快照").unwrap();
        // 改动 → 第二个快照
        std::fs::write(tmp.join("a.txt"), "v2").unwrap();
        std::fs::write(tmp.join("b.txt"), "新增").unwrap();
        checkpoint(&tmp, "第二次快照").unwrap();
        assert_eq!(std::fs::read_to_string(tmp.join("a.txt")).unwrap(), "v2");
        // 回滚到上一个
        let msg = rollback_last(&tmp).unwrap();
        assert!(msg.contains("第一次快照"));
        assert_eq!(std::fs::read_to_string(tmp.join("a.txt")).unwrap(), "v1");
        assert!(!tmp.join("b.txt").exists(), "未跟踪的新文件应被清掉");
        // 只剩一个快照时不能再回滚
        assert!(rollback_last(&tmp).is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
