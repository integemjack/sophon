// 内置工具实现。
// 对应 docs/ARCHITECTURE.md 的 AgentTools（run_shell / 文件 / 搜索 / 终端 / 子 Agent …）。
use super::{glob_match, resolve_path, Tool, ToolContext, ToolOutput};
use crate::agent::team::AgentProfile;
use crate::models::*;
use crate::util::CancelToken;
use serde_json::{json, Value};
use std::io::Read;

// ── run_shell ───────────────────────────────────────────

pub struct RunShell;

impl Tool for RunShell {
    fn name(&self) -> &str { "run_shell" }
    fn title(&self) -> &str { "执行命令" }
    fn description(&self) -> &str {
        "在 PowerShell 里执行一条命令，返回标准输出和标准错误。高危命令需要用户确认。"
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "要执行的命令"},
                "timeout_seconds": {"type": "integer", "description": "超时秒数，默认 60", "default": 60}
            },
            "required": ["command"]
        })
    }
    fn needs_approval(&self, _args: &Value) -> bool { true }
    fn is_write(&self) -> bool { true }

    fn execute(&self, args: &Value, ctx: &mut ToolContext) -> ToolOutput {
        let command = args["command"].as_str().unwrap_or("").to_string();
        if command.is_empty() {
            return ToolOutput::Err("command 不能为空".to_string());
        }
        let timeout = args["timeout_seconds"].as_u64().unwrap_or(60).max(1);
        // 自杀保护：不执行会结束自己的命令（docs/ARCHITECTURE.md §3.3 selfKillRefusal）
        let me = std::process::id();
        if command.contains(&format!("kill {}", me)) || command.contains(&me.to_string()) && (command.contains("taskkill") || command.contains("kill")) {
            return ToolOutput::Err("拒绝执行：这条命令会结束 Sophon 自己。".to_string());
        }
        let cwd = ctx.working_directory.clone();
        let cancel = ctx.cancel.clone();
        // 在工作线程里阻塞执行（不持全局锁）
        let result = std::thread::spawn(move || {
            run_command(&command, &cwd, timeout, &cancel)
        }).join();
        match result {
            Ok(Ok((out, code))) => {
                let text = if out.is_empty() {
                    format!("（无输出，退出码 {}）", code)
                } else {
                    if code == 0 {
                        out
                    } else {
                        format!("{}\n（退出码 {}）", out, code)
                    }
                };
                ToolOutput::Ok(text)
            }
            Ok(Err(e)) => ToolOutput::Err(e.to_string()),
            Err(_) => ToolOutput::Err("命令线程崩溃".to_string()),
        }
    }
}

fn run_command(command: &str, cwd: &str, timeout: u64, cancel: &CancelToken) -> anyhow::Result<(String, i32)> {
    use std::process::Command;
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("powershell.exe");
        c.arg("-NoProfile").arg("-Command").arg(command);
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(command);
        c
    };
    cmd.current_dir(cwd);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.stdin(std::process::Stdio::null());
    let mut child = cmd.spawn()?;
    let start = std::time::Instant::now();
    let timeout_dur = std::time::Duration::from_secs(timeout);
    // 轮询等待，支持取消与超时
    loop {
        if cancel.is_cancelled() {
            let _ = child.kill();
            return Ok(("（已停止）".to_string(), -1));
        }
        match child.try_wait()? {
            Some(status) => {
                let code = status.code().unwrap_or(-1);
                let mut out = String::new();
                if let Some(mut so) = child.stdout.take() {
                    let _ = so.read_to_string(&mut out);
                }
                if let Some(mut se) = child.stderr.take() {
                    let mut err = String::new();
                    let _ = se.read_to_string(&mut err);
                    if !err.is_empty() {
                        if !out.is_empty() {
                            out.push('\n');
                        }
                        out.push_str(&err);
                    }
                }
                // 截断超长输出（docs：AgentTools::truncate 超长留开头 3/4 结尾 1/4）
                let out = truncate_output(&out, 60000);
                return Ok((out, code));
            }
            None => {
                if start.elapsed() >= timeout_dur {
                    let _ = child.kill();
                    return Ok((format!("（超时 {} 秒，已终止）", timeout), -1));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
}

fn truncate_output(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head = s.chars().take(max * 3 / 4).collect::<String>();
    let tail = s.chars().skip(s.chars().count() - max / 4).collect::<String>();
    format!("{}\n…（中间省略 {} 字）…\n{}", head, s.chars().count() - max, tail)
}

// ── read_file ───────────────────────────────────────────

pub struct ReadFile;

impl Tool for ReadFile {
    fn name(&self) -> &str { "read_file" }
    fn title(&self) -> &str { "读文件" }
    fn description(&self) -> &str { "读取文本文件内容，可指定起始行和行数。" }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "文件路径（相对工作目录或绝对）"},
                "offset": {"type": "integer", "description": "起始行（1 起）"},
                "limit": {"type": "integer", "description": "最多读几行"}
            },
            "required": ["path"]
        })
    }
    fn execute(&self, args: &Value, ctx: &mut ToolContext) -> ToolOutput {
        let path = args["path"].as_str().unwrap_or("");
        let full = resolve_path(&ctx.working_directory, path);
        let content = match std::fs::read_to_string(&full) {
            Ok(c) => c,
            Err(e) => return ToolOutput::Err(format!("读文件失败 {}：{}", full.display(), e)),
        };
        let offset = args["offset"].as_u64().unwrap_or(1) as usize;
        let limit = args["limit"].as_u64().map(|l| l as usize);
        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();
        let start = offset.saturating_sub(1).min(total);
        let end = match limit {
            Some(l) => (start + l).min(total),
            None => total,
        };
        let mut out = String::new();
        for (i, line) in lines[start..end].iter().enumerate() {
            out.push_str(&format!("{:>5}│ {}\n", start + i + 1, line));
        }
        if total > end - start {
            out.push_str(&format!("（共 {} 行，这里显示 {}–{} 行）\n", total, start + 1, end));
        }
        ToolOutput::Ok(out)
    }
}

