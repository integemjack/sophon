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
use crate::util::sophon_dir;

/// 清单与提示词文件名（都在桌面 sophon 目录下）
pub const SUB_FILE_NAME: &str = "sophon.sub";
pub const PROMPT_FILE_NAME: &str = "sophon.prompt";

/// 核心内置提示词（首次启动写入 sophon.prompt；旧版在 ensure_prompt 里自动升级）
pub const DEFAULT_PROMPT: &str = crate::prompts::RESEARCH;

/// 旧版内置调研提示词（升级判定用，与升级前版本一致）
pub const LEGACY_DEFAULT_PROMPT: &str = crate::prompts::LEGACY_RESEARCH;
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

/// 远程 API 清单路径：桌面 sophon 目录 / sophon.sub（目录见 util::sophon_dir）
pub fn sub_file() -> Option<std::path::PathBuf> {
    sophon_dir().map(|d| d.join(SUB_FILE_NAME))
}

/// 提示词文件路径：桌面 sophon 目录 / sophon.prompt
pub fn prompt_file() -> Option<std::path::PathBuf> {
    sophon_dir().map(|d| d.join(PROMPT_FILE_NAME))
}

/// 启动时保证提示词就绪：sophon.prompt 不存在则写入内置提示词；
/// 还是旧版内置调研提示词则自动升级；用户自己改过的原样保留。
pub fn ensure_prompt() -> anyhow::Result<String> {
    let p = prompt_file().context("无法定位桌面 sophon 目录")?;
    if p.exists() {
        let cur = std::fs::read_to_string(&p).with_context(|| format!("读取 {}", p.display()))?;
        if cur.split_whitespace().collect::<String>()
            == LEGACY_DEFAULT_PROMPT.split_whitespace().collect::<String>()
        {
            std::fs::write(&p, DEFAULT_PROMPT).with_context(|| format!("写入 {}", p.display()))?;
            return Ok(DEFAULT_PROMPT.to_string());
        }
        return Ok(cur);
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


// ── 流式对话（ask 实时输出）────────────────────────────

/// SSE data 行的增量：`data: {...}` → (正文增量, 思考增量)；`data: [DONE]` / 无增量 → None
pub fn parse_sse_delta(line: &str) -> Option<(Option<String>, Option<String>)> {
    let data = line.strip_prefix("data:")?.trim();
    if data == "[DONE]" {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(data).ok()?;
    let d = &v["choices"][0]["delta"];
    let content = d["content"].as_str().map(|s| s.to_string());
    let reasoning = d["reasoning_content"].as_str().map(|s| s.to_string());
    if content.is_none() && reasoning.is_none() {
        return None;
    }
    Some((content, reasoning))
}

/// 流式对话：每收到一段就回调 (文本, 是否思考)，实时打印由调用方做；
/// 返回完整正文（思考不计入正文）。服务端不支持 stream（回整体 JSON）时自动回退非流式。
pub fn chat_stream(
    base_url: &str,
    api_key: &str,
    model: &str,
    system_prompt: &str,
    prompt: &str,
    timeout: std::time::Duration,
    on_delta: &mut dyn FnMut(&str, bool),
) -> anyhow::Result<String> {
    use std::io::BufRead;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let mut messages = Vec::new();
    if !system_prompt.trim().is_empty() {
        messages.push(serde_json::json!({"role": "system", "content": system_prompt}));
    }
    messages.push(serde_json::json!({"role": "user", "content": prompt}));
    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": true,
    });
    let resp = ureq::post(&url)
        .timeout(timeout)
        .set("Authorization", &format!("Bearer {}", api_key))
        .set("Accept", "text/event-stream")
        .send_json(body)
        .with_context(|| format!("POST {}", url))?;
    // 服务端没按 SSE 回（忽略 stream=true）→ 整体 JSON，回退非流式
    if !resp.content_type().contains("text/event-stream") {
        let text = resp.into_string().context("读取对话响应体")?;
        let v: serde_json::Value = serde_json::from_str(&text)
            .with_context(|| format!("解析对话响应：{}", &text[..text.len().min(200)]))?;
        let s = v["choices"][0]["message"]["content"].as_str().unwrap_or(&text).to_string();
        on_delta(&s, false);
        return Ok(s);
    }
    let reader = std::io::BufReader::new(resp.into_reader());
    let mut full = String::new();
    for line in reader.lines() {
        let line = line.context("读取流式响应中断")?;
        if !line.starts_with("data:") {
            continue; // SSE 空行 / 注释 / event: 行
        }
        if let Some((content, reasoning)) = parse_sse_delta(&line) {
            if let Some(t) = reasoning {
                on_delta(&t, true);
            }
            if let Some(t) = content {
                on_delta(&t, false);
                full.push_str(&t);
            }
        }
    }
    Ok(full)
}

// ── 联网调研结果自动入库（回答中的标记 → 验证 → 追加到 sophon.sub）──

pub const APPEND_OPEN: &str = "<SOPHON_APPEND>";
pub const APPEND_CLOSE: &str = "</SOPHON_APPEND>";

/// ask 时注入系统提示词的工具说明：告诉模型清单文件路径与追加协议，
/// 模型找到候选 API 后输出标记，由本机程序验证并落盘——模型无需（也不能）直接写文件。
pub fn append_tool_note(sub_path_text: &str) -> String {
    format!(
        "\n\n〔本地能力〕本机的远程 API 清单文件：{p}（每行一条：`<api地址> <api_key> [模型名]`）。\n\
         当用户要你「找可用的远程模型 API 并添加到清单」时，直接联网调研；每确定一个候选，就在回答里单独一行输出：\n\
         {o}https://api.example.com/v1 sk-xxxx 模型名{c}\n\
         地址与密钥必须有，模型名可省略；可以输出多个标记。本机程序会逐个验证（拉取 /models 确认可用）后自动追加到清单末尾——你有写入清单的能力，输出标记即可，不要说无法访问本地文件。",
        p = sub_path_text, o = APPEND_OPEN, c = APPEND_CLOSE
    )
}

/// 从模型回答提取全部标记内容（保持出现顺序；未闭合 / 空内容丢弃）
pub fn parse_append_marks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find(APPEND_OPEN) {
        let after = &rest[i + APPEND_OPEN.len()..];
        match after.find(APPEND_CLOSE) {
            Some(j) => {
                let inner = after[..j].trim();
                if !inner.is_empty() {
                    out.push(inner.to_string());
                }
                rest = &after[j + APPEND_CLOSE.len()..];
            }
            None => break,
        }
    }
    out
}

