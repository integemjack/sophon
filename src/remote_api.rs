// 远程 API 清单与提示词：固定放在桌面 sophon 目录（无论核心从哪里启动都读这里；
// 环境变量 SOPHON_DIR 可整体换根）。核心在哪个目录运行，工作环境（产物落盘）就在哪个目录。
//
// 一、sophon.sub——远程 API 清单，文本格式，每行一条（`#` 注释、空行忽略）：
//   <api地址> <api_key>              → 启动时拉取 <地址>/models，选用版本号最大的模型
//   <api地址> <api_key> <模型名>     → 直接使用指定模型（不自动选）
// 启动默认从最后一行往上逐条尝试：
//   - 拉取 /models 失败（网络 / 鉴权 / 空列表）→ 先确认这一行仍原样存在（多个核心同时
//     运行时避免误删别的行 / 重复删），确认后才删除，再尝试上一行；
//   - 成功 → 返回（地址, 密钥, 模型），整个启动期间只用这一条。
//
// 二、sophon.prompt——系统提示词：启动时不存在 → 把核心内置提示词写入该文件；
//   存在 → 直接读它作为系统提示词（ask 对话时作为 system 消息）。

use anyhow::Context;

/// 清单与提示词文件名（都在桌面 sophon 目录下）
pub const SUB_FILE_NAME: &str = "sophon.sub";
pub const PROMPT_FILE_NAME: &str = "sophon.prompt";

/// 核心内置提示词（首次启动写入 sophon.prompt）
pub const DEFAULT_PROMPT: &str = "你是智子（Sophon），一个全自主智能体核心。\n职责：根据用户指令进行联网调研与信息整合（search / fetch / get / research），给出准确、简洁、有依据的回答。\n风格：先结论后依据，重要结论给出来源 URL；不编造事实，查不到就直说。";
/// 探测 /models 的超时
pub const MODEL_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);
/// ask 对话的超时
pub const CHAT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// 清单中的一条远程 API
#[derive(Clone, Debug)]
pub struct RemoteApiEntry {
    pub base_url: String,
    pub api_key: String,
    /// 行内显式指定的模型名（第三列，可省略）
    pub model: Option<String>,
    /// 该行原文（用于「确认后删除」的精确匹配）
    pub raw: String,
}

/// 启动解析结果
#[derive(Clone, Debug)]
pub struct Resolved {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    /// true = 从模型列表按版本号最大选出；false = 行内显式指定
    pub from_list: bool,
}

/// 桌面上的 sophon 目录（SOPHON_DIR 可整体换根；与核心从哪里启动无关）
pub fn sophon_dir() -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("SOPHON_DIR") {
        if !p.trim().is_empty() {
            return Some(std::path::PathBuf::from(p));
        }
    }
    if let Some(d) = dirs::desktop_dir() {
        return Some(d.join("sophon"));
    }
    dirs::home_dir().map(|h| h.join("Desktop").join("sophon"))
}

/// 远程 API 清单路径：桌面 sophon 目录 / sophon.sub
pub fn sub_file() -> Option<std::path::PathBuf> {
    sophon_dir().map(|d| d.join(SUB_FILE_NAME))
}

/// 提示词文件路径：桌面 sophon 目录 / sophon.prompt
pub fn prompt_file() -> Option<std::path::PathBuf> {
    sophon_dir().map(|d| d.join(PROMPT_FILE_NAME))
}

/// 启动时保证提示词就绪：sophon.prompt 不存在则写入内置提示词；存在则读出来用
pub fn ensure_prompt() -> anyhow::Result<String> {
    let p = prompt_file().context("无法定位桌面 sophon 目录")?;
    if p.exists() {
        return std::fs::read_to_string(&p).with_context(|| format!("读取 {}", p.display()));
    }
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("创建 {}", parent.display()))?;
    }
    std::fs::write(&p, DEFAULT_PROMPT).with_context(|| format!("写入 {}", p.display()))?;
    Ok(DEFAULT_PROMPT.to_string())
}