// ── write_file ──────────────────────────────────────────

pub struct WriteFile;

impl Tool for WriteFile {
    fn name(&self) -> &str { "write_file" }
    fn title(&self) -> &str { "写文件" }
    fn description(&self) -> &str { "新建或覆盖写一个文本文件。" }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "文件路径"},
                "content": {"type": "string", "description": "完整内容"}
            },
            "required": ["path", "content"]
        })
    }
    fn needs_approval(&self, _args: &Value) -> bool { true }
    fn is_write(&self) -> bool { true }
    fn execute(&self, args: &Value, ctx: &mut ToolContext) -> ToolOutput {
        let path = args["path"].as_str().unwrap_or("");
        let content = args["content"].as_str().unwrap_or("");
        let full = resolve_path(&ctx.working_directory, path);
        if let Some(parent) = full.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return ToolOutput::Err(format!("创建目录失败：{}", e));
            }
        }
        match std::fs::write(&full, content) {
            Ok(_) => ToolOutput::Ok(format!("已写入 {}（{} 字）", full.display(), content.chars().count())),
            Err(e) => ToolOutput::Err(format!("写文件失败 {}：{}", full.display(), e)),
        }
    }
}

// ── edit_file ───────────────────────────────────────────

pub struct EditFile;

impl Tool for EditFile {
    fn name(&self) -> &str { "edit_file" }
    fn title(&self) -> &str { "改文件" }
    fn description(&self) -> &str { "把文件里的一段文字换成另一段（精确匹配），可一次改多处。" }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "old_text": {"type": "string", "description": "要替换的原文"},
                "new_text": {"type": "string", "description": "换成的新内容"},
                "replace_all": {"type": "boolean", "description": "替换所有出现处"}
            },
            "required": ["path", "old_text", "new_text"]
        })
    }
    fn needs_approval(&self, _args: &Value) -> bool { true }
    fn is_write(&self) -> bool { true }
    fn execute(&self, args: &Value, ctx: &mut ToolContext) -> ToolOutput {
        let path = args["path"].as_str().unwrap_or("");
        let old_text = args["old_text"].as_str().unwrap_or("");
        let new_text = args["new_text"].as_str().unwrap_or("");
        let replace_all = args["replace_all"].as_bool().unwrap_or(false);
        let full = resolve_path(&ctx.working_directory, path);
        let content = match std::fs::read_to_string(&full) {
            Ok(c) => c,
            Err(e) => return ToolOutput::Err(format!("读文件失败 {}：{}", full.display(), e)),
        };
        let count = content.matches(old_text).count();
        if count == 0 {
            return ToolOutput::Err(format!("没找到要替换的文本（{} 处）", count));
        }
        if !replace_all && count > 1 {
            return ToolOutput::Err(format!("要替换的文本出现 {} 处，不唯一；请加 replace_all 或加更多上下文", count));
        }
        let new_content = if replace_all {
            content.replace(old_text, new_text)
        } else {
            content.replacen(old_text, new_text, 1)
        };
        if let Err(e) = std::fs::write(&full, new_content) {
            return ToolOutput::Err(format!("写文件失败 {}：{}", full.display(), e));
        }
        ToolOutput::Ok(format!("已替换 {} 处（{}）", count, full.display()))
    }
}

// ── find_files ──────────────────────────────────────────

pub struct FindFiles;

impl Tool for FindFiles {
    fn name(&self) -> &str { "find_files" }
    fn title(&self) -> &str { "找文件" }
    fn description(&self) -> &str { "按文件名通配在目录里找文件（含子目录）。" }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "文件名通配，如 *.rs"},
                "path": {"type": "string", "description": "目录（默认工作目录）"}
            },
            "required": ["pattern"]
        })
    }
    fn execute(&self, args: &Value, ctx: &mut ToolContext) -> ToolOutput {
        let pattern = args["pattern"].as_str().unwrap_or("*");
        let base = args["path"].as_str().unwrap_or(".");
        let base = resolve_path(&ctx.working_directory, base);
        let mut found = Vec::new();
        walk(&base, &mut |path| {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if glob_match(pattern, name) {
                    found.push(path.to_path_buf());
                }
            }
        });
        found.sort();
        found.truncate(200);
        if found.is_empty() {
            return ToolOutput::Ok(format!("没找到匹配 {} 的文件", pattern));
        }
        let mut out = String::new();
        for p in &found {
            out.push_str(&format!("{}\n", p.display()));
        }
        ToolOutput::Ok(out)
    }
}

