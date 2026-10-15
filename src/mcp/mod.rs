// MCP 客户端：连接本机的 stdio MCP 服务，取它的工具。
// 对应 docs/ARCHITECTURE.md 的 McpClient / McpServerConfig。
// 只支持 stdio（本机进程），不联网。
use crate::engine::ToolDescriptor;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};

/// MCP 服务配置
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// 一个 MCP 连接
pub struct McpConnection {
    child: Child,
    stdin: std::process::ChildStdin,
    reader: BufReader<std::process::ChildStdout>,
    request_id: u64,
    tools: Vec<ToolDescriptor>,
}

impl McpConnection {
    fn spawn(config: &McpServerConfig) -> anyhow::Result<Self> {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args);
        for (k, v) in &config.env {
            cmd.env(k, v);
        }
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::null());
        let mut child = cmd.spawn()
            .with_context(|| format!("启动 MCP 服务失败：{}", config.command))?;
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);
        Ok(McpConnection {
            child,
            stdin,
            reader,
            request_id: 0,
            tools: Vec::new(),
        })
    }

    /// 发一个 JSON-RPC 请求，读回复
    fn request(&mut self, method: &str, params: serde_json::Value) -> anyhow::Result<serde_json::Value> {
        self.request_id += 1;
        let id = self.request_id;
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let mut line = serde_json::to_string(&req)?;
        line.push('\n');
        self.stdin.write_all(line.as_bytes())?;
        self.stdin.flush()?;
        // 读回复（跳过通知）
        loop {
            let mut buf = String::new();
            let n = self.reader.read_line(&mut buf)?;
            if n == 0 {
                anyhow::bail!("MCP 服务关闭了连接");
            }
            let buf = buf.trim();
            if buf.is_empty() {
                continue;
            }
            let value: serde_json::Value = match serde_json::from_str(buf) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if value.get("id").and_then(|i| i.as_u64()) == Some(id) {
                if let Some(err) = value.get("error") {
                    anyhow::bail!("MCP 错误：{}", err);
                }
                return Ok(value.get("result").cloned().unwrap_or(serde_json::Value::Null));
            }
            // 否则是通知，忽略
        }
    }

    /// initialize + tools/list
    fn init(&mut self) -> anyhow::Result<()> {
        let init_result = self.request(
            "initialize",
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "sophon", "version": env!("CARGO_PKG_VERSION")},
            }),
        )?;
        let _ = init_result;
        // initialized 通知
        let notif = serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        let mut line = serde_json::to_string(&notif)?;
        line.push('\n');
        let _ = self.stdin.write_all(line.as_bytes());
        let _ = self.stdin.flush();
        // tools/list
        let tools_result = self.request("tools/list", serde_json::json!({}))?;
        let mut tools = Vec::new();
        if let Some(arr) = tools_result.get("tools").and_then(|t| t.as_array()) {
            for t in arr {
                let name = t.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                let description = t
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();
                let parameters = t
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or(serde_json::json!({"type": "object", "properties": {}}));
                tools.push(ToolDescriptor {
                    name,
                    title: String::new(),
                    description,
                    parameters,
                });
            }
        }
        self.tools = tools;
        Ok(())
    }

    /// 调一个 MCP 工具
    fn call_tool(&mut self, name: &str, arguments: serde_json::Value) -> anyhow::Result<String> {
        let result = self.request(
            "tools/call",
            serde_json::json!({"name": name, "arguments": arguments}),
        )?;
        // 解析 content
        if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
            let mut out = String::new();
            for c in content {
                if let Some(text) = c.get("text").and_then(|t| t.as_str()) {
                    out.push_str(text);
                    out.push('\n');
                }
            }
            return Ok(out);
        }
        Ok(serde_json::to_string_pretty(&result)?)
    }
}

/// MCP 管理器
pub struct McpManager {
    pub servers: Vec<McpServerConfig>,
    pub connections: HashMap<String, McpConnection>,
    file: std::path::PathBuf,
}

impl McpManager {
    pub fn load(path: &std::path::Path) -> Self {
        let servers = crate::util::read_json_file(path)
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        McpManager {
            servers,
            connections: HashMap::new(),
            file: path.to_path_buf(),
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let value = serde_json::to_value(&self.servers)?;
        crate::util::write_json_file(&self.file, &value)?;
        Ok(())
    }

    /// 连接所有启用的服务
    pub fn connect_all(&mut self) {
        let configs: Vec<McpServerConfig> = self
            .servers
            .iter()
            .filter(|s| s.enabled)
            .cloned()
            .collect();
        for config in configs {
            match McpConnection::spawn(&config) {
                Ok(mut conn) => {
                    if conn.init().is_ok() {
                        self.connections.insert(config.name.clone(), conn);
                    } else {
                        let _ = conn.child.kill();
                    }
                }
                Err(_) => {}
            }
        }
    }

    /// 全部 MCP 工具描述
    pub fn tool_descriptors(&self) -> Vec<ToolDescriptor> {
        let mut out = Vec::new();
        for (name, conn) in &self.connections {
            for t in &conn.tools {
                let mut t = t.clone();
                t.title = format!("MCP:{}", name);
                out.push(t);
            }
        }
        out
    }

    /// 调 MCP 工具（名字带 "mcp:服务名:工具名" 或直接工具名）
    pub fn call_tool(&mut self, name: &str, arguments: serde_json::Value) -> Option<anyhow::Result<String>> {
        // 找哪个连接拥有这个工具
        for (server_name, conn) in self.connections.iter_mut() {
            if conn.tools.iter().any(|t| t.name == name) {
                return Some(conn.call_tool(name, arguments));
            }
            let _ = server_name;
        }
        None
    }

    pub fn add_server(&mut self, config: McpServerConfig) -> anyhow::Result<()> {
        self.servers.push(config.clone());
        self.save()?;
        if config.enabled {
            if let Ok(mut conn) = McpConnection::spawn(&config) {
                if conn.init().is_ok() {
                    self.connections.insert(config.name, conn);
                }
            }
        }
        Ok(())
    }

    pub fn remove_server(&mut self, name: &str) -> bool {
        self.connections.remove(name);
        let before = self.servers.len();
        self.servers.retain(|s| s.name != name);
        let removed = self.servers.len() < before;
        if removed {
            let _ = self.save();
        }
        removed
    }
}

impl Drop for McpManager {
    fn drop(&mut self) {
        for (_, conn) in self.connections.iter_mut() {
            let _ = conn.child.kill();
        }
    }
}
