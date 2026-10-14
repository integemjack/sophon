// 内置终端：PTY（Windows ConPTY）+ VT 模拟器 + 管理器。
// 对应 docs/TERMINAL.md：格子/颜色/滚动缓冲 5000 行/标题/标签页。
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;
use vte::{Params, Perform};

/// 颜色（8 基本 + 亮色 + RGB + 默认）
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Color {
    Default,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
    Rgb(u8, u8, u8),
}

impl Color {
    /// 转成 UI 颜色（由界面层映射到具体调色板）
    pub fn index(&self) -> (u8, Option<(u8, u8, u8)>) {
        match self {
            Color::Default => (255, None),
            Color::Black => (0, None),
            Color::Red => (1, None),
            Color::Green => (2, None),
            Color::Yellow => (3, None),
            Color::Blue => (4, None),
            Color::Magenta => (5, None),
            Color::Cyan => (6, None),
            Color::White => (7, None),
            Color::BrightBlack => (8, None),
            Color::BrightRed => (9, None),
            Color::BrightGreen => (10, None),
            Color::BrightYellow => (11, None),
            Color::BrightBlue => (12, None),
            Color::BrightMagenta => (13, None),
            Color::BrightCyan => (14, None),
            Color::BrightWhite => (15, None),
            Color::Rgb(r, g, b) => (255, Some((*r, *g, *b))),
        }
    }
}

#[derive(Clone, Debug)]
struct Cell {
    ch: char,
    fg: Color,
    bg: Color,
    bold: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Cell { ch: ' ', fg: Color::Default, bg: Color::Default, bold: false }
    }
}

/// 一行（用于渲染：合并相邻同色格子）
#[derive(Clone, Debug)]
pub struct RenderSpan {
    pub text: String,
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
}

const SCROLLBACK: usize = 5000;

/// VT 模拟器（vte::Perform）
pub struct TerminalEmulator {
    pub cols: usize,
    pub rows: usize,
    screen: Vec<Vec<Cell>>,
    scrollback: Vec<Vec<Cell>>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    cur_fg: Color,
    cur_bg: Color,
    cur_bold: bool,
    pub title: String,
    /// 鼠标追踪模式（报告给 Agent；本实现不真正捕获鼠标）
    pub mouse_tracking: bool,
    saved_cursor: (usize, usize),
}

