// 存储：会话库、用户设置、密钥库、引擎库。
// 对应 docs/ARCHITECTURE.md §5（持久化，与 Swift 版同构）。
use crate::models::*;
use crate::util::{self, CancelToken};
use anyhow::Context;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

// ── 会话库 ──────────────────────────────────────────────

pub struct ConversationStore {
    pub conversations: Vec<Conversation>,
    file: std::path::PathBuf,
}

impl ConversationStore {
    pub fn load(path: &Path) -> Self {
        let conversations = util::read_json_file(path)
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        ConversationStore {
            conversations,
            file: path.to_path_buf(),
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let value = serde_json::to_value(&self.conversations)?;
        util::write_json_file(&self.file, &value)
            .with_context(|| "保存 conversations.json 失败")
    }

    pub fn create(&mut self, working_directory: Option<String>) -> String {
        let conv = Conversation::new(working_directory);
        let id = conv.id.clone();
        self.conversations.push(conv);
        let _ = self.save();
        id
    }

    pub fn get(&self, id: &str) -> Option<&Conversation> {
        self.conversations.iter().find(|c| c.id == id)
    }
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Conversation> {
        self.conversations.iter_mut().find(|c| c.id == id)
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.conversations.len();
        self.conversations.retain(|c| c.id != id);
        let removed = self.conversations.len() < before;
        if removed {
            let _ = self.save();
        }
        removed
    }

    pub fn rename(&mut self, id: &str, title: String) {
        if let Some(c) = self.get_mut(id) {
            c.title = title;
            c.touch();
            let _ = self.save();
        }
    }

    pub fn set_pinned(&mut self, id: &str, pinned: bool) {
        if let Some(c) = self.get_mut(id) {
            c.pinned = pinned;
            let _ = self.save();
        }
    }

    pub fn set_draft(&mut self, id: &str, draft: String) {
        if let Some(c) = self.get_mut(id) {
            c.draft = draft;
            let _ = self.save();
        }
    }

    pub fn set_engine(&mut self, id: &str, engine_id: Option<String>) {
        if let Some(c) = self.get_mut(id) {
            c.engine_id = engine_id;
            let _ = self.save();
        }
    }

    pub fn set_permission_mode(&mut self, id: &str, mode: PermissionMode) {
        if let Some(c) = self.get_mut(id) {
            c.permission_mode = mode;
            let _ = self.save();
        }
    }

    pub fn set_thinking(&mut self, id: &str, level: Option<ThinkingLevel>) {
        if let Some(c) = self.get_mut(id) {
            c.thinking = level;
            let _ = self.save();
        }
    }

    pub fn set_team_mode(&mut self, id: &str, mode: TeamMode) {
        if let Some(c) = self.get_mut(id) {
            c.team_mode = mode;
            let _ = self.save();
        }
    }

    pub fn set_working_directory(&mut self, id: &str, dir: Option<String>) {
        if let Some(c) = self.get_mut(id) {
            c.working_directory = dir;
            let _ = self.save();
        }
    }

    /// 加一条消息，返回消息 id
    pub fn add_message(&mut self, id: &str, message: ChatMessage) -> String {
        let mid = message.id.clone();
        if let Some(c) = self.get_mut(id) {
            c.nodes.push(message);
            c.touch();
            let _ = self.save();
        }
        mid
    }

    /// 原地更新一条消息
    pub fn update_message(&mut self, id: &str, message_id: &str, f: impl FnOnce(&mut ChatMessage)) {
        if let Some(c) = self.get_mut(id) {
            if let Some(m) = c.nodes.iter_mut().find(|m| m.id == message_id) {
                f(m);
                c.touch();
            }
            let _ = self.save();
        }
    }

    /// 从某条消息 fork 成新消息（作为其子分支）
    pub fn fork(&mut self, id: &str, message_id: &str) -> Option<String> {
        let parent = self.get(id)?.nodes.iter().find(|m| m.id == message_id)?.clone();
        let mut child = ChatMessage::assistant();
        child.parent_id = Some(message_id.to_string());
        // fork 的新消息从被 fork 的消息之后开始（空助手消息）
        let new_id = child.id.clone();
        if let Some(c) = self.get_mut(id) {
            c.nodes.push(child);
            c.selected_children
                .insert(parent.id.clone(), new_id.clone());
            c.touch();
            let _ = self.save();
        }
        Some(new_id)
    }

    pub fn select_branch(&mut self, id: &str, parent_id: &str, child_id: &str) {
        if let Some(c) = self.get_mut(id) {
            c.selected_children.insert(parent_id.to_string(), child_id.to_string());
            let _ = self.save();
        }
    }

    /// 搜索（标题 + 消息内容）
    pub fn search(&self, query: &str) -> Vec<SearchHit> {
        let q = query.to_lowercase();
        let mut hits = Vec::new();
        for c in &self.conversations {
            if c.title.to_lowercase().contains(&q) {
                hits.push(SearchHit {
                    conversation_id: c.id.clone(),
                    title: c.title.clone(),
                    snippet: "（标题匹配）".to_string(),
                });
                continue;
            }
            for m in &c.nodes {
                let text = m.text_content().to_lowercase();
                if text.contains(&q) {
                    let snippet = m.text_content().chars().take(80).collect::<String>();
                    hits.push(SearchHit {
                        conversation_id: c.id.clone(),
                        title: c.title.clone(),
                        snippet,
                    });
                    break;
                }
            }
        }
        hits
    }
}

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub conversation_id: String,
    pub title: String,
    pub snippet: String,
}

