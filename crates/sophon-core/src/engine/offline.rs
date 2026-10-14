// 内置离线引擎：不联网，规则路由。
// 读懂用户意图并真实驱动工具链（找文件、读文件、写文件、跑命令、派子 Agent…），
// 问答则用本地知识库回答。保证「功能完全可用」在零配置、零网络下成立。
use super::{EngineEvent, StreamRequest, ToolDescriptor};
use crate::models::AgentUsage;
use anyhow::anyhow;
use std::time::Duration;

pub struct OfflineEngine {
    pub id: String,
    pub title: String,
}

impl OfflineEngine {
    pub fn new() -> Self {
        OfflineEngine {
            id: "offline".to_string(),
            title: "内置离线引擎".to_string(),
        }
    }

    /// 把一段文字切成小块，模拟流式输出（让界面能看到逐字效果）
    fn stream_text(&self, text: &str, sink: &mut dyn FnMut(EngineEvent)) {
        let mut rest = text;
        while !rest.is_empty() {
            let take = rest.char_indices().nth(24).map(|(i, _)| i).unwrap_or(rest.len());
            let (chunk, tail) = rest.split_at(take);
            sink(EngineEvent::Chunk(chunk.to_string()));
            std::thread::sleep(Duration::from_millis(12));
            rest = tail;
        }
    }
}

impl super::ChatEngine for OfflineEngine {
    fn id(&self) -> &str {
        &self.id
    }
    fn title(&self) -> &str {
        &self.title
    }
    fn model(&self) -> &str {
        "sophon-offline"
    }
    fn supports_tools(&self) -> bool {
        true
    }

