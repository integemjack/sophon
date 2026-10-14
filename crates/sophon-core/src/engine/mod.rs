// 大脑（ChatEngine）接口。
// 对应 docs/ARCHITECTURE.md §6：对话用引擎，本机自己跑的 OpenAI 兼容服务
// （Ollama、LM Studio、llama.cpp…）也按这个接口接入。
// 本程序按用户约束不接任何云端 API：引擎只允许指向本机（127.0.0.1 / localhost），
// 另有一个不联网的内置离线引擎作默认，保证全部功能离线可用。
use crate::models::*;
use serde::Serialize;

/// 流式事件
#[derive(Debug, Clone)]
pub enum EngineEvent {
    /// 正文增量
    Chunk(String),
    /// 思考增量
    Thinking(String),
    /// 模型请求调用工具
    ToolCall {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
    /// 本轮结束
    Done { usage: Option<AgentUsage> },
}

/// 工具描述（发给模型）
#[derive(Serialize, Clone, Debug)]
pub struct ToolDescriptor {
    pub name: String,
    pub title: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Clone)]
pub struct StreamRequest {
    pub system: String,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDescriptor>,
    pub thinking: Option<ThinkingLevel>,
    pub temperature: f32,
}

pub trait ChatEngine: Send + Sync {
    fn id(&self) -> &str;
    fn title(&self) -> &str;
    fn model(&self) -> &str;
    fn supports_tools(&self) -> bool;
    /// 流式调用：边生成边通过 sink 送出事件。出错返回 Err。
    fn stream(&self, req: &StreamRequest, sink: &mut dyn FnMut(EngineEvent)) -> anyhow::Result<()>;
}

pub mod offline;
pub mod openai_compat;

pub use offline::OfflineEngine;
pub use openai_compat::OpenAICompatEngine;

/// 引擎配置（对应 docs §5.3 的 EngineConfig；apiKeySecret 只存密钥名）
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EngineConfig {
    pub id: String,
    pub title: String,
    pub base_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_secret: Option<String>,
    pub model: String,
    #[serde(default = "default_true")]
    pub supports_tools: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl EngineConfig {
    /// 只允许本机地址（用户约束：严禁云端 API / 联网依赖）
    pub fn is_local(&self) -> bool {
        let host = self
            .base_url
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .split('/')
            .next()
            .unwrap_or("");
        let host = host.split(':').next().unwrap_or(host);
        host.is_empty()
            || host == "localhost"
            || host == "127.0.0.1"
            || host == "::1"
            || host.starts_with("127.")
    }
}

use serde::Deserialize;
