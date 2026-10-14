// 应用状态：持有核心、焦点、滚动、输入、快捷键动作。
// 界面线程持有 App；工作线程经 MainThread 队列改核心。
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use parking_lot::Mutex;
use ratatui::backend::CrosstermBackend;
use ratatui::style::{Modifier, Style};
use ratatui::Terminal;

use sophon_core::agent::runner::AgentRunner;
use sophon_core::models::*;
use sophon_core::store::AppCore;
use sophon_core::util::{MainThreadReceiver, Paths};

use crate::theme::Theme;
use crate::ui;

/// 焦点区
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    Chat,
    Input,
    RightPanel,
}

/// 右栏页签
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RightTab {
    AgentTree,
    Terminal,
    Board,
    Assistant,
    Settings,
}

impl RightTab {
    pub fn label(self) -> &'static str {
        match self {
            RightTab::AgentTree => "Agent 树",
            RightTab::Terminal => "终端",
            RightTab::Board => "状态板",
            RightTab::Assistant => "助手",
            RightTab::Settings => "设置",
        }
    }
    pub fn all() -> [RightTab; 5] {
        [RightTab::AgentTree, RightTab::Terminal, RightTab::Board, RightTab::Assistant, RightTab::Settings]
    }
}

/// 退出确认
#[derive(Clone, Debug)]
pub enum Confirm {
    Quit,
    DeleteConversation(String),
}

pub struct App {
    pub core: Arc<Mutex<AppCore>>,
    pub main_rx: MainThreadReceiver,
    pub dirty: Arc<AtomicBool>,

    // UI 状态
    pub focus: Focus,
    pub right_tab: RightTab,
    pub selected_conv: Option<String>,
    pub sidebar_scroll: usize,
    pub chat_scroll: usize,
    pub right_scroll: usize,
    pub input: String,
    pub input_cursor: usize,
    pub input_history: Vec<String>,
    pub history_idx: Option<usize>,
    pub search: String,
    pub searching: bool,
    pub status: String,
    pub confirm: Option<Confirm>,
    pub show_help: bool,
    /// 终端面板滚屏（行数，向上滚为正）
    pub term_scroll: usize,
    /// 当前终端 id（右栏终端页签用）
    pub term_id: Option<String>,
}

impl App {
    pub fn new(
        core: Arc<Mutex<AppCore>>,
        main_rx: MainThreadReceiver,
        dirty: Arc<AtomicBool>,
    ) -> Self {
        // 默认选最近更新的会话
        let selected_conv = {
            let c = core.lock();
            let mut convs: Vec<&Conversation> = c.store.conversations.iter().collect();
            convs.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            convs.first().map(|c| c.id.clone())
        };
        App {
            core,
            main_rx,
            dirty,
            focus: Focus::Input,
            right_tab: RightTab::AgentTree,
            selected_conv,
            sidebar_scroll: 0,
            chat_scroll: 0,
            right_scroll: 0,
            input: String::new(),
            input_cursor: 0,
            input_history: Vec::new(),
            history_idx: None,
            search: String::new(),
            searching: false,
            status: String::new(),
            confirm: None,
            show_help: false,
            term_scroll: 0,
            term_id: None,
        }
    }

    /// 测试用空 App（不连核心）
    #[cfg(test)]
    pub fn dummy() -> Self {
        let (main_tx, main_rx) = sophon_core::util::main_thread::channel();
        let paths = sophon_core::util::Paths::resolve();
        let core = std::sync::Arc::new(parking_lot::Mutex::new(
            sophon_core::store::AppCore::load(paths, main_tx).expect("dummy core"),
        ));
        let dirty = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        App {
            core,
            main_rx,
            dirty,
            focus: Focus::Input,
            right_tab: RightTab::AgentTree,
            selected_conv: None,
            sidebar_scroll: 0,
            chat_scroll: 0,
            right_scroll: 0,
            input: String::new(),
            input_cursor: 0,
            input_history: Vec::new(),
            history_idx: None,
            search: String::new(),
            searching: false,
            status: String::new(),
            confirm: None,
            show_help: false,
            term_scroll: 0,
            term_id: None,
        }
    }

