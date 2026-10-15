// 沙盒（九条准则之一）：只要运行命令都必须在沙盒内，包括命令行运行自身。
//
// 设计（软沙盒 + 硬边界）：
// - 每个实例一个沙盒目录 `<启动目录>/.sophon/sandbox/<实例id>/`：exec 的默认落地目录、
//   回滚快照、临时产物都圈在这里，乱跑命令不会弄脏工作区。
// - 不锁目录：命令可以用绝对路径访问任何地方，代码不做任何路径拦截（不给限制）。
// - 递归自身：exec 注入 SOPHON_SANDBOX 环境变量；沙盒内再用命令行启动 sophon 自身时，
//   自动识别并落回沙盒内的嵌套子沙盒——自身递归永远跑在沙盒边界内。
// - 多实例：实例 id 唯一 → 沙盒目录互不冲突；数据目录用文件锁保护，
//   同目录第二个实例自动分叉出独立数据目录（多实例互不冲突）。
// - 进程树终止：Windows 上 exec 的超时/取消用 `taskkill /F /T` 连孙进程一起杀，
//   命令行运行自身产生的子进程不会泄漏成孤儿。
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 环境变量名：标记「此进程的命令都在该沙盒内」
pub const ENV_SANDBOX: &str = "SOPHON_SANDBOX";
pub const ENV_DEPTH: &str = "SOPHON_SANDBOX_DEPTH";
pub const ENV_INSTANCE: &str = "SOPHON_INSTANCE";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Sandbox {
    /// 沙盒根目录（exec 默认 cwd、快照与产物落地处）
    pub root: PathBuf,
    /// 实例 id（多实例隔离；也是沙盒目录名）
    pub instance_id: String,
    /// 嵌套深度（0 = 用户直接启动；命令行运行自身时 +1）
    pub depth: u32,
}

/// 新实例 id：时间戳 + 短随机，避免同秒多实例撞名
fn fresh_id() -> String {
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let rnd = uuid::Uuid::new_v4().simple().to_string();
    format!("{}-{}", ts, &rnd[..6])
}

impl Sandbox {
    /// 解析当前进程应处的沙盒。
    /// - 环境变量 SOPHON_SANDBOX 存在（父实例 exec 启动的子 sophon）→ 沙盒内的嵌套子沙盒
    /// - 否则新建顶层沙盒 `<cwd>/.sophon/sandbox/<实例id>/`
    pub fn resolve(cwd: &Path) -> Sandbox {
        if let Ok(parent) = std::env::var(ENV_SANDBOX) {
            if !parent.trim().is_empty() {
                let base = PathBuf::from(parent);
                let d = std::env::var(ENV_DEPTH)
                    .ok()
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(1);
                let id = fresh_id();
                let root = base.join("sub").join(&id);
                let _ = std::fs::create_dir_all(&root);
                return Sandbox { root, instance_id: id, depth: d + 1 };
            }
        }
        let id = fresh_id();
        let root = cwd.join(".sophon").join("sandbox").join(&id);
        let _ = std::fs::create_dir_all(&root);
        Sandbox { root, instance_id: id, depth: 0 }
    }

    /// 是否嵌套实例（命令行运行自身启动的）
    pub fn is_nested(&self) -> bool {
        self.depth > 0
    }

    /// 注入给每个 exec 子进程的环境变量：
    /// 子进程再用命令行启动 sophon 自身时，读到这里自动落回沙盒内。
    pub fn env_pairs(&self) -> Vec<(String, String)> {
        vec![
            (ENV_SANDBOX.to_string(), self.root.to_string_lossy().to_string()),
            (ENV_DEPTH.to_string(), self.depth.to_string()),
            (ENV_INSTANCE.to_string(), self.instance_id.clone()),
        ]
    }

    /// 嵌套实例的数据目录（完全独立，随沙盒丢弃，不碰宿主真实数据）
    pub fn data_dir(&self) -> PathBuf {
        self.root.join(".sophon-data")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root.join(".sophon-cache")
    }

    /// 快照记录文件（rollback.rs 维护）
    pub fn checkpoints_file(&self) -> PathBuf {
        self.root.join(".sophon").join("checkpoints.jsonl")
    }
}

// ── 实例锁（多实例不冲突）──────────────────────────────

/// 数据目录实例锁：Windows 上用共享模式 0 独占打开锁文件（第二实例打开即失败）；
/// 进程退出句柄自动释放，锁文件残留无害。
pub struct InstanceLock {
    _file: std::fs::File,
    pub path: PathBuf,
}

impl InstanceLock {
    /// 尝试独占目录；拿不到返回 None（说明已有实例在用）
    pub fn acquire(dir: &Path) -> Option<InstanceLock> {
        let _ = std::fs::create_dir_all(dir);
        let path = dir.join("instance.lock");
        let file = open_exclusive(&path)?;
        Some(InstanceLock { _file: file, path })
    }
}

fn open_exclusive(path: &Path) -> Option<std::fs::File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0) // 独占：其他进程打不开
            .open(path)
            .ok()
    }
    #[cfg(not(windows))]
    {
        // 非 Windows：create_new 尽力而为（残留文件时删掉再试一次）
        match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(f) => Some(f),
            Err(_) => {
                std::fs::remove_file(path).ok()?;
                std::fs::OpenOptions::new().write(true).create_new(true).open(path).ok()
            }
        }
    }
}

// ── 进程树终止（沙盒边界兜底）──────────────────────────

/// 强制终止整棵进程树（Windows：taskkill /F /T；含命令行运行自身启动的 sophon）。
/// 返回是否执行成功。no_window 避免闪黑框。
pub fn kill_tree(pid: u32) -> bool {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        use std::os::windows::process::CommandExt;
        std::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .is_ok()
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_resolve_top_level() {
        // 无环境变量 → 顶层沙盒
        std::env::remove_var(ENV_SANDBOX);
        let tmp = std::env::temp_dir().join(format!("sophon_sb_test_{}", std::process::id()));
        let sb = Sandbox::resolve(&tmp);
        assert!(!sb.is_nested());
        assert!(sb.root.starts_with(tmp.join(".sophon").join("sandbox")));
        assert!(!sb.instance_id.is_empty());
        let pairs = sb.env_pairs();
        assert_eq!(pairs.len(), 3);
        assert_eq!(pairs[0].0, ENV_SANDBOX);
        assert!(sb.checkpoints_file().to_string_lossy().contains("checkpoints"));
        let _ = std::fs::remove_dir_all(tmp.join(".sophon"));
    }

    #[test]
    fn instance_lock_exclusive() {
        let tmp = std::env::temp_dir().join(format!("sophon_lock_test_{}", std::process::id()));
        let l1 = InstanceLock::acquire(&tmp);
        assert!(l1.is_some(), "第一次应拿到锁");
        let l2 = InstanceLock::acquire(&tmp);
        assert!(l2.is_none(), "同一目录第二把锁必须拿不到（多实例分叉）");
        drop(l1);
        let l3 = InstanceLock::acquire(&tmp);
        assert!(l3.is_some(), "释放后应可重入");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
