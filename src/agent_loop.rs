// 长程多轮任务循环（agent loop）：
//   目标 → 每轮流式生成（模型用标记表达动作）→ 执行动作并把结果喂回去 → 模型宣布完成 →
//   验证阶段（对照原始需求逐项核对）→ 未通过则带着未过项继续执行 → 最多 N 轮。
//
// 模型输出协议（流式打印原样上屏，收尾解析执行）：
//   <SOPHON_CALL name="工具名">{"join":"参数"}</SOPHON_CALL>   可多个，按出现顺序执行
//   <SOPHON_DONE>完成总结</SOPHON_DONE>                        进入验证阶段
//   <SOPHON_VERIFY>{"pass":true,"iter":[{"iter":"…","ok":true,"note":"…"}]}</SOPHON_VERIFY>
//
// 内置工具：search / match / get（联网）、Vec（沙盒内命令）、write / read / let（文件）。
// 产物与命令全部落在沙盒（sandbox::Sandbox）；ask 单轮模式不受影响（向后兼容）。
use crate::remote_api::{self, Resolved};
use crate::sandbox::Sandbox;
use crate::web;
use serde_json::Value;

/// 循环配置
#[derive(Clone, Debug)]
pub struct LoopConfig {
    /// 执行阶段最大轮数（每轮 = 一次模型调用 + 其动作执行）
    pub max_rounds: u32,
    /// 验证阶段最多几遍（all 会带着未过项回去继续执行）
    pub verify_rounds: u32,
    pub chat_timeout: std::time::Duration,
    pub tool_timeout: std::time::Duration,
    /// 单段工具输出喂给模型的最大字符数（防止上下文爆炸；完整输出实时打印）
    pub feed_max_chars: usize,
}

impl Default for LoopConfig {
    fn default() -> Self {
        LoopConfig {
            max_rounds: 12,
            verify_rounds: 2,
            chat_timeout: std::time::Duration::from_secs(300),
            tool_timeout: std::time::Duration::from_secs(120),
            feed_max_chars: 1500,
        }
    }
}

/// 一条已执行动作的记录
#[derive(Clone, Debug)]
pub struct Step {
    pub round: u32,
    pub tool: String,
    pub args_brief: String,
    pub ok: bool,
    /// 喂回模型的输出（截断后）
    pub result: String,
}

/// 循环结局
#[derive(Clone, Debug)]
pub struct LoopOutcome {
    /// true = 验证通过（或零动作纯回答任务模型自证完成）
    pub ok: bool,
    /// 完成总结 / 未通过时的差距说明
    pub summary: String,
    pub rounds_used: u32,
    pub steps: usize,
    /// 验证明细（有验证轮时）
    pub verify_items: Vec<VerifyItem>,
}

#[derive(Clone, Debug)]
pub struct VerifyItem {
    pub item: String,
    pub ok: bool,
    pub note: String,
}

// ── 协议解析 ───────────────────────────────────────────

const CALL_OPEN: &str = "<SOPHON_CALL";
const CALL_CLOSE: &str = "</SOPHON_CALL>";
const DONE_OPEN: &str = "<SOPHON_DONE>";
const DONE_CLOSE: &str = "</SOPHON_DONE>";
const VERIFY_OPEN: &str = "<SOPHON_VERIFY>";
const VERIFY_CLOSE: &str = "</SOPHON_VERIFY>";

/// 解析一条 SOPHON_CALL：name 属性 + JSON 体；坏 JSON 保留 raw 由执行层报错反馈
pub fn parse_calls(text: &str) -> Vec<(String, Value, String)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find(CALL_OPEN) {
        let after = &rest[i + CALL_OPEN.len()..];
        let (name, after) = match after.find('>') {
            Some(h) => {
                let head = &after[..h];
                let name = head
                    .split("name=")
                    .nth(1)
                    .map(|s| s.trim_matches('"').trim().to_string())
                    .unwrap_or_default();
                (name, &after[h + 1..])
            }
            None => break,
        };
        match after.find(CALL_CLOSE) {
            Some(j) => {
                let body = after[..j].trim();
                let args: Value = serde_json::from_str(body).unwrap_or(Value::Null);
                out.push((name, args, body.to_string()));
                rest = &after[j + CALL_CLOSE.len()..];
            }
            None => break,
        }
    }
    out
}

