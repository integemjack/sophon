// 数据模型：与 docs/ARCHITECTURE.md §5.3 的 JSON 形状同构（字段名一致；
// 线格式用干净的 serde 表示，不做 Swift Codable 的 {"text":{"_0":…}} 关联值编码，
// 因为本程序是独立 Rust 实现，不需要与 Swift 版互读存档）。
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 大写带连字符的 UUID（docs §5.2：Json::uuidString）
pub fn new_uuid() -> String {
    Uuid::new_v4().hyphenated().to_string().to_uppercase()
}

/// ISO8601 UTC、无毫秒（docs §5.2：Json::isoDate）
pub fn now_iso() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

pub fn iso_of(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

use chrono::DateTime;

// ── 权限模式 / 思考档位 / 任务模式 ─────────────────────────────

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum PermissionMode {
    /// 自动执行（仍按 requireApproval 审批高危操作）
    #[default]
    Auto,
    /// 危险操作前询问
    Ask,
    /// 只读
    ReadOnly,
    /// 全部都要确认
    Manual,
}

impl PermissionMode {
    pub fn label(self) -> &'static str {
        match self {
            PermissionMode::Auto => "自动",
            PermissionMode::Ask => "询问",
            PermissionMode::ReadOnly => "只读",
            PermissionMode::Manual => "手动",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {
    Off,
    Low,
    #[default]
    Medium,
    High,
    Ultra,
}

impl ThinkingLevel {
    pub fn label(self) -> &'static str {
        match self {
            ThinkingLevel::Off => "关",
            ThinkingLevel::Low => "低",
            ThinkingLevel::Medium => "中",
            ThinkingLevel::High => "高",
            ThinkingLevel::Ultra => "极限",
        }
    }
    pub fn allows_nesting(self) -> bool {
        matches!(self, ThinkingLevel::Ultra)
    }
}

/// 任务模式（docs/ASSISTANT.md §3.4，2.9 起每段对话各自记）
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum TeamMode {
    /// 不派子 Agent
    #[default]
    Solo,
    /// 重活派子 Agent
    DelegatesHeavy,
    /// 只派子 Agent
    DelegatesOnly,
    /// 按需派
    DelegatesWhenNeeded,
    /// 极限性能
    Ultra,
}

impl TeamMode {
    pub fn label(self) -> &'static str {
        match self {
            TeamMode::Solo => "单干",
            TeamMode::DelegatesHeavy => "重活派发",
            TeamMode::DelegatesOnly => "只派发",
            TeamMode::DelegatesWhenNeeded => "按需派发",
            TeamMode::Ultra => "极限性能",
        }
    }
}

// ── 会话与消息 ────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    Tool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    /// 消息树（线性数组时按顺序补 parentID，见 §5.3）
    pub nodes: Vec<ChatMessage>,
    /// parentID -> 选中的子消息 id（fork 分支选择）
    #[serde(default)]
    pub selected_children: std::collections::HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    pub permission_mode: PermissionMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingLevel>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub team_mode: TeamMode,
    #[serde(default)]
    pub draft: String,
}

impl Conversation {
    pub fn new(working_directory: Option<String>) -> Self {
        let now = now_iso();
        Conversation {
            id: new_uuid(),
            title: "新对话".to_string(),
            created_at: now.clone(),
            updated_at: now,
            nodes: Vec::new(),
            selected_children: Default::default(),
            engine_id: None,
            working_directory,
            permission_mode: PermissionMode::Auto,
            thinking: None,
            pinned: false,
            team_mode: TeamMode::Solo,
            draft: String::new(),
        }
    }

    /// 沿 selected_children 从根走到当前可见的末尾消息
    pub fn visible_messages(&self) -> Vec<&ChatMessage> {
        let mut out = Vec::new();
        // 根：没有 parentID 的消息里最早的一条
        let mut current: Option<String> = None;
        let roots: Vec<&ChatMessage> = self
            .nodes
            .iter()
            .filter(|m| m.parent_id.is_none())
            .collect();
        if let Some(first) = roots.first() {
            current = Some(first.id.clone());
        }
        while let Some(id) = current {
            let msg = self.nodes.iter().find(|m| m.id == id);
            if let Some(m) = msg {
                out.push(m);
                // 选中的子分支，否则最后一个子
                let children: Vec<&ChatMessage> =
                    self.nodes.iter().filter(|c| c.parent_id.as_deref() == Some(m.id.as_str())).collect();
                current = if let Some(sel) = self.selected_children.get(&m.id) {
                    children.iter().find(|c| c.id == *sel).map(|c| c.id.clone())
                } else {
                    children.last().map(|c| c.id.clone())
                };
            } else {
                break;
            }
        }
        out
    }