    pub fn mark_dirty(&self) {
        self.dirty.store(true, Ordering::Relaxed);
    }

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.status = s.into();
        self.mark_dirty();
    }

    /// 事件循环（main.rs 调用）；返回 Ok(true) 表示退出
    pub fn run(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        loop {
            // 执行核心命令队列（工作线程投来的修改）
            {
                let mut core = self.core.lock();
                if self.main_rx.drain(&mut core) {
                    self.mark_dirty();
                }
            }
            if self.dirty.swap(false, Ordering::Relaxed) {
                terminal.draw(|f| ui::draw(f, self))?;
            }
            if event::poll(Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(k) => {
                        if k.kind == crossterm::event::KeyEventKind::Press {
                            if self.on_key(k)? {
                                break;
                            }
                        }
                    }
                    Event::Mouse(m) => {
                        let sz = terminal.size().unwrap_or_default();
                        let area = ratatui::layout::Rect::new(0, 0, sz.width, sz.height);
                        self.on_mouse(m, area);
                    }
                    Event::Resize(_, _) => self.mark_dirty(),
                    _ => {}
                }
            }
        }
        Ok(())
    }

    // ── 会话操作 ──────────────────────────────────

    pub fn new_conversation(&mut self) {
        let wd = {
            let c = self.core.lock();
            c.settings.working_directory.clone()
        };
        let id = {
            let mut c = self.core.lock();
            c.store.create(wd)
        };
        self.selected_conv = Some(id);
        self.input.clear();
        self.input_cursor = 0;
        self.chat_scroll = 0;
        self.focus = Focus::Input;
        self.set_status("已新建会话");
    }

    pub fn delete_conversation(&mut self, id: &str) {
        let removed = {
            let mut c = self.core.lock();
            c.store.remove(id)
        };
        if removed {
            if self.selected_conv.as_deref() == Some(id) {
                let c = self.core.lock();
                self.selected_conv = c.store.conversations.first().map(|c| c.id.clone());
            }
            self.set_status("会话已删除");
        }
        self.confirm = None;
    }

    pub fn selected_conversation(&self) -> Option<Conversation> {
        let c = self.core.lock();
        self.selected_conv
            .as_deref()
            .and_then(|id| c.store.get(id))
            .cloned()
    }

    pub fn visible_messages(&self) -> Vec<ChatMessage> {
        let c = self.core.lock();
        self.selected_conv
            .as_deref()
            .and_then(|id| c.store.get(id))
            .map(|conv| conv.visible_messages().into_iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn is_generating(&self) -> bool {
        let c = self.core.lock();
        let gen = c.generating.lock();
        self.selected_conv
            .as_deref()
            .and_then(|id| gen.get(id))
            .map(|g| g.is_generating)
            .unwrap_or(false)
    }

    pub fn pending_approval(&self) -> bool {
        let c = self.core.lock();
        let gen = c.generating.lock();
        self.selected_conv
            .as_deref()
            .and_then(|id| gen.get(id))
            .map(|g| g.pending_approval)
            .unwrap_or(false)
    }

    /// 待确认的工具调用事件 id（扫描当前助手消息里 AwaitingApproval 的工具块）
    pub fn pending_approval_event(&self) -> Option<String> {
        for m in self.visible_messages().iter().rev() {
            if m.role != Role::Assistant {
                continue;
            }
            for b in &m.blocks {
                if let BlockKind::Tool { tool } = &b.kind {
                    if tool.status == ToolStatus::AwaitingApproval {
                        return Some(tool.id.clone());
                    }
                }
            }
        }
        None
    }

    /// 会话列表（按工作目录分组、置顶在前、搜索过滤）
    pub fn conversation_list(&self) -> Vec<Conversation> {
        let c = self.core.lock();
        let q = self.search.to_lowercase();
        let mut convs: Vec<Conversation> = c
            .store
            .conversations
            .iter()
            .filter(|c| {
                q.is_empty()
                    || c.title.to_lowercase().contains(&q)
                    || c.nodes
                        .iter()
                        .any(|m| m.text_content().to_lowercase().contains(&q))
            })
            .cloned()
            .collect();
        convs.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then(b.updated_at.cmp(&a.updated_at))
        });
        convs
    }

    // ── 发送 / 停止 ───────────────────────────────

    pub fn send(&mut self) {
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        // 没有会话则新建
        if self.selected_conv.is_none() {
            self.new_conversation();
        }
        let conv_id = match self.selected_conv.clone() {
            Some(id) => id,
            None => return,
        };
        // 记历史
        self.input_history.push(text.clone());
        self.history_idx = None;
        self.input.clear();
        self.input_cursor = 0;
        self.chat_scroll = 0;
        self.set_status("");
        // 发送（工作线程跑回合）
        let core = self.core.clone();
        AgentRunner::new(core).send(conv_id, text);
        self.mark_dirty();
    }

    pub fn stop(&mut self) {
        if let Some(id) = self.selected_conv.clone() {
            let core = self.core.clone();
            AgentRunner::new(core).stop(&id);
            self.set_status("正在停止…");
        }
    }

    pub fn interject(&mut self) {
        // 工作中发送 = 插话
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        if let Some(id) = self.selected_conv.clone() {
            let core = self.core.clone();
            AgentRunner::new(core).interject(&id, text);
            self.input.clear();
            self.input_cursor = 0;
            self.set_status("已作为补充排队");
        }
    }

    // ── 终端 ──────────────────────────────────────

    pub fn ensure_terminal(&mut self) -> Option<String> {
        // 已有终端就用最近的
        {
            let c = self.core.lock();
            let first = c.terminal_manager.lock().list().first().map(|t| t.id.clone());
            if let Some(id) = first {
                self.term_id = Some(id.clone());
                return Some(id);
            }
        }
        let id = {
            let mut c = self.core.lock();
            let created = c.terminal_manager.lock().create(None, None, 80, 24).ok();
            created
        };
        let id = id?;
        self.term_id = Some(id.clone());
        Some(id)
    }

    // ── 键盘 ──────────────────────────────────────

    /// 处理一个按键；返回 true = 退出
    pub fn on_key(&mut self, k: KeyEvent) -> anyhow::Result<bool> {
        // 全局确认框优先
        if self.confirm.is_some() {
            return self.on_confirm_key(k);
        }
        if self.show_help {
            if k.code == KeyCode::Esc || k.code == KeyCode::F(1) {
                self.show_help = false;
                self.mark_dirty();
            }
            return Ok(false);
        }
        // 全局快捷键（Ctrl 组合）
        if k.modifiers.contains(KeyModifiers::CONTROL) {
            match k.code {
                KeyCode::Char('q') => return Ok(true),
                KeyCode::Char('n') => { self.new_conversation(); return Ok(false); }
                KeyCode::Char('t') => { self.open_terminal(); return Ok(false); }
                KeyCode::Char('c') => {
                    if self.is_generating() { self.stop(); } else { return Ok(true); }
                    return Ok(false);
                }
                KeyCode::Char('s') => {
                    self.focus = Focus::Sidebar;
                    self.searching = true;
                    self.mark_dirty();
                    return Ok(false);
                }
                // 待确认调用：y 批准 / n 拒绝（无修饰键时）
                KeyCode::Char('y') if self.pending_approval() => {
                    if let (Some(conv), Some(event_id)) = (self.selected_conv.clone(), self.pending_approval_event()) {
                        let core = self.core.clone();
                        AgentRunner::new(core).resolve_approval(&conv, &event_id, true);
                        self.set_status("已允许调用");
                    }
                    return Ok(false);
                }
                KeyCode::Char('n') if self.pending_approval() => {
                    if let (Some(conv), Some(event_id)) = (self.selected_conv.clone(), self.pending_approval_event()) {
                        let core = self.core.clone();
                        AgentRunner::new(core).resolve_approval(&conv, &event_id, false);
                        self.set_status("已拒绝调用");
                    }
                    return Ok(false);
                }
                KeyCode::Char('1') => { self.right_tab = RightTab::AgentTree; self.focus = Focus::RightPanel; self.mark_dirty(); return Ok(false); }
                KeyCode::Char('2') => { self.right_tab = RightTab::Terminal; self.focus = Focus::RightPanel; self.mark_dirty(); return Ok(false); }
                KeyCode::Char('3') => { self.right_tab = RightTab::Board; self.focus = Focus::RightPanel; self.mark_dirty(); return Ok(false); }
                KeyCode::Char('4') => { self.right_tab = RightTab::Assistant; self.focus = Focus::RightPanel; self.mark_dirty(); return Ok(false); }
                KeyCode::Char('5') => { self.right_tab = RightTab::Settings; self.focus = Focus::RightPanel; self.mark_dirty(); return Ok(false); }
                KeyCode::Tab => { self.cycle_right_tab(1); return Ok(false); }
                KeyCode::BackTab => { self.cycle_right_tab(RightTab::all().len() - 1); return Ok(false); }
                // P3：循环自主模式（自主 → 谨慎 → 全自动）
                KeyCode::Char('g') => {
                    let (label, saved) = {
                        let mut c = self.core.lock();
                        c.settings.autonomy_mode = c.settings.autonomy_mode.cycle();
                        let label = c.settings.autonomy_mode.label().to_string();
                        let saved = c.settings.save(&c.paths.settings_file()).is_ok();
                        (label, saved)
                    };
                    self.set_status(format!("自主级别：{}{}（Ctrl+G 切换）", label, if saved { "" } else { "（保存失败）" }));
                    return Ok(false);
                }
                // P3：交付自检开关
                KeyCode::Char('d') => {
                    let (on, saved) = {
                        let mut c = self.core.lock();
                        c.settings.delivery_check = !c.settings.delivery_check;
                        let on = c.settings.delivery_check;
                        let saved = c.settings.save(&c.paths.settings_file()).is_ok();
                        (on, saved)
                    };
                    self.set_status(format!("交付自检：{}{}（Ctrl+D 切换）", if on { "开" } else { "关" }, if saved { "" } else { "（保存失败）" }));
                    return Ok(false);
                }
                KeyCode::Char('j') if self.focus == Focus::Input => {
                    self.insert_char('\n');
                    return Ok(false);
                }
                _ => {}
            }
        }
        match self.focus {
            Focus::Input => self.on_input_key(k),
            Focus::Sidebar => self.on_sidebar_key(k),
            Focus::Chat => self.on_chat_key(k),
            Focus::RightPanel => self.on_right_key(k),
        }
        Ok(false)
    }

    fn cycle_right_tab(&mut self, step: usize) {
        let all = RightTab::all();
        let idx = all.iter().position(|t| *t == self.right_tab).unwrap_or(0);
        self.right_tab = all[(idx + step) % all.len()];
        self.mark_dirty();
    }

    fn on_confirm_key(&mut self, k: KeyEvent) -> anyhow::Result<bool> {
        match k.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                if let Some(Confirm::DeleteConversation(id)) = self.confirm.clone() {
                    self.delete_conversation(&id);
                } else {
                    return Ok(true); // 确认退出
                }
            }
            _ => {}
        }
        self.confirm = None;
        self.mark_dirty();
        Ok(false)
    }

    fn on_input_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Enter => {
                if self.is_generating() {
                    self.interject();
                } else {
                    self.send();
                }
            }
            KeyCode::Up => {
                if !self.input_history.is_empty() {
                    let idx = match self.history_idx {
                        Some(i) if i > 0 => i - 1,
                        _ => self.input_history.len() - 1,
                    };
                    self.history_idx = Some(idx);
                    self.input = self.input_history[idx].clone();
                    self.input_cursor = self.input.chars().count();
                }
            }
            KeyCode::Down => {
                if let Some(idx) = self.history_idx {
                    if idx + 1 < self.input_history.len() {
                        self.history_idx = Some(idx + 1);
                        self.input = self.input_history[idx + 1].clone();
                    } else {
                        self.history_idx = None;
                        self.input.clear();
                    }
                    self.input_cursor = self.input.chars().count();
                }
            }
            KeyCode::Left => {
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                }
            }
            KeyCode::Right => {
                if self.input_cursor < self.input.chars().count() {
                    self.input_cursor += 1;
                }
            }
            KeyCode::Home => self.input_cursor = 0,
            KeyCode::End => self.input_cursor = self.input.chars().count(),
            KeyCode::Backspace => {
                if self.input_cursor > 0 {
                    let mut chars: Vec<char> = self.input.chars().collect();
                    chars.remove(self.input_cursor - 1);
                    self.input = chars.into_iter().collect();
                    self.input_cursor -= 1;
                }
            }
            KeyCode::Delete => {
                let mut chars: Vec<char> = self.input.chars().collect();
                if self.input_cursor < chars.len() {
                    chars.remove(self.input_cursor);
                    self.input = chars.into_iter().collect();
                }
            }
            KeyCode::Char(c) => self.insert_char(c),
            KeyCode::Esc => {
                self.input.clear();
                self.input_cursor = 0;
            }
            KeyCode::Tab => {
                self.focus = Focus::Sidebar;
            }
            _ => {}
        }
        self.mark_dirty();
    }

    fn insert_char(&mut self, c: char) {
        let mut chars: Vec<char> = self.input.chars().collect();
        chars.insert(self.input_cursor, c);
        self.input = chars.into_iter().collect();
        self.input_cursor += 1;
    }

    fn on_sidebar_key(&mut self, k: KeyEvent) {
        let count = self.conversation_list().len();
        match k.code {
            KeyCode::Char(c) if self.searching => {
                self.search.push(c);
            }
            KeyCode::Backspace if self.searching => {
                self.search.pop();
            }
            KeyCode::Esc if self.searching => {
                self.searching = false;
                self.search.clear();
                self.focus = Focus::Input;
            }
            KeyCode::Enter => {
                self.searching = false;
                self.focus = Focus::Input;
            }
            KeyCode::Up => {
                if self.selected_conv.is_some() && self.sidebar_scroll > 0 {
                    self.sidebar_scroll -= 1;
                }
            }
            KeyCode::Down => {
                if self.sidebar_scroll + 1 < count {
                    self.sidebar_scroll += 1;
                }
            }
            KeyCode::Tab => self.focus = Focus::Chat,
            KeyCode::Char('d') => {
                if let Some(id) = self.selected_conv.clone() {
                    self.confirm = Some(Confirm::DeleteConversation(id));
                }
            }
            KeyCode::Char('r') => {
                if let Some(id) = self.selected_conv.clone() {
                    let pinned = {
                        let mut c = self.core.lock();
                        let pinned = c.store.get(&id).map(|c| c.pinned).unwrap_or(false);
                        c.store.set_pinned(&id, !pinned);
                        pinned
                    };
                    self.set_status(if pinned { "已取消置顶" } else { "已置顶" });
                }
            }
            _ => {}
        }
        // 方向键选择会话
        if matches!(k.code, KeyCode::Up | KeyCode::Down) {
            let list = self.conversation_list();
            if let Some(c) = list.get(self.sidebar_scroll) {
                self.selected_conv = Some(c.id.clone());
                self.chat_scroll = 0;
            }
        }
        self.mark_dirty();
    }

    fn on_chat_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Up | KeyCode::PageUp => {
                self.chat_scroll = self.chat_scroll.saturating_add(3);
            }
            KeyCode::Down | KeyCode::PageDown => {
                self.chat_scroll = self.chat_scroll.saturating_sub(3);
            }
            KeyCode::Tab => self.focus = Focus::RightPanel,
            KeyCode::Esc => self.focus = Focus::Input,
            KeyCode::Char('f') => {
                // fork 最后一条助手消息
                if let (Some(conv_id), Some(msg)) = (
                    self.selected_conv.clone(),
                    self.visible_messages().into_iter().rev().find(|m| m.role == Role::Assistant),
                ) {
                    {
                        let mut c = self.core.lock();
                        c.store.fork(&conv_id, &msg.id);
                    }
                    self.set_status("已 fork");
                }
            }
            _ => {}
        }
        self.mark_dirty();
    }

    fn on_right_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Up | KeyCode::PageUp => self.right_scroll = self.right_scroll.saturating_add(1),
            KeyCode::Down | KeyCode::PageDown => self.right_scroll = self.right_scroll.saturating_sub(1),
            KeyCode::Tab => self.focus = Focus::Input,
            KeyCode::Esc => self.focus = Focus::Input,
            KeyCode::Char('s') if self.right_tab == RightTab::AgentTree => {
                // 停止选中的子 Agent
                if let Some(run_id) = self.selected_agent_run() {
                    {
                        let mut c = self.core.lock();
                        c.team.lock().stop_agent(&run_id);
                    }
                    self.set_status("已停止子 Agent");
                }
            }
            _ => {}
        }
        self.mark_dirty();
    }

    pub fn selected_agent_run(&self) -> Option<String> {
        let c = self.core.lock();
        let team = c.team.lock();
        team.all_runs().first().map(|r| r.id.clone())
    }

    pub fn open_terminal(&mut self) {
        self.right_tab = RightTab::Terminal;
        self.focus = Focus::RightPanel;
        self.ensure_terminal();
        self.set_status("终端已打开（右栏）");
    }

    // ── 鼠标 ──────────────────────────────────────

    pub fn on_mouse(&mut self, m: MouseEvent, area: ratatui::layout::Rect) {
        match m.kind {
            MouseEventKind::ScrollUp => match self.focus {
                Focus::Chat => self.chat_scroll = self.chat_scroll.saturating_add(2),
                Focus::RightPanel => self.right_scroll = self.right_scroll.saturating_add(1),
                _ => self.sidebar_scroll = self.sidebar_scroll.saturating_add(1),
            },
            MouseEventKind::ScrollDown => match self.focus {
                Focus::Chat => self.chat_scroll = self.chat_scroll.saturating_sub(2),
                Focus::RightPanel => self.right_scroll = self.right_scroll.saturating_sub(1),
                _ => self.sidebar_scroll = self.sidebar_scroll.saturating_sub(1),
            },
            MouseEventKind::Down(_btn) => self.on_click(m.column, m.row, area),
            _ => {}
        }
    }

    /// 点击命中：坐标判定与 ui::draw 的布局完全一致（单一来源 ui::SIDEBAR_W / ui::RIGHT_W）。
    /// 布局：header y=0；main y∈[1,h-4)；输入框 y∈[h-4,h-1)；状态栏 y=h-1；
    /// 主区横向：侧栏 x∈[0,28)、对话 x∈[28,w-42)、右栏 x∈[w-42,w)。
    fn on_click(&mut self, x: u16, y: u16, area: ratatui::layout::Rect) {
        let (w, h) = (area.width, area.height);
        // 顶部标题行 / 底部状态栏：无点击行为
        if y == 0 || y + 1 >= h {
            return;
        }
        // 底部输入框（3 行）：聚焦输入框，点击后可直接打字
        if y + 4 >= h {
            self.focus = Focus::Input;
            self.searching = false;
            self.mark_dirty();
            return;
        }
        // 主区：按真实布局边界判定
        if x < ui::SIDEBAR_W {
            // 侧栏：搜索框（y∈[1,4)）进搜索；会话列表条目（y≥5，每条 2 行）选中
            self.focus = Focus::Sidebar;
            if y < 4 {
                self.searching = true;
            } else if y >= 5 {
                let idx = ((y - 5) / 2) as usize;
                let convs = self.conversation_list();
                if let Some(c) = convs.get(idx) {
                    self.selected_conv = Some(c.id.clone());
                    self.sidebar_scroll = idx;
                }
            }
        } else if x >= w.saturating_sub(ui::RIGHT_W) {
            // 右栏：页签行（chunk y∈[1,4)，文字在 y=2）切页签；其余聚焦右栏
            self.focus = Focus::RightPanel;
            self.searching = false;
            if y == 2 {
                let rx = x.saturating_sub(w - ui::RIGHT_W);
                // 页签从列 1 起，5 个均分（每格约 8 列）
                let i = (rx.saturating_sub(1) / 8).min(4) as usize;
                self.right_tab = RightTab::all()[i];
            }
        } else {
            // 对话区
            self.focus = Focus::Chat;
            self.searching = false;
        }
        self.mark_dirty();
    }

    // ── 帮助 ──────────────────────────────────────

    pub fn help_lines() -> Vec<(String, String)> {
        vec![
            ("Ctrl+N".into(), "新建会话".into()),
            ("Enter".into(), "发送（生成中 = 插话补充）".into()),
            ("Ctrl+J".into(), "输入框内换行".into()),
            ("Tab".into(), "切换焦点区".into()),
            ("Ctrl+1/2/3/4".into(), "右栏：Agent 树 / 终端 / 状态板 / 助手".into()),
            ("Ctrl+T".into(), "打开终端".into()),
            ("Ctrl+S".into(), "搜索会话".into()),
            ("Ctrl+C".into(), "停止生成（无生成时退出）".into()),
            ("Ctrl+Q".into(), "退出".into()),
            ("F1".into(), "本帮助".into()),
            ("侧边栏".into(), "↑↓ 选择 · d 删除 · r 置顶".into()),
            ("对话区".into(), "↑↓ 滚动 · f fork".into()),
            ("Agent 树".into(), "↑↓ 滚动 · s 停止选中".into()),
        ]
    }

    pub fn focus_style(&self, f: Focus) -> Style {
        if self.focus == f {
            Style::default().fg(Theme::accent()).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Theme::dim())
        }
    }
}

