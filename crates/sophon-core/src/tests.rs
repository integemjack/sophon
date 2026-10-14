// 核心功能单元测试（逐项自测）。
// 运行：cargo test -p sophon-core
#![allow(dead_code)]

use crate::agent::team::{AgentLogEntry, AgentLogKind, AgentProfile, AgentStatus, AgentTeamState};
use crate::assistant::{AssistantRuntime, TimelineKind};
use crate::engine::{ChatEngine, EngineConfig, EngineEvent, OfflineEngine, OpenAICompatEngine, StreamRequest, ToolDescriptor};
use crate::models::*;
use crate::store::{ConversationStore, SecretStore};
use crate::terminal::{TerminalEmulator, TerminalManager};
use crate::tools::{glob_match, ToolContext, ToolOutput, ToolRegistry};
use crate::util::{main_thread, CancelToken, Paths};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

// ── 工具：glob 匹配 ──────────────────────────────

#[test]
fn test_glob_match() {
    assert!(glob_match("*.rs", "main.rs"));
    assert!(glob_match("*.rs", "src/lib.rs"));
    assert!(glob_match("Cargo.*", "Cargo.toml"));
    assert!(glob_match("*", "anything"));
    assert!(glob_match("*.{rs,toml}", "a.rs"));
    assert!(!glob_match("*.rs", "main.toml"));
    assert!(!glob_match("main.rs", "lib.rs"));
}

// ── util::json：UUID 大写、ISO8601 ───────────────

#[test]
fn test_json_helpers() {
    let u = crate::util::json::uuid();
    assert_eq!(u.len(), 36);
    assert!(u.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-'), "UUID 应大写: {}", u);
    let t = crate::util::json::now();
    // ISO8601 UTC 无毫秒：2026-10-14T13:36:40Z
    assert!(t.ends_with('Z'), "ISO 时间应以 Z 结尾: {}", t);
    assert_eq!(t.len(), 20, "ISO 时间应无毫秒: {}", t);
}

// ── 取消令牌：层级取消 ───────────────────────────

#[test]
fn test_cancel_token_hierarchy() {
    let parent = CancelToken::new();
    let child = parent.child();
    let grand = child.child();
    assert!(!parent.is_cancelled());
    assert!(!child.is_cancelled());
    // 父取消 → 子、孙一起取消
    parent.cancel();
    assert!(parent.is_cancelled());
    assert!(child.is_cancelled());
    assert!(grand.is_cancelled());
    // 子取消不影响父
    let p2 = CancelToken::new();
    let c2 = p2.child();
    c2.cancel();
    assert!(c2.is_cancelled());
    assert!(!p2.is_cancelled(), "子取消不应影响父");
}

// ── 模型：会话 / 消息 / 工具事件 ─────────────────

#[test]
fn test_models() {
    let mut conv = Conversation::new(Some("C:\\work".to_string()));
    assert_eq!(conv.title, "新对话");
    // 线性消息树：m2 的父是 m1
    let m1 = ChatMessage::user("你好".to_string());
    let mut m2 = ChatMessage::assistant();
    m2.parent_id = Some(m1.id.clone());
    conv.nodes.push(m1.clone());
    conv.nodes.push(m2.clone());
    let vis = conv.visible_messages();
    assert_eq!(vis.len(), 2);
    // 文本内容
    assert_eq!(m1.text_content(), "你好");
    // 工具事件
    let ev = ToolEvent::new("run_shell", json!({"command": "ls"}), Some("运行命令".to_string()));
    assert_eq!(ev.name, "run_shell");
    assert_eq!(ev.status, ToolStatus::Running);
    assert_eq!(ev.arguments["command"], "ls");
}

// fork 与停止标记
#[test]
fn test_fork_and_stopped_flag() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("conversations.json");
    let mut store = ConversationStore::load(&file);
    let cid = store.create(None);
    // 用户消息 + 助手消息（成链）
    let m1 = ChatMessage::user("问题".to_string());
    let mid1 = m1.id.clone();
    store.add_message(&cid, m1);
    let mut m2 = ChatMessage::assistant();
    m2.parent_id = Some(mid1.clone());
    let mid2 = m2.id.clone();
    store.add_message(&cid, m2);
    // fork：从助手消息 fork 出新分支
    let new_id = store.fork(&cid, &mid2).expect("fork 应成功");
    let conv = store.get(&cid).unwrap();
    let new_msg = conv.nodes.iter().find(|m| m.id == new_id).unwrap();
    assert_eq!(new_msg.role, Role::Assistant);
    assert_eq!(new_msg.parent_id.as_deref(), Some(mid2.as_str()));
    assert!(!new_msg.stopped_by_user);
    // 选中分支：mid2 的选中子是 new_id
    assert_eq!(conv.selected_children.get(&mid2).map(|s| s.as_str()), Some(new_id.as_str()));
}