// ── 用户设置（docs §5.4 settings.ini 键）────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppSettings {
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
    #[serde(default)]
    pub enable_thinking: bool,
    #[serde(default)]
    pub thinking_level: Option<ThinkingLevel>,
    #[serde(default = "default_true")]
    pub require_approval: bool,
    #[serde(default)]
    pub default_permission_mode: PermissionMode,
    #[serde(default = "default_true")]
    pub confirm_paid_calls: bool,
    #[serde(default)]
    pub working_directory: Option<String>,
    #[serde(default)]
    pub recent_directories: Vec<String>,
    #[serde(default)]
    pub disabled_tools: Vec<String>,
    #[serde(default)]
    pub disallowed_sub_agent_engines: Vec<String>,
    #[serde(default)]
    pub default_engine_id: Option<String>,
    #[serde(default = "default_true")]
    pub auto_update: bool,
    #[serde(default)]
    pub update_channel: String,
    #[serde(default = "default_true")]
    pub show_menu_bar_icon: bool,
    #[serde(default)]
    pub collapsed_conversation_groups: String,
    /// 「这些事停下来交给我」没勾的项（默认全勾 = 全部要停）
    #[serde(default)]
    pub user_stops_off: Vec<UserStop>,
    /// 助手页名称
    #[serde(default = "default_assistant_name")]
    pub assistant_name: String,
    /// 自主级别（SOPHON.md §4：autonomous 默认 / safe / yolo）
    #[serde(default)]
    pub autonomy_mode: crate::autonomy::AutonomyMode,
    /// 交付自检：回合内改了代码且目录有 Cargo.toml 时，交付前自动跑构建/测试
    #[serde(default = "default_true")]
    pub delivery_check: bool,
}

fn default_system_prompt() -> String {
    "你是智子（Sophon），全自主终端智能体。你运行在用户本机，完全离线。\
     全自主完成用户的本地任务：遇到问题自己想办法解决，直到成功交付；\
     不确定时先确认。每一步都如实汇报。"
        .to_string()
}

fn default_assistant_name() -> String {
    "Sophon".to_string()
}

fn default_true() -> bool {
    true
}

/// 用户拍板事项（docs/ARCHITECTURE.md §3.3 agent/UserStops）
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserStop {
    SpendMoney,
    RegisterAccount,
    ProvideSecret,
    NewAuthorization,
    UserTradeoff,
    UserVerify,
}

impl UserStop {
    pub fn label(self) -> &'static str {
        match self {
            UserStop::SpendMoney => "花钱",
            UserStop::RegisterAccount => "注册或登录账号",
            UserStop::ProvideSecret => "要用户提供密钥",
            UserStop::NewAuthorization => "这段对话还没给过的授权",
            UserStop::UserTradeoff => "要用户拍板的取舍",
            UserStop::UserVerify => "只有用户亲手才能验证",
        }
    }
    pub fn all() -> Vec<UserStop> {
        vec![
            UserStop::SpendMoney,
            UserStop::RegisterAccount,
            UserStop::ProvideSecret,
            UserStop::NewAuthorization,
            UserStop::UserTradeoff,
            UserStop::UserVerify,
        ]
    }
    /// 默认全部勾上（要停）；user_stops_off 里列的是没勾的
    pub fn should_stop(&self, off: &[UserStop]) -> bool {
        !off.contains(self)
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            system_prompt: default_system_prompt(),
            enable_thinking: false,
            thinking_level: None,
            require_approval: true,
            default_permission_mode: PermissionMode::Auto,
            confirm_paid_calls: true,
            working_directory: dirs::home_dir().map(|p| p.to_string_lossy().to_string()),
            recent_directories: Vec::new(),
            disabled_tools: Vec::new(),
            disallowed_sub_agent_engines: Vec::new(),
            default_engine_id: None,
            auto_update: true,
            update_channel: "stable".to_string(),
            show_menu_bar_icon: true,
            collapsed_conversation_groups: String::new(),
            user_stops_off: Vec::new(),
            assistant_name: default_assistant_name(),
            autonomy_mode: crate::autonomy::AutonomyMode::default(),
            delivery_check: true,
        }
    }
}

