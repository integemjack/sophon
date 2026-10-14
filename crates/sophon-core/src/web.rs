// 联网能力（用户需求：不调搜索 API——直接抓搜索引擎结果页 + headless 浏览器读 JS 渲染后的页面）。
// 渲染用本机 Edge/Chrome 的 headless --dump-dom：真实 Chromium 内核，JS 完全执行后输出 DOM，
// 零安装（Windows 自带 Edge），不额外占用下载/磁盘资源。
use std::io::Read;
use std::time::{Duration, Instant};

// ── 浏览器探测 ──────────────────────────────────────

/// ASCII 范围内的小写折叠（保字节长度：非 ASCII 原样），保证 lower 与原文索引一致
fn ascii_lower(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii() { c.to_ascii_lowercase() } else { c })
        .collect()
}

/// 找一个 Chromium 内核浏览器（headless 渲染用）
pub fn browser_path() -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("SOPHON_BROWSER") {
        if !p.is_empty() && std::path::Path::new(&p).exists() {
            return Some(std::path::PathBuf::from(p));
        }
    }
    for p in [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files\Chromium\Application\chrome.exe",
    ] {
        if std::path::Path::new(p).exists() {
            return Some(std::path::PathBuf::from(p));
        }
    }
    None
}

/// 带超时地跑一个进程，收集 stdout+stderr
fn run_with_timeout(cmd: &mut std::process::Command, timeout: Duration) -> Result<(String, String), String> {
    use std::process::Stdio;
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("启动失败：{}", e))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return Err(format!("超时（{} 秒）", timeout.as_secs()));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(format!("等待进程失败：{}", e)),
        }
    }
    let mut out = String::new();
    let mut err = String::new();
    if let Some(mut o) = child.stdout.take() {
        let _ = o.read_to_string(&mut out);
    }
    if let Some(mut e) = child.stderr.take() {
        let _ = e.read_to_string(&mut err);
    }
    Ok((out, err))
}

// ── headless 渲染抓取 ───────────────────────────────

/// 用 headless Chromium 内核浏览器加载页面（执行 JS）后返回完整 DOM。
/// virtual_time_ms：给页面 JS 的虚拟时间预算（毫秒），太少则异步内容没加载完。
pub fn fetch_rendered(url: &str, virtual_time_ms: u32, timeout: Duration) -> Result<String, String> {
    let browser = browser_path().ok_or_else(|| {
        "找不到 Edge/Chrome 浏览器（headless 渲染需要）。可设环境变量 SOPHON_BROWSER 指向浏览器路径".to_string()
    })?;
    let mut cmd = std::process::Command::new(browser);
    cmd.args([
        "--headless=new",
        "--disable-gpu",
        "--no-first-run",
        "--disable-extensions",
        "--mute-audio",
        "--no-sandbox",
        &format!("--virtual-time-budget={}", virtual_time_ms),
        &format!(
            "--user-agent=Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36"
        ),
        "--dump-dom",
        url,
    ]);
    let (out, _err) = run_with_timeout(&mut cmd, timeout)?;
    let t = out.trim();
    if t.is_empty() {
        return Err("页面返回空内容（网络不通或被拦截）".to_string());
    }
    Ok(out)
}

// ── 正文提取 ────────────────────────────────────────

/// 从渲染后的 HTML 提取正文文本：去 script/style/svg、留结构空白、解实体、压空白。
pub fn extract_main_text(html: &str, max_chars: usize) -> String {
    let mut s = html.to_string();
    // 去掉不可见/无关块
    for tag in ["script", "style", "noscript", "svg", "template", "head", "iframe", "canvas"] {
        s = remove_tag_blocks(&s, tag);
    }
    s = remove_comments(&s);
    // 块级元素转换行，保持段落结构
    for t in [
        "br", "p", "div", "li", "tr", "h1", "h2", "h3", "h4", "h5", "h6", "section", "article",
    ] {
        s = replace_tag_with(&s, t, "\n");
    }
    let text = strip_tags(&s);
    // 压缩空白：每行 trim，去掉连续空行
    let mut lines: Vec<String> = Vec::new();
    for l in text.lines() {
        let l = l.trim();
        if l.is_empty() {
            if lines.last().map(|p: &String| !p.is_empty()).unwrap_or(false) {
                lines.push(String::new());
            }
        } else {
            lines.push(l.to_string());
        }
    }
    let joined = lines.join("\n");
    if joined.chars().count() <= max_chars {
        joined
    } else {
        format!("{}…（已截断）", joined.chars().take(max_chars).collect::<String>())
    }
}