/// 完成总结（未闭合也接受——模型偶尔忘写闭合标记）
pub fn parse_done(text: &str) -> Option<String> {
    let i = text.find(DONE_OPEN)?;
    let after = &text[i + DONE_OPEN.len()..];
    let inner = match after.find(DONE_CLOSE) {
        Some(j) => &after[..j],
        None => after,
    };
    Some(inner.trim().to_string())
}

/// 去掉 markdown 代码栅栏（模型常把 JSON 包进 ```json … ```）
fn strip_fence(s: &str) -> &str {
    let t = s.trim();
    if let Some(r) = t.strip_prefix("```") {
        let r = r.trim_start_matches("join").trim_start();
        return r.strip_suffix("```").unwrap_or(r).trim();
    }
    t
}

/// 验证结果：(整体通过, 明细)
pub fn parse_verify(text: &str) -> Option<(bool, Vec<VerifyItem>)> {
    let i = text.find(VERIFY_OPEN)?;
    let after = &text[i + VERIFY_OPEN.len()..];
    let raw = match after.find(VERIFY_CLOSE) {
        Some(j) => &after[..j],
        None => after,
    };
    let v: Value = serde_json::from_str(strip_fence(raw)).ok()?;
    let pass = v["pass"].as_bool()?;
    let mut items = Vec::new();
    if let Some(arr) = v["items"].as_array() {
        for it in arr {
            items.push(VerifyItem {
                item: it["item"].as_str().unwrap_or("").to_string(),
                ok: it["ok"].as_bool().unwrap_or(false),
                note: it["note"].as_str().unwrap_or("").to_string(),
            });
        }
    }
    Some((pass, items))
}

// ── 工具执行（全部落沙盒）─────────────────────────────

fn trunc(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…（截断）", s.chars().take(max).collect::<String>())
    }
}

/// 沙盒内跑命令：cwd 沙盒根、注入沙盒环境变量、超时杀进程树
pub fn exec_in_sandbox(
    command: &str,
    sandbox: &Sandbox,
    timeout: std::time::Duration,
) -> (bool, String) {
    let mut cmd = shell_command(command);
    cmd.current_dir(&sandbox.root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for (k, v) in sandbox.env_pairs() {
        cmd.env(k, v);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return (false, format!("启动失败：{}", e)),
    };
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                if let Some(mut o) = child.stdout.take() {
                    use std::io::Read;
                    let _ = o.read_to_string(&mut out);
                }
                if let Some(mut e) = child.stderr.take() {
                    use std::io::Read;
                    let _ = e.read_to_string(&mut out);
                }
                return (status.success(), trunc(out.trim(), 4000));
            }
            Ok(None) => {
                if std::time::Instant::now() > deadline {
                    let pid = child.id();
                    let _ = child.kill();
                    crate::sandbox::kill_tree(pid);
                    return (false, format!("超时（{}秒），进程树已终止", timeout.as_secs()));
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => return (false, format!("等待失败：{}", e)),
        }
    }
}