fn walk(dir: &std::path::Path, f: &mut dyn FnMut(&std::path::Path)) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            // 跳过隐藏目录和依赖/构建目录
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.starts_with('.') || matches!(name, "node_modules" | "target" | "build" | "dist" | "third_party") {
                    continue;
                }
                walk(&path, f);
            } else {
                f(&path);
            }
        }
    }
}

// ── search_files（正则）─────────────────────────────────

pub struct SearchFiles;

impl Tool for SearchFiles {
    fn name(&self) -> &str { "search_files" }
    fn title(&self) -> &str { "搜内容（正则）" }
    fn description(&self) -> &str { "在文件内容里按正则搜索，返回 文件:行号:那一行。" }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "正则表达式"},
                "path": {"type": "string", "description": "目录或文件（默认工作目录）"},
                "glob": {"type": "string", "description": "只搜文件名匹配此通配的文件"},
                "ignore_case": {"type": "boolean", "description": "不分大小写"}
            },
            "required": ["pattern"]
        })
    }
    fn execute(&self, args: &Value, ctx: &mut ToolContext) -> ToolOutput {
        let pattern = args["pattern"].as_str().unwrap_or("");
        let ignore_case = args["ignore_case"].as_bool().unwrap_or(false);
        let glob = args["glob"].as_str().map(|s| s.to_string());
        let re = match if ignore_case {
            regex::RegexBuilder::new(pattern).case_insensitive(true).build()
        } else {
            regex::Regex::new(pattern)
        } {
            Ok(r) => r,
            Err(e) => return ToolOutput::Err(format!("正则错误：{}", e)),
        };
        let base = args["path"].as_str().unwrap_or(".");
        let base = resolve_path(&ctx.working_directory, base);
        let mut hits = Vec::new();
        let mut count = 0;
        if base.is_file() {
            search_in_file(&base, &re, &glob, &mut hits, &mut count);
        } else {
            walk(&base, &mut |path| {
                if count >= 200 {
                    return;
                }
                search_in_file(path, &re, &glob, &mut hits, &mut count);
            });
        }
        if hits.is_empty() {
            return ToolOutput::Ok(format!("没有匹配 /{}// 的内容", pattern));
        }
        ToolOutput::Ok(hits.join("\n"))
    }
}

fn search_in_file(
    path: &std::path::Path,
    re: &regex::Regex,
    glob: &Option<String>,
    hits: &mut Vec<String>,
    count: &mut usize,
) {
    if *count >= 200 {
        return;
    }
    if let Some(g) = glob {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !glob_match(g, name) {
            return;
        }
    }
    // 跳过二进制和超大文件
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };
    if content.chars().count() > 2_000_000 {
        return;
    }
    for (i, line) in content.lines().enumerate() {
        if re.is_match(line) {
            hits.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
            *count += 1;
            if *count >= 200 {
                return;
            }
        }
    }
}

// ── search_keywords ─────────────────────────────────────

pub struct SearchKeywords;

impl Tool for SearchKeywords {
    fn name(&self) -> &str { "search_keywords" }
    fn title(&self) -> &str { "按关键词搜" }
    fn description(&self) -> &str { "按关键词在文件内容里找：几个词按字面找，一个文件里几个词都有就算。" }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "keywords": {"type": "string", "description": "空格分开的几个词"},
                "path": {"type": "string", "description": "目录（默认工作目录）"},
                "glob": {"type": "string", "description": "只搜文件名匹配此通配的文件"},
                "match_any": {"type": "boolean", "description": "有其中任一个词就算"}
            },
            "required": ["keywords"]
        })
    }
    fn execute(&self, args: &Value, ctx: &mut ToolContext) -> ToolOutput {
        let keywords = args["keywords"].as_str().unwrap_or("");
        let words: Vec<String> = keywords.split_whitespace().map(|s| s.to_lowercase()).collect();
        if words.is_empty() {
            return ToolOutput::Err("keywords 不能为空".to_string());
        }
        let match_any = args["match_any"].as_bool().unwrap_or(false);
        let glob = args["glob"].as_str().map(|s| s.to_string());
        let base = args["path"].as_str().unwrap_or(".");
        let base = resolve_path(&ctx.working_directory, base);
        let mut hits: Vec<(String, Vec<String>)> = Vec::new();
        walk(&base, &mut |path| {
            if hits.len() >= 50 {
                return;
            }
            if let Some(g) = &glob {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !glob_match(g, name) {
                    return;
                }
            }
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => return,
            };
            if content.chars().count() > 2_000_000 {
                return;
            }
            let lower = content.to_lowercase();
            let matched = if match_any {
                words.iter().any(|w| lower.contains(w))
            } else {
                words.iter().all(|w| lower.contains(w))
            };
            if matched {
                // 找最相关的几行
                let mut relevant = Vec::new();
                for (i, line) in content.lines().enumerate() {
                    let ll = line.to_lowercase();
                    let hit = if match_any {
                        words.iter().any(|w| ll.contains(w))
                    } else {
                        words.iter().all(|w| ll.contains(w))
                    };
                    if hit {
                        relevant.push(format!("{}: {}", i + 1, line.trim()));
                        if relevant.len() >= 5 {
                            break;
                        }
                    }
                }
                hits.push((path.display().to_string(), relevant));
            }
        });
        if hits.is_empty() {
            return ToolOutput::Ok(format!("没有同时包含这些词的文件：{}", keywords));
        }
        let mut out = String::new();
        for (path, lines) in &hits {
            out.push_str(&format!("{}\n", path));
            for l in lines {
                out.push_str(&format!("  {}\n", l));
            }
        }
        ToolOutput::Ok(out)
    }
}

