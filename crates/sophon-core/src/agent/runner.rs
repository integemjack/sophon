// 主 Agent 循环：回合线程模型、停止状态机、审批、插话。
// 对应 docs/port/core-loop.md 与 docs/ARCHITECTURE.md §3。
use crate::engine::{EngineEvent, StreamRequest};
use crate::models::*;
use crate::store::{AppCore, GeneratingState};
use crate::tools::{ToolContext, ToolOutput, ToolRegistry};
use crate::util::CancelToken;
use parking_lot::Mutex;
use std::sync::Arc;

/// 一段会话的回合运行器（每个会话一个；界面线程驱动）
pub struct AgentRunner {
    pub core: Arc<Mutex<AppCore>>,
}

impl AgentRunner {
    pub fn new(core: Arc<Mutex<AppCore>>) -> Self {
        AgentRunner { core }
    }

    /// 发送用户消息（界面线程调用；工作线程跑回合）
    pub fn send(&self, conversation_id: String, text: String) {
        let core = self.core.clone();
        // 界面线程：建用户消息 + 空助手消息
        let (_user_msg_id, assistant_msg_id, turn_engine_id, turn_thinking) = {
            let mut c = core.lock();
            // 本轮开始时的大脑与思考强度（不可变借用；用于「下一轮生效」指示）
            let turn_engine_id = c
                .store
                .get(&conversation_id)
                .and_then(|conv| c.resolved_engine(conv).map(|e| e.id));
            let turn_thinking = c.store.get(&conversation_id).and_then(|conv| conv.thinking);
            let conv = match c.store.get_mut(&conversation_id) {
                Some(v) => v,
                None => return,
            };
            // 父 = 当前可见的最后一条消息（多轮连续成链）
            let parent_id = conv.visible_messages().last().map(|m| m.id.clone());
            let mut user = ChatMessage::user(&text);
            user.parent_id = parent_id;
            let user_id = user.id.clone();
            conv.nodes.push(user);
            let mut assistant = ChatMessage::assistant();
            assistant.parent_id = Some(user_id.clone());
            let assistant_id = assistant.id.clone();
            conv.nodes.push(assistant);
            let _ = c.store.save();
            (user_id, assistant_id, turn_engine_id, turn_thinking)
        };
        // 初始化生成状态
        {
            let c = core.lock();
            c.generating.lock().insert(
                conversation_id.clone(),
                GeneratingState {
                    is_generating: true,
                    current_message_id: Some(assistant_msg_id.clone()),
                    started_at: Some(now_iso()),
                    turn_engine_id,
                    turn_thinking,
                    ..Default::default()
                },
            );
            c.update_runner_state();
        }
        core.lock().request_repaint();
        // 工作线程跑回合
        let cancel = self.conversation_token(&conversation_id);
        std::thread::spawn(move || {
            Self::run_turn(
                core.clone(),
                conversation_id.clone(),
                assistant_msg_id.clone(),
                text,
                cancel,
            );
        });
    }

    /// 停止生成（docs §3.2：is_stopping 只认第一次）
    pub fn stop(&self, conversation_id: &str) {
        let msg_id = {
            let c = self.core.lock();
            let mut gen = c.generating.lock();
            let mut mid = None;
            if let Some(g) = gen.get_mut(conversation_id) {
                if !g.is_stopping {
                    g.is_stopping = true;
                    g.stopped_by_user = true;
                    mid = g.current_message_id.clone();
                }
            }
            drop(gen);
            // 取消这段会话的令牌
            c.cancel_conversation(conversation_id);
            c.update_runner_state();
            c.request_repaint();
            mid
        };
        // 标记当前助手消息为「用户停止」（底栏显示「已停止」）
        if let Some(msg_id) = msg_id {
            let mut c = self.core.lock();
            c.store.update_message(conversation_id, &msg_id, |m| {
                m.stopped_by_user = true;
            });
        }
    }