impl TerminalEmulator {
    pub fn new(cols: usize, rows: usize) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let screen = (0..rows).map(|_| vec![Cell::default(); cols]).collect();
        TerminalEmulator {
            cols,
            rows,
            screen,
            scrollback: Vec::new(),
            cursor_row: 0,
            cursor_col: 0,
            cur_fg: Color::Default,
            cur_bg: Color::Default,
            cur_bold: false,
            title: String::new(),
            mouse_tracking: false,
            saved_cursor: (0, 0),
        }
    }

    fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(1);
        let rows = rows.max(1);
        self.cols = cols;
        self.rows = rows;
        for row in &mut self.screen {
            row.resize(cols, Cell::default());
        }
        while self.screen.len() < rows {
            self.screen.push(vec![Cell::default(); cols]);
        }
        self.cursor_row = self.cursor_row.min(rows - 1);
        self.cursor_col = self.cursor_col.min(cols - 1);
    }

    fn scroll_up(&mut self) {
        let first = self.screen.remove(0);
        self.scrollback.push(first);
        if self.scrollback.len() > SCROLLBACK {
            self.scrollback.remove(0);
        }
        self.screen.push(vec![Cell::default(); self.cols]);
    }

    fn put_char(&mut self, ch: char) {
        if self.cursor_col >= self.cols {
            self.cursor_col = 0;
            self.cursor_row += 1;
            if self.cursor_row >= self.rows {
                self.scroll_up();
                self.cursor_row = self.rows - 1;
            }
        }
        if self.cursor_row < self.rows && self.cursor_col < self.cols {
            let cell = &mut self.screen[self.cursor_row][self.cursor_col];
            cell.ch = ch;
            cell.fg = self.cur_fg;
            cell.bg = self.cur_bg;
            cell.bold = self.cur_bold;
        }
        self.cursor_col += 1;
    }

    fn newline(&mut self) {
        self.cursor_row += 1;
        if self.cursor_row >= self.rows {
            self.scroll_up();
            self.cursor_row = self.rows - 1;
        }
    }

    fn carriage_return(&mut self) {
        self.cursor_col = 0;
    }

    fn backspace(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        }
    }

    fn erase_in_line(&mut self, mode: u16) {
        if self.cursor_row >= self.rows {
            return;
        }
        let row = &mut self.screen[self.cursor_row];
        match mode {
            0 => {
                // 光标到行尾
                for c in row.iter_mut().skip(self.cursor_col) {
                    *c = Cell::default();
                }
            }
            1 => {
                for c in row.iter_mut().take(self.cursor_col + 1) {
                    *c = Cell::default();
                }
            }
            2 => {
                for c in row.iter_mut() {
                    *c = Cell::default();
                }
            }
            _ => {}
        }
    }

    fn erase_in_screen(&mut self, mode: u16) {
        match mode {
            0 => {
                if self.cursor_row < self.rows {
                    for c in self.screen[self.cursor_row].iter_mut().skip(self.cursor_col) {
                        *c = Cell::default();
                    }
                    for row in self.screen.iter_mut().skip(self.cursor_row + 1) {
                        for c in row.iter_mut() {
                            *c = Cell::default();
                        }
                    }
                }
            }
            2 => {
                for row in &mut self.screen {
                    for c in row.iter_mut() {
                        *c = Cell::default();
                    }
                }
            }
            _ => {}
        }
    }

    fn set_cursor(&mut self, row: usize, col: usize) {
        self.cursor_row = row.min(self.rows - 1);
        self.cursor_col = col.min(self.cols - 1);
    }

    /// 可见屏 + 滚动缓冲的渲染行（从 scrollback 末尾取 scroll 行）
    pub fn render_lines(&self, scroll: usize) -> Vec<Vec<RenderSpan>> {
        let mut all: Vec<&Vec<Cell>> = Vec::new();
        let take_from_scrollback = scroll.min(self.scrollback.len());
        if take_from_scrollback > 0 {
            let start = self.scrollback.len() - take_from_scrollback;
            for line in &self.scrollback[start..] {
                all.push(line);
            }
        }
        for line in &self.screen {
            all.push(line);
        }
        all.into_iter()
            .map(|line| {
                let mut spans: Vec<RenderSpan> = Vec::new();
                for cell in line {
                    if let Some(last) = spans.last_mut() {
                        if last.fg == cell.fg
                            && last.bg == cell.bg
                            && last.bold == cell.bold
                            && !cell.ch.is_control()
                        {
                            last.text.push(cell.ch);
                            continue;
                        }
                    }
                    spans.push(RenderSpan {
                        text: cell.ch.to_string(),
                        fg: cell.fg,
                        bg: cell.bg,
                        bold: cell.bold,
                    });
                }
                spans
            })
            .collect()
    }

    pub fn visible_lines(&self) -> Vec<Vec<RenderSpan>> {
        self.render_lines(0)
    }
}

impl Perform for TerminalEmulator {
    fn print(&mut self, c: char) {
        self.put_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' | 0x0b | 0x0c => self.newline(),
            b'\r' => self.carriage_return(),
            b'\t' => {
                let tab = 8;
                let next = (self.cursor_col / tab + 1) * tab;
                self.cursor_col = next.min(self.cols - 1);
            }
            0x08 => self.backspace(),
            0x07 => {} // bell：忽略
            _ => {}
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        // OSC 0 / 2：标题
        if let Some(first) = params.first() {
            if let Ok(s) = std::str::from_utf8(first) {
                // 形如 "0;标题" 或 "2;标题"
                let title = s.splitn(2, ';').nth(1).unwrap_or(s).trim();
                if !title.is_empty() {
                    self.title = title.to_string();
                }
            }
        }
    }