// ── 存储：会话库 ─────────────────────────────────

#[test]
fn test_conversation_store() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("conversations.json");
    let mut store = ConversationStore::load(&file);
    assert!(store.conversations.is_empty());
    // 新建
    let id = store.create(Some("C:\\work".to_string()));
    assert_eq!(store.conversations.len(), 1);
    // 加消息
    let msg = ChatMessage::user("测试消息".to_string());
    let msg_id = store.add_message(&id, msg);
    // 更新消息
    store.update_message(&id, &msg_id, |m| {
        m.blocks.push(MessageBlock::text("补充".to_string()));
    });
    // 改名 / 置顶
    store.rename(&id, "重命名".to_string());
    store.set_pinned(&id, true);
    assert_eq!(store.conversations[0].title, "重命名");
    assert!(store.conversations[0].pinned);
    // 搜索
    let hits = store.search("测试");
    assert!(!hits.is_empty());
    // 删除
    assert!(store.remove(&id));
    assert!(store.conversations.is_empty());
    // 持久化
    store.save().unwrap();
    assert!(file.exists());
}

// ── 引擎：离线路由 ───────────────────────────────

fn offline_req(text: &str, tools: Vec<ToolDescriptor>) -> StreamRequest {
    StreamRequest {
        system: "test".to_string(),
        messages: vec![ChatMessage::user(text.to_string())],
        tools,
        thinking: None,
        temperature: 0.0,
    }
}

fn tool_desc(name: &str) -> ToolDescriptor {
    ToolDescriptor {
        name: name.to_string(),
        title: name.to_string(),
        description: format!("{} 工具", name),
        parameters: Value::Null,
    }
}

#[test]
fn test_offline_engine_routes_tools() {
    let engine = OfflineEngine::new();
    let tools = vec![
        tool_desc("run_shell"),
        tool_desc("find_files"),
        tool_desc("read_file"),
        tool_desc("write_file"),
        tool_desc("search_files"),
        tool_desc("delegate_task"),
    ];
    // 找文件
    let mut events = Vec::new();
    engine.stream(&offline_req("找文件 \"*.rs\"", tools.clone()), &mut |e| events.push(e)).unwrap();
    match &events[0] {
        EngineEvent::ToolCall { name, arguments, .. } => {
            assert_eq!(name, "find_files");
            assert_eq!(arguments["pattern"], "*.rs");
        }
        _ => panic!("应路由到 find_files, 实际: {:?}", events[0]),
    }
    // 读文件
    let mut events = Vec::new();
    engine.stream(&offline_req("读文件 \"Cargo.toml\"", tools.clone()), &mut |e| events.push(e)).unwrap();
    match &events[0] {
        EngineEvent::ToolCall { name, arguments, .. } => {
            assert_eq!(name, "read_file");
            assert_eq!(arguments["path"], "Cargo.toml");
        }
        _ => panic!("应路由到 read_file"),
    }
    // 运行命令
    let mut events = Vec::new();
    engine.stream(&offline_req("运行命令 echo 你好", tools.clone()), &mut |e| events.push(e)).unwrap();
    match &events[0] {
        EngineEvent::ToolCall { name, arguments, .. } => {
            assert_eq!(name, "run_shell");
            assert_eq!(arguments["command"], "echo 你好");
        }
        _ => panic!("应路由到 run_shell"),
    }
    // 派子 Agent
    let mut events = Vec::new();
    engine.stream(&offline_req("派子智能体调查结构", tools.clone()), &mut |e| events.push(e)).unwrap();
    match &events[0] {
        EngineEvent::ToolCall { name, .. } => assert_eq!(name, "delegate_task"),
        _ => panic!("应路由到 delegate_task"),
    }
}