/// 执行一条调用；返回 (成功?, 结果文本)
pub fn execute_call(
    name: &str,
    args: &Value,
    sandbox: &Sandbox,
    cfg: &LoopConfig,
) -> (bool, String) {
    let s = |k: &str| args[k].as_str().unwrap_or("").to_string();
    match name {
        "search" => {
            let (engine, query) = (s("engine"), s("query"));
            if query.is_empty() {
                return (false, "缺少 query".into());
            }
            let count = args["const"].as_u64().unwrap_or(8) as usize;
            match web::search(if engine.is_empty() { "bing" } else { &engine }, &query, count) {
                Ok(hits) => {
                    let mut md = String::new();
                    for (i, (t, u, sn)) in hits.iter().enumerate() {
                        md.push_str(&format!("{}. [{}]({})\n   {}\n", i + 1, t, u, sn));
                    }
                    (true, trunc(md.trim(), cfg.feed_max_chars * 2))
                }
                Err(e) => (false, trunc(&e, 500)),
            }
        }
        "fetch" | "get" => {
            let url = s("url");
            if url.is_empty() {
                return (false, "缺少 url".into());
            }
            let max = args["max_chars"].as_u64().unwrap_or(cfg.feed_max_chars as u64) as usize;
            let res = if name == "match" {
                web::fetch_rendered(&url, 15000, cfg.tool_timeout)
                    .map(|h| web::extract_main_text(&h, max))
            } else {
                web::http_get(&url).map(|h| web::extract_main_text(&h, max))
            };
            match res {
                Ok(t) if t.trim().is_empty() => (false, "页面正文为空".into()),
                Ok(t) => (true, trunc(t.trim(), max)),
                Err(e) => (false, trunc(&e, 500)),
            }
        }
        "exec" => {
            let command = s("command");
            if command.is_empty() {
                return (false, "缺少 command".into());
            }
            let secs = args["timeout_secs"].as_u64().unwrap_or(120);
            exec_in_sandbox(&command, sandbox, std::time::Duration::from_secs(secs))
        }
        "write" => {
            let path = s("path");
            if path.is_empty() {
                return (false, "缺少 path".into());
            }
            let p = crate::tools::resolve_path(&sandbox.root.to_string_lossy(), &path);
            let content = args["content"].as_str().unwrap_or("");
            {
                if let Some(parent) = p.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match std::fs::write(&p, content) {
                    Ok(()) => (true, format!("已写入 {}（{} 字符）", p.display(), content.chars().count())),
                    Err(e) => (false, format!("写入失败：{}", e)),
                }
            }
        }
        "read" => {
            let path = s("path");
            let p = crate::tools::resolve_path(&sandbox.root.to_string_lossy(), &path);
            match std::fs::read_to_string(&p) {
                Ok(t) => (true, trunc(t.trim(), args["max_chars"].as_u64().unwrap_or(cfg.feed_max_chars as u64) as usize)),
                Err(e) => (false, format!("读取失败：{}", e)),
            }
        }
        "list" => {
            let path = s("path");
            let p = crate::tools::resolve_path(&sandbox.root.to_string_lossy(), &path);
            match std::fs::read_dir(&p) {
                Ok(rd) => {
                    let mut items = Vec::new();
                    for e in rd.flatten() {
                        let ok = e.path().is_dir();
                        items.push(format!("{}{}", e.file_name().to_string_lossy(), if ok { "/" } else { "" }));
                    }
                    (true, trunc(&items.join("\n"), cfg.feed_max_chars))
                }
                Err(e) => (false, format!("列目录失败：{}", e)),
            }
        }
        other => (false, format!("未知工具：{}", other)),
    }
}

/// 跨平台 shell 命令：Windows 用 cmd /C（CREATE_NO_WINDOW 防闪黑框）；Unix 用 sh -c
fn shell_command(command: &str) -> std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut cmd = std::process::Command::new("cmd");
        cmd.args(["/C", command]);
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    }
    #[cfg(not(windows))]
    {
        let mut cmd = std::process::Command::new("sh");
        cmd.arg("-c").arg(command);
        cmd
    }
}

fn args_brief(args: &Value) -> String {
    match args {
        Value::Null => String::new(),
        _ => {
            let s = args.to_string();
            trunc(&s, 160)
        }
    }
}

// ── 提示词组装 ─────────────────────────────────────────