impl AppSettings {
    pub fn load(path: &Path) -> Self {
        util::read_json_file(path)
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    }
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let value = serde_json::to_value(self)?;
        util::write_json_file(path, &value)?;
        Ok(())
    }
    pub fn remember_directory(&mut self, dir: String) {
        self.recent_directories.retain(|d| *d != dir);
        self.recent_directories.insert(0, dir);
        self.recent_directories.truncate(10);
    }
}

// ── 密钥库（docs §5.3 secrets.json）────────────────────

pub struct SecretStore {
    secrets: HashMap<String, String>,
    file: std::path::PathBuf,
}

impl SecretStore {
    pub fn load(path: &Path) -> Self {
        let secrets = util::read_json_file(path)
            .and_then(|v| {
                v.as_object().map(|m| {
                    m.iter()
                        .filter_map(|(k, val)| val.as_str().map(|s| (k.clone(), s.to_string())))
                        .collect()
                })
            })
            .unwrap_or_default();
        SecretStore { secrets, file: path.to_path_buf() }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let mut map = serde_json::Map::new();
        for (k, v) in &self.secrets {
            map.insert(k.clone(), serde_json::Value::String(v.clone()));
        }
        util::write_json_file(&self.file, &serde_json::Value::Object(map))?;
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<String> {
        self.secrets.get(name).cloned()
    }
    pub fn set(&mut self, name: String, value: String) -> anyhow::Result<()> {
        self.secrets.insert(name, value);
        self.save()
    }
    pub fn remove(&mut self, name: &str) -> bool {
        let removed = self.secrets.remove(name).is_some();
        if removed {
            let _ = self.save();
        }
        removed
    }
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.secrets.keys().cloned().collect();
        names.sort();
        names
    }
    pub fn contains(&self, name: &str) -> bool {
        self.secrets.contains_key(name)
    }
    /// 替换文本里的 ${secret:名字}
    pub fn substitute(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (name, value) in &self.secrets {
            let placeholder = format!("${{secret:{}}}", name);
            out = out.replace(&placeholder, value);
        }
        out
    }
    pub fn missing_references(&self, text: &str) -> Vec<String> {
        let mut missing = Vec::new();
        for cap in extract_secret_refs(text) {
            if !self.secrets.contains_key(&cap) {
                missing.push(cap);
            }
        }
        missing
    }
}

fn extract_secret_refs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("${secret:") {
        let after = &rest[start + "${secret:".len()..];
        if let Some(end) = after.find('}') {
            out.push(after[..end].to_string());
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    out
}

// ── 引擎库（docs §5.3 engines.json）────────────────────

pub struct EngineStore {
    pub engines: Vec<crate::engine::EngineConfig>,
    file: std::path::PathBuf,
}

impl EngineStore {
    pub fn load(path: &Path) -> Self {
        let engines = util::read_json_file(path)
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        EngineStore { engines, file: path.to_path_buf() }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let value = serde_json::to_value(&self.engines)?;
        util::write_json_file(&self.file, &value)?;
        Ok(())
    }

    pub fn add(&mut self, config: crate::engine::EngineConfig) -> anyhow::Result<()> {
        self.engines.push(config);
        self.save()
    }
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.engines.len();
        self.engines.retain(|e| e.id != id);
        let removed = self.engines.len() < before;
        if removed {
            let _ = self.save();
        }
        removed
    }
    pub fn set_enabled(&mut self, id: &str, enabled: bool) {
        if let Some(e) = self.engines.iter_mut().find(|e| e.id == id) {
            e.enabled = enabled;
            let _ = self.save();
        }
    }
    pub fn get(&self, id: &str) -> Option<&crate::engine::EngineConfig> {
        self.engines.iter().find(|e| e.id == id)
    }
    pub fn enabled_engines(&self) -> Vec<&crate::engine::EngineConfig> {
        self.engines.iter().filter(|e| e.enabled).collect()
    }
    /// 选哪个：会话选的 → 新对话默认 → 第一个接入的（docs §6）
    pub fn resolved(&self, conversation_engine: Option<&str>, default_engine: Option<&str>) -> Option<&crate::engine::EngineConfig> {
        if let Some(id) = conversation_engine {
            if let Some(e) = self.get(id) {
                if e.enabled {
                    return Some(e);
                }
            }
        }
        if let Some(id) = default_engine {
            if let Some(e) = self.get(id) {
                if e.enabled {
                    return Some(e);
                }
            }
        }
        self.enabled_engines().into_iter().next()
    }
}