fn remove_tag_blocks(s: &str, tag: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let lower = ascii_lower(s);
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let mut i = 0usize;
    while i < lower.len() {
        // 按字节找（lower 与 s 同长度：to_lowercase 对 ASCII 长度不变；非 ASCII 标签名不存在）
        match lower[i..].find(&open) {
            Some(p) if p == 0 || !is_tag_char(lower.as_bytes()[i + p + open.len()..].first().copied().unwrap_or(b'>')) => {
                let abs = i + p;
                out.push_str(&s[i..abs]);
                match lower[abs..].find(&close) {
                    Some(q) => {
                        i = abs + q + close.len();
                    }
                    None => {
                        i = s.len();
                    }
                }
            }
            _ => {
                out.push_str(&s[i..]);
                break;
            }
        }
    }
    out
}

fn is_tag_char(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

fn remove_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find("<!--") {
        out.push_str(&rest[..p]);
        rest = match rest[p..].find("-->") {
            Some(q) => &rest[p + q + 3..],
            None => "",
        };
        if rest.is_empty() {
            break;
        }
    }
    out.push_str(rest);
    out
}

fn replace_tag_with(s: &str, tag: &str, to: &str) -> String {
    let lower = ascii_lower(s);
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < lower.len() {
        let open = format!("<{}", tag);
        match lower[i..].find(&open) {
            Some(p) => {
                let abs = i + p;
                out.push_str(&s[i..abs]);
                match lower[abs..].find('>') {
                    Some(q) => {
                        out.push_str(to);
                        i = abs + q + 1;
                    }
                    None => {
                        out.push_str(&s[abs..]);
                        break;
                    }
                }
            }
            None => {
                out.push_str(&s[i..]);
                break;
            }
        }
    }
    out
}

// ── URL / HTML 辅助（从 tools/builtin.rs 提升，双方共用）──