    /// 插话（工作中补充；docs §3.4 interjection）
    pub fn interject(&self, conversation_id: &str, text: String) {
        let c = self.core.lock();
        let mut gen = c.generating.lock();
        if let Some(g) = gen.get_mut(conversation_id) {
            g.queue.push(QueuedInput {
                id: new_uuid(),
                text,
                attachments: Vec::new(),
                created_at: now_iso(),
                from: None,
            });
        }
        c.request_repaint();
    }

    /// 审批结果
    pub fn resolve_approval(&self, conversation_id: &str, event_id: &str, approved: bool) {
        let c = self.core.lock();
        c.approvals.resolve(event_id, approved);
        let mut gen = c.generating.lock();
        if let Some(g) = gen.get_mut(conversation_id) {
            g.pending_approval = false;
        }
        c.update_runner_state();
        c.request_repaint();
    }

    /// 重试失败的消息
    pub fn retry(&self, conversation_id: &str) {
        let (text, assistant_id) = {
            let mut c = self.core.lock();
            let conv = match c.store.get_mut(conversation_id) {
                Some(v) => v,
                None => return,
            };
            // 找最后一条用户消息和最后一条助手消息
            let last_user = conv
                .nodes
                .iter()
                .rev()
                .find(|m| matches!(m.role, Role::User))
                .map(|m| m.text_content());
            let last_assistant = conv
                .nodes
                .iter()
                .rev()
                .find(|m| matches!(m.role, Role::Assistant))
                .map(|m| m.id.clone());
            match (last_user, last_assistant) {
                (Some(text), Some(id)) => (text, id),
                _ => return,
            }
        };
        // 清空助手消息，重新生成
        {
            let mut c = self.core.lock();
            c.store.update_message(conversation_id, &assistant_id, |m| {
                m.blocks.clear();
            });
            let mut gen = c.generating.lock();
            gen.insert(
                conversation_id.to_string(),
                GeneratingState {
                    is_generating: true,
                    current_message_id: Some(assistant_id.clone()),
                    started_at: Some(now_iso()),
                    ..Default::default()
                },
            );
            c.update_runner_state();
        }
        let cancel = self.conversation_token(conversation_id);
        let core = self.core.clone();
        let cid = conversation_id.to_string();
        std::thread::spawn(move || {
            Self::run_turn(core, cid, assistant_id, text, cancel);
        });
    }

    fn conversation_token(&self, conversation_id: &str) -> CancelToken {
        let c = self.core.lock();
        c.conversation_token(conversation_id)
    }

