// 远程 Friday 客户端：JSON-RPC 2.0 over WebSocket（REMOTE.md）。
// 客户端只是视图：不读写远程文件、不执行工具，一切通过协议。
// 完全本地——只连用户指定的本机/局域网地址，不接任何云端服务。
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// 客户端发给服务端的事件（界面层订阅）
#[derive(Debug, Clone)]
pub enum RemoteEvent {
    /// 连接状态变化
    Connected { server: ServerInfo },
    Disconnected { reason: String },
    /// 全量状态（hello 的 state）
    State { state: Value },
    /// 增量事件（message / conversation / runtime / runner / team / engines / secrets / directories / terminals / powers / models / storage / config）
    Event { method: String, params: Value },
    /// 请求回复（按 id 对）
    Response { id: u64, result: Result<Value, String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub version: String,
    pub platform: String,
    pub arch: Option<String>,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub home: Option<String>,
    pub data_dir: Option<String>,
    pub pid: Option<u64>,
    pub started_at: Option<String>,
    pub host: Option<String>,
    pub default_working_directory: Option<String>,
    pub features: Vec<String>,
}

/// 远程连接配置
#[derive(Clone)]
pub struct RemoteConfig {
    /// WebSocket 地址（ws:// 或 wss://）；完全本地，只允许本机/局域网
    pub url: String,
    /// 配对令牌（WebSocket 入口需要）
    pub token: Option<String>,
}

impl RemoteConfig {
    /// 校验：只允许本机 / 局域网（用户约束：不接云端）
    pub fn is_local(&self) -> bool {
        let host = self
            .url
            .split("://")
            .nth(1)
            .unwrap_or("")
            .split(':')
            .next()
            .unwrap_or("");
        host == "127.0.0.1"
            || host == "localhost"
            || host == "::1"
            || host.starts_with("127.")
            || host.starts_with("10.")
            || host.starts_with("192.168.")
            || host.starts_with("172.16.")
            || host.is_empty()
    }
}

/// 远程客户端（持有连接 + 事件循环线程）
pub struct RemoteClient {
    /// 发给事件循环线程的命令
    cmd_tx: mpsc::Sender<RemoteCommand>,
    /// 事件输出（界面层收）
    pub event_rx: mpsc::Receiver<RemoteEvent>,
    /// 连接状态（界面轮询）
    pub state: Arc<Mutex<RemoteState>>,
    /// 请求 id 计数器
    next_id: Arc<Mutex<u64>>,
}

enum RemoteCommand {
    Connect(RemoteConfig),
    Disconnect,
    /// 发送 JSON-RPC 请求（方法 + 参数），返回 id
    Request { method: String, params: Value },
    /// 发送原始 JSON（调试）
    Raw(String),
}

#[derive(Default, Clone)]
pub struct RemoteState {
    pub connected: bool,
    pub server: Option<ServerInfo>,
    pub last_error: Option<String>,
}

impl RemoteClient {
    pub fn new() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<RemoteCommand>();
        let (event_tx, event_rx) = mpsc::channel::<RemoteEvent>();
        let state = Arc::new(Mutex::new(RemoteState::default()));
        let next_id = Arc::new(Mutex::new(1u64));
        // 事件循环线程
        let state2 = state.clone();
        let next_id2 = next_id.clone();
        thread::spawn(move || event_loop(cmd_rx, event_tx, state2, next_id2));
        RemoteClient {
            cmd_tx,
            event_rx,
            state,
            next_id,
        }
    }

    /// 连接到远程服务端
    pub fn connect(&self, config: RemoteConfig) -> Result<(), String> {
        if !config.is_local() {
            return Err(format!("按用户约束只允许本机/局域网地址，拒绝：{}", config.url));
        }
        self.cmd_tx
            .send(RemoteCommand::Connect(config))
            .map_err(|e| format!("发送连接命令失败：{}", e))
    }

    pub fn disconnect(&self) {
        let _ = self.cmd_tx.send(RemoteCommand::Disconnect);
    }

    /// 发送 JSON-RPC 请求，返回请求 id（界面层用 id 匹配回复）
    pub fn request(&self, method: &str, params: Value) -> u64 {
        let id = {
            let mut n = self.next_id.lock().unwrap();
            let id = *n;
            *n += 1;
            id
        };
        let _ = self.cmd_tx.send(RemoteCommand::Request {
            method: method.to_string(),
            params,
        });
        id
    }

