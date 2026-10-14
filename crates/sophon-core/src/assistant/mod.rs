// 助手运行时：时间线、状态板、上下文、记忆、话题摘要。
// 对应 docs/ASSISTANT.md 与 docs/ASSISTANT_SPEC.md。
// 本实现是本地版：时间线记录本机所有会话的动态，状态板汇总运行状态，
// 记忆是本地文件，话题摘要用本地规则生成（不联网）。
use crate::models::*;
use crate::store::AppCore;
use crate::util::Paths;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// 时间线条目（REMOTE.md §5 TimelineEvent）
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TimelineEvent {
    pub id: String,
    pub kind: TimelineKind,
    pub at: String,
    pub conversation_id: Option<String>,
    pub agent_id: Option<String>,
    pub agent_name: Option<String>,
    pub title: String,
    pub detail: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum TimelineKind {
    UserMessage,
    AgentMessage,
    ToolCall,
    AgentRun,
    Approval,
    System,
}

/// 状态板条目（REMOTE.md §5 BoardState）
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BoardItem {
    pub id: String,
    pub kind: BoardKind,
    pub title: String,
    pub detail: Option<String>,
    pub at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum BoardKind {
    Generating,
    Stopping,
    PendingApproval,
    BackgroundAgent,
    Idle,
}

/// 记忆条目
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MemoryEntry {
    pub id: String,
    pub at: String,
    pub content: String,
    pub source: String,
}

/// 话题摘要
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TopicSummary {
    pub conversation_id: String,
    pub title: String,
    pub summary: String,
    pub updated_at: String,
}

/// 助手运行时
pub struct AssistantRuntime {
    pub timeline: Mutex<Vec<TimelineEvent>>,
    pub memories: Mutex<Vec<MemoryEntry>>,
    pub summaries: Mutex<HashMap<String, TopicSummary>>,
    pub name: String,
    timeline_file: std::path::PathBuf,
    memory_file: std::path::PathBuf,
}

impl AssistantRuntime {
    pub fn load(paths: &Paths) -> anyhow::Result<Self> {
        let timeline: Vec<TimelineEvent> = crate::util::read_json_file(&paths.assistant_timeline_file())
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        let memories: Vec<MemoryEntry> = crate::util::read_json_file(&paths.assistant_memory_file())
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        Ok(AssistantRuntime {
            timeline: Mutex::new(timeline),
            memories: Mutex::new(memories),
            summaries: Mutex::new(HashMap::new()),
            name: "Sophon".to_string(),
            timeline_file: paths.assistant_timeline_file(),
            memory_file: paths.assistant_memory_file(),
        })
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let timeline = self.timeline.lock().clone();
        crate::util::write_json_file(&self.timeline_file, &serde_json::to_value(&timeline)?)?;
        let memories = self.memories.lock().clone();
        crate::util::write_json_file(&self.memory_file, &serde_json::to_value(&memories)?)?;
        Ok(())
    }

    /// 记录时间线事件
    pub fn log(&self, kind: TimelineKind, conversation_id: Option<String>, agent_id: Option<String>, agent_name: Option<String>, title: String, detail: Option<String>) {
        let event = TimelineEvent {
            id: new_uuid(),
            kind,
            at: now_iso(),
            conversation_id,
            agent_id,
            agent_name,
            title,
            detail,
        };
        {
            let mut tl = self.timeline.lock();
            tl.push(event);
            let len = tl.len();
            if len > 1000 {
                tl.drain(0..len - 1000);
            }
        }
        let _ = self.save();
    }

    pub fn timeline(&self) -> Vec<TimelineEvent> {
        let tl = self.timeline.lock();
        let mut v: Vec<TimelineEvent> = tl.iter().rev().cloned().collect();
        v.truncate(200);
        v
    }

    /// 状态板：从核心状态汇总
    pub fn board(&self, core: &AppCore) -> Vec<BoardItem> {
        let mut items = Vec::new();
        let gen = core.generating.lock();
        for (id, g) in gen.iter() {
            if g.is_generating {
                items.push(BoardItem {
                    id: id.clone(),
                    kind: BoardKind::Generating,
                    title: format!("正在生成：{}", id),
                    detail: g.current_message_id.clone(),
                    at: g.started_at.clone().unwrap_or_default(),
                });
            }
            if g.pending_approval {
                items.push(BoardItem {
                    id: id.clone(),
                    kind: BoardKind::PendingApproval,
                    title: format!("待确认：{}", id),
                    detail: None,
                    at: String::new(),
                });
            }
        }
        let team = core.team.lock();
        for run in team.background_running() {
            items.push(BoardItem {
                id: run.id.clone(),
                kind: BoardKind::BackgroundAgent,
                title: format!("后台子智能体：{}", run.name),
                detail: Some(run.task.clone()),
                at: run.started_at.clone(),
            });
        }
        if items.is_empty() {
            items.push(BoardItem {
                id: "idle".to_string(),
                kind: BoardKind::Idle,
                title: "空闲".to_string(),
                detail: None,
                at: String::new(),
            });
        }
        items
    }

    /// 记忆：加一条
    pub fn add_memory(&self, content: String, source: String) {
        let entry = MemoryEntry {
            id: new_uuid(),
            at: now_iso(),
            content,
            source,
        };
        {
            let mut m = self.memories.lock();
            m.push(entry);
            let len = m.len();
            if len > 500 {
                m.drain(0..len - 500);
            }
        }
        let _ = self.save();
    }

    pub fn memories(&self) -> Vec<MemoryEntry> {
        let m = self.memories.lock();
        let mut v: Vec<MemoryEntry> = m.iter().rev().cloned().collect();
        v.truncate(100);
        v
    }

    /// 话题摘要：用本地规则从会话内容生成（取前几条消息的关键句）
    pub fn summarize(&self, conversation: &Conversation) -> TopicSummary {
        let mut sentences = Vec::new();
        for m in &conversation.nodes {
            let text = m.text_content();
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.len() < 4 {
                    continue;
                }
                // 取陈述句（以句号结尾）
                if line.ends_with('。') || line.ends_with('.') {
                    sentences.push(line.to_string());
                }
                if sentences.len() >= 5 {
                    break;
                }
            }
            if sentences.len() >= 5 {
                break;
            }
        }
        let summary = if sentences.is_empty() {
            "（暂无内容）".to_string()
        } else {
            sentences.join(" ")
        };
        TopicSummary {
            conversation_id: conversation.id.clone(),
            title: conversation.title.clone(),
            summary,
            updated_at: now_iso(),
        }
    }

    /// 助手输入：处理用户在助手页输入的一句话
    pub fn handle_input(&self, core: &Arc<Mutex<AppCore>>, text: String) -> String {
        let lower = text.to_lowercase();
        // 查记忆
        if lower.contains("记忆") || lower.contains("记住") {
            let memories = self.memories();
            if memories.is_empty() {
                return "记忆是空的。".to_string();
            }
            return memories
                .iter()
                .map(|m| format!("· {}（{}）", m.content, m.source))
                .collect::<Vec<_>>()
                .join("\n");
        }
        // 状态
        if lower.contains("状态") || lower.contains("在跑") || lower.contains("进度") {
            let c = core.lock();
            let board = self.board(&c);
            return board
                .iter()
                .map(|b| format!("· [{}] {}", match b.kind {
                    BoardKind::Generating => "生成中",
                    BoardKind::Stopping => "停止中",
                    BoardKind::PendingApproval => "待确认",
                    BoardKind::BackgroundAgent => "后台",
                    BoardKind::Idle => "空闲",
                }, b.title))
                .collect::<Vec<_>>()
                .join("\n");
        }
        // 时间
        if lower.contains("时间") || lower.contains("几点") {
            use chrono::Local;
            return format!("现在是 {}", Local::now().format("%Y-%m-%d %H:%M:%S"));
        }
        // 默认：把这句话发到当前/新建会话
        let conv_id = {
            let mut c = core.lock();
            // 取最后一段会话
            let last = c.store.conversations.last().map(|v| v.id.clone());
            match last {
                Some(id) => id,
                None => {
                    let wd = c.settings.working_directory.clone();
                    c.store.create(wd)
                }
            }
        };
        let runner = crate::agent::runner::AgentRunner::new(core.clone());
        runner.send(conv_id.clone(), text.clone());
        format!("已把这句话发到会话，并开始处理。")
    }
}