// ── 审批中介：工作线程等用户确认 ───────────────────────

pub struct ApprovalBroker {
    pending: Mutex<HashMap<String, crossbeam_channel::Sender<bool>>>,
}

impl ApprovalBroker {
    pub fn new() -> Self {
        ApprovalBroker { pending: Mutex::new(HashMap::new()) }
    }
    pub fn register(&self, event_id: String) -> crossbeam_channel::Receiver<bool> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.pending.lock().insert(event_id, tx);
        rx
    }
    /// 用户点了允许/拒绝；返回是否真的有这个待审批
    pub fn resolve(&self, event_id: &str, approved: bool) -> bool {
        if let Some(tx) = self.pending.lock().remove(event_id) {
            let _ = tx.send(approved);
            true
        } else {
            false
        }
    }
    pub fn cancel_all(&self) {
        let senders: Vec<_> = self.pending.lock().drain().map(|(_, tx)| tx).collect();
        for tx in senders {
            let _ = tx.send(false);
        }
    }
}

/// 全局核心状态（界面线程持有 Arc<Mutex<AppCore>>；工作线程短暂加锁读写）
pub struct AppCore {
    pub paths: util::Paths,
    pub store: ConversationStore,
    pub settings: AppSettings,
    pub secrets: Arc<Mutex<SecretStore>>,
    pub engines: EngineStore,
    pub approvals: Arc<ApprovalBroker>,
    pub main_thread: crate::util::MainThread,
    pub terminal_manager: Arc<Mutex<crate::terminal::TerminalManager>>,
    pub team: Arc<Mutex<crate::agent::team::AgentTeamState>>,
    pub task_states: Arc<Mutex<HashMap<String, TaskState>>>,
    pub mcp: Arc<Mutex<crate::mcp::McpManager>>,
    pub assistant: crate::assistant::AssistantRuntime,
    /// 每段会话的生成状态
    pub generating: Mutex<HashMap<String, GeneratingState>>,
    /// 每段会话的取消令牌（父令牌）
    pub conv_tokens: Mutex<HashMap<String, CancelToken>>,
    /// 全局运行状态（侧边栏显示）
    pub runner_state: Mutex<RunnerState>,
    /// 经验库 / 错误模式库（P4 自我学习）
    pub lessons: Arc<Mutex<crate::learning::LessonStore>>,
    /// 重绘回调（由界面设置；egui::Context 是 Send+Sync，工作线程可调 request_repaint）
    pub repaint: Arc<std::sync::RwLock<Option<Arc<dyn Fn() + Send + Sync>>>>,
}

/// 一段会话的生成状态
#[derive(Clone, Debug, Default)]
pub struct GeneratingState {
    pub is_generating: bool,
    pub is_stopping: bool,
    pub stopped_by_user: bool,
    pub current_message_id: Option<String>,
    pub queue: Vec<QueuedInput>,
    pub queue_paused: bool,
    pub pending_approval: bool,
    pub failed_message_id: Option<String>,
    pub can_retry: bool,
    pub started_at: Option<String>,
    /// 本轮开始时的大脑 id（用于「下一轮生效」指示）
    pub turn_engine_id: Option<String>,
    /// 本轮开始时的思考强度
    pub turn_thinking: Option<crate::models::ThinkingLevel>,
}

/// 全局运行状态（REMOTE.md §5 RunnerState）
#[derive(Clone, Debug, Default)]
pub struct RunnerState {
    pub generating: Vec<String>,
    pub stopping: Vec<String>,
    pub pending_approval: Vec<String>,
    pub background_agents: Vec<String>,
    pub live_agents: u64,
    pub live_speed: f64,
}