    /// 回合主循环（工作线程）
    fn run_turn(
        core: Arc<Mutex<AppCore>>,
        conversation_id: String,
        assistant_msg_id: String,
        user_text: String,
        cancel: CancelToken,
    ) {
        // 读历史与配置（短暂加锁，克隆出来）
        let (history, system_prompt, engine, tools_descriptors, working_directory, permission_mode,
             require_approval, thinking, temperature, secrets, terminal_manager, team, task_states,
             main_thread, settings, lessons) = {
            let c = core.lock();
            let conv = match c.store.get(&conversation_id) {
                Some(v) => v.clone(),
                None => return,
            };
            let history: Vec<ChatMessage> = conv.visible_messages().into_iter().cloned().collect();
            let system_prompt = c.settings.system_prompt.clone();
            let engine = match c.resolved_engine(&conv) {
                Some(cfg) => match c.build_engine(&cfg) {
                    Ok(e) => e,
                    Err(_) => c.build_engine(&crate::engine::EngineConfig {
                        id: "offline".into(),
                        title: "内置离线引擎".into(),
                        base_url: String::new(),
                        api_key_secret: None,
                        model: "offline".into(),
                        supports_tools: true,
                        enabled: true,
                    }).unwrap(),
                },
                None => c.build_engine(&crate::engine::EngineConfig {
                    id: "offline".into(),
                    title: "内置离线引擎".into(),
                    base_url: String::new(),
                    api_key_secret: None,
                    model: "offline".into(),
                    supports_tools: true,
                    enabled: true,
                }).unwrap(),
            };
            let tools_descriptors = ToolRegistry::builtin().descriptors();
            let working_directory = conv.working_directory.clone().unwrap_or_else(|| {
                c.settings.working_directory.clone().unwrap_or_else(|| ".".to_string())
            });
            let permission_mode = conv.permission_mode;
            let require_approval = c.settings.require_approval;
            let thinking = c.settings.thinking_level;
            let temperature = 0.7;
            let secrets = c.secrets.clone();
            let terminal_manager = c.terminal_manager.clone();
            let team = c.team.clone();
            let task_states = c.task_states.clone();
            let main_thread = c.main_thread.clone();
            let settings = c.settings.clone();
            let lessons = c.lessons.clone();
            (history, system_prompt, engine, tools_descriptors, working_directory, permission_mode,
             require_approval, thinking, temperature, secrets, terminal_manager, team, task_states,
             main_thread, settings, lessons)
        };

        let mut messages = history;
        // P4 自我学习：新任务开始前检索经验库，相关教训拼进系统提示
        let system_prompt_full = {
            let guard = lessons.lock();
            let hits = guard.search(&user_text, 3);
            match crate::learning::lessons_prompt(&hits) {
                Some(extra) => format!("{}\n\n{}", system_prompt, extra),
                None => system_prompt.clone(),
            }
        };
        let registry = ToolRegistry::builtin();
        let mut usage = AgentUsage::default();
        let mut tool_events: Vec<(String, String, ToolStatus, Option<String>, Option<String>)> = Vec::new();
        let mut wrote_something = false;
        let mut delivery_rounds = 0;
        let max_rounds = 30;
        let mut round = 0;

        // 工具上下文
        let _ctx = ToolContext {
            working_directory: working_directory.clone(),
            conversation_id: conversation_id.clone(),
            terminal_manager: terminal_manager.clone(),
            team: team.clone(),
            task_states: task_states.clone(),
            cancel: cancel.clone(),
            main_thread: main_thread.clone(),
            depth: 0,
            engine: engine.clone(),
            secrets: secrets.clone(),
            system_prompt: system_prompt.clone(),
        };

        loop {
            round += 1;
            if round > max_rounds || cancel.is_cancelled() {
                break;
            }
            // 检查插话队列
            let queued = {
                let c = core.lock();
                let mut gen = c.generating.lock();
                if let Some(g) = gen.get_mut(&conversation_id) {
                    let q: Vec<String> = g.queue.drain(..).map(|q| q.text).collect();
                    g.queue_paused = false;
                    q
                } else {
                    Vec::new()
                }
            };
            for q in queued {
                messages.push(ChatMessage::user(&format!("（补充）{}", q)));
            }

            // 构造请求
            let req = StreamRequest {
                system: system_prompt_full.clone(),
                messages: messages.clone(),
                tools: tools_descriptors.clone(),
                thinking,
                temperature,
            };

            // 流式调用引擎
            let mut text = String::new();
            let mut thinking_text = String::new();
            let mut tool_calls: Vec<(String, String, serde_json::Value)> = Vec::new();
            let assistant_msg = assistant_msg_id.clone();
            let conv_id = conversation_id.clone();
            let core2 = core.clone();
            let sink = &mut |ev: EngineEvent| {
                let mut c = core2.lock();
                match ev {
                    EngineEvent::Chunk(chunk) => {
                        text.push_str(&chunk);
                        c.store.update_message(&conv_id, &assistant_msg, |m| {
                            // 更新最后一个文本块
                            if let Some(b) = m.blocks.iter_mut().rev().find(|b| matches!(b.kind, BlockKind::Text { .. })) {
                                if let BlockKind::Text { text: t } = &mut b.kind {
                                    *t = text.clone();
                                }
                            } else {
                                m.blocks.push(MessageBlock::text(text.clone()));
                            }
                        });
                    }
                    EngineEvent::Thinking(chunk) => {
                        thinking_text.push_str(&chunk);
                        c.store.update_message(&conv_id, &assistant_msg, |m| {
                            if let Some(b) = m.blocks.iter_mut().rev().find(|b| matches!(b.kind, BlockKind::Thinking { .. })) {
                                if let BlockKind::Thinking { text: t } = &mut b.kind {
                                    *t = thinking_text.clone();
                                }
                            } else {
                                m.blocks.push(MessageBlock::thinking(thinking_text.clone()));
                            }
                        });
                    }
                    EngineEvent::ToolCall { id, name, arguments } => {
                        tool_calls.push((id.clone(), name.clone(), arguments.clone()));
                        // 加一个运行中的工具块
                        let mut ev = ToolEvent::new(&name, arguments.clone(), Some(name.clone()));
                        ev.id = id.clone();
                        c.store.update_message(&conv_id, &assistant_msg, |m| {
                            m.blocks.push(MessageBlock::tool(ev));
                        });
                    }
                    EngineEvent::Done { usage: u } => {
                        if let Some(u) = u {
                            usage = u;
                        }
                    }
                }
                c.request_repaint();
            };
            if let Err(e) = engine.stream(&req, sink) {
                // 引擎错误：标记失败
                let mut c = core.lock();
                c.store.update_message(&conversation_id, &assistant_msg_id, |m| {
                    if m.blocks.is_empty() {
                        m.blocks.push(MessageBlock::text(format!("引擎错误：{}", e)));
                    }
                });
                let mut gen = c.generating.lock();
                if let Some(g) = gen.get_mut(&conversation_id) {
                    g.is_generating = false;
                    g.failed_message_id = Some(assistant_msg_id.clone());
                    g.can_retry = true;
                }
                c.update_runner_state();
                c.request_repaint();
                return;
            }

            // 更新文本块为最终文本
            {
                let mut c = core.lock();
                c.store.update_message(&conversation_id, &assistant_msg_id, |m| {
                    if let Some(b) = m.blocks.iter_mut().rev().find(|b| matches!(b.kind, BlockKind::Text { .. })) {
                        if let BlockKind::Text { text: t } = &mut b.kind {
                            *t = text.clone();
                        }
                    } else if !text.is_empty() {
                        m.blocks.push(MessageBlock::text(text.clone()));
                    }
                });
            }

            if tool_calls.is_empty() {
                // 交付自检（P3）：本回合改过代码且工作目录是 Rust 项目 → 交付前自动构建+测试
                // （在 cargo test 环境里跳过：递归构建会锁死 target 目录）
                if settings.delivery_check
                    && !in_cargo_test()
                    && wrote_something
                    && delivery_rounds < 2
                    && std::path::Path::new(&working_directory).join("Cargo.toml").exists()
                {
                    delivery_rounds += 1;
                    match crate::selfimprove::verify_only(&working_directory) {
                        Ok(_) => {
                            let mut c = core.lock();
                            c.store.update_message(&conversation_id, &assistant_msg_id, |m| {
                                m.blocks.push(MessageBlock::text("\n\n（交付自检：构建+测试全绿 ✓）".to_string()));
                            });
                            c.request_repaint();
                        }
                        Err(out) => {
                            messages.push(ChatMessage::user(&format!(
                                "（交付自检未通过，请先修复以下问题再交付）\n{}",
                                truncate_text(&out, 4000)
                            )));
                            continue;
                        }
                    }
                }
                // 没有工具调用 → 回合结束
                break;
            }

            // 执行工具调用
            for (call_id, name, arguments) in &tool_calls {
                if cancel.is_cancelled() {
                    break;
                }
                // 检查权限模式
                let tool = registry.get(name);
                let is_write = tool.map(|t| t.is_write()).unwrap_or(false);
                let needs_approval = tool.map(|t| t.needs_approval(arguments)).unwrap_or(false);

                // ReadOnly 模式：写操作直接拒绝
                if permission_mode == PermissionMode::ReadOnly && is_write {
                    let mut c = core.lock();
                    c.store.update_message(&conversation_id, &assistant_msg_id, |m| {
                        if let Some(b) = m.blocks.iter_mut().rev().find(|b| {
                            matches!(&b.kind, BlockKind::Tool { tool } if tool.id.as_str() == call_id.as_str())
                        }) {
                            if let BlockKind::Tool { tool } = &mut b.kind {
                                tool.status = ToolStatus::Denied;
                                tool.result = Some("ReadOnly 模式拒绝写操作".to_string());
                            }
                        }
                    });
                    c.request_repaint();
                    continue;
                }

                // ── P3 自主策略：autonomy 决策（SOPHON.md §4）──
                let autonomy_mode = settings.autonomy_mode;
                let mut effective_args = arguments.clone();
                if !effective_args.is_object() {
                    effective_args = serde_json::json!({});
                }
                let mut autonomy_note: Option<String> = None;
                let mut deny_reason: Option<String> = None;
                let mut confirm_reason: Option<String> = None;
                if name == "run_shell" {
                    let cmd = arguments.get("command").and_then(|v| v.as_str()).unwrap_or("");
                    match crate::autonomy::check_shell_command(cmd, autonomy_mode, &working_directory) {
                        crate::autonomy::Decision::Deny(r) => deny_reason = Some(r),
                        crate::autonomy::Decision::Transform(new_cmd, note) => {
                            effective_args["command"] = serde_json::Value::String(new_cmd);
                            autonomy_note = Some(note);
                        }
                        crate::autonomy::Decision::Confirm(r) => confirm_reason = Some(r),
                        crate::autonomy::Decision::Allow => {}
                    }
                } else {
                    match crate::autonomy::approve_tool(name, is_write, autonomy_mode) {
                        crate::autonomy::Decision::Confirm(r) => confirm_reason = Some(r),
                        crate::autonomy::Decision::Deny(r) => deny_reason = Some(r),
                        _ => {}
                    }
                }
                // 硬拒绝：直接拒绝，结果回给大脑
                if let Some(reason) = deny_reason {
                    let mut c = core.lock();
                    c.store.update_message(&conversation_id, &assistant_msg_id, |m| {
                        if let Some(b) = m.blocks.iter_mut().rev().find(|b| {
                            matches!(&b.kind, BlockKind::Tool { tool } if tool.id.as_str() == call_id.as_str())
                        }) {
                            if let BlockKind::Tool { tool } = &mut b.kind {
                                tool.status = ToolStatus::Denied;
                                tool.result = Some(reason.clone());
                            }
                        }
                    });
                    c.request_repaint();
                    let mut ev = ToolEvent::new(name, serde_json::Value::Null, Some(name.clone()));
                    ev.id = call_id.clone();
                    ev.status = ToolStatus::Denied;
                    ev.result = Some(reason.clone());
                    ev.finished_at = Some(now_iso());
                    let mut tool_msg = ChatMessage::tool();
                    tool_msg.blocks = vec![MessageBlock::tool(ev)];
                    messages.push(tool_msg);
                    tool_events.push((call_id.clone(), name.clone(), ToolStatus::Denied, Some(reason), None));
                    continue;
                }

                // 是否需要审批（自主模式覆盖默认策略）
                let ask = match permission_mode {
                    PermissionMode::Manual => true,
                    PermissionMode::Ask => needs_approval,
                    PermissionMode::Auto => match autonomy_mode {
                        crate::autonomy::AutonomyMode::Autonomous => confirm_reason.is_some(),
                        crate::autonomy::AutonomyMode::Safe => true,
                        crate::autonomy::AutonomyMode::Yolo => false,
                    },
                    PermissionMode::ReadOnly => false,
                };

                if ask {
                    // 待确认状态
                    {
                        let mut c = core.lock();
                        c.store.update_message(&conversation_id, &assistant_msg_id, |m| {
                            if let Some(b) = m.blocks.iter_mut().rev().find(|b| {
                                matches!(&b.kind, BlockKind::Tool { tool } if tool.id.as_str() == call_id.as_str())
                            }) {
                                if let BlockKind::Tool { tool } = &mut b.kind {
                                    tool.status = ToolStatus::AwaitingApproval;
                                }
                            }
                        });
                        let mut gen = c.generating.lock();
                        if let Some(g) = gen.get_mut(&conversation_id) {
                            g.pending_approval = true;
                        }
                        c.update_runner_state();
                    }
                    core.lock().request_repaint();
                    // 注册审批并等待
                    let rx = {
                        let c = core.lock();
                        c.approvals.register(call_id.clone())
                    };
                    match rx.recv() {
                        Ok(true) => {
                            // 允许
                            let mut c = core.lock();
                            c.store.update_message(&conversation_id, &assistant_msg_id, |m| {
                                if let Some(b) = m.blocks.iter_mut().rev().find(|b| {
                                    matches!(&b.kind, BlockKind::Tool { tool } if tool.id.as_str() == call_id.as_str())
                                }) {
                                    if let BlockKind::Tool { tool } = &mut b.kind {
                                        tool.status = ToolStatus::Running;
                                    }
                                }
                            });
                            let mut gen = c.generating.lock();
                            if let Some(g) = gen.get_mut(&conversation_id) {
                                g.pending_approval = false;
                            }
                            c.update_runner_state();
                            c.request_repaint();
                        }
                        _ => {
                            // 拒绝
                            let mut c = core.lock();
                            c.store.update_message(&conversation_id, &assistant_msg_id, |m| {
                                if let Some(b) = m.blocks.iter_mut().rev().find(|b| {
                                    matches!(&b.kind, BlockKind::Tool { tool } if tool.id.as_str() == call_id.as_str())
                                }) {
                                    if let BlockKind::Tool { tool } = &mut b.kind {
                                        tool.status = ToolStatus::Denied;
                                        tool.result = Some("用户拒绝".to_string());
                                    }
                                }
                            });
                            let mut gen = c.generating.lock();
                            if let Some(g) = gen.get_mut(&conversation_id) {
                                g.pending_approval = false;
                            }
                            c.update_runner_state();
                            c.request_repaint();
                            continue;
                        }
                    }
                }

                // 执行工具（不持全局锁）
                let result = {
                    let mut ctx = ToolContext {
                        working_directory: working_directory.clone(),
                        conversation_id: conversation_id.clone(),
                        terminal_manager: terminal_manager.clone(),
                        team: team.clone(),
                        task_states: task_states.clone(),
                        cancel: cancel.clone(),
                        main_thread: main_thread.clone(),
                        depth: 0,
                        engine: engine.clone(),
                        secrets: secrets.clone(),
                        system_prompt: system_prompt.clone(),
                    };
                    registry.execute(name, &effective_args, &mut ctx)
                };

                // 记录结果；失败时查错误模式库（P4：同类错误第 2 次直接给已知修法）
                let (mut status, mut result_text, mut error_text) = match result {
                    ToolOutput::Ok(r) => {
                        if is_write {
                            wrote_something = true;
                        }
                        (ToolStatus::Succeeded, Some(r), None)
                    }
                    ToolOutput::Err(e) => {
                        let known_fix = lessons.lock().record_error(&e, None);
                        let mut e2 = e;
                        if let Some(fix) = known_fix {
                            e2.push_str(&format!("\n（经验库已知修法：{}）", fix));
                        }
                        (ToolStatus::Failed, None, Some(e2))
                    }
                };
                // 可逆替代时在结果里标注
                if let Some(note) = &autonomy_note {
                    let r = format!(
                        "{}\n（自主策略：{}）",
                        result_text.clone().or(error_text.clone()).unwrap_or_default(),
                        note
                    );
                    if result_text.is_some() {
                        result_text = Some(r);
                    } else {
                        error_text = Some(r);
                    }
                }
                let mut c = core.lock();
                c.store.update_message(&conversation_id, &assistant_msg_id, |m| {
                    if let Some(b) = m.blocks.iter_mut().rev().find(|b| {
                        matches!(&b.kind, BlockKind::Tool { tool } if tool.id.as_str() == call_id.as_str())
                    }) {
                        if let BlockKind::Tool { tool } = &mut b.kind {
                            tool.status = status;
                            tool.result = result_text.clone().or(error_text.clone());
                        }
                    }
                });
                c.request_repaint();
                // 工具结果进历史
                let mut ev = ToolEvent::new(name, serde_json::Value::Null, Some(name.clone()));
                ev.id = call_id.clone();
                ev.status = status;
                ev.result = result_text.clone().or(error_text.clone());
                ev.finished_at = Some(now_iso());
                let mut tool_msg = ChatMessage::tool();
                tool_msg.blocks = vec![MessageBlock::tool(ev)];
                messages.push(tool_msg);
                tool_events.push((call_id.clone(), name.clone(), status, result_text, error_text));
            }
        }

        // 回合结束：写统计、状态
        {
            let mut c = core.lock();
            c.store.update_message(&conversation_id, &assistant_msg_id, |m| {
                m.stats = Some(GenerationStats {
                    token_count: usage.generated_tokens,
                    prompt_token_count: Some(usage.prompt_tokens),
                    tokens_per_second: if usage.generate_time_ms > 0 {
                        usage.generated_tokens as f64 / (usage.generate_time_ms as f64 / 1000.0)
                    } else {
                        0.0
                    },
                    engine_name: usage.model.clone(),
                    ..Default::default()
                });
            });
            let mut gen = c.generating.lock();
            if let Some(g) = gen.get_mut(&conversation_id) {
                g.is_generating = false;
                g.is_stopping = false;
                g.current_message_id = None;
            }
            drop(gen);
            c.update_runner_state();
            let _ = c.store.save();
        }
        core.lock().request_repaint();

        // P4：交付后总结写经验（成功与失败都写）
        {
            let failed = tool_events.iter().any(|(_, _, s, _, _)| *s == ToolStatus::Failed);
            let attempts: Vec<String> = tool_events
                .iter()
                .map(|(_, n, s, _, _)| format!("{}（{:?}）", n, s))
                .collect();
            lessons.lock().record(crate::learning::Lesson {
                ts: now_iso(),
                problem: user_text.clone(),
                attempts,
                result: if failed { "部分工具失败".into() } else { "成功".into() },
                lesson: if failed {
                    format!("任务「{}」执行中有工具失败，同类问题注意避开", truncate_text(&user_text, 60))
                } else {
                    format!("任务「{}」顺利完成", truncate_text(&user_text, 60))
                },
                tags: vec!["交付".into()],
            });
        }
    }
}