/// 百分号编码（UTF-8）
pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// 去 HTML 标签 + 解码常见实体
pub fn strip_tags(s: &str) -> String {
    let t = strip_tags_raw(s);
    t.replace("&quot;", "\"")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_tags_raw(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

// ── 搜索（抓搜索结果页，不用搜索 API）────────────────

pub type SearchHit = (String, String, String);

/// Bing 结果解析（页面 li.b_algo）
pub fn parse_bing_results(html: &str, max: usize) -> Vec<SearchHit> {
    let mut out = Vec::new();
    let lower = ascii_lower(html);
    let mut i = 0usize;
    while out.len() < max {
        let some = match lower[i..].find("<li class=\"b_algo") {
            Some(p) => i + p,
            None => break,
        };
        let end = match lower[some..].find("</li>") {
            Some(e) => some + e + 5,
            None => break,
        };
        let block = &html[some..end.min(html.len())];
        if let Some(hit) = parse_bing_item(block) {
            out.push(hit);
        }
        i = end;
    }
    out
}

fn parse_bing_item(block: &str) -> Option<SearchHit> {
    let h2 = block.find("<h2")?;
    let a_start = block[h2..].find("<a ")?;
    let href_at = block[h2 + a_start..].find("href=\"")? + h2 + a_start + 6;
    let href_end = block[href_at..].find('"')? + href_at;
    let url = block[href_at..href_end].to_string();
    let gt = block[href_end..].find('>')? + href_end + 1;
    let title_end = block[gt..].find("</a>")? + gt;
    let title = strip_tags(&block[gt..title_end]);
    let snippet_start = block.find("<p").map(|p| block[p..].find('>').map(|q| p + q + 1)).flatten().unwrap_or(0);
    let snippet_end = block[snippet_start..].find("</p>").map(|e| snippet_start + e).unwrap_or(snippet_start);
    let snippet = strip_tags(&block[snippet_start..snippet_end]);
    if url.is_empty() || title.is_empty() {
        return None;
    }
    Some((title, url, snippet))
}

/// 百度结果解析（div.result / div.c-container，h3 a）
pub fn parse_baidu_results(html: &str, max: usize) -> Vec<SearchHit> {
    let mut out = Vec::new();
    let lower = ascii_lower(html);
    let mut i = 0usize;
    while out.len() < max {
        let p1 = match lower[i..].find("<div class=\"result") {
            Some(p) => i + p,
            None => match lower[i..].find("<div class=\"c-container") {
                Some(p) => i + p,
                None => break,
            },
        };
        let end = match lower[p1..].find("</div>") {
            // 结果块内嵌套 div，粗略取到下一个结果开头或固定窗口
            Some(_) => {
                let next = lower[p1 + 10..].find("<div class=\"result").or_else(|| lower[p1 + 10..].find("<div class=\"c-container")).map(|p| p1 + 10 + p);
                next.unwrap_or((p1 + 20000).min(html.len()))
            }
            None => break,
        };
        let block = &html[p1..end.min(html.len())];
        if let Some(hit) = parse_baidu_item(block) {
            out.push(hit);
        }
        i = p1 + 10;
    }
    out
}

fn parse_baidu_item(block: &str) -> Option<SearchHit> {
    let h3 = block.find("<h3")?;
    let a_at = block[h3..].find("href=\"")? + h3 + 6;
    let a_end = block[a_at..].find('"')? + a_at;
    let url = block[a_at..a_end].to_string();
    let gt = block[a_end..].find('>')? + a_end + 1;
    let title_end = block[gt..].find("</a>").map(|e| gt + e).unwrap_or((gt + 200).min(block.len()));
    let title = strip_tags(&block[gt..title_end]);
    // 摘要：取块里第一段较长文本
    let snippet = {
        let t = strip_tags(&block[title_end..]);
        let t = t.trim();
        if t.chars().count() > 40 {
            t.chars().take(160).collect()
        } else {
            String::new()
        }
    };
    if url.is_empty() || title.is_empty() {
        return None;
    }
    Some((title, url, snippet))
}

/// 搜索（Bing 为主，百度补充；去重）。engine:"bing"/"baidu"
pub fn search(engine: &str, query: &str, count: usize) -> Result<Vec<SearchHit>, String> {
    let (html, parse): (String, fn(&str, usize) -> Vec<SearchHit>) = match engine {
        "baidu" => {
            let url = format!("https://www.baidu.com/s?wd={}&rn={}", urlencode(query), count.max(10));
            let html = http_get(&url)?;
            (html, parse_baidu_results)
        }
        _ => {
            let url = format!(
                "https://www.bing.com/search?q={}&mkt=zh-CN&count={}",
                urlencode(query),
                count
            );
            let html = http_get(&url)?;
            (html, parse_bing_results)
        }
    };
    let hits = parse(&html, count);
    if hits.is_empty() {
        return Err(format!("{} 搜索无结果或页面结构变化（query={}）", engine, query));
    }
    Ok(hits)
}

/// 简单 HTTP GET（静态抓取；重搜索页用 fetch_rendered）
pub fn http_get(url: &str) -> Result<String, String> {
    let resp = ureq::get(url)
        .set(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36",
        )
        .set("Accept-Language", "zh-CN,zh;q=0.9")
        .timeout(Duration::from_secs(15))
        .call();
    match resp {
        Ok(r) => {
            let mut s = String::new();
            r.into_reader()
                .take(3_000_000)
                .read_to_string(&mut s)
                .map_err(|e| format!("读取失败：{}", e))?;
            Ok(s)
        }
        Err(e) => Err(format!("请求失败：{}", e)),
    }
}

// ── 自动联网调研（用户需求核心流程）──────────────────

/// 根据问题自动：搜索 → 选相关页 → headless 渲染读取 → 汇总为上下文文本。
/// max_pages：读取几个页面（每页 headless 启动+渲染约 3-8 秒）。
pub fn web_research(question: &str, max_pages: usize) -> Result<String, String> {
    // 1. 搜索（Bing + 百度并集去重）
    let mut hits: Vec<SearchHit> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for eng in ["bing", "baidu"] {
        if let Ok(mut r) = search(eng, question, max_pages + 2) {
            for h in r.drain(..) {
                if seen.insert(h.1.clone()) {
                    hits.push(h);
                }
            }
        }
    }
    if hits.is_empty() {
        return Err(format!("「{}」搜索无结果（两个引擎都失败，请检查网络）", question));
    }

    // 2. 逐个 headless 读取（跳过搜索引擎自身链接），汇总
    let mut out = format!("联网调研「{}」：\n\n=== 搜索结果 ===\n", question);
    for (i, (title, url, snippet)) in hits.iter().enumerate() {
        out.push_str(&format!("{}. {}\n   {}\n   {}\n", i + 1, title, url, snippet));
    }
    let mut read_count = 0usize;
    for (title, url, _) in hits.iter() {
        if read_count >= max_pages {
            break;
        }
        if url.contains("bing.com") || url.contains("baidu.com") {
            continue;
        }
        out.push_str(&format!("\n=== 页面：{}（{}）===\n", title, url));
        match fetch_rendered(url, 6000, Duration::from_secs(30)) {
            Ok(html) => {
                out.push_str(&extract_main_text(&html, 4000));
                out.push('\n');
            }
            Err(e) => out.push_str(&format!("（读取失败：{}）\n", e)),
        }
        read_count += 1;
    }
    if read_count == 0 {
        out.push_str("\n（未读取到页面内容：搜索结果都不可访问。上面的摘要仍可作为参考）");
    }
    Ok(out)
}

// ── 测试 ────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_strips_scripts_and_keeps_text() {
        let html = r#"<html><head><style>.x{color:red}</style><script>var a=1;</script></head>
        <body><nav>导航</nav><article><h1>标题</h1><p>第一段内容</p><p>第二段</p></article><script>bad()</script></body></html>"#;
        let t = extract_main_text(html, 500);
        assert!(t.contains("标题"));
        assert!(t.contains("第一段内容"));
        assert!(!t.contains("var a=1"));
        assert!(!t.contains("color:red"));
        assert!(!t.contains("bad()"));
    }

    #[test]
    fn extract_truncates() {
        let long = format!("<p>{}</p>", "字".repeat(1000));
        let t = extract_main_text(&long, 100);
        assert!(t.chars().count() < 120);
        assert!(t.ends_with("…（已截断）"));
    }

    #[test]
    fn bing_parser_works() {
        let html = r#"<li class="b_algo"><h2><a href="https://a.example">标题A</a></h2><p>摘要内容A</p></li>
        <li class="b_algo"><h2><a href="https://b.example">标题B</a></h2><p>摘要B</p></li>"#;
        let r = parse_bing_results(html, 5);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].0, "标题A");
        assert_eq!(r[0].1, "https://a.example");
    }

    #[test]
    fn baidu_parser_works() {
        let html = r#"<div class="result c-container new-pmd" srcid="1599"><h3 class="c-title t t tts-title"><a href="http://www.baidu.com/link?url=xyz">百度标题</a></h3><div class="c-row"><span>这是百度结果的摘要文本，足够长以便被提取出来作为摘要展示使用。</span></div></div>
        <div class="result c-container"><h3><a href="http://www.baidu.com/link?url=abc">第二个</a></h3><span>第二个结果摘要也要足够长才能被当成摘要提取出来显示。</span></div>"#;
        let r = parse_baidu_results(html, 5);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].0, "百度标题");
        assert!(r[0].1.starts_with("http://www.baidu.com/link"));
    }

    #[test]
    fn urlencode_matches() {
        assert_eq!(urlencode("智子"), "%E6%99%BA%E5%AD%90");
    }

    #[test]
    #[ignore] // 真实联网 + headless：cargo test -p sophon-core web_live -- --ignored
    fn web_research_live() {
        let out = web_research("rust programming language", 2).expect("调研应成功");
        assert!(out.contains("搜索结果"));
        assert!(out.contains("=== 页面："));
    }

    #[test]
    #[ignore]
    fn fetch_rendered_live() {
        let html = fetch_rendered("https://example.com", 5000, Duration::from_secs(20)).expect("渲染抓取应成功");
        assert!(html.contains("Example Domain") || html.contains("文档示例"));
    }
}