impl AppCore {
    pub fn load(paths: util::Paths, main_thread: crate::util::MainThread) -> anyhow::Result<Self> {
        paths.ensure_all()?;
        let store = ConversationStore::load(&paths.conversations_file());
        let settings = AppSettings::load(&paths.settings_file());
        let secrets = Arc::new(Mutex::new(SecretStore::load(&paths.secrets_file())));
        let engines = EngineStore::load(&paths.engines_file());
        let approvals = Arc::new(ApprovalBroker::new());
        let terminal_manager = Arc::new(Mutex::new(crate::terminal::TerminalManager::new()));
        let team = Arc::new(Mutex::new(crate::agent::team::AgentTeamState::new()));
        let task_states = Arc::new(Mutex::new(HashMap::new()));
        let mcp = Arc::new(Mutex::new(crate::mcp::McpManager::load(&paths.mcp_file())));
        let assistant = crate::assistant::AssistantRuntime::load(&paths)?;
        let lessons = Arc::new(Mutex::new(crate::learning::LessonStore::load(&paths.data_dir)));
        Ok(AppCore {
            paths,
            store,
            settings,
            secrets,
            engines,
            approvals,
            main_thread,
            terminal_manager,
            team,
            task_states,
            mcp,
            assistant,
            generating: Mutex::new(HashMap::new()),
            conv_tokens: Mutex::new(HashMap::new()),
            runner_state: Mutex::new(RunnerState::default()),
            lessons,
            repaint: Arc::new(std::sync::RwLock::new(None)),
        })
    }

    /// 触发界面重绘（工作线程调用）
    pub fn request_repaint(&self) {
        if let Ok(guard) = self.repaint.read() {
            if let Some(f) = guard.as_ref() {
                f();
            }
        }
    }

    /// 设置重绘回调（界面启动时调一次）
    pub fn set_repaint(&self, f: std::sync::Arc<dyn Fn() + Send + Sync>) {
        if let Ok(mut g) = self.repaint.write() {
            *g = Some(f);
        }
    }

    /// 探测引擎在线状态（GET /models，1.5s 超时；离线引擎直接标可用）
    pub fn probe_engine(&self, id: &str) -> String {
        if id == "offline" {
            return "内置可用".to_string();
        }
        let Some(cfg) = self.engines.get(id).cloned() else {
            return "已删除".to_string();
        };
        let url = format!("{}/models", cfg.base_url.trim_end_matches('/'));
        let mut req = ureq::get(&url).timeout(std::time::Duration::from_millis(1500));
        if let Some(name) = &cfg.api_key_secret {
            if let Some(key) = AppCore::with_secret(name) {
                if !key.is_empty() {
                    req = req.set("Authorization", &format!("Bearer {}", key));
                }
            }
        }
        match req.call() {
            Ok(r) => match r.into_string() {
                Ok(body) => {
                    let n = serde_json::from_str::<serde_json::Value>(&body)
                        .ok()
                        .and_then(|v| v["data"].as_array().map(|a| a.len()))
                        .unwrap_or(0);
                    if n > 0 {
                        format!("在线（{} 个模型）", n)
                    } else {
                        "在线".to_string()
                    }
                }
                Err(_) => "在线".to_string(),
            },
            Err(e) => format!("不可达（{}）", e),
        }
    }

    /// 取当前生效的引擎（会话选 → 默认 → 第一个）
    pub fn resolved_engine(&self, conversation: &Conversation) -> Option<crate::engine::EngineConfig> {
        self.engines
            .resolved(conversation.engine_id.as_deref(), self.settings.default_engine_id.as_deref())
            .cloned()
    }

    /// 构造引擎实例
    pub fn build_engine(&self, config: &crate::engine::EngineConfig) -> anyhow::Result<std::sync::Arc<dyn crate::engine::ChatEngine>> {
        if config.id == "offline" || config.base_url.is_empty() {
            Ok(std::sync::Arc::new(crate::engine::OfflineEngine::new()))
        } else {
            Ok(std::sync::Arc::new(crate::engine::OpenAICompatEngine::new(config.clone())?))
        }
    }

    /// 供引擎读取密钥（按名字）
    pub fn with_secret(name: &str) -> Option<String> {
        // 由 AppCore 实例设置的全局密钥读取器（见 set_secret_reader）
        crate::store::secret_reader().read()(name)
    }

    pub fn set_secret_reader(&self) {
        let secrets = self.secrets.clone();
        *crate::store::secret_reader().write() = Box::new(move |name: &str| {
            secrets.lock().get(name)
        });
    }

    /// 保存全部
    pub fn save_all(&self) -> anyhow::Result<()> {
        self.store.save()?;
        self.settings.save(&self.paths.settings_file())?;
        self.engines.save()?;
        Ok(())
    }
}

// 全局密钥读取器（引擎在任意线程读密钥用）
static SECRET_READER: std::sync::LazyLock<parking_lot::RwLock<Box<dyn Fn(&str) -> Option<String> + Send + Sync>>> =
    std::sync::LazyLock::new(|| parking_lot::RwLock::new(Box::new(|_| None)));

pub fn secret_reader() -> &'static parking_lot::RwLock<Box<dyn Fn(&str) -> Option<String> + Send + Sync>> {
    &SECRET_READER
}