    pub fn touch(&mut self) {
        self.updated_at = now_iso();
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatMessage {
    pub id: String,
    pub role: Role,
    pub blocks: Vec<MessageBlock>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<GenerationStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// 用户消息发出时的整份任务状态
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folded: Option<bool>,
    /// 助手消息被谁发的（助手代发时，如「助手 · 微信」；本地无渠道，留作扩展）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent_by: Option<String>,
    /// 这一轮是用户点停止结束的（docs §3.2 stoppedByUser）
    #[serde(default)]
    pub stopped_by_user: bool,
}

impl ChatMessage {
    pub fn user(text: impl Into<String>) -> Self {
        ChatMessage {
            id: new_uuid(),
            role: Role::User,
            blocks: vec![MessageBlock::text(text.into())],
            created_at: now_iso(),
            stats: None,
            error: None,
            notice: None,
            parent_id: None,
            task_note: None,
            folded: None,
            sent_by: None,
            stopped_by_user: false,
        }
    }
    pub fn assistant() -> Self {
        ChatMessage {
            id: new_uuid(),
            role: Role::Assistant,
            blocks: Vec::new(),
            created_at: now_iso(),
            stats: None,
            error: None,
            notice: None,
            parent_id: None,
            task_note: None,
            folded: None,
            sent_by: None,
            stopped_by_user: false,
        }
    }
    pub fn tool() -> Self {
        ChatMessage {
            id: new_uuid(),
            role: Role::Tool,
            blocks: Vec::new(),
            created_at: now_iso(),
            stats: None,
            error: None,
            notice: None,
            parent_id: None,
            task_note: None,
            folded: None,
            sent_by: None,
            stopped_by_user: false,
        }
    }
    pub fn text_content(&self) -> String {
        self.blocks
            .iter()
            .filter_map(|b| match &b.kind {
                BlockKind::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MessageBlock {
    pub id: String,
    #[serde(flatten)]
    pub kind: BlockKind,
}

impl MessageBlock {
    pub fn text(text: String) -> Self {
        MessageBlock { id: new_uuid(), kind: BlockKind::Text { text } }
    }
    pub fn thinking(text: String) -> Self {
        MessageBlock { id: new_uuid(), kind: BlockKind::Thinking { text } }
    }
    pub fn tool(tool: ToolEvent) -> Self {
        MessageBlock { id: new_uuid(), kind: BlockKind::Tool { tool } }
    }
    pub fn media(media: MediaAttachment) -> Self {
        MessageBlock { id: new_uuid(), kind: BlockKind::Media { media } }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum BlockKind {
    Text { text: String },
    Thinking { text: String },
    Tool { tool: ToolEvent },
    Media { media: MediaAttachment },
}

// ── 工具事件 / 媒体 / 统计 ────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolStatus {
    Running,
    Succeeded,
    Failed,
    Denied,
    AwaitingApproval,
    Stopped,
    Queued,
}

impl ToolStatus {
    pub fn label(self) -> &'static str {
        match self {
            ToolStatus::Running => "运行中",
            ToolStatus::Succeeded => "成功",
            ToolStatus::Failed => "失败",
            ToolStatus::Denied => "已拒绝",
            ToolStatus::AwaitingApproval => "待确认",
            ToolStatus::Stopped => "已停止",
            ToolStatus::Queued => "排队中",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ToolEvent {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    pub status: ToolStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    /// 子 Agent 的运行记录（嵌套块）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transcript: Vec<MessageBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(default)]
    pub queued: bool,
    /// 后台子 Agent 结束、result 改成结论之前，模型当时拿到的结果
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_result: Option<String>,
    #[serde(default)]
    pub usage: AgentUsage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
}

impl ToolEvent {
    pub fn new(name: &str, arguments: serde_json::Value, title: Option<String>) -> Self {
        ToolEvent {
            id: new_uuid(),
            name: name.to_string(),
            arguments,
            status: ToolStatus::Running,
            result: None,
            title,
            symbol: None,
            preview: None,
            transcript: Vec::new(),
            agent_name: None,
            queued: false,
            model_result: None,
            usage: AgentUsage::default(),
            started_at: Some(now_iso()),
            finished_at: None,
        }
    }
}

/// 一次工具调用的用量（对应 docs 的 AgentUsage）
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AgentUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub generated_tokens: u64,
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub generate_time_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MediaAttachment {
    pub id: String,
    pub kind: String,
    pub relative_path: String,
    pub file_name: String,
    pub mime_type: String,
    pub byte_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct GenerationStats {
    #[serde(default)]
    pub tokens_per_second: f64,
    #[serde(default)]
    pub token_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_token_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens_per_second: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_token_count: Option<u64>,
    #[serde(default)]
    pub live: bool,
    #[serde(default)]
    pub prefilling: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_name: Option<String>,
}

// ── 助手：简报 / 状态板 / 活动事件（docs/ASSISTANT.md §4、REMOTE.md §5）──

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum BriefStatus {
    #[default]
    Done,
    Stopped,
    Error,
    Question,
    Quota,
}

/// 一轮的简报（状态和数字由代码写，不经模型）
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct TurnBrief {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    pub status: BriefStatus,
    /// 「说了什么」那一句
    pub line: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    #[serde(default)]
    pub files_changed: u64,
    #[serde(default)]
    pub commands: u64,
    #[serde(default)]
    pub commands_failed: u64,
    #[serde(default)]
    pub sub_agents: u64,
    #[serde(default)]
    pub seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    #[serde(default)]
    pub steps_done: u64,
    #[serde(default)]
    pub steps_total: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    /// 最后那条回答去掉 Markdown 的正文（最多 1200 字，助手核对用）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PendingApproval {
    pub event_id: String,
    pub tool: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    #[default]
    Idle,
    Running,
    Approval,
    Question,
    Quota,
    Error,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct TaskStep {
    pub text: String,
    pub status: StepStatus,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum StepStatus {
    #[default]
    Todo,
    Doing,
    Done,
}

/// 某段会话的运行状态（服务端算好，客户端不重算业务规则）
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SessionActivity {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    pub status: SessionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<TaskState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_brief: Option<TurnBrief>,
    #[serde(default)]
    pub approvals: Vec<PendingApproval>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_by: Option<String>,
    pub updated_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct TaskState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    #[serde(default)]
    pub steps: Vec<TaskStep>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ActivityKind {
    Started,
    Step,
    Tool,
    Approval,
    Quota,
    Stalled,
    Finished,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ActivityEvent {
    pub conversation_id: String,
    pub kind: ActivityKind,
    pub seq: u64,
    pub at: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<ActivityTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approvals: Option<Vec<PendingApproval>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<TaskStep>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brief: Option<TurnBrief>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ActivityTool {
    pub event_id: String,
    pub tool: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

// ── 子 Agent 团队状态（REMOTE.md §5 TeamState）────────────────

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AgentActivity {
    Generating,
    Tool,
    WaitingChildren,
    WaitingApproval,
    Queued,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AgentRun {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub background: bool,
    #[serde(default)]
    pub depth: u32,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub detached: bool,
    #[serde(default)]
    pub stop_requested: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity: Option<AgentActivity>,
    #[serde(default)]
    pub steps: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline: Option<String>,
    #[serde(default)]
    pub speed: f64,
    #[serde(default)]
    pub pass_tokens: u64,
    #[serde(default)]
    pub queue: Vec<QueuedInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<AgentProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub usage: AgentUsage,
    #[serde(default)]
    pub log: Vec<AgentLogEntry>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AgentLogEntry {
    pub at: String,
    pub kind: AgentLogKind,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type")]
pub enum AgentLogKind {
    Tool { name: String, status: ToolStatus },
    Text { text: String },
    Note { text: String },
}

/// 子 Agent 的角色定义（delegate_task 的 profile）
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AgentProfile {
    pub name: String,
    pub role: String,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct QueuedInput {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub attachments: Vec<String>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}