/// 单条追加结果
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppendOutcome {
    Added,
    Exists,
    Rejected(String),
}

/// 验证并追加候选条目：解析 → 去重 → 拉 /models 确认可用 → 原子追加（文件不存在则创建）。
/// 返回 (标记原文, 结果) 列表，顺序与输入一致。
pub fn append_verified(
    path: &std::path::Path,
    marks: &[String],
    timeout: std::time::Duration,
) -> Vec<(String, AppendOutcome)> {
    let mut results = Vec::new();
    for m in marks {
        let outcome = match parse_line(m) {
            None => AppendOutcome::Rejected("格式不对（需要：<api地址> <api_key> [模型名]）".into()),
            Some(e) => {
                let dup = read_entries(path).iter().any(|x| {
                    x.base_url == e.base_url && x.api_key == e.api_key && x.model == e.model
                });
                if dup {
                    AppendOutcome::Exists
                } else {
                    match fetch_models(&e.base_url, &e.api_key, timeout) {
                        Ok(models) if !models.is_empty() => {
                            match append_line_atomic(path, &e.raw) {
                                Ok(()) => AppendOutcome::Added,
                                Err(err) => AppendOutcome::Rejected(format!("写入失败：{}", err)),
                            }
                        }
                        Ok(_) => AppendOutcome::Rejected("模型列表为空".into()),
                        Err(err) => AppendOutcome::Rejected(format!("验证失败：{}", shorten_err(&err.to_string(), 80))),
                    }
                }
            }
        };
        results.push((m.clone(), outcome));
    }
    results
}