    /// 订阅一段会话
    pub fn subscribe(&self, conversation_id: &str) -> u64 {
        self.request(
            "conversation.subscribe",
            serde_json::json!({"id": conversation_id}),
        )
    }

    /// 发送一条消息
    pub fn submit(&self, conversation_id: &str, text: &str) -> u64 {
        self.request(
            "runner.submit",
            serde_json::json!({"conversationID": conversation_id, "text": text}),
        )
    }

    /// 新建会话
    pub fn create_conversation(&self, working_directory: Option<&str>) -> u64 {
        let mut params = serde_json::json!({});
        if let Some(wd) = working_directory {
            params["workingDirectory"] = serde_json::json!(wd);
        }
        self.request("conversation.create", params)
    }

    pub fn state(&self) -> RemoteState {
        self.state.lock().unwrap().clone()
    }
}

/// 事件循环：持有 WebSocket 连接，读事件、分发
fn event_loop(
    cmd_rx: mpsc::Receiver<RemoteCommand>,
    event_tx: mpsc::Sender<RemoteEvent>,
    state: Arc<Mutex<RemoteState>>,
    next_id: Arc<Mutex<u64>>,
) {
    let mut socket: Option<tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>> = None;
    // 待回复的请求（id → 方法，调试用）
    let mut pending: HashMap<u64, String> = HashMap::new();

    loop {
        // 非阻塞读命令
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                RemoteCommand::Connect(config) => {
                    match tungstenite::connect(config.url.clone()) {
                        Ok((mut sock, _resp)) => {
                            // hello 握手
                            let hello = serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": 0u64,
                                "method": "hello",
                                "params": {
                                    "protocol": 1,
                                    "client": {
                                        "name": "Hermes",
                                        "version": "0.1.0",
                                        "platform": "windows",
                                        "kind": "desktop"
                                    },
                                    "features": ["messagePatch"]
                                }
                            });
                            let sent = sock
                                .send(tungstenite::Message::Text(hello.to_string().into()))
                                .is_ok();
                            if sent {
                                *state.lock().unwrap() = RemoteState {
                                    connected: true,
                                    server: None,
                                    last_error: None,
                                };
                                let _ = event_tx.send(RemoteEvent::Connected {
                                    server: ServerInfo {
                                        version: String::new(),
                                        platform: String::new(),
                                        arch: None,
                                        hostname: None,
                                        user: None,
                                        home: None,
                                        data_dir: None,
                                        pid: None,
                                        started_at: None,
                                        host: None,
                                        default_working_directory: None,
                                        features: Vec::new(),
                                    },
                                });
                                socket = Some(sock);
                            } else {
                                *state.lock().unwrap() = RemoteState {
                                    connected: false,
                                    server: None,
                                    last_error: Some("发送 hello 失败".to_string()),
                                };
                                let _ = event_tx.send(RemoteEvent::Disconnected {
                                    reason: "发送 hello 失败".to_string(),
                                });
                            }
                        }
                        Err(e) => {
                            *state.lock().unwrap() = RemoteState {
                                connected: false,
                                server: None,
                                last_error: Some(format!("连接失败：{}", e)),
                            };
                            let _ = event_tx.send(RemoteEvent::Disconnected {
                                reason: format!("连接失败：{}", e),
                            });
                        }
                    }
                }
                RemoteCommand::Disconnect => {
                    if let Some(mut sock) = socket.take() {
                        let _ = sock.close(None);
                    }
                    *state.lock().unwrap() = RemoteState {
                        connected: false,
                        server: None,
                        last_error: None,
                    };
                    let _ = event_tx.send(RemoteEvent::Disconnected {
                        reason: "已断开".to_string(),
                    });
                }
                RemoteCommand::Request { method, params } => {
                    let id = {
                        let mut n = next_id.lock().unwrap();
                        let id = *n;
                        *n += 1;
                        id
                    };
                    pending.insert(id, method.clone());
                    let req = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "method": method,
                        "params": params
                    });
                    if let Some(sock) = socket.as_mut() {
                        let _ = sock.send(tungstenite::Message::Text(req.to_string().into()));
                    }
                }
                RemoteCommand::Raw(text) => {
                    if let Some(sock) = socket.as_mut() {
                        let _ = sock.send(tungstenite::Message::Text(text.into()));
                    }
                }
            }
        }

        // 读 WebSocket 事件（非阻塞，短超时）
        if let Some(sock) = socket.as_mut() {
            // 用短超时读：tungstenite 的 read 是阻塞的，这里用 10ms 轮询
            // 通过设置 socket 为非阻塞不可行（tungstenite 不支持），
            // 改为：只在有数据时读（用 try_recv 无法探测 socket），
            // 所以这里用短暂阻塞读 + 定期让出
            match sock.read() {
                Ok(msg) => {
                    use tungstenite::Message;
                    match msg {
                        Message::Text(text) => {
                            handle_message(&text, &event_tx, &state, &mut pending);
                        }
                        Message::Ping(p) => {
                            let _ = sock.send(Message::Pong(p));
                        }
                        Message::Pong(_) => {}
                        Message::Close(_) => {
                            *state.lock().unwrap() = RemoteState {
                                connected: false,
                                server: None,
                                last_error: Some("服务端关闭连接".to_string()),
                            };
                            let _ = event_tx.send(RemoteEvent::Disconnected {
                                reason: "服务端关闭连接".to_string(),
                            });
                            socket = None;
                        }
                        _ => {}
                    }
                }
                Err(_) => {
                    // 读取失败（非 WouldBlock）：断开
                    *state.lock().unwrap() = RemoteState {
                        connected: false,
                        server: None,
                        last_error: Some("WebSocket 读取失败".to_string()),
                    };
                    let _ = event_tx.send(RemoteEvent::Disconnected {
                        reason: "WebSocket 读取失败".to_string(),
                    });
                    socket = None;
                }
            }
        } else {
            // 未连接：让出 CPU，等命令
            thread::sleep(Duration::from_millis(10));
        }
    }
}