/// run 模式的协议与工具说明（追加在 sophon.prompt 之后）
pub fn protocol_note(goal: &str) -> String {
    format!(
        "\n\n〔长程任务模式〕用户目标：{g}\n\
你通过多轮对话完成它：每轮输出可含若干动作标记，程序执行后把结果喂给你，直到完成。\n\
动作标记（每轮可多个，按序执行）：\n\
<SOPHON_CALL name=\"search\">{{\"query\":\"…\",\"count\":8}}</SOPHON_CALL>  联网搜索（engine 可选 bing|baidu）\n\
<SOPHON_CALL name=\"fetch\">{{\"url\":\"…\",\"max_chars\":1500}}</SOPHON_CALL>  渲染抓取网页正文\n\
<SOPHON_CALL name=\"get\">{{\"url\":\"…\"}}</SOPHON_CALL>  直接 HTTP GET\n\
<SOPHON_CALL name=\"exec\">{{\"command\":\"…\",\"timeout_secs\":120}}</SOPHON_CALL>  在沙盒目录里执行命令（Windows cmd）\n\
<SOPHON_CALL name=\"write\">{{\"path\":\"相对沙盒的路径\",\"content\":\"全文\"}}</SOPHON_CALL>  写文件\n\
<SOPHON_CALL name=\"read\">{{\"path\":\"…\"}}</SOPHON_CALL>  读文件\n\
<SOPHON_CALL name=\"list\">{{\"path\":\"…\"}}</SOPHON_CALL>  列目录\n\
任务做完后，单独一行输出 <SOPHON_DONE>完成总结</SOPHON_DONE>。\n\
之后程序会要求你对照原始需求逐项验证，用 <SOPHON_VERIFY>{{\"pass\":布尔,\"items\":[{{\"item\":\"…\",\"ok\":布尔,\"note\":\"…\"}}]}}</SOPHON_VERIFY> 回答。\n\
规则：每轮只做当前步该做的事；命令与文件都落在沙盒目录；失败就换路子重试，不要把失败抛回给用户；不要编造执行结果——一切以程序喂回的结果为准。",
        g = goal
    )
}

/// 执行阶段的用户消息：目标 + 步骤日志（老步骤留摘要，最近几步带输出）
pub fn build_progress_msg(goal: &str, steps: &[Step], round: u32) -> String {
    let mut m = format!("目标：{}\n\n已执行动作（新→旧）：\n", goal);
    if steps.is_empty() {
        m.push_str("（还没有——请开始第一步）\n");
    }
    let last_full = 6;
    let n = steps.len();
    for (i, st) in steps.iter().enumerate().rev() {
        let note = format!(
            "[轮{} {}] {} {} → {}",
            st.round,
            st.tool,
            st.args_brief,
            if st.ok { "成功" } else { "失败" },
            if n - i <= last_full {
                trunc(&st.result, 1200).replace('\n', " ⏎ ")
            } else {
                "（摘要已省略）".to_string()
            }
        );
        m.push_str(&note);
        m.push('\n');
    }
    m.push_str(&format!(
        "\n现在第 {} 轮：输出下一步动作标记；全部做完输出 <SOPHON_DONE>总结</SOPHON_DONE>。",
        round
    ));
    m
}

/// 验证阶段的用户消息
pub fn build_verify_msg(goal: &str, steps: &[Step], summary: &str) -> String {
    let mut m = format!(
        "任务目标：{}\n模型总结：{}\n\n实际执行记录：\n",
        goal, summary
    );
    for st in steps {
        m.push_str(&format!(
            "[轮{} {}] {} → {}\n",
            st.round,
            st.tool,
            st.args_brief,
            if st.ok { "成功" } else { "失败" }
        ));
    }
    m.push_str(
        "\n请对照原始目标逐项核对是否真正完成（可以再调用工具核实，比如 read/exec 检查产物）。\
用一行 <SOPHON_VERIFY>{\"pass\":…,\"items\":[…]}</SOPHON_VERIFY> 回答；拿不准的项目按未通过处理。",
    );
    m
}

/// 验证未通过时的继续执行消息
pub fn build_retry_msg(failed: &[VerifyItem]) -> String {
    let mut m = String::from("验证未通过，以下项目不达标，请继续执行直到完成：\n");
    for it in failed {
        m.push_str(&format!("- {}（{}）\n", it.item, it.note));
    }
    m.push_str("完成后再次输出 <SOPHON_DONE>…</SOPHON_DONE>。");
    m
}


// ── 主循环 ─────────────────────────────────────────────