#[test]
fn test_offline_engine_answers_questions() {
    let engine = OfflineEngine::new();
    let tools = vec![tool_desc("run_shell")];
    // 问候 → 文本流 + Done
    let mut events = Vec::new();
    engine.stream(&offline_req("你好", tools), &mut |e| events.push(e)).unwrap();
    assert!(events.iter().any(|e| matches!(e, EngineEvent::Chunk(_))), "应有文本块");
    assert!(events.iter().any(|e| matches!(e, EngineEvent::Done { .. })), "应有 Done");
    let text: String = events.iter().filter_map(|e| match e { EngineEvent::Chunk(c) => Some(c.clone()), _ => None }).collect();
    assert!(text.contains("Sophon"), "回答应含 Sophon: {}", text);
}

// ── 引擎：本机地址校验（严禁云端）───────────────

#[test]
fn test_engine_local_only() {
    let local = EngineConfig {
        id: "ollama".to_string(),
        title: "Ollama".to_string(),
        base_url: "http://127.0.0.1:11434/v1".to_string(),
        api_key_secret: None,
        model: "qwen".to_string(),
        supports_tools: true,
        enabled: true,
    };
    assert!(local.is_local());
    assert!(OpenAICompatEngine::new(local).is_ok(), "127.0.0.1 应允许");

    let localhost = EngineConfig {
        id: "lm".to_string(),
        title: "LM".to_string(),
        base_url: "http://localhost:1234/v1".to_string(),
        api_key_secret: None,
        model: "m".to_string(),
        supports_tools: true,
        enabled: true,
    };
    assert!(localhost.is_local());

    let remote = EngineConfig {
        id: "openai".to_string(),
        title: "OpenAI".to_string(),
        base_url: "https://api.openai.com/v1".to_string(),
        api_key_secret: None,
        model: "gpt".to_string(),
        supports_tools: true,
        enabled: true,
    };
    assert!(!remote.is_local(), "云端地址应拒绝");
    assert!(OpenAICompatEngine::new(remote).is_err(), "云端地址应构造失败");
}

// ── 工具：实际执行 ───────────────────────────────

fn test_context(workdir: &str) -> ToolContext {
    let (mt, _rx) = main_thread::channel();
    let dir = tempfile::tempdir().unwrap();
    let secrets = SecretStore::load(&dir.path().join("secrets.json"));
    ToolContext {
        working_directory: workdir.to_string(),
        conversation_id: "test-conv".to_string(),
        terminal_manager: Arc::new(Mutex::new(TerminalManager::new())),
        team: Arc::new(Mutex::new(AgentTeamState::new())),
        task_states: Arc::new(Mutex::new(HashMap::new())),
        cancel: CancelToken::new(),
        main_thread: mt,
        depth: 0,
        engine: Arc::new(OfflineEngine::new()),
        secrets: Arc::new(Mutex::new(secrets)),
        system_prompt: "test".to_string(),
    }
}