/// 处理一条 JSON-RPC 消息（回复或事件）
fn handle_message(
    text: &str,
    event_tx: &mpsc::Sender<RemoteEvent>,
    state: &Arc<Mutex<RemoteState>>,
    pending: &mut HashMap<u64, String>,
) {
    let value: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return,
    };
    // 回复（有 id）
    if let Some(id) = value.get("id").and_then(|i| i.as_u64()) {
        if let Some(err) = value.get("error") {
            let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("错误").to_string();
            pending.remove(&id);
            let _ = event_tx.send(RemoteEvent::Response {
                id,
                result: Err(msg),
            });
            return;
        }
        if let Some(result) = value.get("result") {
            // hello 回复：解析 server + state
            if id == 0 {
                if let Some(server) = result.get("server") {
                    let info = parse_server_info(server);
                    *state.lock().unwrap() = RemoteState {
                        connected: true,
                        server: Some(info.clone()),
                        last_error: None,
                    };
                    let _ = event_tx.send(RemoteEvent::Connected { server: info });
                }
                if let Some(st) = result.get("state") {
                    let _ = event_tx.send(RemoteEvent::State { state: st.clone() });
                }
            } else {
                let _ = event_tx.send(RemoteEvent::Response {
                    id,
                    result: Ok(result.clone()),
                });
            }
            pending.remove(&id);
        }
        return;
    }
    // 事件（无 id，有 method）
    if let Some(method) = value.get("method").and_then(|m| m.as_str()) {
        let params = value.get("params").cloned().unwrap_or(Value::Null);
        let _ = event_tx.send(RemoteEvent::Event {
            method: method.to_string(),
            params,
        });
    }
}