/// 供核心启动调用：sophon.sub 不存在时返回 Ok(None)（联网研究等能力不受影响）
pub fn resolve_default() -> anyhow::Result<Option<Resolved>> {
    match sub_file() {
        None => Ok(None),
        Some(p) if !p.exists() => Ok(None),
        Some(p) => resolve(&p).map(Some),
    }
}

/// 从最后一行往上找第一条可用的远程 API
pub fn resolve(path: &std::path::Path) -> anyhow::Result<Resolved> {
    let entries = read_entries(path);
    if entries.is_empty() {
        if path.exists() {
            anyhow::bail!("{} 没有可用条目（每行格式：<api地址> <api_key> [模型名]）", path.display());
        }
        anyhow::bail!("{} 不存在", path.display());
    }
    let mut tried = 0usize;
    for e in entries.into_iter().rev() {
        tried += 1;
        match fetch_models(&e.base_url, &e.api_key, MODEL_PROBE_TIMEOUT) {
            Ok(models) if !models.is_empty() => {
                let (model, from_list) = match &e.model {
                    Some(m) => (m.clone(), false),
                    None => {
                        let m = pick_max_version(&models)
                            .unwrap_or_else(|| models[0].clone());
                        (m, true)
                    }
                };
                return Ok(Resolved { base_url: e.base_url, api_key: e.api_key, model, from_list });
            }
            Ok(_) => {
                eprintln!("remote api: {} 返回空模型列表，确认后删除该行", e.base_url);
                drop_line(path, &e);
            }
            Err(err) => {
                eprintln!("remote api: {} 拉取模型列表失败（{}），确认后删除该行", e.base_url, err);
                drop_line(path, &e);
            }
        }
    }
    anyhow::bail!("remote api：清单中 {} 条全部不可用（失效行已确认后删除）", tried)
}

/// 删除一条失效行：先确认原行仍在文件里，再精确删除这一处
fn drop_line(path: &std::path::Path, e: &RemoteApiEntry) {
    match remove_line_confirmed(path, &e.raw) {
        Ok(true) => eprintln!("remote api: 已从 {} 删除「{}」", path.display(), e.base_url),
        Ok(false) => eprintln!("remote api: 该行已被其他进程删除，跳过"),
        Err(err) => eprintln!("remote api: 删除失效行失败：{}", err),
    }
}

/// 解析一行：`<api地址> <api_key> [模型名]`；注释行 / 空行 / 字段不足返回 None（不删除）
pub fn parse_line(line: &str) -> Option<RemoteApiEntry> {
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') {
        return None;
    }
    let mut it = t.split_whitespace();
    let base_url = it.next()?.to_string();
    let api_key = it.next()?.to_string();
    let model = it.next().map(|s| s.to_string());
    Some(RemoteApiEntry { base_url, api_key, model, raw: line.to_string() })
}

/// 读取全部条目（文件不存在 / 无法解码返回空）
pub fn read_entries(path: &std::path::Path) -> Vec<RemoteApiEntry> {
    match std::fs::read_to_string(path) {
        Ok(content) => content.lines().filter_map(parse_line).collect(),
        Err(_) => Vec::new(),
    }
}

/// 模型列表地址（OpenAI 兼容：<base>/models；base 已以 /models 结尾则原样）
pub fn models_url(base_url: &str) -> String {
    let b = base_url.trim_end_matches('/');
    if b.ends_with("/models") { b.to_string() } else { format!("{}/models", b) }
}

/// GET {base}/models，返回模型 id 列表（兼容 {"data":[{"id":..}]} 与 {"models":[..]}）
pub fn fetch_models(base_url: &str, api_key: &str, timeout: std::time::Duration) -> anyhow::Result<Vec<String>> {
    let url = models_url(base_url);
    let mut req = ureq::get(&url).timeout(timeout);
    if !api_key.is_empty() {
        req = req.set("Authorization", &format!("Bearer {}", api_key));
    }
    let body = req.call().with_context(|| format!("GET {}", url))?.into_string().context("读取响应体")?;
    let v: serde_json::Value = serde_json::from_str(&body).context("解析模型列表 JSON")?;
    let mut out = Vec::new();
    if let Some(arr) = v["data"].as_array() {
        for m in arr {
            if let Some(id) = m["id"].as_str() { out.push(id.to_string()); }
        }
    } else if let Some(arr) = v["models"].as_array() {
        for m in arr {
            if let Some(id) = m.as_str() {
                out.push(id.to_string());
            } else if let Some(id) = m["id"].as_str().or_else(|| m["name"].as_str()) {
                out.push(id.to_string());
            }
        }
    }
    Ok(out)
}