#[test]
fn test_tool_run_shell() {
    let mut ctx = test_context(".");
    let reg = ToolRegistry::builtin();
    let out = reg.execute("run_shell", &json!({"command": "echo sophon-test"}), &mut ctx);
    match out {
        ToolOutput::Ok(s) => assert!(s.contains("sophon-test"), "输出应含 sophon-test: {}", s),
        ToolOutput::Err(e) => panic!("run_shell 失败: {}", e),
    }
}

#[test]
fn test_tool_read_write_edit_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("test.txt");
    let file_str = file.to_string_lossy().to_string();
    let mut ctx = test_context(dir.path().to_string_lossy().as_ref());
    let reg = ToolRegistry::builtin();
    // 写文件
    let out = reg.execute("write_file", &json!({"path": &file_str, "content": "第一行\n第二行\n"}), &mut ctx);
    assert!(out.is_ok(), "write_file 应成功: {:?}", out);
    assert!(file.exists());
    // 读文件
    let out = reg.execute("read_file", &json!({"path": &file_str}), &mut ctx);
    match out {
        ToolOutput::Ok(s) => assert!(s.contains("第一行"), "读回应含内容: {}", s),
        ToolOutput::Err(e) => panic!("read_file 失败: {}", e),
    }
    // 编辑文件
    let out = reg.execute("edit_file", &json!({"path": &file_str, "old_text": "第一行", "new_text": "首行"}), &mut ctx);
    assert!(out.is_ok(), "edit_file 应成功: {:?}", out);
    let content = std::fs::read_to_string(&file).unwrap();
    assert!(content.contains("首行"), "编辑后应含首行: {}", content);
    assert!(!content.contains("第一行"), "编辑后不应含旧文本");
}

#[test]
fn test_tool_find_and_search() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn main() {}\n").unwrap();
    std::fs::write(dir.path().join("b.rs"), "let x = 1;\n").unwrap();
    std::fs::write(dir.path().join("c.toml"), "[package]\n").unwrap();
    let mut ctx = test_context(dir.path().to_string_lossy().as_ref());
    let reg = ToolRegistry::builtin();
    // 找文件
    let out = reg.execute("find_files", &json!({"pattern": "*.rs", "path": "."}), &mut ctx);
    match out {
        ToolOutput::Ok(s) => {
            assert!(s.contains("a.rs"), "应找到 a.rs: {}", s);
            assert!(s.contains("b.rs"), "应找到 b.rs: {}", s);
            assert!(!s.contains("c.toml"), "不应找到 c.toml");
        }
        ToolOutput::Err(e) => panic!("find_files 失败: {}", e),
    }
    // 内容搜索
    let out = reg.execute("search_files", &json!({"pattern": "fn main", "path": "."}), &mut ctx);
    match out {
        ToolOutput::Ok(s) => assert!(s.contains("a.rs"), "应搜到 a.rs: {}", s),
        ToolOutput::Err(e) => panic!("search_files 失败: {}", e),
    }
    // 关键词搜索
    let out = reg.execute("search_keywords", &json!({"keywords": "let x", "path": "."}), &mut ctx);
    match out {
        ToolOutput::Ok(s) => assert!(s.contains("b.rs"), "应搜到 b.rs: {}", s),
        ToolOutput::Err(e) => panic!("search_keywords 失败: {}", e),
    }
}

// ── 终端：VT 模拟 ────────────────────────────────

#[test]
fn test_terminal_emulator() {
    let mut emu = TerminalEmulator::new(40, 10);
    let mut parser = vte::Parser::new();
    // 普通文本 + 换行
    for &b in b"hello\r\nworld" {
        parser.advance(&mut emu, b);
    }
    let lines = emu.visible_lines();
    let text: String = lines.iter().flat_map(|l| l.iter().map(|s| s.text.clone())).collect();
    assert!(text.contains("hello"), "应含 hello: {}", text);
    assert!(text.contains("world"), "应含 world: {}", text);
    // 颜色：红字
    let mut emu2 = TerminalEmulator::new(40, 10);
    let mut p2 = vte::Parser::new();
    for &b in b"\x1b[31mRED\x1b[0m" {
        p2.advance(&mut emu2, b);
    }
    let lines2 = emu2.visible_lines();
    let red_span = lines2.iter().flat_map(|l| l.iter()).find(|s| s.text.contains("RED"));
    assert!(red_span.is_some(), "应有 RED");
    assert_eq!(red_span.unwrap().fg, crate::terminal::Color::Red, "RED 应为红色");
}