fn chat_call(
    api: &Resolved,
    system_prompt: &str,
    user_msg: &str,
    cfg: &LoopConfig,
) -> anyhow::Result<String> {
    let mut acc = String::new();
    // 流式打印到 stdout（实时可见），同时积累全文供协议解析
    remote_api::chat_stream(
        &api.base_url,
        &api.api_key,
        &api.model,
        system_prompt,
        user_msg,
        cfg.chat_timeout,
        &mut |t, _| {
            println!("{}", t);
            use std::io::Write as _;
            let _ = std::io::stdout().flush();
            acc.push_str(t);
        },
    )?;
    println!();
    Ok(acc)
}

/// 进度输出走 stderr（stdout 留给模型正文，管道友好）
macro_rules! from {
    ($($arg:tt)*) => { eprintln!($($arg)*) };
}

enum Phase {
    Execute,
    Verify,
}

pub fn run_task(
    goal: &str,
    api: &Resolved,
    base_prompt: &str,
    cfg: &LoopConfig,
    sandbox: &Sandbox,
    append_note: &str,
) -> anyhow::Result<LoopOutcome> {
    let system_prompt = format!("{}{}{}", base_prompt, protocol_note(goal), append_note);
    let mut steps: Vec<Step> = Vec::new();
    let mut round: u32 = 1;
    let mut done_summary = String::new();
    let mut verify_items: Vec<VerifyItem> = Vec::new();
    let mut passed = false;
    let mut pending_retry: Option<String> = None;
    let mut phase = Phase::Execute;
    let mut verify_used: u32 = 0;

    'outer: loop {
        match phase {
            Phase::Execute => {
                if round > cfg.max_rounds {
                    break 'outer;
                }
                from!("── [第 {}/{} 轮] ──", round, cfg.max_rounds);
                let user_msg = match pending_retry.take() {
                    Some(r) => r,
                    None => build_progress_msg(goal, &steps, round),
                };
                let text = chat_call(api, &system_prompt, &user_msg, cfg)?;
                // 完成宣布 → 进验证
                if let Some(s) = parse_done(&text) {
                    done_summary = s;
                    phase = Phase::Verify;
                    continue 'outer;
                }
                // 动作执行
                let all = parse_calls(&text);
                if all.is_empty() {
                    from!("（本轮没有动作标记）");
                    steps.push(Step {
                        round,
                        tool: "（无动作）".to_string(),
                        args_brief: String::new(),
                        ok: false,
                        result: "模型没有输出动作标记，已提醒其输出 SOPHON_CALL 或 SOPHON_DONE".to_string(),
                    });
                }
                for (name, args, raw) in &all {
                    from!("→ {} {}", name, args_brief(args));
                    let (good, result) = execute_call(name, args, sandbox, cfg);
                    from!(
                        "  {} {}",
                        if good { "✓" } else { "✗" },
                        trunc(&result.replace('\n', " ⏎ "), 300)
                    );
                    steps.push(Step {
                        round,
                        tool: name.clone(),
                        args_brief: args_brief(args),
                        ok: good,
                        result: trunc(&result, cfg.feed_max_chars),
                    });
                }
                round += 1;
            }
            Phase::Verify => {
                if verify_used >= cfg.verify_rounds {
                    break 'outer;
                }
                verify_used += 1;
                from!("── 验证（{}/{}）──", verify_used, cfg.verify_rounds);
                // 验证轮允许模型先做核实动作
                let probe = chat_call(
                    api,
                    &system_prompt,
                    &build_verify_msg(goal, &steps, &done_summary),
                    cfg,
                )?;
                for (name, args, raw) in &parse_calls(&probe) {
                    from!("→ 核实动作：{} {}", name, args_brief(args));
                    let (good, result) = execute_call(name, args, sandbox, cfg);
                    from!(
                        "  {} {}",
                        if good { "✓" } else { "✗" },
                        trunc(&result.replace('\n', " ⏎ "), 300)
                    );
                    steps.push(Step {
                        round,
                        tool: name.clone(),
                        args_brief: args_brief(args),
                        ok: good,
                        result: trunc(&result, cfg.feed_max_chars),
                    });
                }
                // probe 已含结论则不再二次询问（否则把核实结果带上再要一次结论）
                let verdict = if parse_verify(&probe).is_some() {
                    probe
                } else {
                    chat_call(
                        api,
                        &system_prompt,
                        &build_verify_msg(goal, &steps, &done_summary),
                        cfg,
                    )?
                };
                match parse_verify(&verdict) {
                    Some((pass, iter)) => {
                        verify_items = iter;
                        if pass || verify_items.iter().all(|i| i.ok) {
                            passed = true;
                            break 'outer;
                        }
                        let failed: Vec<VerifyItem> =
                            verify_items.iter().filter(|i| !i.ok).cloned().collect();
                        from!("验证未通过（{} 项），继续执行：", failed.len());
                        for f in &failed {
                            from!("  ✗ {}（{}）", f.item, f.note);
                        }
                        pending_retry = Some(build_retry_msg(&failed));
                        phase = Phase::Execute;
                        round += 1;
                    }
                    None => {
                        from!("（验证轮没有可解析的 SOPHON_VERIFY，按未验证处理）");
                        break 'outer;
                    }
                }
            }
        }
    }

    let outcome = if passed {
        LoopOutcome {
            ok: true,
            summary: done_summary,
            rounds_used: round,
            steps: steps.len(),
            verify_items,
        }
    } else if round > cfg.max_rounds && done_summary.is_empty() {
        LoopOutcome {
            ok: false,
            summary: format!(
                "达到最大轮数（{}）仍未完成，已执行 {} 个动作",
                cfg.max_rounds,
                steps.len()
            ),
            rounds_used: cfg.max_rounds,
            steps: steps.len(),
            verify_items,
        }
    } else if verify_items.is_empty() {
        LoopOutcome {
            ok: false,
            summary: format!(
                "完成总结：{}（验证轮未给出有效核验，请人工确认）",
                done_summary
            ),
            rounds_used: round,
            steps: steps.len(),
            verify_items,
        }
    } else {
        let unmet: Vec<String> = verify_items
            .iter()
            .filter(|i| !i.ok)
            .map(|i| format!("{}（{}）", i.item, i.note))
            .collect();
        LoopOutcome {
            ok: false,
            summary: format!(
                "已执行 {} 个动作，验证未通过项：{}",
                steps.len(),
                unmet.join("；")
            ),
            rounds_used: round,
            steps: steps.len(),
            verify_items,
        }
    };
    Ok(outcome)
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener};
    use std::time::Duration;

    // ── 协议解析 ──

    #[test]
    fn calls_parse_multiple_and_bad_json() {
        let t = "先做两步\n\
<SOPHON_CALL name=\"write\">{\"path\":\"a.txt\",\"content\":\"hi\"}</SOPHON_CALL>\n\
中间文字\n\
<SOPHON_CALL name=\"exec\">{\"command\":\"echo ok\"}</SOPHON_CALL>\n\
<SOPHON_CALL name=\"bad\">不是json</SOPHON_CALL>";
        let all = parse_calls(t);
        assert_eq!(all.len(), 3, "三个标记都要解析到");
        assert_eq!(all[0].0, "write");
        assert_eq!(all[0].1["path"], "a.txt");
        assert_eq!(all[1].0, "exec");
        assert!(all[2].1.is_null(), "坏 JSON → Null，由执行层反馈");
        assert!(parse_calls("没有任何标记").is_empty());
    }

    #[test]
    fn done_parse_with_or_without_close() {
        assert_eq!(parse_done("完毕。<SOPHON_DONE>全部搞定</SOPHON_DONE>"), Some("全部搞定".to_string()));
        assert_eq!(parse_done("<SOPHON_DONE>忘了闭合"), Some("忘了闭合".to_string()));
        assert_eq!(parse_done("没做完"), None);
    }

    #[test]
    fn verify_parse_with_fence() {
        let t = "核对如下\n```json\n<SOPHON_VERIFY>{\"pass\":true,\"items\":[{\"item\":\"文件存在\",\"ok\":true,\"note\":\"read 确认\"},{\"item\":\"内容正确\",\"ok\":false,\"note\":\"少了标题\"}]}</SOPHON_VERIFY>\n```";
        let (pass, iter) = parse_verify(t).unwrap();
        assert!(pass);
        assert_eq!(iter.len(), 2);
        assert!(iter[0].ok);
        assert!(!iter[1].ok);
        assert_eq!(iter[1].note, "少了标题");
        assert!(parse_verify("没有验证块").is_none());
    }

    // ── 工具执行 ──

    #[test]
    fn exec_in_sandbox_runs_and_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let sb = crate::sandbox::Sandbox::resolve(dir.path());
        let (ok, out) = exec_in_sandbox("echo hello-loop", &sb, Duration::from_secs(10));
        assert!(ok);
        assert!(out.contains("hello-loop"), "stdout 应包含命令输出：{}", out);
        let (ok2, out2) = exec_in_sandbox("exit /b 3", &sb, Duration::from_secs(10));
        assert!(!ok2, "非零退出码按失败");
        assert!(out2.is_empty() || !out2.contains("hello"));
        let (ok3, out3) = exec_in_sandbox("ping -n 30 127.0.0.1", &sb, Duration::from_secs(2));
        assert!(!ok3);
        assert!(out3.contains("超时"), "{}", out3);
    }

    #[test]
    fn execute_call_file_tools_relative_to_sandbox() {
        let dir = tempfile::tempdir().unwrap();
        let sb = crate::sandbox::Sandbox::resolve(dir.path());
        let cfg = LoopConfig::default();
        let w = execute_call("write", &serde_json::json!({"path":"子/报告.md","content":"# 报告"}), &sb, &cfg);
        assert!(w.0, "{}", w.1);
        let r = execute_call("read", &serde_json::json!({"path":"子/报告.md"}), &sb, &cfg);
        assert!(r.0);
        assert_eq!(r.1, "# 报告");
        let l = execute_call("list", &serde_json::json!({"path":"."}), &sb, &cfg);
        assert!(l.0 && l.1.contains("子/"));
        let bad = execute_call("read", &serde_json::json!({"path":"不存在.txt"}), &sb, &cfg);
        assert!(!bad.0);
    }

    // ── 全链路（mock SSE 编排：执行轮 → 完成宣布 → 核实动作 → 核验结论）──

    /// mock 服务器：每次 /chat/completions 请求从响应队列取下一段 SSE
    fn spawn_scripted(responses: Vec<String>) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let queue = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::from(responses)));
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut s = stream;
                let mut buf: Vec<u8> = Vec::new();
                let mut chunk = [0u8; 8192];
                let head_end = loop {
                    match s.read(&mut chunk) {
                        Ok(0) | Err(_) => break buf.len(),
                        Ok(n) => {
                            buf.extend_from_slice(&chunk[..n]);
                            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                break pos + 4;
                            }
                            if buf.len() > (1 << 20) {
                                break buf.len();
                            }
                        }
                    }
                };
                let head = String::from_utf8_lossy(&buf[..head_end]).to_lowercase();
                let len: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0)))
                    .unwrap_or(0);
                let mut have = buf.len() - head_end;
                while have < len {
                    match s.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => have += n,
                    }
                }
                let next = {
                    let q = queue.lock().unwrap();
                    q.front().cloned()
                };
                let body = next.unwrap_or_else(|| "data: [DONE]\n\n".to_string());
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes());
                let _ = s.flush();
                let _done = { queue.lock().unwrap().pop_front(); };
                let _ = _done;
            }
        });
        addr
    }

    fn sse_of(text: &str) -> String {
        let mut sse = String::new();
        for piece in split_pieces(text) {
            let v = serde_json::json!({"choices":[{"delta":{"content": piece}}]});
            sse.push_str(&format!("data: {}\n\n", v));
        }
        sse.push_str("data: [DONE]\n\n");
        sse
    }

    fn split_pieces(text: &str) -> Vec<String> {
    // 按字符边界切成小段，模拟逐 token 下发（不能按字节切，会切断 UTF-8）
    let chars: Vec<char> = text.chars().collect();
    let mut pieces: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        let j = (i + 24).min(chars.len());
        pieces.push(chars[i..j].iter().collect());
        i = j;
    }
    if pieces.is_empty() {
        pieces.push(String::new());
    }
    pieces
}

    #[test]
    fn full_loop_execute_verify_pass() {
        let dir = tempfile::tempdir().unwrap();
        let sb = crate::sandbox::Sandbox::resolve(dir.path());
        let addr = spawn_scripted(vec![
            sse_of("<SOPHON_CALL name=\"write\">{\"path\":\"结果.txt\",\"content\":\"任务产出42\"}</SOPHON_CALL><SOPHON_CALL name=\"exec\">{\"command\":\"echo step-done\"}</SOPHON_CALL>"),
            sse_of("<SOPHON_DONE>已写入结果文件并验证命令。</SOPHON_DONE>"),
            sse_of("<SOPHON_CALL name=\"read\">{\"path\":\"结果.txt\"}</SOPHON_CALL>"),
            sse_of("<SOPHON_VERIFY>{\"pass\":true,\"items\":[{\"item\":\"结果.txt 含任务产出42\",\"ok\":true,\"note\":\"read 核实\"}]}</SOPHON_VERIFY>"),
        ]);
        let api = Resolved {
            base_url: format!("http://127.0.0.1:{}", addr.port()),
            api_key: "k".to_string(),
            model: "m".to_string(),
            from_list: false,
        };
        let cfg = LoopConfig { max_rounds: 4, verify_rounds: 2, ..Default::default() };
        let o = run_task("写含任务产出42的文件", &api, "", &cfg, &sb, "").unwrap();
        assert!(o.ok, "{}", o.summary);
        assert_eq!(o.rounds_used, 2);
        assert!(sb.root.join("结果.txt").exists());
    }

    #[test]
    fn verify_fail_feeds_back_until_pass() {
        let dir = tempfile::tempdir().unwrap();
        let sb = crate::sandbox::Sandbox::resolve(dir.path());
        let addr = spawn_scripted(vec![
            sse_of("<SOPHON_DONE>自认为完成</SOPHON_DONE>"),
            sse_of("<SOPHON_VERIFY>{\"pass\":false,\"items\":[{\"item\":\"产物.txt\",\"ok\":false,\"note\":\"没写\"}]}</SOPHON_VERIFY>"),
            sse_of("<SOPHON_CALL name=\"write\">{\"path\":\"产物.txt\",\"content\":\"补上\"}</SOPHON_CALL>"),
            sse_of("<SOPHON_DONE>已补写</SOPHON_DONE>"),
            sse_of("<SOPHON_VERIFY>{\"pass\":true,\"items\":[{\"item\":\"产物.txt\",\"ok\":true,\"note\":\"ok\"}]}</SOPHON_VERIFY>"),
        ]);
        let api = Resolved {
            base_url: format!("http://127.0.0.1:{}", addr.port()),
            api_key: "k".to_string(),
            model: "m".to_string(),
            from_list: false,
        };
        let cfg = LoopConfig { max_rounds: 6, verify_rounds: 2, ..Default::default() };
        let o = run_task("创建产物.txt", &api, "", &cfg, &sb, "").unwrap();
        assert!(o.ok, "{}", o.summary);
        assert!(sb.root.join("产物.txt").exists());
    }

    #[test]
    fn max_rounds_reports_honest_failure() {
        let dir = tempfile::tempdir().unwrap();
        let sb = crate::sandbox::Sandbox::resolve(dir.path());
        let addr = spawn_scripted(vec![
            sse_of("再想想。"),
            sse_of("又想想。"),
            sse_of("还是想想。"),
        ]);
        let api = Resolved {
            base_url: format!("http://127.0.0.1:{}", addr.port()),
            api_key: "k".to_string(),
            model: "m".to_string(),
            from_list: false,
        };
        let cfg = LoopConfig { max_rounds: 2, verify_rounds: 1, ..Default::default() };
        let o = run_task("不可能的目标", &api, "", &cfg, &sb, "").unwrap();
        assert!(!o.ok, "达到最大轮数应如实报失败");
        assert!(o.summary.contains("最大轮数"), "{}", o.summary);
    }
}
