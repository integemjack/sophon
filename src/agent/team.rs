// 子 Agent 系统：团队状态 + 子智能体运行器。
// 对应 docs/port/subagents.md 与 docs/ARCHITECTURE.md §4。
use crate::engine::{ChatEngine, EngineEvent, StreamRequest};
use crate::models::*;
use crate::tools::{ToolContext, ToolOutput, ToolRegistry};
use crate::util::CancelToken;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// 子 Agent 角色（对应 docs 的 AgentProfile）
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AgentProfile {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub role: String,
    /// 类型（prompts::SubagentKind::as_str；空 = 旧式自由角色，用 role 拼）
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
}

impl AgentProfile {
    /// 类型（kind 解析不了时给默认 General）
    pub fn subagent_kind(&self) -> crate::prompts::SubagentKind {
        crate::prompts::SubagentKind::parse_or_default(&self.kind)
    }
}

/// 运行状态（REMOTE.md §5 AgentRun.status）
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentStatus {
    Running,
    WaitingApproval,
    Queued,
    Stopping,
    Stopped,
    Done,
    Failed,
}

impl AgentStatus {
    pub fn label(self) -> &'static str {
        match self {
            AgentStatus::Running => "运行中",
            AgentStatus::WaitingApproval => "待确认",
            AgentStatus::Queued => "排队",
            AgentStatus::Stopping => "停止中",
            AgentStatus::Stopped => "已停止",
            AgentStatus::Done => "完成",
            AgentStatus::Failed => "失败",
        }
    }
}

/// 记录条目（docs/ARCHITECTURE.md §4 AgentLogEntry）
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AgentLogEntry {
    pub at: String,
    pub kind: AgentLogKind,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum AgentLogKind {
    Tool { name: String, status: ToolStatus, detail: String },
    Text { text: String },
    Note { text: String },
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolStatus {
    Running,
    Success,
    Failed,
    AwaitingApproval,
    Approved,
    Denied,
    Stopped,
}

impl ToolStatus {
    pub fn label(self) -> &'static str {
        match self {
            ToolStatus::Running => "运行中",
            ToolStatus::Success => "成功",
            ToolStatus::Failed => "失败",
            ToolStatus::AwaitingApproval => "待确认",
            ToolStatus::Approved => "已允许",
            ToolStatus::Denied => "已拒绝",
            ToolStatus::Stopped => "已停止",
        }
    }
    pub fn color(self) -> &'static str {
        match self {
            ToolStatus::Running => "blue",
            ToolStatus::Success => "green",
            ToolStatus::Failed => "red",
            ToolStatus::AwaitingApproval => "orange",
            ToolStatus::Approved => "green",
            ToolStatus::Denied => "red",
            ToolStatus::Stopped => "gray",
        }
    }
}

/// 一次子 Agent 运行（REMOTE.md §5 AgentRun）
#[derive(Serialize, Clone, Debug)]
pub struct AgentRun {
    pub id: String,
    pub name: String,
    pub conversation_id: String,
    pub depth: u32,
    pub background: bool,
    pub status: AgentStatus,
    pub profile: Option<AgentProfile>,
    pub task: String,
    pub result: Option<String>,
    pub log: Vec<AgentLogEntry>,
    pub inbox: VecDeque<String>,
    #[serde(skip)]
    pub cancel: Option<CancelToken>,
    pub started_at: String,
}

/// 团队状态（docs/ARCHITECTURE.md §4 AgentTeamState）
#[derive(Serialize, Clone, Default)]
pub struct AgentTeamState {
    pub runs: Vec<AgentRun>,
    /// 父 → 子 关系（用于 Agent 树）
    pub children: HashMap<String, Vec<String>>,
    pub parent: HashMap<String, String>,
}

impl AgentTeamState {
    pub fn new() -> Self {
        AgentTeamState::default()
    }

    pub fn start_run(
        &mut self,
        id: String,
        name: String,
        conversation_id: String,
        depth: u32,
        background: bool,
        profile: AgentProfile,
        task: String,
        cancel: CancelToken,
    ) {
        let run = AgentRun {
            id: id.clone(),
            name,
            conversation_id,
            depth,
            background,
            status: AgentStatus::Running,
            profile: Some(profile),
            task,
            result: None,
            log: Vec::new(),
            inbox: VecDeque::new(),
            cancel: Some(cancel),
            started_at: crate::models::now_iso(),
        };
        self.runs.push(run);
    }

