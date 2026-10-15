// sophon-core：无界面核心（数据模型、存储、引擎、工具、Agent、终端、助手、MCP）。
pub mod models;
pub mod util;
pub mod engine;
pub mod store;
pub mod terminal;
pub mod agent;
pub mod tools;
pub mod assistant;
pub mod mcp;
pub mod remote;
pub mod remote_api;
pub mod autonomy;
pub mod learning;
pub mod selfimprove;
pub mod sandbox;
pub mod context_compress;
pub mod rollback;
pub mod selfcheck;
pub mod web;
pub mod agent_loop;

pub use models::*;
pub use util::{CancelToken, MainThread, Paths};
pub use engine::{ChatEngine, EngineConfig, EngineEvent, StreamRequest, ToolDescriptor};

#[cfg(test)]
mod tests;
pub use store::{AppCore, AppSettings, ConversationStore, EngineStore, SecretStore};
pub use terminal::{TerminalEmulator, TerminalManager};
pub use agent::team::{AgentProfile, AgentRun, AgentStatus, AgentTeamState};
pub use agent::runner::AgentRunner;
pub use tools::{ToolContext, ToolRegistry, ToolOutput};
pub use assistant::AssistantRuntime;
pub use mcp::{McpManager, McpServerConfig};
pub use remote::{RemoteClient, RemoteConfig, RemoteEvent, RemoteState, ServerInfo};
