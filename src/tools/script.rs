// 脚本工具：用户创建的外部工具。
// 存放与加载都固定在桌面 sophon 目录的 tools 文件夹（util::sophon_tools_dir）：
//   <tools>/<名字>.json：
//     {
//       "name": "weather",                    // 必填，唯一；与已有工具重名会被跳过
//       "title": "查天气",                     // 可选
//       "description": "按城市查询天气",        // 可选，建议写清
//       "parameters": { "type": "object", ... } // 可选，JSON Schema，默认 object
//       "command": "python weather.py",       // 必填；参数 JSON 从 stdin 传入，stdout 即结果
//       "needs_approval": true                // 可选，默认 true（外部命令默认需要审批）
//     }
// 执行：Windows 用 cmd /C，其他用 sh -c；工作目录 = 工具上下文的 working_directory；
// 退出码 0 → stdout 为 ToolOutput::Ok，否则 Err（带 stderr）。
use super::{Tool, ToolContext, ToolOutput};
use anyhow::Context as _;
use serde_json::Value;
use std::process::{Command, Stdio};

#[derive(Clone, Debug, serde::Deserialize)]
pub struct ScriptToolSpec {
    pub name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub parameters: Value,
    pub command: String,
    #[serde(default = "default_true")]
    pub needs_approval: bool,
}

fn default_true() -> bool {
    true
}

pub struct ScriptTool {
    spec: ScriptToolSpec,
}

impl ScriptTool {
    pub fn from_file(path: &std::path::Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let spec: ScriptToolSpec =
            serde_json::from_str(&text).with_context(|| format!("解析 {}", path.display()))?;
        if spec.name.trim().is_empty() || spec.command.trim().is_empty() {
            anyhow::bail!("{} 缺少 name 或 command", path.display());
        }
        Ok(ScriptTool { spec })
    }

    /// 执行（working_directory 为空则用当前目录；供 execute 与测试共用）
    pub fn run(&self, args: &Value, working_directory: &str) -> ToolOutput {
        let input = args.to_string();
        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(&self.spec.command);
            c
        } else {
            let mut c = Command::new("sh");
            c.arg("-c").arg(&self.spec.command);
            c
        };
        if !working_directory.trim().is_empty() {
            cmd.current_dir(working_directory);
        }
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return ToolOutput::Err(format!("启动失败：{}", e)),
        };
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(input.as_bytes()); // stdin 随 drop 关闭
        }
        match child.wait_with_output() {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                if out.status.success() {
                    if stdout.is_empty() {
                        ToolOutput::Ok("（无输出）".to_string())
                    } else {
                        ToolOutput::Ok(stdout)
                    }
                } else {
                    let detail = if stderr.is_empty() { stdout } else { stderr };
                    ToolOutput::Err(format!("exit {:?}：{}", out.status.code(), detail))
                }
            }
            Err(e) => ToolOutput::Err(format!("执行失败：{}", e)),
        }
    }
}

impl Tool for ScriptTool {
    fn name(&self) -> &str {
        &self.spec.name
    }
    fn title(&self) -> &str {
        if self.spec.title.is_empty() { self.spec.name.as_str() } else { self.spec.title.as_str() }
    }
    fn description(&self) -> &str {
        &self.spec.description
    }
    fn parameters(&self) -> Value {
        if self.spec.parameters.is_null() {
            serde_json::json!({"type": "object", "properties": {}})
        } else {
            self.spec.parameters.clone()
        }
    }
    fn needs_approval(&self, _args: &Value) -> bool {
        self.spec.needs_approval
    }
    fn is_write(&self) -> bool {
        self.spec.needs_approval
    }
    fn execute(&self, args: &Value, ctx: &mut ToolContext) -> ToolOutput {
        self.run(args, &ctx.working_directory)
    }
}

/// 从桌面 sophon 目录的 tools 文件夹加载全部脚本工具（目录不存在 → 空）
pub fn load_script_tools() -> Vec<ScriptTool> {
    match crate::util::sophon_tools_dir() {
        Some(dir) => load_script_tools_from(&dir),
        None => Vec::new(),
    }
}

/// 同上，但目录显式给出（测试用）；按文件名排序保证顺序稳定，坏文件跳过并告警
pub fn load_script_tools_from(dir: &std::path::Path) -> Vec<ScriptTool> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "json").unwrap_or(false))
        .collect();
    files.sort();
    let mut out = Vec::new();
    for f in files {
        match ScriptTool::from_file(&f) {
            Ok(t) => out.push(t),
            Err(e) => eprintln!("tools: 跳过无效工具 {}：{}", f.display(), e),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 跨平台的「回显 stdin」命令：Windows 用 PowerShell $input，其他用 cat
    /// （more.com 在管道输入下会报 Internal error，不用它）
    fn echo_cmd() -> &'static str {
        if cfg!(windows) { r#"powershell -NoProfile -Command "$input""# } else { "cat" }
    }

    fn write_tool(dir: &std::path::Path, file: &str, json: &str) {
        std::fs::write(dir.join(file), json).unwrap();
    }

    #[test]
    fn load_and_run_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let spec = serde_json::json!({
            "name": "hello", "title": "回显", "description": "原样回显参数",
            "parameters": {"type": "object"}, "command": echo_cmd(),
        });
        write_tool(dir.path(), "hello.json", &spec.to_string());
        write_tool(dir.path(), "说明.txt", "非 json 文件应被忽略");
        let tools = load_script_tools_from(dir.path());
        assert_eq!(tools.len(), 1, "非 json 文件忽略");
        assert_eq!(tools[0].name(), "hello");
        assert_eq!(tools[0].title(), "回显");
        let out = tools[0].run(&serde_json::json!({"x": 1}), "");
        assert!(out.is_ok(), "{:?}", out);
        assert!(out.into_text().contains("\"x\""), "参数 JSON 应经 stdin 传给命令并回显");
    }

    #[test]
    fn bad_files_skipped() {
        let dir = tempfile::tempdir().unwrap();
        write_tool(dir.path(), "a.json", "{ 不是 json");
        write_tool(dir.path(), "b.json", r#"{"name":"b"}"#, );
        write_tool(dir.path(), "c.json", r#"{"name":"","command":"x"}"#);
        assert!(
            load_script_tools_from(dir.path()).is_empty(),
            "坏 JSON / 缺 command / 空名字都跳过"
        );
    }

    #[test]
    fn parameters_defaults_to_object() {
        let dir = tempfile::tempdir().unwrap();
        write_tool(dir.path(), "t.json", &serde_json::json!({"name":"t","command":echo_cmd()}).to_string());
        let t = &load_script_tools_from(dir.path())[0];
        assert_eq!(t.parameters()["type"], "object");
        assert!(t.needs_approval(&serde_json::json!({})), "外部命令默认需要审批");
    }
}
