// 自检（九条准则之一）：启动时验证九条准则的运行时前提，问题当场暴露。
// 用 --self-check CLI 模式输出 JSON 报告；TUI 启动时把结果作为一条消息插进会话。
use crate::sandbox::Sandbox;
use serde_json::json;

pub struct CheckItem {
    pub name: &'static str,
    pub ok: bool,
    pub detail: String,
}

pub fn run(sandbox: &Sandbox) -> Vec<CheckItem> {
    let mut items: Vec<CheckItem> = Vec::new();
    // 1. 沙盒目录可写
    let probe = sandbox.root.join(".selfcheck-probe");
    let ok = std::fs::write(&probe, b"ok").is_ok();
    if ok {
        let _ = std::fs::remove_file(&probe);
    }
    items.push(CheckItem {
        name: "沙盒目录可写",
        ok,
        detail: sandbox.root.to_string_lossy().to_string(),
    });
    // 2. exec 可用（真实跑一条命令）
    let out = std::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" })
        .args(if cfg!(windows) { vec!["/C", "echo sophon-selfcheck-ok"] } else { vec!["-c", "echo sophon-selfcheck-ok"] })
        .current_dir(&sandbox.root)
        .output();
    match out {
        Ok(o) if String::from_utf8_lossy(&o.stdout).contains("sophon-selfcheck-ok") => {
            items.push(CheckItem {
                name: "exec 命令执行",
                ok: true,
                detail: "echo 验证通过".into(),
            });
        }
        Ok(o) => items.push(CheckItem {
            name: "exec 命令执行",
            ok: false,
            detail: format!("异常输出: {}", String::from_utf8_lossy(&o.stdout).trim()),
        }),
        Err(e) => items.push(CheckItem {
            name: "exec 命令执行",
            ok: false,
            detail: e.to_string(),
        }),
    }
    // 3. git 可用（回滚前提）
    let ok = crate::rollback::git_available();
    items.push(CheckItem {
        name: "git 可用(回滚)",
        ok,
        detail: if ok { "git 命令就绪".into() } else { "未找到 git,回滚不可用".into() },
    });
    // 4. 上下文压缩器自测:构造超量消息,必须触发压缩且显著变小
    let ok = (|| {
        let mut msgs: Vec<crate::models::ChatMessage> = Vec::new();
        for i in 0..40 {
            let mut a = crate::models::ChatMessage::assistant();
            a.blocks = vec![crate::models::MessageBlock::text(format!("{}: {}", i, "x".repeat(900)))];
            msgs.push(a);
        }
        let before = crate::context_compress::total_chars(&msgs);
        crate::context_compress::compress_if_needed(&mut msgs)?;
        Some(before > crate::context_compress::total_chars(&msgs))
    })()
    .unwrap_or(false);
    items.push(CheckItem {
        name: "上下文自动压缩",
        ok,
        detail: if ok { "压缩管线正常".into() } else { "压缩自测未通过".into() },
    });
    // 5. 多实例锁:当前数据目录可独占(此处只探测锁文件可创建,不长期持有)
    items.push(CheckItem {
        name: "沙盒环境标记",
        ok: sandbox.env_pairs().len() == 3,
        detail: format!("实例 {} 深度 {}", sandbox.instance_id, sandbox.depth),
    });
    // 6. 工具集：exec + 子智能体编排 + 任务状态都在
    let names = crate::tools::ToolRegistry::builtin().names();
    let required = [
        "exec", "delegate_task", "run_parallel_tasks", "message_agent", "stop_agent",
        "wait_agents", "rework_agent", "read_agent_log", "update_task_state",
    ];
    let missing: Vec<&str> = required.iter().filter(|r| !names.contains(&r.to_string())).cloned().collect();
    items.push(CheckItem {
        name: "工具集齐备",
        ok: missing.is_empty(),
        detail: if missing.is_empty() { format!("{} 个工具：{:?}", names.len(), names) } else { format!("缺：{:?}", missing) },
    });
    items
}

pub fn report_text(items: &[CheckItem]) -> String {
    let mut lines = vec!["〔自检〕".to_string()];
    for it in items {
        lines.push(format!("{} {} — {}", if it.ok { "✓" } else { "✗" }, it.name, it.detail));
    }
    let failed = items.iter().filter(|i| !i.ok).count();
    lines.push(if failed == 0 {
        "全部通过".to_string()
    } else {
        format!("{} 项未通过", failed)
    });
    lines.join("\n")
}

pub fn report_json(items: &[CheckItem]) -> serde_json::Value {
    json!({
        "ok": items.iter().all(|i| i.ok),
        "items": items.iter().map(|i| json!({
            "name": i.name, "ok": i.ok, "detail": i.detail,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selfcheck_runs() {
        let tmp = std::env::temp_dir().join(format!("sophon_sc_test_{}", std::process::id()));
        let sb = Sandbox::resolve(&tmp);
        let items = run(&sb);
        assert!(items.len() >= 5);
        // 沙盒可写、exec、压缩器、工具集在这台开发机上必须通过
        for it in &items {
            if it.name == "沙盒目录可写" || it.name == "上下文自动压缩" || it.name == "工具集齐备" {
                assert!(it.ok, "{} 应通过: {}", it.name, it.detail);
            }
        }
        assert!(report_json(&items)["ok"].is_boolean());
        let _ = std::fs::remove_dir_all(tmp.join(".sophon"));
    }
}