// ── 终端工具 ────────────────────────────────────────────

pub struct ReadTerminal;

impl Tool for ReadTerminal {
    fn name(&self) -> &str { "read_terminal" }
    fn title(&self) -> &str { "读终端" }
    fn description(&self) -> &str { "读取内置终端的当前内容。" }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "terminal_id": {"type": "string", "description": "终端 id"},
                "lines": {"type": "integer", "description": "读最后几行"}
            },
            "required": ["terminal_id"]
        })
    }
    fn execute(&self, args: &Value, ctx: &mut ToolContext) -> ToolOutput {
        let id = args["terminal_id"].as_str().unwrap_or("");
        let lines = args["lines"].as_u64().unwrap_or(50) as usize;
        let session = match ctx.terminal_manager.lock().get(id) {
            Some(s) => s,
            None => return ToolOutput::Err(format!("没有这个终端：{}", id)),
        };
        let render = session.lock().render(0);
        let start = render.len().saturating_sub(lines);
        let mut out = String::new();
        for spans in &render[start..] {
            for span in spans {
                out.push_str(&span.text);
            }
            out.push('\n');
        }
        ToolOutput::Ok(out)
    }
}

pub struct RunInTerminal;

impl Tool for RunInTerminal {
    fn name(&self) -> &str { "run_in_terminal" }
    fn title(&self) -> &str { "在终端里运行" }
    fn description(&self) -> &str { "在内置终端里运行一条命令（可以是一直运行的命令）。" }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "terminal_id": {"type": "string", "description": "终端 id（不填则新建）"},
                "command": {"type": "string", "description": "要运行的命令"}
            },
            "required": ["command"]
        })
    }
    fn is_write(&self) -> bool { true }
    fn execute(&self, args: &Value, ctx: &mut ToolContext) -> ToolOutput {
        let command = args["command"].as_str().unwrap_or("");
        let term_id = args["terminal_id"].as_str().map(|s| s.to_string());
        let id = match term_id {
            Some(id) => id,
            None => {
                let mut tm = ctx.terminal_manager.lock();
                match tm.create(Some(ctx.conversation_id.clone()), None, 120, 32) {
                    Ok(id) => id,
                    Err(e) => return ToolOutput::Err(format!("建终端失败：{}", e)),
                }
            }
        };
        let session = match ctx.terminal_manager.lock().get(&id) {
            Some(s) => s,
            None => return ToolOutput::Err(format!("没有这个终端：{}", id)),
        };
        // 写命令 + 回车
        let mut bytes = command.as_bytes().to_vec();
        bytes.push(b'\r');
        session.lock().write(&bytes);
        ToolOutput::Ok(format!("已在终端 {} 里运行：{}", id, command))
    }
}

// ── 任务状态 ────────────────────────────────────────────

pub struct UpdateTaskState;

impl Tool for UpdateTaskState {
    fn name(&self) -> &str { "update_task_state" }
    fn title(&self) -> &str { "更新任务状态" }
    fn description(&self) -> &str { "记录这段对话的目标和步骤进度（界面左上角显示）。" }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "goal": {"type": "string", "description": "目标"},
                "steps": {"type": "array", "items": {"type": "object", "properties": {"text": {"type": "string"}, "status": {"type": "string", "enum": ["todo", "doing", "done"]}}}},
                "remove": {"type": "boolean", "description": "清空任务状态"}
            }
        })
    }
    fn execute(&self, args: &Value, ctx: &mut ToolContext) -> ToolOutput {
        let mut states = ctx.task_states.lock();
        if args["remove"].as_bool().unwrap_or(false) {
            states.remove(&ctx.conversation_id);
            return ToolOutput::Ok("已清空任务状态".to_string());
        }
        let state = states.entry(ctx.conversation_id.clone()).or_default();
        if let Some(goal) = args["goal"].as_str() {
            state.goal = Some(goal.to_string());
        }
        if let Some(steps) = args["steps"].as_array() {
            state.steps = steps
                .iter()
                .map(|s| TaskStep {
                    text: s["text"].as_str().unwrap_or("").to_string(),
                    status: match s["status"].as_str().unwrap_or("todo") {
                        "doing" => StepStatus::Doing,
                        "done" => StepStatus::Done,
                        _ => StepStatus::Todo,
                    },
                })
                .collect();
        }
        ToolOutput::Ok("任务状态已更新".to_string())
    }
}

// ── 子 Agent 工具 ───────────────────────────────────────

pub struct DelegateTask;