/// 模型名中的版本号：名字里第一段连续的 `数字[.数字]*`（如 gemini-2.5-pro → [2,5]，
/// gpt-4o → [4]，deepseek-v3.2 → [3,2]；qwen3-32b 只取 3，不把参数量 32 当版本）
pub fn version_key(model_id: &str) -> Vec<u64> {
    static RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\d+(?:\.\d+)*").unwrap());
    RE.find(model_id)
        .map(|m| m.as_str().split('.').map(|s| s.parse().unwrap_or(0)).collect())
        .unwrap_or_default()
}

/// 版本号最大的模型；并列时取字典序更大者（结果确定）
pub fn pick_max_version(models: &[String]) -> Option<String> {
    models.iter().max_by(|a, b| {
        version_key(a).cmp(&version_key(b)).then_with(|| a.cmp(b))
    }).cloned()
}

/// 确认后删除一行：重新读文件，该行原文仍在（精确匹配）才删除这一处并原子重写；
/// 返回 false 表示行已不在（别的进程删过），什么都不动——防止多个核心把行删多
pub fn remove_line_confirmed(path: &std::path::Path, raw: &str) -> anyhow::Result<bool> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    let lines: Vec<&str> = content.lines().collect();
    let Some(pos) = lines.iter().position(|l| *l == raw) else {
        return Ok(false);
    };
    let mut out: Vec<&str> = Vec::with_capacity(lines.len().saturating_sub(1));
    out.extend_from_slice(&lines[..pos]);
    out.extend_from_slice(&lines[pos + 1..]);
    let mut text = out.join("\n");
    if !out.is_empty() {
        text.push('\n');
    }
    let tmp = path.with_extension("txt.tmp");
    std::fs::write(&tmp, &text)?;
    std::fs::rename(&tmp, path)?;
    Ok(true)
}

