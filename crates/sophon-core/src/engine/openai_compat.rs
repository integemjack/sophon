// 本机 OpenAI 兼容引擎：指向用户本机跑的服务（Ollama / llama.cpp / LM Studio…）。
// 只允许 127.0.0.1 / localhost（用户约束：严禁云端 API）。SSE 流式解析。
use super::{EngineConfig, EngineEvent, StreamRequest, ToolDescriptor};
use crate::models::{AgentUsage, ChatMessage, Role};
use anyhow::{anyhow, Context};
use base64::Engine as _Base64Engine;

pub struct OpenAICompatEngine {
    pub config: EngineConfig,
}

impl OpenAICompatEngine {
    pub fn new(config: EngineConfig) -> anyhow::Result<Self> {
        if !config.is_local() {
            return Err(anyhow!(
                "按用户约束只允许本机地址（127.0.0.1 / localhost），拒绝：{}",
                config.base_url
            ));
        }
        Ok(OpenAICompatEngine { config })
    }

    fn endpoint(&self) -> String {
        let base = self.config.base_url.trim_end_matches('/');
        format!("{}/chat/completions", base)
    }
}

impl super::ChatEngine for OpenAICompatEngine {
    fn id(&self) -> &str {
        &self.config.id
    }
    fn title(&self) -> &str {
        &self.config.title
    }
    fn model(&self) -> &str {
        &self.config.model
    }
    fn supports_tools(&self) -> bool {
        self.config.supports_tools
    }

    fn stream(&self, req: &StreamRequest, sink: &mut dyn FnMut(EngineEvent)) -> anyhow::Result<()> {
        let url = self.endpoint();
        let mut body = serde_json::json!({
            "model": self.config.model,
            "stream": true,
            "temperature": req.temperature,
            "messages": [],
        });
        // system
        body["messages"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"role": "system", "content": req.system}));
        // history
        for m in &req.messages {
            let role = match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
            };
            // 收集图像块（vision：Media 块且 mime 为 image/*）
            let images: Vec<&crate::models::MediaAttachment> = m
                .blocks
                .iter()
                .filter_map(|b| match &b.kind {
                    crate::models::BlockKind::Media { media }
                        if media.mime_type.starts_with("image/") =>
                    {
                        Some(media)
                    }
                    _ => None,
                })
                .collect();
            let content = if images.is_empty() {
                serde_json::json!(m.text_content())
            } else {
                // OpenAI vision 格式：content 为数组（text + image_url）
                let mut parts = vec![serde_json::json!({"type": "text", "text": m.text_content()})];
                for media in &images {
                    if let Ok(img_bytes) = std::fs::read(&media.relative_path) {
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&img_bytes);
                        parts.push(serde_json::json!({
                            "type": "image_url",
                            "image_url": {"url": format!("data:{};base64,{}", media.mime_type, b64)}
                        }));
                    }
                }
                serde_json::json!(parts)
            };
            body["messages"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({"role": role, "content": content}));
        }
        // tools
        if self.config.supports_tools && !req.tools.is_empty() {
            let tools: Vec<serde_json::Value> = req
                .tools
                .iter()
                .map(|t: &ToolDescriptor| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters,
                        }
                    })
                })
                .collect();
            body["tools"] = serde_json::Value::Array(tools);
        }

        let api_key = self
            .config
            .api_key_secret
            .as_deref()
            .and_then(|name| crate::store::AppCore::with_secret(name));
        let mut request = ureq::post(&url)
            .set("Content-Type", "application/json")
            .set("Accept", "text/event-stream");
        if let Some(key) = &api_key {
            if !key.is_empty() {
                request = request.set("Authorization", &format!("Bearer {}", key));
            }
        }
        let response = request
            .send_json(body)
            .with_context(|| format!("连接本机引擎失败：{}", url))?;
        let status = response.status();
        if !(200..300).contains(&status) {
            let text = response.into_string().unwrap_or_default();
            return Err(anyhow!("本机引擎返回 {}：{}", status, truncate(&text, 300)));
        }

        // SSE 解析
        let reader = std::io::BufReader::new(response.into_reader());
        use std::io::BufRead;
        let mut full_text = String::new();
        let mut usage = AgentUsage::default();
        for line in reader.lines() {
            let line = line?;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let data = match line.strip_prefix("data:") {
                Some(d) => d.trim(),
                None => continue,
            };
            if data == "[DONE]" {
                break;
            }
            let value: serde_json::Value = match serde_json::from_str(data) {
                Ok(v) => v,
                Err(_) => continue,
            };
            // 增量
            if let Some(delta) = value["choices"][0]["delta"].as_object() {
                if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                    if !content.is_empty() {
                        full_text.push_str(content);
                        sink(EngineEvent::Chunk(content.to_string()));
                    }
                }
                if let Some(reasoning) = delta.get("reasoning_content").and_then(|c| c.as_str()) {
                    if !reasoning.is_empty() {
                        sink(EngineEvent::Thinking(reasoning.to_string()));
                    }
                }
                // 工具调用
                if let Some(tc) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                    for call in tc {
                        let name = call["function"]["name"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();
                        let args = call["function"]["arguments"]
                            .as_str()
                            .map(|s| serde_json::from_str(s).unwrap_or(serde_json::Value::Null))
                            .unwrap_or(serde_json::Value::Null);
                        let id = call["id"]
                            .as_str()
                            .unwrap_or(&crate::models::new_uuid())
                            .to_string();
                        if !name.is_empty() {
                            sink(EngineEvent::ToolCall { id, name, arguments: args });
                        }
                    }
                }
            }
            // 用量
            if let Some(u) = value.get("usage").and_then(|u| u.as_object()) {
                usage.prompt_tokens = u
                    .get("prompt_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                usage.generated_tokens = u
                    .get("completion_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
            }
        }
        usage.model = Some(self.config.model.clone());
        usage.service = Some(self.config.title.clone());
        sink(EngineEvent::Done { usage: Some(usage) });
        Ok(())
    }
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect::<String>() + if s.chars().count() > n { "…" } else { "" }
}

// ChatMessage 导入供类型标注用
#[allow(unused_imports)]
use ChatMessage as _ChatMessage;