impl Tool for DelegateTask {
    fn name(&self) -> &str { "delegate_task" }
    fn title(&self) -> &str { "派子智能体" }
    fn description(&self) -> &str { "把一个独立任务派给子智能体。同步等待它完成并交回结果。" }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "profile": {"type": "object", "description": "角色定义 {name, role, tools?, model?}"},
                "task": {"type": "string", "description": "完整、自包含的任务"},
                "background": {"type": "boolean", "description": "后台运行（默认 false）"}
            },
            "required": ["profile", "task"]
        })
    }
    fn needs_approval(&self, _args: &Value) -> bool { true }
    fn execute(&self, args: &Value, ctx: &mut ToolContext) -> ToolOutput {
        let profile = serde_json::from_value::<AgentProfile>(args["profile"].clone())
            .unwrap_or(AgentProfile { name: "助手".to_string(), role: "你是通才助手".to_string(), tools: Vec::new(), model: None });
        let task = args["task"].as_str().unwrap_or("").to_string();
        let background = args["background"].as_bool().unwrap_or(false);
        let engine = ctx.engine.clone();
        let team = ctx.team.clone();
        let main_thread = ctx.main_thread.clone();
        let secrets = ctx.secrets.clone();
        let terminal_manager = ctx.terminal_manager.clone();
        let task_states = ctx.task_states.clone();
        let working_directory = ctx.working_directory.clone();
        let conversation_id = ctx.conversation_id.clone();
        let system_prompt = ctx.system_prompt.clone();
        let depth = ctx.depth + 1;
        let cancel = ctx.cancel.child();
        let run_id = crate::models::new_uuid();
        let name = profile.name.clone();
        // 登记运行
        {
            let mut t = team.lock();
            t.start_run(run_id.clone(), name.clone(), conversation_id.clone(), depth, background, profile.clone(), task.clone(), cancel.clone());
        }
        // 通知界面
        main_thread.post(move |core| {
            core.request_repaint();
        });
        let run_id2 = run_id.clone();
        let team2 = team.clone();
        let main_thread2 = main_thread.clone();
        // worker 拥有自己的克隆（thread::spawn 要求 'static）
        let run_id_w = run_id.clone();
        let profile_w = profile.clone();
        let task_w = task.clone();
        let engine_w = engine.clone();
        let working_directory_w = working_directory.clone();
        let conversation_id_w = conversation_id.clone();
        let terminal_manager_w = terminal_manager.clone();
        let task_states_w = task_states.clone();
        let cancel_w = cancel.clone();
        let main_thread_w = main_thread2.clone();
        let secrets_w = secrets.clone();
        let system_prompt_w = system_prompt.clone();
        let team_w = team2.clone();
        let worker = move || -> String {
            let mut sub_ctx = ToolContext {
                working_directory: working_directory_w,
                conversation_id: conversation_id_w,
                terminal_manager: terminal_manager_w,
                team: team_w,
                task_states: task_states_w,
                cancel: cancel_w,
                main_thread: main_thread_w,
                depth,
                engine: engine_w.clone(),
                secrets: secrets_w,
                system_prompt: system_prompt_w,
            };
            crate::agent::team::run_subagent(&run_id_w, &profile_w, &task_w, &engine_w, &mut sub_ctx)
        };
        if background {
            let team_bg = team.clone();
            let main_thread_post = main_thread2.clone();
            std::thread::spawn(move || {
                let result = worker();
                let mut t = team_bg.lock();
                t.finish_run(&run_id2, result.clone());
                drop(t);
                main_thread_post.post(move |core| {
                    core.request_repaint();
                });
            });
            ToolOutput::Ok(format!("已后台派发子智能体「{}」（{}）", name, run_id))
        } else {
            let result = worker();
            let mut t = team.lock();
            t.finish_run(&run_id, result.clone());
            ToolOutput::Ok(result)
        }
    }
}

pub struct RunParallelTasks;

impl Tool for RunParallelTasks {
    fn name(&self) -> &str { "run_parallel_tasks" }
    fn title(&self) -> &str { "并行派发" }
    fn description(&self) -> &str { "把多个互不依赖的任务同时派给多个子智能体，等全部完成后交回结果。" }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tasks": {"type": "array", "items": {"type": "object", "properties": {"profile": {"type": "object"}, "task": {"type": "string"}, "name": {"type": "string"}}}, "description": "任务列表"}
            },
            "required": ["tasks"]
        })
    }
    fn needs_approval(&self, _args: &Value) -> bool { true }
    fn execute(&self, args: &Value, ctx: &mut ToolContext) -> ToolOutput {
        let tasks = match args["tasks"].as_array() {
            Some(t) => t.clone(),
            None => return ToolOutput::Err("tasks 必须是数组".to_string()),
        };
        if tasks.is_empty() {
            return ToolOutput::Err("tasks 不能为空".to_string());
        }
        let engine = ctx.engine.clone();
        let team = ctx.team.clone();
        let main_thread = ctx.main_thread.clone();
        let secrets = ctx.secrets.clone();
        let terminal_manager = ctx.terminal_manager.clone();
        let task_states = ctx.task_states.clone();
        let working_directory = ctx.working_directory.clone();
        let conversation_id = ctx.conversation_id.clone();
        let system_prompt = ctx.system_prompt.clone();
        let depth = ctx.depth + 1;
        let parent_cancel = ctx.cancel.clone();

        let mut handles = Vec::new();
        let mut run_ids = Vec::new();
        for (i, task) in tasks.iter().enumerate() {
            let profile = serde_json::from_value::<AgentProfile>(task["profile"].clone())
                .unwrap_or(AgentProfile {
                    name: format!("子任务 {}", i + 1),
                    role: "你是通才助手".to_string(),
                    tools: Vec::new(),
                    model: None,
                });
            let task_text = task["task"].as_str().unwrap_or("").to_string();
            let name = task["name"].as_str().unwrap_or(&profile.name).to_string();
            let run_id = crate::models::new_uuid();
            let cancel = parent_cancel.child();
            {
                let mut t = team.lock();
                t.start_run(run_id.clone(), name.clone(), conversation_id.clone(), depth, false, profile.clone(), task_text.clone(), cancel.clone());
            }
            let engine = engine.clone();
            let team = team.clone();
            let main_thread = main_thread.clone();
            let secrets = secrets.clone();
            let terminal_manager = terminal_manager.clone();
            let task_states = task_states.clone();
            let working_directory = working_directory.clone();
            let conversation_id = conversation_id.clone();
            let system_prompt = system_prompt.clone();
            let run_id2 = run_id.clone();
            run_ids.push((run_id.clone(), name.clone()));
            handles.push(std::thread::spawn(move || {
                let mut sub_ctx = ToolContext {
                    working_directory: working_directory.clone(),
                    conversation_id: conversation_id.clone(),
                    terminal_manager: terminal_manager.clone(),
                    team: team.clone(),
                    task_states: task_states.clone(),
                    cancel: cancel.clone(),
                    main_thread: main_thread.clone(),
                    depth,
                    engine: engine.clone(),
                    secrets: secrets.clone(),
                    system_prompt: system_prompt.clone(),
                };
                let result = crate::agent::team::run_subagent(&run_id2, &profile, &task_text, &engine, &mut sub_ctx);
                let mut t = team.lock();
                t.finish_run(&run_id2, result.clone());
                drop(t);
                main_thread.post(move |core| {
                    core.request_repaint();
                });
                (run_id2, name, result)
            }));
        }
        // 等全部完成
        let mut results = Vec::new();
        for h in handles {
            if let Ok((_id, name, result)) = h.join() {
                results.push(format!("【{}】{}", name, result));
            }
        }
        let _ = run_ids;
        ToolOutput::Ok(results.join("\n\n"))
    }
}