    fn csi_dispatch(&mut self, params: &Params, _intermediates: &[u8], _ignore: bool, action: char) {
        let params: Vec<u16> = params.iter().map(|p| p.first().copied().unwrap_or(0)).collect();
        let first = params.first().copied().unwrap_or(1);
        match action {
            'H' | 'f' => {
                // 光标定位（1-based）
                let row = params.first().copied().unwrap_or(1).max(1) - 1;
                let col = params.get(1).copied().unwrap_or(1).max(1) - 1;
                self.set_cursor(row as usize, col as usize);
            }
            'A' => {
                let n = first.max(1) as usize;
                self.cursor_row = self.cursor_row.saturating_sub(n);
            }
            'B' => {
                let n = first.max(1) as usize;
                self.cursor_row = (self.cursor_row + n).min(self.rows - 1);
            }
            'C' => {
                let n = first.max(1) as usize;
                self.cursor_col = (self.cursor_col + n).min(self.cols - 1);
            }
            'D' => {
                let n = first.max(1) as usize;
                self.cursor_col = self.cursor_col.saturating_sub(n);
            }
            'G' => {
                self.cursor_col = first.max(1) as usize - 1;
                self.cursor_col = self.cursor_col.min(self.cols - 1);
            }
            'J' => self.erase_in_screen(first),
            'K' => self.erase_in_line(first),
            'm' => {
                // SGR：颜色/粗体
                if params.is_empty() {
                    self.cur_fg = Color::Default;
                    self.cur_bg = Color::Default;
                    self.cur_bold = false;
                    return;
                }
                let mut i = 0;
                while i < params.len() {
                    let p = params[i];
                    match p {
                        0 => {
                            self.cur_fg = Color::Default;
                            self.cur_bg = Color::Default;
                            self.cur_bold = false;
                        }
                        1 => self.cur_bold = true,
                        22 => self.cur_bold = false,
                        30..=37 => self.cur_fg = match p {
                            30 => Color::Black,
                            31 => Color::Red,
                            32 => Color::Green,
                            33 => Color::Yellow,
                            34 => Color::Blue,
                            35 => Color::Magenta,
                            36 => Color::Cyan,
                            _ => Color::White,
                        },
                        38 => {
                            // 38;5;N 或 38;2;R;G;B
                            if i + 1 < params.len() {
                                match params[i + 1] {
                                    5 => {
                                        if i + 2 < params.len() {
                                            self.cur_fg = palette256(params[i + 2] as usize);
                                            i += 2;
                                        }
                                    }
                                    2 => {
                                        if i + 4 < params.len() {
                                            self.cur_fg = Color::Rgb(
                                                params[i + 2] as u8,
                                                params[i + 3] as u8,
                                                params[i + 4] as u8,
                                            );
                                            i += 4;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        39 => self.cur_fg = Color::Default,
                        40..=47 => self.cur_bg = match p {
                            40 => Color::Black,
                            41 => Color::Red,
                            42 => Color::Green,
                            43 => Color::Yellow,
                            44 => Color::Blue,
                            45 => Color::Magenta,
                            46 => Color::Cyan,
                            _ => Color::White,
                        },
                        48 => {
                            if i + 1 < params.len() {
                                match params[i + 1] {
                                    5 => {
                                        if i + 2 < params.len() {
                                            self.cur_bg = palette256(params[i + 2] as usize);
                                            i += 2;
                                        }
                                    }
                                    2 => {
                                        if i + 4 < params.len() {
                                            self.cur_bg = Color::Rgb(
                                                params[i + 2] as u8,
                                                params[i + 3] as u8,
                                                params[i + 4] as u8,
                                            );
                                            i += 4;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        49 => self.cur_bg = Color::Default,
                        90..=97 => self.cur_fg = match p {
                            90 => Color::BrightBlack,
                            91 => Color::BrightRed,
                            92 => Color::BrightGreen,
                            93 => Color::BrightYellow,
                            94 => Color::BrightBlue,
                            95 => Color::BrightMagenta,
                            96 => Color::BrightCyan,
                            _ => Color::BrightWhite,
                        },
                        100..=107 => self.cur_bg = match p {
                            100 => Color::BrightBlack,
                            101 => Color::BrightRed,
                            102 => Color::BrightGreen,
                            103 => Color::BrightYellow,
                            104 => Color::BrightBlue,
                            105 => Color::BrightMagenta,
                            106 => Color::BrightCyan,
                            _ => Color::BrightWhite,
                        },
                        _ => {}
                    }
                    i += 1;
                }
            }
            'h' => {
                // 设置模式：2004 bracketed paste、1000/1002/1003 mouse
                for &p in &params {
                    if p == 1000 || p == 1002 || p == 1003 {
                        self.mouse_tracking = true;
                    }
                }
            }
            'l' => {
                for &p in &params {
                    if p == 1000 || p == 1002 || p == 1003 {
                        self.mouse_tracking = false;
                    }
                }
            }
            's' => self.saved_cursor = (self.cursor_row, self.cursor_col),
            'u' => {
                let (r, c) = self.saved_cursor;
                self.set_cursor(r, c);
            }
            'n' => {} // 设备状态报告：简化不回应
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        match byte {
            b'7' => self.saved_cursor = (self.cursor_row, self.cursor_col),
            b'8' => {
                let (r, c) = self.saved_cursor;
                self.set_cursor(r, c);
            }
            _ => {}
        }
    }
}

fn palette256(n: usize) -> Color {
    match n {
        0..=7 => match n {
            0 => Color::Black,
            1 => Color::Red,
            2 => Color::Green,
            3 => Color::Yellow,
            4 => Color::Blue,
            5 => Color::Magenta,
            6 => Color::Cyan,
            _ => Color::White,
        },
        8..=15 => match n {
            8 => Color::BrightBlack,
            9 => Color::BrightRed,
            10 => Color::BrightGreen,
            11 => Color::BrightYellow,
            12 => Color::BrightBlue,
            13 => Color::BrightMagenta,
            14 => Color::BrightCyan,
            _ => Color::BrightWhite,
        },
        16..=231 => {
            let n = n - 16;
            let r = (n / 36) % 6;
            let g = (n / 6) % 6;
            let b = n % 6;
            let f = |v: usize| -> u8 {
                if v == 0 {
                    0
                } else {
                    (55 + 40 * v) as u8
                }
            };
            Color::Rgb(f(r), f(g), f(b))
        }
        _ => {
            // 232..255 灰度
            let v = (n - 232) as u32;
            let g = (8 + v * 10) as u8;
            Color::Rgb(g, g, g)
        }
    }
}

// ── PTY 会话 ────────────────────────────────────────────

pub struct PtySession {
    pub id: String,
    pub conversation_id: Option<String>,
    pub title: String,
    pub shell: String,
    pub command: Option<String>,
    pub cols: u16,
    pub rows: u16,
    pub exit_code: Option<i32>,
    emulator: Arc<Mutex<TerminalEmulator>>,
    writer: Option<std::sync::mpsc::Sender<Vec<u8>>>,
    _reader: Option<std::thread::JoinHandle<()>>,
    master: Option<Box<dyn portable_pty::MasterPty + Send>>,
}

impl PtySession {
    /// 渲染当前屏幕（从第 from_row 行起）
    pub fn render(&self, from_row: usize) -> Vec<Vec<RenderSpan>> {
        self.emulator.lock().render_lines(from_row)
    }

    pub fn spawn(
        id: String,
        conversation_id: Option<String>,
        command: Option<String>,
        cols: u16,
        rows: u16,
    ) -> anyhow::Result<Self> {
        let pty_system = portable_pty::native_pty_system();
        let pair = pty_system.openpty(portable_pty::PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let shell = default_shell();
        let cmd = if let Some(c) = &command {
            if cfg!(target_os = "windows") {
                let mut b = portable_pty::CommandBuilder::new("cmd.exe");
                b.arg("/C");
                b.arg(c);
                b
            } else {
                let mut b = portable_pty::CommandBuilder::new("sh");
                b.arg("-c");
                b.arg(c);
                b
            }
        } else {
            if cfg!(target_os = "windows") {
                portable_pty::CommandBuilder::new("cmd.exe")
            } else {
                portable_pty::CommandBuilder::new(&shell)
            }
        };
        let _child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);
        let master = pair.master;
        let emulator = Arc::new(Mutex::new(TerminalEmulator::new(cols as usize, rows as usize)));
        let (write_tx, write_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let writer_handle = {
            let mut writer = master.take_writer()?;
            std::thread::spawn(move || {
                for data in write_rx {
                    let _ = writer.write_all(&data);
                }
            })
        };
        // 读线程
        let emu = emulator.clone();
        let reader = {
            let mut reader = master.try_clone_reader()?;
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                let mut parser = vte::Parser::new();
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            let mut emu = emu.lock();
                            for &b in &buf[..n] {
                                parser.advance(&mut *emu, b);
                            }
                        }
                        Err(_) => break,
                    }
                }
            })
        };
        let _ = writer_handle;
        Ok(PtySession {
            id,
            conversation_id,
            title: shell.clone(),
            shell,
            command,
            cols,
            rows,
            exit_code: None,
            emulator,
            writer: Some(write_tx),
            _reader: Some(reader),
            master: Some(master),
        })
    }

    pub fn write(&self, data: &[u8]) {
        if let Some(tx) = &self.writer {
            let _ = tx.send(data.to_vec());
        }
    }

    pub fn emulator(&self) -> &Arc<Mutex<TerminalEmulator>> {
        &self.emulator
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols;
        self.rows = rows;
        if let Some(master) = &self.master {
            let _ = master.resize(portable_pty::PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
        self.emulator.lock().resize(cols as usize, rows as usize);
    }
}

fn default_shell() -> String {
    if cfg!(target_os = "windows") {
        "cmd.exe".to_string()
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string())
    }
}

// ── 终端管理器 ──────────────────────────────────────────

pub struct TerminalManager {
    pub terminals: HashMap<String, Arc<Mutex<PtySession>>>,
    pub available: bool,
    pub reason: Option<String>,
}

impl TerminalManager {
    pub fn new() -> Self {
        TerminalManager {
            terminals: HashMap::new(),
            available: true,
            reason: None,
        }
    }

    pub fn create(
        &mut self,
        conversation_id: Option<String>,
        command: Option<String>,
        cols: u16,
        rows: u16,
    ) -> anyhow::Result<String> {
        let id = crate::models::new_uuid();
        let session = PtySession::spawn(id.clone(), conversation_id.clone(), command, cols, rows)?;
        let title = session.title.clone();
        self.terminals.insert(id.clone(), Arc::new(Mutex::new(session)));
        let _ = title;
        Ok(id)
    }

    pub fn remove(&mut self, id: &str) -> bool {
        self.terminals.remove(id).is_some()
    }

    pub fn get(&self, id: &str) -> Option<Arc<Mutex<PtySession>>> {
        self.terminals.get(id).cloned()
    }

    pub fn list(&self) -> Vec<TerminalInfo> {
        let mut out = Vec::new();
        for (id, session) in &self.terminals {
            let s = session.lock();
            out.push(TerminalInfo {
                id: id.clone(),
                conversation_id: s.conversation_id.clone(),
                title: s.title.clone(),
                shell: s.shell.clone(),
                command: s.command.clone(),
                running: s.exit_code.is_none(),
                exit_code: s.exit_code,
                cols: s.cols,
                rows: s.rows,
            });
        }
        out
    }
}

impl Default for TerminalManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 终端信息（REMOTE.md §5 TerminalInfo）
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TerminalInfo {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    pub title: String,
    pub shell: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub cols: u16,
    pub rows: u16,
}