/// 非流式对话：POST {base}/chat/completions，返回首条回复文本；system_prompt 非空时作为 system 消息
pub fn chat(base_url: &str, api_key: &str, model: &str, system_prompt: &str, prompt: &str, timeout: std::time::Duration) -> anyhow::Result<String> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let mut messages = Vec::new();
    if !system_prompt.trim().is_empty() {
        messages.push(serde_json::json!({"role": "system", "content": system_prompt}));
    }
    messages.push(serde_json::json!({"role": "user", "content": prompt}));
    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": false,
    });
    let resp = ureq::post(&url)
        .timeout(timeout)
        .set("Authorization", &format!("Bearer {}", api_key))
        .send_json(body)
        .with_context(|| format!("POST {}", url))?;
    let text = resp.into_string().context("读取对话响应体")?;
    let v: serde_json::Value = serde_json::from_str(&text).with_context(|| format!("解析对话响应：{}", &text[..text.len().min(200)]))?;
    Ok(v["choices"][0]["message"]["content"].as_str().unwrap_or(&text).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, SocketAddr};

    /// 起一个 mock HTTP 服务：读完请求头（避免未读数据触发 RST 吞掉响应）再回固定响应
    fn spawn_mock(status: &'static str, body: &'static str) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut s = stream;
                // GET 无 body：读到 \r\n\r\n（请求头结束）即可
                let mut buf = [0u8; 8192];
                let mut total = 0usize;
                loop {
                    match s.read(&mut buf[total..]) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            total += n;
                            if buf[..total].windows(4).any(|w| w == b"\r\n\r\n") || total == buf.len() { break; }
                        }
                    }
                }
                let resp = format!(
                    "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status, body.len(), body
                );
                let _ = s.write_all(resp.as_bytes());
                let _ = s.flush();
            }
        });
        addr
    }

    fn write_file(dir: &std::path::Path, text: &str) -> std::path::PathBuf {
        let p = dir.join(SUB_FILE_NAME);
        std::fs::write(&p, text).unwrap();
        p
    }

    #[test]
    fn sophon_dir_and_prompt_flow() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("SOPHON_DIR", dir.path());
        // 两个配置文件都定位到 SOPHON_DIR 指定的目录
        assert_eq!(sub_file().unwrap(), dir.path().join(SUB_FILE_NAME));
        assert_eq!(prompt_file().unwrap(), dir.path().join(PROMPT_FILE_NAME));
        // 提示词不存在 → 写入内置提示词
        let pf = prompt_file().unwrap();
        assert!(!pf.exists());
        assert_eq!(ensure_prompt().unwrap(), DEFAULT_PROMPT);
        assert_eq!(std::fs::read_to_string(&pf).unwrap(), DEFAULT_PROMPT);
        // 提示词已存在 → 原样读取使用，不覆盖
        std::fs::write(&pf, "自定义提示词").unwrap();
        assert_eq!(ensure_prompt().unwrap(), "自定义提示词");
        std::env::remove_var("SOPHON_DIR");
    }

    #[test]
    fn parse_line_formats() {
        let e = parse_line("https://api.x.com/v1 sk-abc").unwrap();
        assert_eq!(e.base_url, "https://api.x.com/v1");
        assert_eq!(e.api_key, "sk-abc");
        assert_eq!(e.model, None);

        let e = parse_line("https://api.x.com/v1 sk-abc my-model").unwrap();
        assert_eq!(e.model.as_deref(), Some("my-model"));

        assert!(parse_line("").is_none());
        assert!(parse_line("  # 注释").is_none());
        assert!(parse_line("只有地址没有key").is_none());
    }

    #[test]
    fn version_compare() {
        assert_eq!(version_key("gemini-2.5-pro"), vec![2, 5]);
        assert_eq!(version_key("gpt-4o"), vec![4]);
        assert_eq!(version_key("deepseek-v3.2"), vec![3, 2]);
        assert_eq!(version_key("qwen3-32b"), vec![3], "参数量 32 不能当版本号");
        assert!(version_key("chat-beta").is_empty());

        let m = |s: &[&str]| -> Vec<String> { s.iter().map(|x| x.to_string()).collect() };
        assert_eq!(pick_max_version(&m(&["deepseek-v3.2", "deepseek-v3.1", "deepseek-chat"])).unwrap(), "deepseek-v3.2");
        assert_eq!(pick_max_version(&m(&["gpt-3.5-turbo", "gpt-4o"])).unwrap(), "gpt-4o");
        assert_eq!(pick_max_version(&m(&["m-2.5", "m-2.11"])).unwrap(), "m-2.11", "2.11 > 2.5 要按数值比");
        assert_eq!(pick_max_version(&m(&["a", "b"])).unwrap(), "b", "无版本号并列时取字典序大者");
        assert_eq!(pick_max_version(&[]), None);
    }

    #[test]
    fn models_url_variants() {
        assert_eq!(models_url("https://api.x.com/v1"), "https://api.x.com/v1/models");
        assert_eq!(models_url("https://api.x.com/v1/"), "https://api.x.com/v1/models");
        assert_eq!(models_url("https://api.x.com/v1/models"), "https://api.x.com/v1/models");
    }

    #[test]
    fn remove_line_confirmed_only_exact_line() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("list.txt");
        std::fs::write(&p, "a 1\nb 2\nc 3\n").unwrap();

        assert!(remove_line_confirmed(&p, "b 2").unwrap());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "a 1\nc 3\n");

        // 行不在了 → 不动文件（其他进程删过的情况）
        assert!(!remove_line_confirmed(&p, "b 2").unwrap());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "a 1\nc 3\n");

        // 重复行每次只删一处
        std::fs::write(&p, "x 1\nx 1\n").unwrap();
        assert!(remove_line_confirmed(&p, "x 1").unwrap());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "x 1\n");
        assert!(remove_line_confirmed(&p, "x 1").unwrap());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "");
    }

    #[test]
    fn resolve_picks_last_line_and_max_version_model() {
        let dead = spawn_mock("404 Not Found", r#"{"error":"no"}"#);
        let live = spawn_mock("200 OK", r#"{"data":[{"id":"m-1.0"},{"id":"m-2.5"},{"id":"m-2.11"}]}"#);
        let dir = tempfile::tempdir().unwrap();
        let p = write_file(
            dir.path(),
            &format!("http://127.0.0.1:{} k-dead\nhttp://127.0.0.1:{} k-live\n", dead.port(), live.port()),
        );

        let r = resolve(&p).unwrap();
        assert_eq!(r.base_url, format!("http://127.0.0.1:{}", live.port()));
        assert_eq!(r.model, "m-2.11", "应取版本号最大的模型");
        assert!(r.from_list);
        // 成功行不删：文件原样
        assert_eq!(std::fs::read_to_string(&p).unwrap().lines().count(), 2);
    }

    #[test]
    fn resolve_uses_specified_model() {
        let live = spawn_mock("200 OK", r#"{"data":[{"id":"m-2.11"},{"id":"m-9.9"}]}"#);
        let dir = tempfile::tempdir().unwrap();
        let p = write_file(dir.path(), &format!("http://127.0.0.1:{} k my-model\n", live.port()));
        let r = resolve(&p).unwrap();
        assert_eq!(r.model, "my-model", "行内指定了模型名就按指定的用");
        assert!(!r.from_list);
    }

    #[test]
    fn resolve_falls_back_upward_deleting_only_failed_line() {
        // 最后一行失效 → 确认删除它，回退到上一行；上一行成功后不再动更上面的行
        let live = spawn_mock("200 OK", r#"{"data":[{"id":"m-1.0"}]}"#);
        let dead = spawn_mock("404 Not Found", r#"{"error":"x"}"#);
        let dir = tempfile::tempdir().unwrap();
        let live_line = format!("http://127.0.0.1:{} k-live\n", live.port());
        let p = write_file(dir.path(), &format!("{}http://127.0.0.1:{} k-dead\n", live_line, dead.port()));

        let r = resolve(&p).unwrap();
        assert_eq!(r.base_url, format!("http://127.0.0.1:{}", live.port()));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), live_line, "只删确认失效的最后一行");
    }

    #[test]
    fn resolve_all_dead_empties_file_and_errors() {
        let dead1 = spawn_mock("401 Unauthorized", r#"{"error":"bad key"}"#);
        let dead2 = spawn_mock("403 Forbidden", "no");
        let dir = tempfile::tempdir().unwrap();
        let p = write_file(
            dir.path(),
            &format!("http://127.0.0.1:{} k1\nhttp://127.0.0.1:{} k2\n", dead1.port(), dead2.port()),
        );

        assert!(resolve(&p).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "", "失效行全部确认删除");
        // 再次解析：没有可用条目
        assert!(resolve(&p).is_err());
    }

    #[test]
    fn empty_model_list_counts_as_failure() {
        let empty = spawn_mock("200 OK", r#"{"data":[]}"#);
        let dir = tempfile::tempdir().unwrap();
        let p = write_file(dir.path(), &format!("http://127.0.0.1:{} k\n", empty.port()));
        assert!(resolve(&p).is_err(), "拉不到模型列表按失效处理");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "");
    }

    #[test]
    fn malformed_lines_never_deleted() {
        let live = spawn_mock("200 OK", r#"{"data":[{"id":"m-1"}]}"#);
        let dir = tempfile::tempdir().unwrap();
        let p = write_file(dir.path(), &format!("没有key的行\nhttp://127.0.0.1:{} k\n", live.port()));
        let r = resolve(&p).unwrap();
        assert_eq!(r.model, "m-1");
        let left = std::fs::read_to_string(&p).unwrap();
        assert!(left.contains("没有key的行"), "解析不了的行不动");
    }
}