pub struct MessageAgent;

impl Tool for MessageAgent {
    fn name(&self) -> &str { "message_agent" }
    fn title(&self) -> &str { "给子智能体发消息" }
    fn description(&self) -> &str { "给正在运行的子智能体发一条消息（追加到它的收件箱）。" }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent_id": {"type": "string"},
                "message": {"type": "string"}
            },
            "required": ["agent_id", "message"]
        })
    }
    fn execute(&self, args: &Value, ctx: &mut ToolContext) -> ToolOutput {
        let id = args["agent_id"].as_str().unwrap_or("");
        let message = args["message"].as_str().unwrap_or("");
        let mut t = ctx.team.lock();
        if t.send_to_agent(id, message.to_string()) {
            ToolOutput::Ok(format!("已发给子智能体 {}", id))
        } else {
            ToolOutput::Err(format!("没有正在运行的子智能体：{}", id))
        }
    }
}

pub struct StopAgent;

impl Tool for StopAgent {
    fn name(&self) -> &str { "stop_agent" }
    fn title(&self) -> &str { "停止子智能体" }
    fn description(&self) -> &str { "停止一个子智能体。" }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": { "agent_id": {"type": "string"} },
            "required": ["agent_id"]
        })
    }
    fn execute(&self, args: &Value, ctx: &mut ToolContext) -> ToolOutput {
        let id = args["agent_id"].as_str().unwrap_or("");
        let mut t = ctx.team.lock();
        if t.stop_agent(id) {
            ToolOutput::Ok(format!("已停止子智能体 {}", id))
        } else {
            ToolOutput::Err(format!("没有这个子智能体：{}", id))
        }
    }
}

pub struct WaitAgents;

