# -*- coding: utf-8 -*-
# 重写 agent_loop.rs 第 403 行之后：chat + run_task（状态机，标签作用域正确）
from pathlib import Path
p = Path("src/agent_loop.rs")
lines = p.read_text(encoding="utf-8").splitlines()
HEAD = "\n".join(lines[:402])  # 保留 1-402 行

TAIL = r'''
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
            print!("{}", t);
            use std::io::Write as _;
            let _ = std::io::stdout().flush();
            acc.push_str(t);
        },
    )?;
    println!();
    Ok(acc)
}

/// 进度输出走 stderr（stdout 留给模型正文，管道友好）
macro_rules! prog {
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
                prog!("── [第 {}/{} 轮] ──", round, cfg.max_rounds);
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
                let calls = parse_calls(&text);
                if calls.is_empty() {
                    prog!("（本轮没有动作标记）");
                    steps.push(Step {
                        round,
                        tool: "（无动作）".into(),
                        args_brief: String::new(),
                        ok: false,
                        result: "模型没有输出动作标记，已提醒其输出 SOPHON_CALL 或 SOPHON_DONE".into(),
                    });
                }
                for (name, args, _raw) in &calls {
                    prog!("→ {} {}", name, args_brief(args));
                    let (good, result) = execute_call(name, args, sandbox, cfg);
                    prog!(
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
                prog!("── 验证（{}/{}）──", verify_used, cfg.verify_rounds);
                // 验证轮允许模型先做核实动作
                let probe = chat_call(
                    api,
                    &system_prompt,
                    &build_verify_msg(goal, &steps, &done_summary),
                    cfg,
                )?;
                for (name, args, _raw) in &parse_calls(&probe) {
                    prog!("→ 核实动作：{} {}", name, args_brief(args));
                    let (good, result) = execute_call(name, args, sandbox, cfg);
                    prog!(
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
                // 带着核实结果要最终核验结论
                let verdict = chat_call(
                    api,
                    &system_prompt,
                    &build_verify_msg(goal, &steps, &done_summary),
                    cfg,
                )?;
                match parse_verify(&verdict) {
                    Some((pass, items)) => {
                        verify_items = items;
                        if pass || verify_items.iter().all(|i| i.ok) {
                            passed = true;
                            break 'outer;
                        }
                        let failed: Vec<VerifyItem> =
                            verify_items.iter().filter(|i| !i.ok).cloned().collect();
                        prog!("验证未通过（{} 项），继续执行：", failed.len());
                        for f in &failed {
                            prog!("  ✗ {}（{}）", f.item, f.note);
                        }
                        pending_retry = Some(build_retry_msg(&failed));
                        phase = Phase::Execute;
                        round += 1;
                    }
                    None => {
                        prog!("（验证轮没有可解析的 SOPHON_VERIFY，按未验证处理）");
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
'''

p.write_text(HEAD + "\n" + TAIL, encoding="utf-8", newline="\n")
print("rewritten tail, total lines:", len((HEAD + TAIL).splitlines()))