// ── Agent 树：子智能体运行 ───────────────────────

#[test]
fn test_agent_team() {
    let mut team = AgentTeamState::new();
    let profile = AgentProfile {
        name: "研究员".to_string(),
        role: "你是研究员".to_string(),
        tools: Vec::new(),
        model: None,
    };
    let run_id = "run-1".to_string();
    team.start_run(run_id.clone(), "研究员".to_string(), "conv-1".to_string(), 0, false, profile, "调查结构".to_string(), CancelToken::new());
    // 记录日志
    team.log(&run_id, AgentLogEntry {
        at: crate::models::now_iso(),
        kind: AgentLogKind::Tool { name: "find_files".to_string(), status: crate::agent::team::ToolStatus::Success, detail: "找到 3 个".to_string() },
    });
    team.log(&run_id, AgentLogEntry {
        at: crate::models::now_iso(),
        kind: AgentLogKind::Text { text: "正在分析".to_string() },
    });
    // 完成
    team.finish_run(&run_id, "结论：项目结构清晰".to_string());
    let run = team.get_run(&run_id).unwrap();
    assert_eq!(run.status, AgentStatus::Done);
    assert_eq!(run.result.as_deref(), Some("结论：项目结构清晰"));
    assert_eq!(run.log.len(), 2);
    // 树：根节点
    let runs = team.all_runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].id, "run-1");
}

// ── 助手：时间线 / 记忆 / 话题摘要 ───────────────

#[test]
fn test_assistant_runtime() {
    let paths = Paths::resolve();
    // 用临时目录避免污染真实数据
    let dir = tempfile::tempdir().unwrap();
    let mut test_paths = paths;
    test_paths.data_dir = dir.path().to_path_buf();
    let asst = AssistantRuntime::load(&test_paths).unwrap();
    // 时间线
    asst.log(TimelineKind::ToolCall, Some("conv-1".to_string()), Some("run-1".to_string()), Some("研究员".to_string()), "调用了 find_files".to_string(), Some("找到 3 个文件".to_string()));
    let tl = asst.timeline();
    assert!(!tl.is_empty(), "时间线应有事件");
    assert_eq!(tl[0].title, "调用了 find_files");
    // 记忆
    asst.add_memory("用户偏好用深色主题".to_string(), "对话".to_string());
    let mem = asst.memories();
    assert!(!mem.is_empty(), "记忆应有条目");
    assert!(mem.iter().any(|m| m.content.contains("深色主题")));
    // 话题摘要
    let mut conv = Conversation::new(None);
    conv.nodes.push(ChatMessage::user("我们决定用 Rust 重写。".to_string()));
    conv.nodes.push(ChatMessage::user("目标是离线优先。".to_string()));
    let summary = asst.summarize(&conv);
    assert!(!summary.summary.is_empty(), "话题摘要应有内容: {}", summary.summary);
}

// ── 密钥库 ───────────────────────────────────────

#[test]
fn test_secret_store() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("secrets.json");
    let mut secrets = SecretStore::load(&file);
    assert!(secrets.names().is_empty());
    secrets.set("DASHSCOPE_API_KEY".to_string(), "sk-test-123".to_string()).unwrap();
    assert_eq!(secrets.get("DASHSCOPE_API_KEY").as_deref(), Some("sk-test-123"));
    assert!(secrets.contains("DASHSCOPE_API_KEY"));
    assert!(!secrets.contains("OTHER"));
    secrets.remove("DASHSCOPE_API_KEY");
    assert!(!secrets.contains("DASHSCOPE_API_KEY"));
}