/// 入口：装载核心（main.rs 调用）
pub fn load_core() -> anyhow::Result<Arc<Mutex<AppCore>>> {
    let paths = Paths::resolve();
    let (main_tx, _main_rx) = sophon_core::util::main_thread::channel();
    let core = Arc::new(Mutex::new(AppCore::load(paths, main_tx)?));
    core.lock().set_secret_reader();
    Ok(core)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    fn test_app(name: &str) -> (App, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("sophon_tui_test_{}_{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut paths = sophon_core::util::Paths::resolve();
        paths.data_dir = dir.join("data");
        paths.cache_dir = dir.join("cache");
        let (mt, rx) = sophon_core::util::main_thread::channel();
        let core = Arc::new(Mutex::new(AppCore::load(paths, mt).unwrap()));
        core.lock().set_secret_reader();
        let app = App::new(core, rx, Arc::new(AtomicBool::new(true)));
        (app, dir)
    }

    const SIZE: Rect = Rect { x: 0, y: 0, width: 120, height: 36 };

    #[test]
    fn click_input_bar_focuses_input() {
        let (mut app, dir) = test_app("input");
        app.focus = Focus::Chat;
        // 底部输入框：y∈[32,35)（h=36 时）
        app.on_click(60, 33, SIZE);
        assert_eq!(app.focus, Focus::Input);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn click_regions_match_layout() {
        let (mut app, dir) = test_app("regions");
        // 对话区
        app.on_click(60, 10, SIZE);
        assert_eq!(app.focus, Focus::Chat);
        // 右栏（从 120-42=78 列起）
        app.on_click(100, 10, SIZE);
        assert_eq!(app.focus, Focus::RightPanel);
        // 侧栏（0..28 列）
        app.on_click(10, 10, SIZE);
        assert_eq!(app.focus, Focus::Sidebar);
        // 回归旧 bug：x=80 在旧比例逻辑（0.78*120≈93.6）下会被误判成对话区
        app.on_click(80, 10, SIZE);
        assert_eq!(app.focus, Focus::RightPanel);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn click_tabs_switches_tab() {
        let (mut app, dir) = test_app("tabs");
        app.on_click(85, 2, SIZE); // 第 1 个页签：Agent 树
        assert_eq!(app.focus, Focus::RightPanel);
        assert_eq!(app.right_tab, RightTab::AgentTree);
        app.on_click(100, 2, SIZE); // rx=22 → 第 3 个页签：状态板
        assert_eq!(app.right_tab, RightTab::Board);
        app.on_click(119, 2, SIZE); // rx=41 → 第 5 个页签：设置
        assert_eq!(app.right_tab, RightTab::Settings);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn click_search_box_enters_search() {
        let (mut app, dir) = test_app("search");
        app.on_click(10, 2, SIZE); // 侧栏搜索框（y∈[1,4)）
        assert!(app.searching);
        assert_eq!(app.focus, Focus::Sidebar);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn click_header_and_status_ignored() {
        let (mut app, dir) = test_app("hdr");
        app.focus = Focus::Chat;
        app.on_click(60, 0, SIZE); // 标题栏
        assert_eq!(app.focus, Focus::Chat);
        app.on_click(60, 35, SIZE); // 状态栏（y=h-1）
        assert_eq!(app.focus, Focus::Chat);
        assert!(!app.searching);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn scroll_in_chat_adjusts_chat_scroll() {
        let (mut app, dir) = test_app("scroll");
        app.focus = Focus::Chat;
        app.chat_scroll = 4;
        let ev = MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 60,
            row: 10,
            modifiers: KeyModifiers::NONE,
        };
        app.on_mouse(ev, SIZE);
        assert_eq!(app.chat_scroll, 2);
        let _ = std::fs::remove_dir_all(dir);
    }
}
