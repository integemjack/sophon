# -*- coding: utf-8 -*-
# r3: remote_api.rs += tests for chat_stream / marks / append_verified
from pathlib import Path
p = Path(r"C:\Users\ultra\Desktop\works\hermes\src\remote_api.rs")
t = p.read_text(encoding="utf-8")

TESTS = r'''

#[cfg(test)]
mod stream_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::SocketAddr;
    use std::time::Duration;

    /// mock HTTP：自定义 Content-Type；读完请求头并按 Content-Length 读掉 body 后回固定响应
    fn spawn_mock_ct(ct: &'static str, body: &'static str) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut s = stream;
                let mut buf: Vec<u8> = Vec::new();
                let mut chunk = [0u8; 8192];
                // 读到头结束
                let head_end = loop {
                    match s.read(&mut chunk) {
                        Ok(0) | Err(_) => break buf.len(),
                        Ok(n) => {
                            buf.extend_from_slice(&chunk[..n]);
                            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                break pos + 4;
                            }
                            if buf.len() > 1 << 20 {
                                break buf.len();
                            }
                        }
                    }
                };
                // 按 Content-Length 读掉 body（POST 有体；不读会 RST 吞响应）
                let head = String::from_utf8_lossy(&buf[..head_end]).to_lowercase();
                let clen: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0)))
                    .unwrap_or(0);
                let mut have = buf.len() - head_end;
                while have < clen {
                    match s.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            have += n;
                        }
                    }
                }
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    ct,
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes());
                let _ = s.flush();
            }
        });
        addr
    }

    const SSE_BODY: &str = "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"想\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\"你\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\"好\"}}]}\n\n\
data: [DONE]\n\n";

    #[test]
    fn sse_delta_parse() {
        let (c, r) = parse_sse_delta(
            "data: {\"choices\":[{\"delta\":{\"content\":\"你\"}}]}",
        ).unwrap();
        assert_eq!(c.as_deref(), Some("你"));
        assert!(r.is_none());

        let (c, r) = parse_sse_delta(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"想\"}}]}",
        ).unwrap();
        assert!(c.is_none());
        assert_eq!(r.as_deref(), Some("想"));

        // role-only delta（无文本）
        assert!(parse_sse_delta("data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}").is_none());
        // 结束标记
        assert!(parse_sse_delta("data: [DONE]").is_none());
        // 非 data 行 / 坏 JSON
        assert!(parse_sse_delta(": keep-alive").is_none());
        assert!(parse_sse_delta("data: {bad").is_none());
    }

    #[test]
    fn chat_stream_receives_deltas_in_order() {
        let addr = spawn_mock_ct("text/event-stream", SSE_BODY);
        let url = format!("http://127.0.0.1:{}", addr.port());
        let mut got: Vec<(String, bool)> = Vec::new();
        let full = chat_stream(&url, "k", "m", "", "hi", Duration::from_secs(5), &mut |t, r| {
            got.push((t.to_string(), r));
        })
        .unwrap();
        assert_eq!(full, "你好", "只累积正文");
        assert_eq!(
            got,
            vec![("想".to_string(), true), ("你".to_string(), false), ("好".to_string(), false)],
            "思考先于正文、逐段回调"
        );
    }

    #[test]
    fn chat_stream_falls_back_when_not_sse() {
        let addr = spawn_mock_ct(
            "application/json",
            "{\"choices\":[{\"message\":{\"content\":\"整段回答\"}}]}",
        );
        let url = format!("http://127.0.0.1:{}", addr.port());
        let mut got: Vec<(String, bool)> = Vec::new();
        let full = chat_stream(&url, "k", "m", "", "hi", Duration::from_secs(5), &mut |t, r| {
            got.push((t.to_string(), r));
        })
        .unwrap();
        assert_eq!(full, "整段回答");
        assert_eq!(got, vec![("整段回答".to_string(), false)]);
    }

    #[test]
    fn append_marks_extraction() {
        let t = "前言\n<SOPHON_APPEND>http://a/v1 k1 m1</SOPHON_APPEND>\n中间\n<SOPHON_APPEND>http://b/v1  k2</SOPHON_APPEND>\n<SOPHON_APPEND></SOPHON_APPEND>\n<SOPHON_APPEND>没闭合";
        assert_eq!(
            parse_append_marks(t),
            vec!["http://a/v1 k1 m1".to_string(), "http://b/v1  k2".to_string()],
            "按序提取、空内容与未闭合丢弃"
        );
        assert!(parse_append_marks("没有任何标记").is_empty());
    }

    #[test]
    fn append_verified_flows() {
        let live = spawn_mock("200 OK", "{\"data\":[{\"id\":\"m-1\"}]}");
        let dead = spawn_mock("404 Not Found", "no");
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(SUB_FILE_NAME);

        // 可用 → 追加（文件原本不存在 → 创建）
        let live_line = format!("http://127.0.0.1:{} k-live", live.port());
        let res = append_verified(&p, &[live_line.clone()], MODEL_PROBE_TIMEOUT);
        assert_eq!(res[0].1, AppendOutcome::Added);
        assert!(p.exists(), "文件不存在应被创建");
        assert!(std::fs::read_to_string(&p).unwrap().contains(&live_line));

        // 同一条再来 → 已存在跳过（不重复写）
        let res = append_verified(&p, &[live_line.clone()], MODEL_PROBE_TIMEOUT);
        assert_eq!(res[0].1, AppendOutcome::Exists);
        let after = std::fs::read_to_string(&p).unwrap();
        assert_eq!(after.lines().filter(|l| *l == live_line).count(), 1, "不写重复行");

        // 拉不到模型列表 → 拒绝且不写入
        let dead_line = format!("http://127.0.0.1:{} k-dead", dead.port());
        let res = append_verified(&p, &[dead_line], MODEL_PROBE_TIMEOUT);
        assert!(matches!(res[0].1, AppendOutcome::Rejected(_)));
        assert!(!std::fs::read_to_string(&p).unwrap().contains("k-dead"));

        // 格式不对 → 拒绝
        let res = append_verified(&p, &["只有一列".to_string()], MODEL_PROBE_TIMEOUT);
        assert!(matches!(res[0].1, AppendOutcome::Rejected(_)));

        // 多条混合：顺序保持
        let res = append_verified(
            &p,
            &["只有一列".to_string(), live_line.clone()],
            MODEL_PROBE_TIMEOUT,
        );
        assert!(matches!(res[0].1, AppendOutcome::Rejected(_)));
        assert_eq!(res[1].1, AppendOutcome::Exists);
    }

    #[test]
    fn tool_note_mentions_path_and_protocol() {
        let note = append_tool_note("C:\\sophon\\sophon.sub");
        assert!(note.contains("C:\\sophon\\sophon.sub"));
        assert!(note.contains(APPEND_OPEN) && note.contains(APPEND_CLOSE));
    }
}
'''
p.write_text(t + TESTS, encoding="utf-8", newline="\n")
print("tests appended")
