// 调研用命令行入口：直接驱动 sophon_core::web 模块（百度/Bing 搜索 + headless Edge 渲染），
// 把搜索结果与页面正文落盘为证据文件（UTF-8）。
// 用法：
//   sophon search   <bing|baidu> <query> <count> <outfile>
//   sophon fetch    <url> <outfile> [max_chars]
//   sophon get      <url> <outfile> [max_chars]
//   sophon research <question> <max_pages> <outfile>
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: sophon <search|fetch|get|research> ...");
        std::process::exit(2);
    }
    let r = match args[1].as_str() {
        "search" => {
            if args.len() < 6 {
                eprintln!("usage: sophon search <bing|baidu> <query> <count> <outfile>");
                std::process::exit(2);
            }
            let engine = &args[2];
            let query = &args[3];
            let count: usize = args[4].parse().unwrap_or(8);
            let outfile = &args[5];
            match sophon_core::web::search(engine, query, count) {
                Ok(hits) => {
                    let mut md = format!("# 搜索证据：{}（engine={}）\n\n", query, engine);
                    for (i, (t, u, s)) in hits.iter().enumerate() {
                        md.push_str(&format!("{}. [{}]({})\n   {}\n\n", i + 1, t, u, s));
                    }
                    write_out(outfile, &md)
                }
                Err(e) => {
                    eprintln!("search failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        "fetch" => {
            if args.len() < 4 {
                eprintln!("usage: sophon fetch <url> <outfile> [max_chars] [timeout_secs] [virtual_ms]");
                std::process::exit(2);
            }
            let url = &args[2];
            let outfile = &args[3];
            let max_chars: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(20000);
            let timeout: u64 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(90);
            let virtual_ms: u32 = args.get(6).and_then(|s| s.parse().ok()).unwrap_or(15000);
            eprintln!("fetching {} (timeout={}s virtual={}ms)...", url, timeout, virtual_ms);
            match sophon_core::web::fetch_rendered(url, virtual_ms, Duration::from_secs(timeout)) {
                Ok(html) => {
                    let text = sophon_core::web::extract_main_text(&html, max_chars);
                    let md = format!("# 页面证据：{}\n\n（headless Edge 渲染，抓取时间见文件名）\n\n{}", url, text);
                    write_out(outfile, &md)
                }
                Err(e) => {
                    eprintln!("fetch failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        "get" => {
            if args.len() < 4 {
                eprintln!("usage: sophon get <url> <outfile> [max_chars]");
                std::process::exit(2);
            }
            let url = &args[2];
            let outfile = &args[3];
            let max_chars: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(20000);
            eprintln!("http-get {}...", url);
            match sophon_core::web::http_get(url) {
                Ok(html) => {
                    let text = sophon_core::web::extract_main_text(&html, max_chars);
                    write_out(outfile, &text)
                }
                Err(e) => {
                    eprintln!("get failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        "research" => {
            if args.len() < 5 {
                eprintln!("usage: sophon research <question> <max_pages> <outfile>");
                std::process::exit(2);
            }            let q = &args[2];
            let pages: usize = args[3].parse().unwrap_or(3);
            let outfile = &args[4];
            match sophon_core::web::web_research(q, pages) {
                Ok(text) => write_out(outfile, &text),
                Err(e) => {
                    eprintln!("research failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        other => {
            eprintln!("unknown command: {}", other);
            std::process::exit(2);
        }
    };
    match r {
        Ok(n) => println!("saved {} chars", n),
        Err(e) => {
            eprintln!("write failed: {}", e);
            std::process::exit(1);
        }
    }
}

fn write_out(path: &str, text: &str) -> std::io::Result<usize> {
    if let Some(p) = std::path::Path::new(path).parent() {
        let _ = std::fs::create_dir_all(p);
    }
    std::fs::write(path, text)?;
    Ok(text.chars().count())
}
