// 调研用命令行入口：直接驱动 sophon_core::web 模块（百度/Bing 搜索 + headless Edge 渲染），
// 把搜索结果与页面正文落盘为证据文件（UTF-8）。
// 用法：
//   sophon search   <bing|baidu> <query> <count> <outfile>
//   sophon fetch    <url> <outfile> [max_chars]
//   sophon get      <url> <outfile> [max_chars]
//   sophon research <question> <max_pages> <outfile>
//   sophon ask      <prompt>
//   sophon tools    （列出已创建的脚本工具）
// 启动时固定读桌面 sophon 目录（SOPHON_DIR 可换根，与从哪里启动无关）：
//   sophon.sub——远程 API 清单，从最后一行往上试，拉不到模型列表的行确认后删除；
//   sophon.prompt——系统提示词，没有则写入内置提示词，有则读取使用；
// 核心在哪个目录运行，工作环境（产物落盘）就在哪个目录。
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: sophon <search|fetch|get|research|ask|tools> ...");
        std::process::exit(2);
    }
    // 提示词：桌面 sophon 目录的 sophon.prompt 不存在则写入内置提示词，存在则读入使用
    let system_prompt = match sophon_core::remote_api::ensure_prompt() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("warning: {}", e);
            String::new()
        }
    };
    // 远程 API 清单：桌面 sophon 目录的 sophon.sub，从最后一行往上试；
    // 拉不到模型列表的行确认后从文件删除，再试上一行；全部失效则告警但不阻断联网研究。
    let remote = match sophon_core::remote_api::resolve_default() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("warning: {}", e);
            None
        }
    };
    if let Some(r) = &remote {
        eprintln!("remote api: {} | 模型: {}（{}）", r.base_url, r.model, if r.from_list { "清单内版本号最大" } else { "行内指定" });
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
        "ask" => {
            let Some(api) = remote.as_ref() else {
                eprintln!("ask 需要远程 API：请在桌面 sophon 目录创建 sophon.sub");
                eprintln!("每行一条：<api地址> <api_key> [模型名]（指定模型名则不自动选版本号最大者）");
                std::process::exit(2);
            };
            if args.len() < 3 {
                eprintln!("usage: sophon ask <prompt>");
                std::process::exit(2);
            }
            let prompt = args[2..].join(" ");
            eprintln!("ask {}（模型 {}）...", api.base_url, api.model);
            // 注入清单文件路径与追加协议：模型输出 <SOPHON_APPEND> 标记，本机验证后自动入库
            let sys = match sophon_core::remote_api::sub_file() {
                Some(pf) => format!(
                    "{}\n{}",
                    system_prompt,
                    sophon_core::remote_api::append_tool_note(&pf.display().to_string())
                ),
                None => system_prompt,
            };
            use std::io::Write as _;
            // 流式：每收到一段立即打印（思考与正文都实时可见），完成后解析标记入库
            let full = match sophon_core::remote_api::chat_stream(
                &api.base_url,
                &api.api_key,
                &api.model,
                &sys,
                &prompt,
                sophon_core::remote_api::CHAT_TIMEOUT,
                &mut |t, _reasoning| {
                    print!("{}", t);
                    let _ = std::io::stdout().flush();
                },
            ) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("\nask failed: {}", e);
                    std::process::exit(1);
                }
            };
            println!();
            let marks = sophon_core::remote_api::parse_append_marks(&full);
            if !marks.is_empty() {
                eprintln!("--- 远程 API 入库 ---");
                if let Some(pf) = sophon_core::remote_api::sub_file() {
                    let results = sophon_core::remote_api::append_verified(
                        &pf,
                        &marks,
                        sophon_core::remote_api::MODEL_PROBE_TIMEOUT,
                    );
                    for (m, r) in &results {
                        match r {
                            sophon_core::remote_api::AppendOutcome::Added => {
                                eprintln!("已验证并追加：{}", m)
                            }
                            sophon_core::remote_api::AppendOutcome::Exists => {
                                eprintln!("已存在，跳过：{}", m)
                            }
                            sophon_core::remote_api::AppendOutcome::Rejected(reason) => {
                                eprintln!("未追加（{}）：{}", reason, m)
                            }
                        }
                    }
                }
            }
            std::process::exit(0); // 直接结束，不走末尾的 "saved" 提示
        }
        "tools" => {
            use sophon_core::tools::Tool;
            // 列出桌面 sophon 目录 tools 文件夹里的脚本工具（创建的工具只能放这里，使用时也从这里读）
            let Some(dir) = sophon_core::util::sophon_tools_dir() else {
                eprintln!("无法定位桌面 sophon 目录（可用 SOPHON_DIR 指定）");
                std::process::exit(2);
            };
            let _ = std::fs::create_dir_all(&dir);
            let list = sophon_core::tools::script::load_script_tools_from(&dir);
            println!("工具目录：{}", dir.display());
            if list.is_empty() {
                println!("（空——把 <名字>.json 放进该目录即可，格式见 README「创建的工具」）");
            }
            for t in &list {
                println!("- {}\t{}\t{}", t.name(), t.title(), t.description());
            }
            std::process::exit(0);
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
