// 自动压缩上下文（九条准则之一）：
// 对话历史超过阈值时自动把较早消息合并成摘要，回合不因上下文膨胀而失败。
// 摘要是机械的（角色 + 长度 + 首行），不依赖模型；完整历史仍在会话存档里。
use crate::models::{BlockKind, ChatMessage, Role};

/// 总字符量阈值：超过即压缩
pub const COMPRESS_THRESHOLD: usize = 24_000;
/// 压缩时保留最近几条消息
pub const KEEP_RECENT: usize = 12;
/// 压缩后仍超限时，单条工具结果保留的字符数
pub const TOOL_RESULT_LIMIT: usize = 2_000;

pub fn total_chars(messages: &[ChatMessage]) -> usize {
    messages.iter().map(|m| m.text_content().chars().count()).sum()
}

fn cut(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{}…", head)
    }
}

/// 需要时压缩；返回 Some(说明) 表示发生了压缩。
pub fn compress_if_needed(messages: &mut Vec<ChatMessage>) -> Option<String> {
    if total_chars(messages) <= COMPRESS_THRESHOLD {
        return None;
    }
    let n = messages.len();
    if n < 4 {
        return None; // 太短没得压
    }
    let keep_tail = KEEP_RECENT.min(n - 1);
    let cut_at = n - keep_tail; // [0..cut_at) 压缩掉，[cut_at..] 保留
    let dropped: Vec<ChatMessage> = messages.drain(..cut_at).collect();
    let mut lines: Vec<String> = Vec::new();
    for m in &dropped {
        let role = match m.role {
            Role::User => "用户",
            Role::Assistant => "智子",
            _ => "工具",
        };
        let text = m.text_content();
        let first_line = text.lines().next().unwrap_or("");
        lines.push(format!("- {}({}字): {}", role, text.chars().count(), cut(first_line, 120)));
    }
    let summary = format!(
        "〔上下文已自动压缩〕较早的 {} 条消息合并为摘要:\n{}\n完整历史保存在会话存档中。继续完成当前任务。",
        dropped.len(),
        lines.join("\n")
    );
    let dropped_count = dropped.len();
    messages.insert(0, ChatMessage::user(summary));
    // 压缩后仍超限 → 截断保留段里的超长文本块
    if total_chars(messages) > COMPRESS_THRESHOLD {
        for m in messages.iter_mut() {
            for b in m.blocks.iter_mut() {
                match &mut b.kind {
                    BlockKind::Tool { tool } => {
                        if let Some(r) = &tool.result {
                            if r.chars().count() > TOOL_RESULT_LIMIT {
                                tool.result = Some(cut(r, TOOL_RESULT_LIMIT));
                            }
                        }
                    }
                    BlockKind::Text { text } => {
                        if text.chars().count() > TOOL_RESULT_LIMIT * 2 {
                            *text = cut(text, TOOL_RESULT_LIMIT * 2);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    Some(format!("已压缩 {} 条旧消息为 1 条摘要", dropped_count))
}

#[cfg(test)]
mod tests {
    use crate::models::*;
    use super::*;

    #[test]
    fn no_compress_when_small() {
        let mut msgs = vec![ChatMessage::user("hi"), ChatMessage::assistant()];
        msgs[1].blocks = vec![MessageBlock::text("hello".into())];
        assert!(compress_if_needed(&mut msgs).is_none());
        assert_eq!(msgs.len(), 2);
    }

    #[test]
    fn compresses_when_huge() {
        let mut msgs: Vec<ChatMessage> = Vec::new();
        msgs.push(ChatMessage::user("任务目标:测试压缩"));
        for i in 0..40 {
            let mut a = ChatMessage::assistant();
            a.blocks = vec![MessageBlock::text(format!("第{}轮输出: {}", i, "x".repeat(900)))];
            msgs.push(a);
            let mut t = ChatMessage::tool();
            t.blocks = vec![MessageBlock::tool(ToolEvent::new(
                "exec", serde_json::json!({"command": "echo"}), Some("执行".into()),
            ))];
            if let BlockKind::Tool { tool } = &mut t.blocks[0].kind {
                tool.result = Some(format!("结果{}: {}", i, "y".repeat(900)));
            }
            msgs.push(t);
        }
        assert!(total_chars(&msgs) > COMPRESS_THRESHOLD);
        let note = compress_if_needed(&mut msgs);
        assert!(note.is_some(), "超限必须触发压缩");
        assert!(total_chars(&msgs) < 14_000, "压缩后必须显著变小");
        // 首条应是压缩摘要，末尾保留最近消息
        assert!(msgs[0].text_content().contains("自动压缩"));
        assert_eq!(msgs.len(), 1 + KEEP_RECENT);
    }

    #[test]
    fn idempotent_after_compress() {
        let mut msgs: Vec<ChatMessage> = Vec::new();
        for i in 0..40 {
            let mut a = ChatMessage::assistant();
            a.blocks = vec![MessageBlock::text(format!("{}: {}", i, "z".repeat(900)))];
            msgs.push(a);
        }
        assert!(compress_if_needed(&mut msgs).is_some());
        let size_after = total_chars(&msgs);
        // 压缩一次后再调用：不再继续压（在阈值内）
        if size_after <= COMPRESS_THRESHOLD {
            assert!(compress_if_needed(&mut msgs).is_none());
        }
    }
}
