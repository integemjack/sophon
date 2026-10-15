# -*- coding: utf-8 -*-
# r1: remote_api.rs += chat_stream (SSE) + append-mark protocol + verified append
from pathlib import Path
p = Path(r"C:\Users\ultra\Desktop\works\hermes\src\remote_api.rs")
t = p.read_text(encoding="utf-8")

NEW_CODE = r'''
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

'''

anchor = "#[cfg(test)]\nmod tests {"
cnt = t.count(anchor)
print("anchor tests-mod:", cnt)
t = t.replace(anchor, NEW_CODE + anchor, 1)

p.write_text(t, encoding="utf-8", newline="\n")
print("saved remote_api.rs")