impl Tool for WaitAgents {
    fn name(&self) -> &str { "wait_agents" }
    fn title(&self) -> &str { "等子智能体" }
    fn description(&self) -> &str { "等所有后台子智能体交回结果。" }
    fn parameters(&self) -> Value { json!({"type": "object", "properties": {}}) }
    fn execute(&self, _args: &Value, ctx: &mut ToolContext) -> ToolOutput {
        let team = ctx.team.clone();
        let cancel = ctx.cancel.clone();
        // 轮询等待（最多 10 分钟）
        let start = std::time::Instant::now();
        loop {
            let running = team.lock().running_count();
            if running == 0 {
                break;
            }
            if cancel.is_cancelled() {
                return ToolOutput::Err("已停止".to_string());
            }
            if start.elapsed().as_secs() > 600 {
                return ToolOutput::Err("等待超时（10 分钟）".to_string());
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        ToolOutput::Ok("所有后台子智能体都已交回结果".to_string())
    }
}

pub struct ReworkAgent;

impl Tool for ReworkAgent {
    fn name(&self) -> &str { "rework_agent" }
    fn title(&self) -> &str { "打回重做" }
    fn description(&self) -> &str { "把验收意见发给已完成的子智能体，让它带着意见重做。" }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent_id": {"type": "string"},
                "feedback": {"type": "string", "description": "验收意见"}
            },
            "required": ["agent_id", "feedback"]
        })
    }
    fn needs_approval(&self, _args: &Value) -> bool { true }
    fn execute(&self, args: &Value, ctx: &mut ToolContext) -> ToolOutput {
        let id = args["agent_id"].as_str().unwrap_or("");
        let feedback = args["feedback"].as_str().unwrap_or("");
        let engine = ctx.engine.clone();
        let team = ctx.team.clone();
        let main_thread = ctx.main_thread.clone();
        let secrets = ctx.secrets.clone();
        let terminal_manager = ctx.terminal_manager.clone();
        let task_states = ctx.task_states.clone();
        let working_directory = ctx.working_directory.clone();
        let conversation_id = ctx.conversation_id.clone();
        let system_prompt = ctx.system_prompt.clone();
        let depth = ctx.depth + 1;
        let cancel = ctx.cancel.child();
        // 找已完成的运行
        let (profile, task) = {
            let t = team.lock();
            match t.get_run(id) {
                Some(run) => (run.profile.clone().unwrap_or_default(), run.result.clone().unwrap_or_default()),
                None => return ToolOutput::Err(format!("没有这个子智能体：{}", id)),
            }
        };
        let new_task = format!("【打回重做】验收意见：{}\n\n原任务：{}\n\n原结果：{}", feedback, task, task);
        let run_id = crate::models::new_uuid();
        {
            let mut t = team.lock();
            t.start_run(run_id.clone(), profile.name.clone(), conversation_id.clone(), depth, false, profile.clone(), new_task.clone(), cancel.clone());
        }
        let team2 = team.clone();
        let main_thread2 = main_thread.clone();
        let run_id2 = run_id.clone();
        let handle = std::thread::spawn(move || {
            let mut sub_ctx = ToolContext {
                working_directory: working_directory.clone(),
                conversation_id: conversation_id.clone(),
                terminal_manager: terminal_manager.clone(),
                team: team2.clone(),
                task_states: task_states.clone(),
                cancel: cancel.clone(),
                main_thread: main_thread2.clone(),
                depth,
                engine: engine.clone(),
                secrets: secrets.clone(),
                system_prompt: system_prompt.clone(),
            };
            let result = crate::agent::team::run_subagent(&run_id2, &profile, &new_task, &engine, &mut sub_ctx);
            let mut t = team2.lock();
            t.finish_run(&run_id2, result.clone());
            drop(t);
            main_thread2.post(move |core| {
                core.request_repaint();
            });
            result
        });
        match handle.join() {
            Ok(r) => ToolOutput::Ok(r),
            Err(_) => ToolOutput::Err("打回线程崩溃".to_string()),
        }
    }
}

pub struct ReadAgentLog;

impl Tool for ReadAgentLog {
    fn name(&self) -> &str { "read_agent_log" }
    fn title(&self) -> &str { "读子智能体记录" }
    fn description(&self) -> &str { "读子智能体的完整运行过程（调了哪些工具、拿到什么）。" }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent_id": {"type": "string"},
                "offset": {"type": "integer", "description": "从第几条开始（记录太长时分段读）"}
            },
            "required": ["agent_id"]
        })
    }
    fn execute(&self, args: &Value, ctx: &mut ToolContext) -> ToolOutput {
        let id = args["agent_id"].as_str().unwrap_or("");
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let t = ctx.team.lock();
        match t.get_run(id) {
            Some(run) => {
                let log: Vec<String> = run.log.iter().skip(offset).map(|e| format!("[{}] {} {}", e.at, match &e.kind {
                    crate::agent::team::AgentLogKind::Tool { name, status, .. } => format!("工具 {}（{}）", name, status.label()),
                    crate::agent::team::AgentLogKind::Text { text } => format!("文本：{}", text.chars().take(100).collect::<String>()),
                    crate::agent::team::AgentLogKind::Note { text } => text.clone(),
                }, "")).collect();
                if log.is_empty() {
                    ToolOutput::Ok("（没有更多记录）".to_string())
                } else {
                    ToolOutput::Ok(log.join("\n"))
                }
            }
            None => ToolOutput::Err(format!("没有这个子智能体：{}", id)),
        }
    }
}

// ── 杂项 ────────────────────────────────────────────────

pub struct ListTools;

impl Tool for ListTools {
    fn name(&self) -> &str { "list_tools" }
    fn title(&self) -> &str { "列出工具" }
    fn description(&self) -> &str { "列出所有可用的工具。" }
    fn parameters(&self) -> Value { json!({"type": "object", "properties": {}}) }
    fn execute(&self, _args: &Value, _ctx: &mut ToolContext) -> ToolOutput {
        // 从注册表拿（通过 team 不行；这里直接列名字）
        let names = crate::tools::ToolRegistry::builtin().names();
        ToolOutput::Ok(names.join("\n"))
    }
}

pub struct GetCurrentTime;

impl Tool for GetCurrentTime {
    fn name(&self) -> &str { "get_current_time" }
    fn title(&self) -> &str { "当前时间" }
    fn description(&self) -> &str { "当前的本地日期、时间、星期和时区。" }
    fn parameters(&self) -> Value { json!({"type": "object", "properties": {}}) }
    fn execute(&self, _args: &Value, _ctx: &mut ToolContext) -> ToolOutput {
        use chrono::Local;
        let now = Local::now();
        ToolOutput::Ok(format!(
            "{} {} {} (UTC{})",
            now.format("%Y-%m-%d"),
            now.format("%H:%M:%S"),
            now.format("%A"),
            now.format("%z")
        ))
    }
}

