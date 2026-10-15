# -*- coding: utf-8 -*-
# r2: bin/sophon.rs ask branch -> streaming print + mark-driven append
from pathlib import Path
p = Path(r"C:\Users\ultra\Desktop\works\hermes\src\bin\sophon.rs")
lines = p.read_text(encoding="utf-8").splitlines()

# locate "ask" => { ... } with brace matching
start = None
for i, ln in enumerate(lines):
    if ln.strip().startswith('"ask" => {'):
        start = i
        break
print("ask branch starts at line", (start + 1) if start is not None else None)
assert start is not None, "ask branch not found"

depth = 0
end = None
for i in range(start, len(lines)):
    depth += lines[i].count("{") - lines[i].count("}")
    if depth == 0 and i > start:
        end = i
        break
print("ask branch ends at line", (end + 1) if end is not None else None)
assert end is not None

NEW = r'''        "ask" => {
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
        }'''

lines[start:end + 1] = NEW.splitlines()
p.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
print("ask branch replaced")