// ── 端到端：发送 → Agent 回合 → 流式 → 落盘（P1 验证）──────────
#[test]
fn test_agent_send_end_to_end() {
    use crate::agent::runner::AgentRunner;
    use crate::store::AppCore;
    // 临时数据目录，避免污染真实数据
    let dir = tempfile::tempdir().unwrap();
    let mut paths = Paths::resolve();
    paths.data_dir = dir.path().to_path_buf();
    paths.cache_dir = dir.path().join("cache");
    let (mt, _rx) = main_thread::channel();
    let core = Arc::new(Mutex::new(AppCore::load(paths, mt).unwrap()));
    core.lock().set_secret_reader();
    // 建会话
    let conv_id = {
        let mut c = core.lock();
        c.store.create(Some(".".to_string()))
    };
    // 发送（工作线程跑回合；离线引擎答「你好」）
    AgentRunner::new(core.clone()).send(conv_id.clone(), "你好".to_string());
    // 等回合结束（最多 15 秒）
    let mut done = false;
    for _ in 0..300 {
        let gen = {
            let c = core.lock();
            let g = c.generating
                .lock()
                .get(&conv_id)
                .map(|g| g.is_generating)
                .unwrap_or(false);
            g
        };
        if !gen {
            done = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(done, "回合应在 15 秒内结束");
    // 助手消息应有文本内容
    let conv = {
        let c = core.lock();
        c.store.get(&conv_id).cloned()
    }
    .unwrap();
    let assistant: Vec<&ChatMessage> = conv
        .visible_messages()
        .into_iter()
        .filter(|m| m.role == Role::Assistant)
        .collect();
    assert!(!assistant.is_empty(), "应有助手消息");
    let text: String = assistant
        .iter()
        .flat_map(|m| m.blocks.iter())
        .filter_map(|b| match &b.kind {
            BlockKind::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert!(!text.trim().is_empty(), "助手消息应有文本: {:?}", text);
    // 用户消息在历史里
    assert!(conv
        .visible_messages()
        .into_iter()
        .any(|m| m.role == Role::User && m.text_content().contains("你好")));
}

// ── 端到端：本机 llama.cpp（llama-server）OpenAI 兼容引擎 ──────────
// 运行前需启动 llama-server 加载模型，例如：
//   llama-server.exe -m MiniCPM-V-4_7-1B-Q4_K_M.gguf --host 127.0.0.1 --port 8080
// 本测试验证 sophon 的 OpenAICompatEngine 能直连本机 llama-server 跑通问答
// （完全本地、严禁云端）。llama-server 未运行时跳过（不失败）。
#[test]
fn test_llama_server_e2e_qa() {
    // 探测 llama-server 是否在运行
    let alive = ureq::get("http://127.0.0.1:8080/health")
        .timeout(std::time::Duration::from_secs(3))
        .call()
        .is_ok();
    if !alive {
        eprintln!("SKIP: llama-server 未在 127.0.0.1:8080 运行");
        return;
    }
    let config = EngineConfig {
        id: "llama-local".to_string(),
        title: "本机 llama.cpp (MiniCPM-V)".to_string(),
        base_url: "http://127.0.0.1:8080".to_string(),
        api_key_secret: None,
        model: "MiniCPM-V-4.7-1B".to_string(),
        supports_tools: false,
        enabled: true,
    };
    let engine = OpenAICompatEngine::new(config).expect("引擎应构造成功");
    let req = StreamRequest {
        system: "你是智子（Sophon）桌面助理，用中文简洁回答。".to_string(),
        messages: vec![ChatMessage::user("你好，请用一句话介绍你自己".to_string())],
        tools: vec![],
        thinking: None,
        temperature: 0.0,
    };
    let mut chunks: Vec<String> = Vec::new();
    let mut thinking: Vec<String> = Vec::new();
    let mut usage: Option<AgentUsage> = None;
    engine
        .stream(&req, &mut |e| match e {
            EngineEvent::Chunk(c) => chunks.push(c),
            EngineEvent::Thinking(t) => thinking.push(t),
            EngineEvent::Done { usage: u } => usage = u,
            _ => {}
        })
        .expect("流式调用应成功");
    let reply = chunks.join("");
    eprintln!("【思考】{}", thinking.join(""));
    eprintln!("【回复】{}", reply);
    eprintln!("【用量】prompt={} completion={}", usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0), usage.as_ref().map(|u| u.generated_tokens).unwrap_or(0));
    assert!(!reply.trim().is_empty(), "llama-server 回复不应为空");
}

// ── analyze_image 请求构造（mock 引擎，不依赖真实服务）──────────

#[test]
fn test_analyze_image_builds_vision_request() {
    use crate::engine::{ChatEngine, EngineEvent, StreamRequest};
    use crate::models::BlockKind;
    use crate::tools::{Tool, ToolContext, ToolOutput};
    use crate::agent::team::AgentTeamState;
    use crate::store::SecretStore;
    use crate::terminal::TerminalManager;
    use crate::util::CancelToken;
    use parking_lot::Mutex;
    use std::collections::HashMap;
    use std::sync::Arc;

    /// mock 引擎：记录收到的请求，返回固定回答
    struct MockEngine {
        last: Mutex<Option<StreamRequest>>,
    }
    impl ChatEngine for MockEngine {
        fn id(&self) -> &str { "mock" }
        fn title(&self) -> &str { "Mock" }
        fn model(&self) -> &str { "mock" }
        fn supports_tools(&self) -> bool { false }
        fn stream(&self, req: &StreamRequest, sink: &mut dyn FnMut(EngineEvent)) -> anyhow::Result<()> {
            *self.last.lock() = Some(req.clone());
            sink(EngineEvent::Chunk("图片里有一只猫".to_string()));
            sink(EngineEvent::Done { usage: None });
            Ok(())
        }
    }

    // 临时图片文件（最小 PNG 头即可，analyze_image 只读字节）
    let tmp = std::env::temp_dir().join("sophon_test_analyze.png");
    let png: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    std::fs::write(&tmp, &png).expect("写临时图片");

    let engine = Arc::new(MockEngine { last: Mutex::new(None) });
    let (main_thread, _rx) = crate::util::main_thread::channel();
    let mut ctx = ToolContext {
        working_directory: String::new(),
        conversation_id: String::new(),
        terminal_manager: Arc::new(Mutex::new(TerminalManager::new())),
        team: Arc::new(Mutex::new(AgentTeamState::new())),
        task_states: Arc::new(Mutex::new(HashMap::new())),
        cancel: CancelToken::new(),
        main_thread,
        depth: 0,
        engine: engine.clone(),
        secrets: Arc::new(Mutex::new(SecretStore::load(std::path::Path::new("secrets.json")))),
        system_prompt: "test".to_string(),
    };

    let args = serde_json::json!({
        "image": tmp.to_str().unwrap(),
        "question": "这张图片里有什么？"
    });
    let tool = crate::tools::AnalyzeImage;
    match tool.execute(&args, &mut ctx) {
        ToolOutput::Ok(text) => assert!(text.contains("猫"), "应返回 mock 回答"),
        ToolOutput::Err(e) => panic!("analyze_image 失败: {}", e),
    }

    // 验证请求含图像 Media 块（vision 通道）
    let req = engine.last.lock().clone().expect("应记录请求");
    assert_eq!(req.messages.len(), 1, "应有一条用户消息");
    let msg = &req.messages[0];
    let has_image = msg.blocks.iter().any(|b| matches!(
        &b.kind,
        BlockKind::Media { media } if media.mime_type.starts_with("image/")
    ));
    assert!(has_image, "请求应含图像 Media 块");

    let _ = std::fs::remove_file(&tmp);
}