    pub fn get_run(&self, id: &str) -> Option<&AgentRun> {
        self.runs.iter().find(|r| r.id == id)
    }
    pub fn get_run_mut(&mut self, id: &str) -> Option<&mut AgentRun> {
        self.runs.iter_mut().find(|r| r.id == id)
    }

    pub fn finish_run(&mut self, id: &str, result: String) {
        if let Some(run) = self.get_run_mut(id) {
            run.status = AgentStatus::Done;
            run.result = Some(result);
        }
    }

    pub fn fail_run(&mut self, id: &str, error: String) {
        if let Some(run) = self.get_run_mut(id) {
            run.status = AgentStatus::Failed;
            run.result = Some(error);
        }
    }

    pub fn stop_agent(&mut self, id: &str) -> bool {
        if let Some(run) = self.get_run_mut(id) {
            if let Some(cancel) = &run.cancel {
                cancel.cancel();
            }
            run.status = AgentStatus::Stopped;
            return true;
        }
        false
    }

    pub fn send_to_agent(&mut self, id: &str, message: String) -> bool {
        if let Some(run) = self.get_run_mut(id) {
            run.inbox.push_back(message);
            return true;
        }
        false
    }

    pub fn drain_inbox(&mut self, id: &str) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(run) = self.get_run_mut(id) {
            while let Some(m) = run.inbox.pop_front() {
                out.push(m);
            }
        }
        out
    }

    pub fn log(&mut self, id: &str, entry: AgentLogEntry) {
        if let Some(run) = self.get_run_mut(id) {
            run.log.push(entry);
        }
    }

    pub fn running_count(&self) -> usize {
        self.runs
            .iter()
            .filter(|r| matches!(r.status, AgentStatus::Running | AgentStatus::WaitingApproval | AgentStatus::Queued))
            .count()
    }

    /// 后台运行中的（侧边栏显示）
    pub fn background_running(&self) -> Vec<&AgentRun> {
        self.runs
            .iter()
            .filter(|r| r.background && matches!(r.status, AgentStatus::Running | AgentStatus::WaitingApproval))
            .collect()
    }

    /// 全部运行（含历史，按时间倒序）
    pub fn all_runs(&self) -> Vec<&AgentRun> {
        let mut v: Vec<&AgentRun> = self.runs.iter().collect();
        v.reverse();
        v
    }
}

