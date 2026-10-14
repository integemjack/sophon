// 基础设施：路径、JSON 辅助、取消令牌、主线程命令队列。
// 对应 docs/ARCHITECTURE.md 的 Paths、Json、CancelToken、MainThread。
pub mod cancel;
pub mod main_thread;

pub use cancel::CancelToken;
pub use main_thread::{MainThread, MainThreadReceiver};

use std::path::PathBuf;

/// 数据 / 缓存目录（docs §5.1）。环境变量 FRIDAY_DATA_DIR / FRIDAY_CACHE_DIR / FRIDAY_HOME 可覆盖。
pub struct Paths {
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub powers_dir: PathBuf,
}

impl Paths {
    pub fn resolve() -> Self {
        // FRIDAY_HOME：换掉默认位置（<它>/data、<它>/cache、<它>/powers）
        if let Ok(home) = std::env::var("FRIDAY_HOME") {
            let root = PathBuf::from(home);
            return Paths {
                data_dir: root.join("data"),
                cache_dir: root.join("cache"),
                powers_dir: root.join("powers"),
            };
        }
        let data_dir = std::env::var("FRIDAY_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| default_data_dir());
        let cache_dir = std::env::var("FRIDAY_CACHE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| default_cache_dir());
        let powers_dir = data_dir.join("Powers");
        Paths { data_dir, cache_dir, powers_dir }
    }

    pub fn conversations_file(&self) -> PathBuf { self.data_dir.join("conversations.json") }
    pub fn settings_file(&self) -> PathBuf { self.data_dir.join("settings.json") }
    pub fn secrets_file(&self) -> PathBuf { self.data_dir.join("secrets.json") }
    pub fn engines_file(&self) -> PathBuf { self.data_dir.join("engines.json") }
    pub fn mcp_file(&self) -> PathBuf { self.data_dir.join("mcp.json") }
    pub fn journal_dir(&self) -> PathBuf { self.data_dir.join("journal") }
    pub fn assistant_dir(&self) -> PathBuf { self.data_dir.join("Assistant") }
    pub fn assistant_timeline_file(&self) -> PathBuf { self.assistant_dir().join("timeline.json") }
    pub fn assistant_memory_file(&self) -> PathBuf { self.assistant_dir().join("memory.json") }
    pub fn media_dir(&self) -> PathBuf { self.data_dir.join("Media") }
    pub fn tools_dir(&self) -> PathBuf { self.data_dir.join("Tools") }

    pub fn ensure_all(&self) -> std::io::Result<()> {
        for d in [
            &self.data_dir,
            &self.cache_dir,
            &self.powers_dir,
            &self.journal_dir(),
            &self.assistant_dir(),
            &self.media_dir(),
            &self.tools_dir(),
        ] {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
    }
}

fn default_data_dir() -> PathBuf {
    if cfg!(target_os = "windows") {
        PathBuf::from(std::env::var("APPDATA").unwrap_or_else(|_| ".".into())).join("Sophon")
    } else if cfg!(target_os = "macos") {
        dirs::home_dir().unwrap_or_default().join("Library/Application Support/Sophon")
    } else {
        dirs::data_dir().unwrap_or_default().join("sophon")
    }
}

fn default_cache_dir() -> PathBuf {
    if cfg!(target_os = "windows") {
        PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into())).join("Sophon/Cache")
    } else if cfg!(target_os = "macos") {
        dirs::home_dir().unwrap_or_default().join("Library/Caches/Sophon")
    } else {
        dirs::cache_dir().unwrap_or_default().join("sophon")
    }
}

/// JSON 辅助（docs §5.2）：UUID 大写、日期 ISO8601 UTC 无毫秒。
pub mod json {
    use crate::{new_uuid, now_iso};
    pub fn uuid() -> String { new_uuid() }
    pub fn now() -> String { now_iso() }
    /// 展示给用户 / 模型的 JSON 文本（两空格缩进、键排序）
    pub fn pretty(value: &serde_json::Value) -> String {
        // serde_json::to_string_pretty 不排序键；这里手动排序顶层键以对齐 docs 的 sortedKeys
        let mut buf = Vec::new();
        write_sorted(&mut buf, value, 0);
        String::from_utf8(buf).unwrap_or_default()
    }
    fn write_sorted(buf: &mut Vec<u8>, v: &serde_json::Value, indent: usize) {
        use serde_json::Value;
        match v {
            Value::Object(map) => {
                if map.is_empty() {
                    buf.extend_from_slice(b"{}");
                    return;
                }
                buf.push(b'{');
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                for (i, k) in keys.iter().enumerate() {
                    if i > 0 {
                        buf.push(b',');
                    }
                    buf.push(b'\n');
                    buf.extend_from_slice("  ".repeat(indent + 1).as_bytes());
                    buf.extend_from_slice(serde_json::to_string(k).unwrap().as_bytes());
                    buf.extend_from_slice(b" : ");
                    write_sorted(buf, &map[*k], indent + 1);
                }
                buf.push(b'\n');
                buf.extend_from_slice("  ".repeat(indent).as_bytes());
                buf.push(b'}');
            }
            Value::Array(arr) => {
                if arr.is_empty() {
                    buf.extend_from_slice(b"[]");
                    return;
                }
                buf.push(b'[');
                for (i, item) in arr.iter().enumerate() {
                    if i > 0 {
                        buf.push(b',');
                    }
                    buf.push(b'\n');
                    buf.extend_from_slice("  ".repeat(indent + 1).as_bytes());
                    write_sorted(buf, item, indent + 1);
                }
                buf.push(b'\n');
                buf.extend_from_slice("  ".repeat(indent).as_bytes());
                buf.push(b']');
            }
            _ => {
                buf.extend_from_slice(serde_json::to_string(v).unwrap().as_bytes());
            }
        }
    }
}

/// 读取 JSON 文件，缺失时返回 None
pub fn read_json_file(path: &std::path::Path) -> Option<serde_json::Value> {
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

/// 写 JSON 文件（pretty + sortedKeys）
pub fn write_json_file(path: &std::path::Path, value: &serde_json::Value) -> std::io::Result<()> {
    let s = json::pretty(value);
    std::fs::write(path, s)
}