    fn stream(&self, req: &StreamRequest, sink: &mut dyn FnMut(EngineEvent)) -> anyhow::Result<()> {
        // 取最后一条用户消息
        let last_user = req
            .messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, crate::models::Role::User))
            .map(|m| m.text_content())
            .unwrap_or_default();
        let lower = last_user.to_lowercase();
        let tools: Vec<&str> = req.tools.iter().map(|t| t.name.as_str()).collect();
        let has = |name: &str| tools.contains(&name);

        // ── 意图路由：按关键词决定调哪个工具 ──
        // 联网搜索（放在文件搜索之前：「搜索X」「搜一下X」优先联网；明确「搜文件/搜内容」才走本地）
        if (lower.contains("联网")
            || lower.contains("上网")
            || lower.contains("网上")
            || lower.contains("互联网")
            || lower.contains("搜一下")
            || lower.contains("查一下")
            || lower.contains("搜索") && !lower.contains("文件") && !lower.contains("内容")
            || lower.contains("最新")
            || lower.contains("新闻")
            || lower.contains("web_search"))
            && has("web_search")
        {
            let query = extract_quoted(&last_user)
                .or_else(|| {
                    for kw in ["搜索", "搜一下", "查一下", "查查", "联网查", "联网搜索"] {
                        if let Some(pos) = last_user.find(kw) {
                            let rest = last_user[pos + kw.len()..].trim();
                            if !rest.is_empty() {
                                return Some(rest.to_string());
                            }
                        }
                    }
                    None
                })
                .unwrap_or_else(|| last_user.trim().to_string());
            sink(EngineEvent::ToolCall {
                id: crate::models::new_uuid(),
                name: "web_search".to_string(),
                arguments: serde_json::json!({"query": query}),
            });
            return Ok(());
        }
        // 跑命令
        if (lower.contains("运行") || lower.contains("执行") || lower.contains("跑一下") || lower.contains("run"))
            && has("run_shell")
            && (lower.contains("命令") || lower.contains("cmd") || lower.contains("运行 ") || lower.contains("执行 "))
        {
            let cmd = extract_quoted(&last_user)
                .or_else(|| {
                    last_user.splitn(2, ['：', ':']).nth(1).map(|s| s.trim().to_string())
                })
                .or_else(|| {
                    // 「运行命令 xxx」/「运行 xxx」/「执行 xxx」之后取命令
                    for kw in ["运行命令", "执行命令", "运行", "执行"] {
                        if let Some(pos) = last_user.find(kw) {
                            let rest = last_user[pos + kw.len()..].trim();
                            if !rest.is_empty() {
                                return Some(rest.to_string());
                            }
                        }
                    }
                    None
                })
                .unwrap_or_else(|| "echo hello".to_string());
            sink(EngineEvent::ToolCall {
                id: crate::models::new_uuid(),
                name: "run_shell".to_string(),
                arguments: serde_json::json!({"command": cmd}),
            });
            return Ok(());
        }
        // 找文件
        if (lower.contains("找文件") || lower.contains("查找文件") || lower.contains("搜索文件") || lower.contains("find"))
            && has("find_files")
        {
            let pat = extract_quoted(&last_user).unwrap_or_else(|| "*.rs".to_string());
            sink(EngineEvent::ToolCall {
                id: crate::models::new_uuid(),
                name: "find_files".to_string(),
                arguments: serde_json::json!({"pattern": pat}),
            });
            return Ok(());
        }
        // 内容搜索
        if (lower.contains("搜内容") || lower.contains("搜索内容") || lower.contains("哪个文件") || lower.contains("grep") || lower.contains("search"))
            && has("search_files")
        {
            let pat = extract_quoted(&last_user).unwrap_or_else(|| "TODO".to_string());
            sink(EngineEvent::ToolCall {
                id: crate::models::new_uuid(),
                name: "search_files".to_string(),
                arguments: serde_json::json!({"pattern": pat}),
            });
            return Ok(());
        }
        // 读文件
        if (lower.contains("读文件") || lower.contains("读取") || lower.contains("看看") || lower.contains("read"))
            && has("read_file")
        {
            let path = extract_quoted(&last_user).unwrap_or_else(|| "Cargo.toml".to_string());
            sink(EngineEvent::ToolCall {
                id: crate::models::new_uuid(),
                name: "read_file".to_string(),
                arguments: serde_json::json!({"path": path}),
            });
            return Ok(());
        }
        // 写文件
        if (lower.contains("写文件") || lower.contains("创建文件") || lower.contains("write"))
            && has("write_file")
        {
            sink(EngineEvent::ToolCall {
                id: crate::models::new_uuid(),
                name: "write_file".to_string(),
                arguments: serde_json::json!({"path": "output.txt", "content": "由内置离线引擎写入"}),
            });
            return Ok(());
        }
        // 派子 Agent
        if (lower.contains("派子") || lower.contains("子智能体") || lower.contains("delegate") || lower.contains("并行"))
            && has("delegate_task")
        {
            sink(EngineEvent::ToolCall {
                id: crate::models::new_uuid(),
                name: "delegate_task".to_string(),
                arguments: serde_json::json!({
                    "profile": {"name": "研究员", "role": "你是研究员，用 web_search 搜索并总结"},
                    "task": "调查并总结当前目录的项目结构"
                }),
            });
            return Ok(());
        }
        // 任务状态
        if lower.contains("任务状态") && has("update_task_state") {
            sink(EngineEvent::ToolCall {
                id: crate::models::new_uuid(),
                name: "update_task_state".to_string(),
                arguments: serde_json::json!({
                    "goal": "演示任务",
                    "steps": [{"text": "第一步", "status": "doing"}]
                }),
            });
            return Ok(());
        }

        // ── 问答：本地知识库 ──
        let answer: String = if lower.contains("你好") || lower.contains("hello") || lower.contains("hi") {
            "你好！我是智子（Sophon）的内置离线引擎。我不联网，但能驱动本地工具链：找文件、读文件、写文件、跑命令、搜索内容、派子智能体。试试说「列出当前目录的 Rust 文件」或「读 Cargo.toml」。".to_string()
        } else if lower.contains("你是谁") || lower.contains("介绍") {
            "我是智子（Sophon），Friday 桌面助理的 Rust 实现。我的大脑默认是内置离线引擎（规则路由），也可以在设置里指向本机跑的 OpenAI 兼容服务（如 Ollama、llama.cpp，地址限 127.0.0.1）。所有执行细节——工具调用、子智能体动作——都会实时显示在界面上。".to_string()
        } else if lower.contains("帮助") || lower.contains("help") || lower.contains("能做什么") {
            "我能做（全部本地、离线）：\n· 找文件 / 搜内容 / 读文件 / 写文件\n· 运行 shell 命令（带审批）\n· 派子智能体（并行 / 后台），动作实时显示在右侧 Agent 树\n· 内置终端（标签页、VT 模拟）\n· 助手页：时间线、状态板、记忆、话题摘要\n· MCP 客户端（本机 stdio 服务）\n\n试试：「找文件 *.rs」「读 Cargo.toml」「运行命令 echo 你好」。".to_string()
        } else if lower.contains("谢谢") {
            "不客气。有什么本地任务随时交给我。".to_string()
        } else {
            // 回显 + 提示
            format!(
                "（内置离线引擎）我听到了：{}\n\n我不联网，无法回答需要网络的问题。但我可以驱动本地工具链——试试：「找文件 *.rs」「读 Cargo.toml」「运行命令 echo 你好」「派子智能体调查项目结构」。也可以在设置里接入本机的 OpenAI 兼容服务（127.0.0.1）获得真正的模型能力。",
                truncate(&last_user, 120)
            )
        };
        self.stream_text(&answer, sink);
        sink(EngineEvent::Done {
            usage: Some(AgentUsage {
                model: Some(self.model().to_string()),
                service: Some("offline".to_string()),
                ..Default::default()
            }),
        });
        Ok(())
    }
}

fn extract_quoted(s: &str) -> Option<String> {
    let mut chars = s.chars().peekable();
    let mut in_quote = false;
    let mut quote_char = ' ';
    let mut buf = String::new();
    let mut found = false;
    while let Some(c) = chars.next() {
        if !in_quote {
            if c == '"' || c == '\'' || c == '「' {
                in_quote = true;
                quote_char = c;
                found = true;
                buf.clear();
            }
        } else {
            if c == quote_char || (quote_char == '「' && c == '」') {
                return Some(buf.clone());
            }
            buf.push(c);
        }
    }
    if found && !buf.is_empty() {
        Some(buf)
    } else {
        None
    }
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect::<String>() + if s.chars().count() > n { "…" } else { "" }
}

// 让 lint 安静：offline 引擎不直接用的导入
#[allow(unused_imports)]
use ToolDescriptor as _ToolDescriptor;
#[allow(unused_imports)]
use anyhow as _anyhow;