/// 子智能体运行器：跑一个完整的回合循环（引擎 + 工具），把过程记进日志。
/// 返回最终文本结果。
pub fn run_subagent(
    run_id: &str,
    profile: &AgentProfile,
    task: &str,
    engine: &Arc<dyn ChatEngine>,
    ctx: &mut ToolContext,
) -> String {
    // 系统提示词：有类型 → 类型化提示词（借鉴 Claude Code 内置 subagents：
    // 研究型只读、不带主提示词，快且省）；无类型 → 旧式主提示词 + role 拼。
    let has_kind = crate::prompts::SubagentKind::parse(&profile.kind).is_some();
    let kind = profile.subagent_kind();
    let system = if has_kind {
        crate::prompts::subagent_system(kind, &ctx.working_directory)
    } else {
        format!(
            "{}\n\n你是子智能体「{}」。你的角色：{}\n\n\
             你要完成用户交给你的任务。用工具完成，每步如实汇报。\
             完成后把最终结果作为最后一段文字返回（不要只说「完成了」）。",
            ctx.system_prompt, profile.name, profile.role
        )
    };
    let readonly = has_kind && kind.readonly();
    let mut messages = vec![ChatMessage::user(task)];
    let mut final_text = String::new();
    let max_rounds = 25;
    let mut round = 0;
    let registry = ToolRegistry::builtin();
    let mut last_error: Option<String> = None;

    loop {
        round += 1;
        if round > max_rounds || ctx.cancel.is_cancelled() {
            if ctx.cancel.is_cancelled() {
                log_note(run_id, ctx, "已停止");
            }
            break;
        }
        // 检查收件箱（父智能体发来的消息）
        // （团队状态在 ctx.team 里；这里直接读）
        let inbox = {
            let mut t = ctx.team.lock();
            t.drain_inbox(run_id)
        };
        for m in inbox {
            messages.push(ChatMessage::user(&format!("（来自父智能体的消息）{}", m)));
        }

        // 构造请求
        let tools: Vec<_> = if profile.tools.is_empty() {
            registry.descriptors()
        } else {
            registry
                .descriptors()
                .into_iter()
                .filter(|d| profile.tools.contains(&d.name))
                .collect()
        };
        let req = StreamRequest {
            system: system.clone(),
            messages: messages.clone(),
            tools,
            thinking: None,
            temperature: 0.7,
        };

        // 流式调用引擎，收集事件
        let mut text = String::new();
        let mut tool_calls: Vec<(String, String, serde_json::Value)> = Vec::new();
        let sink = &mut |ev: EngineEvent| match ev {
            EngineEvent::Chunk(c) => {
                text.push_str(&c);
            }
            EngineEvent::Thinking(_) => {}
            EngineEvent::ToolCall { id, name, arguments } => {
                tool_calls.push((id, name, arguments));
            }
            EngineEvent::Done { .. } => {}
        };
        if let Err(e) = engine.stream(&req, sink) {
            last_error = Some(format!("引擎错误：{}", e));
            break;
        }

        // 记录文本
        if !text.is_empty() {
            log_text(run_id, ctx, &text);
            final_text = text.clone();
            let mut assistant = ChatMessage::assistant();
            assistant.blocks = vec![MessageBlock::text(text.clone())];
            messages.push(assistant);
        }

        if tool_calls.is_empty() {
            // 没有工具调用 → 任务完成
            break;
        }

        // 执行工具调用
        for (call_id, name, arguments) in &tool_calls {
            log_tool(run_id, ctx, name, ToolStatus::Running, "");
            // 只读闸（explore/plan/review）：exec 命令必须过只读白名单
            let output = if readonly && name == "exec" {
                let cmd = arguments.get("command").and_then(|v| v.as_str()).unwrap_or("");
                if crate::autonomy::is_readonly_command(cmd) {
                    registry.execute(name, arguments, ctx)
                } else {
                    ToolOutput::Err(format!(
                        "只读智能体（{}）：这条命令不在只读白名单，已被拒绝。只能读：Get-Content / Get-ChildItem / Select-String / git status|log|show|diff 等。",
                        kind.label()
                    ))
                }
            } else {
                registry.execute(name, arguments, ctx)
            };
            match &output {
                ToolOutput::Ok(result) => {
                    log_tool(run_id, ctx, name, ToolStatus::Success, &truncate(result, 200));
                    let mut ev = ToolEvent::new(name, serde_json::Value::Null, Some(name.clone()));
                    ev.id = call_id.clone();
                    ev.status = crate::models::ToolStatus::Succeeded;
                    ev.result = Some(result.clone());
                    ev.finished_at = Some(now_iso());
                    let mut tool_msg = ChatMessage::tool();
                    tool_msg.blocks = vec![MessageBlock::tool(ev)];
                    messages.push(tool_msg);
                }
                ToolOutput::Err(err) => {
                    log_tool(run_id, ctx, name, ToolStatus::Failed, &truncate(err, 200));
                    let mut ev = ToolEvent::new(name, serde_json::Value::Null, Some(name.clone()));
                    ev.id = call_id.clone();
                    ev.status = crate::models::ToolStatus::Failed;
                    ev.result = Some(err.clone());
                    ev.finished_at = Some(now_iso());
                    let mut tool_msg = ChatMessage::tool();
                    tool_msg.blocks = vec![MessageBlock::tool(ev)];
                    messages.push(tool_msg);
                }
            }
        }
    }

    if final_text.is_empty() {
        if let Some(e) = last_error {
            format!("子智能体「{}」未能完成：{}", profile.name, e)
        } else {
            format!("子智能体「{}」完成了任务（无文字输出）", profile.name)
        }
    } else {
        final_text
    }
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect::<String>() + if s.chars().count() > n { "…" } else { "" }
}

fn log_tool(run_id: &str, ctx: &mut ToolContext, name: &str, status: ToolStatus, detail: &str) {
    let entry = AgentLogEntry {
        at: crate::models::now_iso(),
        kind: AgentLogKind::Tool {
            name: name.to_string(),
            status,
            detail: detail.to_string(),
        },
    };
    let mut t = ctx.team.lock();
    t.log(run_id, entry);
}

fn log_text(run_id: &str, ctx: &mut ToolContext, text: &str) {
    let entry = AgentLogEntry {
        at: crate::models::now_iso(),
        kind: AgentLogKind::Text { text: text.to_string() },
    };
    let mut t = ctx.team.lock();
    t.log(run_id, entry);
}

fn log_note(run_id: &str, ctx: &mut ToolContext, text: &str) {
    let entry = AgentLogEntry {
        at: crate::models::now_iso(),
        kind: AgentLogKind::Note { text: text.to_string() },
    };
    let mut t = ctx.team.lock();
    t.log(run_id, entry);
}