fn truncate_text(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max_chars).collect::<String>())
    }
}

/// 是否在 cargo test 环境里：测试二进制都放在 target/*/deps/ 下，
/// 真实二进制（target/debug/sophon-tui.exe）不在 deps 里。用这个区分，避免递归构建锁死 target。
fn in_cargo_test() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| {
            p.parent().map(|d| d.file_name() == Some(std::ffi::OsStr::new("deps")))
        })
        .unwrap_or(false)
}

// AppCore 的辅助方法（放在这里避免 store/mod.rs 循环引用）
impl AppCore {
    /// 每段会话一个取消令牌（父令牌；停止时取消）
    pub fn conversation_token(&self, conversation_id: &str) -> CancelToken {
        // 用一个全局 map 存会话令牌
        let mut map = self.conv_tokens.lock();
        map.entry(conversation_id.to_string())
            .or_insert_with(CancelToken::new)
            .child()
    }

    pub fn cancel_conversation(&self, conversation_id: &str) {
        if let Some(token) = self.conv_tokens.lock().get(conversation_id) {
            token.cancel();
        }
    }

    /// 更新全局运行状态（侧边栏显示）
    pub fn update_runner_state(&self) {
        let gen = self.generating.lock();
        let team = self.team.lock();
        let mut rs = self.runner_state.lock();
        rs.generating = gen
            .iter()
            .filter(|(_, g)| g.is_generating)
            .map(|(id, _)| id.clone())
            .collect();
        rs.stopping = gen
            .iter()
            .filter(|(_, g)| g.is_stopping)
            .map(|(id, _)| id.clone())
            .collect();
        rs.pending_approval = gen
            .iter()
            .filter(|(_, g)| g.pending_approval)
            .map(|(id, _)| id.clone())
            .collect();
        rs.background_agents = team
            .background_running()
            .iter()
            .map(|r| r.id.clone())
            .collect();
        rs.live_agents = team.running_count() as u64;
    }
}