// ── 看图（analyze_image）：用本机视觉模型（MiniCPM-V 等）──────────

pub struct AnalyzeImage;

impl Tool for AnalyzeImage {
    fn name(&self) -> &str { "analyze_image" }
    fn title(&self) -> &str { "看图" }
    fn description(&self) -> &str { "看一张图片并回答问题。" }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {"image": {"type": "string", "description": "图片路径"}, "question": {"type": "string", "description": "要问的问题"}}, "required": ["image", "question"]})
    }
    fn execute(&self, args: &Value, ctx: &mut ToolContext) -> ToolOutput {
        let image = args["image"].as_str().unwrap_or("");
        let question = args["question"].as_str().unwrap_or("这张图片里有什么？");
        if image.is_empty() {
            return ToolOutput::Err("缺少图片路径".to_string());
        }
        if !std::path::Path::new(image).exists() {
            return ToolOutput::Err(format!("图片不存在：{}", image));
        }
        // 构造一条带图像的消息（Media 块），经引擎的 vision 通道发问
        let mut msg = ChatMessage::user(question);
        let media = MediaAttachment {
            id: crate::models::new_uuid(),
            kind: "image".to_string(),
            relative_path: image.to_string(),
            file_name: std::path::Path::new(image)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("image")
                .to_string(),
            mime_type: guess_image_mime(image),
            byte_count: std::fs::metadata(image).map(|m| m.len()).unwrap_or(0),
            caption: None,
            width: None,
            height: None,
        };
        msg.blocks.push(MessageBlock::media(media));
        let req = crate::engine::StreamRequest {
            system: ctx.system_prompt.clone(),
            messages: vec![msg],
            tools: vec![],
            thinking: None,
            temperature: 0.3,
        };
        let mut result = String::new();
        let r = ctx.engine.stream(&req, &mut |ev| {
            if let crate::engine::EngineEvent::Chunk(c) = ev {
                result.push_str(&c);
            }
        });
        match r {
            Ok(()) => {
                let t = result.trim();
                if t.is_empty() {
                    ToolOutput::Err("引擎没有返回图片描述（当前引擎可能不支持视觉）".to_string())
                } else {
                    ToolOutput::Ok(t.to_string())
                }
            }
            Err(e) => ToolOutput::Err(format!("看图失败：{}", e)),
        }
    }
}

/// 按扩展名猜图片 MIME
fn guess_image_mime(path: &str) -> String {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "png" => "image/png".to_string(),
        "jpg" | "jpeg" => "image/jpeg".to_string(),
        "gif" => "image/gif".to_string(),
        "webp" => "image/webp".to_string(),
        "bmp" => "image/bmp".to_string(),
        _ => "image/png".to_string(),
    }
}

// ── 说话（speak）：系统 TTS（Windows SAPI，完全本地）──────────────

pub struct Speak;

impl Tool for Speak {
    fn name(&self) -> &str { "speak" }
    fn title(&self) -> &str { "说话" }
    fn description(&self) -> &str { "把一段话说出来（合成语音）。" }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {"text": {"type": "string", "description": "要朗读的文本"}}, "required": ["text"]})
    }
    fn execute(&self, args: &Value, _ctx: &mut ToolContext) -> ToolOutput {
        let text = args["text"].as_str().unwrap_or("");
        if text.is_empty() {
            return ToolOutput::Err("缺少要朗读的文本".to_string());
        }
        speak_text(text);
        ToolOutput::Ok("已朗读".to_string())
    }
}

/// 用系统 TTS 朗读文本（Windows SAPI；完全本地、零模型）
fn speak_text(text: &str) {
    // 写到临时文件再朗读，避免命令行转义问题
    let tmp = std::env::temp_dir().join(format!("sophon_tts_{}.txt", std::process::id()));
    if std::fs::write(&tmp, text).is_ok() {
        let ps = format!(
            "Add-Type -AssemblyName System.Speech; \
             $t = Get-Content -Raw -LiteralPath '{}'; \
             (New-Object System.Speech.Synthesis.SpeechSynthesizer).Speak($t)",
            tmp.display()
        );
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
            .output();
        let _ = std::fs::remove_file(&tmp);
    }
}

// ── 能力工具（本地无提供方时如实报告）──────────────────

macro_rules! capability_stub {
    ($name:ident, $title:expr, $desc:expr, $params:expr) => {
        pub struct $name;
        impl Tool for $name {
            fn name(&self) -> &str { stringify!($name) }
            fn title(&self) -> &str { $title }
            fn description(&self) -> &str { $desc }
            fn parameters(&self) -> Value { $params }
            fn execute(&self, _args: &Value, _ctx: &mut ToolContext) -> ToolOutput {
                ToolOutput::Err(format!(
                    "「{}」需要本地能力提供方。按用户约束（完全本地、不联网），\
                     当前没有接入任何提供方；在设置里接入本机提供方后可用。",
                    $title
                ))
            }
        }
    };
}

capability_stub!(
    GenerateImage,
    "生成图片",
    "按文字描述生成图片。",
    json!({"type": "object", "properties": {"prompt": {"type": "string"}}, "required": ["prompt"]})
);
capability_stub!(
    TranscribeAudio,
    "听录音",
    "听一段录音并转成文字。",
    json!({"type": "object", "properties": {"audio": {"type": "string"}}, "required": ["audio"]})
);