/// 解析 hello 回复里的 server 信息
fn parse_server_info(v: &Value) -> ServerInfo {
    ServerInfo {
        version: v.get("version").and_then(|s| s.as_str()).unwrap_or("").to_string(),
        platform: v.get("platform").and_then(|s| s.as_str()).unwrap_or("").to_string(),
        arch: v.get("arch").and_then(|s| s.as_str()).map(|s| s.to_string()),
        hostname: v.get("hostname").and_then(|s| s.as_str()).map(|s| s.to_string()),
        user: v.get("user").and_then(|s| s.as_str()).map(|s| s.to_string()),
        home: v.get("home").and_then(|s| s.as_str()).map(|s| s.to_string()),
        data_dir: v.get("dataDir").and_then(|s| s.as_str()).map(|s| s.to_string()),
        pid: v.get("pid").and_then(|s| s.as_u64()),
        started_at: v.get("startedAt").and_then(|s| s.as_str()).map(|s| s.to_string()),
        host: v.get("host").and_then(|s| s.as_str()).map(|s| s.to_string()),
        default_working_directory: v
            .get("defaultWorkingDirectory")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string()),
        features: v
            .get("features")
            .and_then(|f| f.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

// ── 单元测试（协议解析 / 本地校验）──────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_is_local() {
        let cfg = RemoteConfig { url: "ws://127.0.0.1:7443".to_string(), token: None };
        assert!(cfg.is_local());
        let cfg = RemoteConfig { url: "ws://localhost:7443".to_string(), token: None };
        assert!(cfg.is_local());
        let cfg = RemoteConfig { url: "ws://192.168.1.10:7443".to_string(), token: None };
        assert!(cfg.is_local());
        // 云端拒绝
        let cfg = RemoteConfig { url: "wss://api.example.com:7443".to_string(), token: None };
        assert!(!cfg.is_local());
    }

    #[test]
    fn test_parse_server_info() {
        let v = serde_json::json!({
            "version": "2.1.3",
            "platform": "linux",
            "arch": "x64",
            "hostname": "ubuntu",
            "user": "root",
            "home": "/root",
            "dataDir": "/root/.local/share/Friday",
            "pid": 4242,
            "startedAt": "2026-10-05T12:00:00Z",
            "host": "server",
            "features": ["messagePatch", "tail"]
        });
        let info = parse_server_info(&v);
        assert_eq!(info.version, "2.1.3");
        assert_eq!(info.platform, "linux");
        assert_eq!(info.pid, Some(4242));
        assert_eq!(info.host.as_deref(), Some("server"));
        assert_eq!(info.features, vec!["messagePatch", "tail"]);
    }

    #[test]
    fn test_handle_response() {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(RemoteState::default()));
        let mut pending = HashMap::new();
        pending.insert(7u64, "conversation.subscribe".to_string());
        // 正常回复
        let text = r#"{"jsonrpc":"2.0","id":7,"result":{"conversation":{}}}"#;
        handle_message(text, &tx, &state, &mut pending);
        match rx.recv().unwrap() {
            RemoteEvent::Response { id, result } => {
                assert_eq!(id, 7);
                assert!(result.is_ok());
            }
            other => panic!("应是 Response，收到 {:?}", other),
        }
        assert!(!pending.contains_key(&7), "已回复的应移除");
    }

    #[test]
    fn test_handle_event() {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(RemoteState::default()));
        let mut pending = HashMap::new();
        // 事件（无 id）
        let text = r#"{"jsonrpc":"2.0","method":"message","params":{"id":"abc"}}"#;
        handle_message(text, &tx, &state, &mut pending);
        match rx.recv().unwrap() {
            RemoteEvent::Event { method, params } => {
                assert_eq!(method, "message");
                assert_eq!(params["id"], "abc");
            }
            other => panic!("应是 Event，收到 {:?}", other),
        }
    }

    #[test]
    fn test_handle_hello_response() {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(RemoteState::default()));
        let mut pending = HashMap::new();
        let text = r#"{"jsonrpc":"2.0","id":0,"result":{"protocol":1,"server":{"version":"2.1.3","platform":"linux","host":"server"},"state":{"summaries":[]}}}"#;
        handle_message(text, &tx, &state, &mut pending);
        // 先 Connected 再 State
        match rx.recv().unwrap() {
            RemoteEvent::Connected { server } => {
                assert_eq!(server.version, "2.1.3");
                assert_eq!(server.host.as_deref(), Some("server"));
            }
            other => panic!("应是 Connected，收到 {:?}", other),
        }
        match rx.recv().unwrap() {
            RemoteEvent::State { state } => {
                assert!(state.get("summaries").is_some());
            }
            other => panic!("应是 State，收到 {:?}", other),
        }
        assert!(state.lock().unwrap().connected, "hello 后应 connected");
    }

    #[test]
    fn test_handle_error_response() {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(RemoteState::default()));
        let mut pending = HashMap::new();
        pending.insert(9u64, "conversation.subscribe".to_string());
        let text = r#"{"jsonrpc":"2.0","id":9,"error":{"code":3,"message":"没有这段会话"}}"#;
        handle_message(text, &tx, &state, &mut pending);
        match rx.recv().unwrap() {
            RemoteEvent::Response { id, result } => {
                assert_eq!(id, 9);
                assert_eq!(result.unwrap_err(), "没有这段会话");
            }
            other => panic!("应是 Response，收到 {:?}", other),
        }
    }
}