/// 追加一行（tmp + rename 原子重写）；文件不存在则创建（含父目录）
pub fn append_line_atomic(path: &std::path::Path, line: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut content = std::fs::read_to_string(path).unwrap_or_default();
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(line.trim_end());
    content.push('\n');
    let tmp = path.with_extension("txt.tmp");
    std::fs::write(&tmp, &content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn shorten_err(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
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
        // 旧版内置调研提示词 → 自动升级为新版
        std::fs::write(&pf, LEGACY_DEFAULT_PROMPT).unwrap();
        assert_eq!(ensure_prompt().unwrap(), DEFAULT_PROMPT);
        assert_eq!(std::fs::read_to_string(&pf).unwrap(), DEFAULT_PROMPT);
        // 提示词已存在（用户改过）→ 原样读取使用，不覆盖
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


#[cfg(test)]
mod stream_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, SocketAddr};
    use std::time::Duration;

    /// mock HTTP：自定义 Content-Type；读完请求头并按 Content-Length 读掉 body 后回固定响应
    fn spawn_mock_ct(ct: &'static str, body: &'static str) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut s = stream;
                let mut buf: Vec<u8> = Vec::new();
                let mut chunk = [0u8; 8192];
                // 读到头结束
                let head_end = loop {
                    match s.read(&mut chunk) {
                        Ok(0) | Err(_) => break buf.len(),
                        Ok(n) => {
                            buf.extend_from_slice(&chunk[..n]);
                            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                break pos + 4;
                            }
                            if buf.len() > 1 << 20 {
                                break buf.len();
                            }
                        }
                    }
                };
                // 按 Content-Length 读掉 body（POST 有体；不读会 RST 吞响应）
                let head = String::from_utf8_lossy(&buf[..head_end]).to_lowercase();
                let clen: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0)))
                    .unwrap_or(0);
                let mut have = buf.len() - head_end;
                while have < clen {
                    match s.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            have += n;
                        }
                    }
                }
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    ct,
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes());
                let _ = s.flush();
            }
        });
        addr
    }

    const SSE_BODY: &str = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"想\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\"你\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\"好\"}}]}\n\n\
data: [DONE]\n\n";

    #[test]
    fn sse_delta_parse() {
        let (c, r) = parse_sse_delta(
            "data: {\"choices\":[{\"delta\":{\"content\":\"你\"}}]}",
        ).unwrap();
        assert_eq!(c.as_deref(), Some("你"));
        assert!(r.is_none());

        let (c, r) = parse_sse_delta(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"想\"}}]}",
        ).unwrap();
        assert!(c.is_none());
        assert_eq!(r.as_deref(), Some("想"));

        // role-only delta（无文本）
        assert!(parse_sse_delta("data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}").is_none());
        // 结束标记
        assert!(parse_sse_delta("data: [DONE]").is_none());
        // 非 data 行 / 坏 JSON
        assert!(parse_sse_delta(": keep-alive").is_none());
        assert!(parse_sse_delta("data: {bad").is_none());
    }

    #[test]
    fn chat_stream_receives_deltas_in_order() {
        let addr = spawn_mock_ct("text/event-stream", SSE_BODY);
        let url = format!("http://127.0.0.1:{}", addr.port());
        let mut got: Vec<(String, bool)> = Vec::new();
        let full = chat_stream(&url, "k", "m", "", "hi", Duration::from_secs(5), &mut |t, r| {
            got.push((t.to_string(), r));
        })
        .unwrap();
        assert_eq!(full, "你好", "只累积正文");
        assert_eq!(
            got,
            vec![("想".to_string(), true), ("你".to_string(), false), ("好".to_string(), false)],
            "思考先于正文、逐段回调"
        );
    }

    #[test]
    fn chat_stream_falls_back_when_not_sse() {
        let addr = spawn_mock_ct(
            "application/json",
            "{\"choices\":[{\"message\":{\"content\":\"整段回答\"}}]}",
        );
        let url = format!("http://127.0.0.1:{}", addr.port());
        let mut got: Vec<(String, bool)> = Vec::new();
        let full = chat_stream(&url, "k", "m", "", "hi", Duration::from_secs(5), &mut |t, r| {
            got.push((t.to_string(), r));
        })
        .unwrap();
        assert_eq!(full, "整段回答");
        assert_eq!(got, vec![("整段回答".to_string(), false)]);
    }

    #[test]
    fn append_marks_extraction() {
        let t = "前言\n<SOPHON_APPEND>http://a/v1 k1 m1</SOPHON_APPEND>\n中间\n<SOPHON_APPEND>http://b/v1  k2</SOPHON_APPEND>\n<SOPHON_APPEND></SOPHON_APPEND>\n<SOPHON_APPEND>没闭合";
        assert_eq!(
            parse_append_marks(t),
            vec!["http://a/v1 k1 m1".to_string(), "http://b/v1  k2".to_string()],
            "按序提取、空内容与未闭合丢弃"
        );
        assert!(parse_append_marks("没有任何标记").is_empty());
    }

    #[test]
    fn append_verified_flows() {
        let live = spawn_mock_ct("application/json", "{\"data\":[{\"id\":\"m-1\"}]}");
        let dead = spawn_mock_ct("application/json", "no");
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(SUB_FILE_NAME);

        // 可用 → 追加（文件原本不存在 → 创建）
        let live_line = format!("http://127.0.0.1:{} k-live", live.port());
        let res = append_verified(&p, &[live_line.clone()], MODEL_PROBE_TIMEOUT);
        assert_eq!(res[0].1, AppendOutcome::Added);
        assert!(p.exists(), "文件不存在应被创建");
        assert!(std::fs::read_to_string(&p).unwrap().contains(&live_line));

        // 同一条再来 → 已存在跳过（不重复写）
        let res = append_verified(&p, &[live_line.clone()], MODEL_PROBE_TIMEOUT);
        assert_eq!(res[0].1, AppendOutcome::Exists);
        let after = std::fs::read_to_string(&p).unwrap();
        assert_eq!(after.lines().filter(|l| *l == live_line).count(), 1, "不写重复行");

        // 拉不到模型列表 → 拒绝且不写入
        let dead_line = format!("http://127.0.0.1:{} k-dead", dead.port());
        let res = append_verified(&p, &[dead_line], MODEL_PROBE_TIMEOUT);
        assert!(matches!(res[0].1, AppendOutcome::Rejected(_)));
        assert!(!std::fs::read_to_string(&p).unwrap().contains("k-dead"));

        // 格式不对 → 拒绝
        let res = append_verified(&p, &["只有一列".to_string()], MODEL_PROBE_TIMEOUT);
        assert!(matches!(res[0].1, AppendOutcome::Rejected(_)));

        // 多条混合：顺序保持
        let res = append_verified(
            &p,
            &["只有一列".to_string(), live_line.clone()],
            MODEL_PROBE_TIMEOUT,
        );
        assert!(matches!(res[0].1, AppendOutcome::Rejected(_)));
        assert_eq!(res[1].1, AppendOutcome::Exists);
    }

    #[test]
    fn tool_note_mentions_path_and_protocol() {
        let note = append_tool_note("C:\\sophon\\sophon.sub");
        assert!(note.contains("C:\\sophon\\sophon.sub"));
        assert!(note.contains(APPEND_OPEN) && note.contains(APPEND_CLOSE));
    }
}
