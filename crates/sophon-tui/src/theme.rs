// 配色：跟随终端深浅色。Catppuccin Mocha（深色）/ Latte（浅色）。
use ratatui::style::{Color, Modifier};

/// Catppuccin 配色主题（深色 Mocha / 浅色 Latte）
pub struct Theme;

impl Theme {
    /// 背景（base）
    pub fn base() -> Color {
        if is_dark() { Color::Rgb(17, 17, 27) } else { Color::Rgb(239, 240, 246) }
    }
    /// 面板背景（crust）
    pub fn crust() -> Color {
        if is_dark() { Color::Rgb(11, 11, 18) } else { Color::Rgb(229, 230, 238) }
    }
    /// 强调色（accent / mauve）
    pub fn accent() -> Color {
        if is_dark() { Color::Rgb(197, 160, 240) } else { Color::Rgb(136, 93, 228) }
    }
    /// 次级文字（subtext）
    pub fn subtext() -> Color {
        if is_dark() { Color::Rgb(166, 173, 200) } else { Color::Rgb(77, 87, 117) }
    }
    /// 弱化文字（dim / overlay）
    pub fn dim() -> Color {
        if is_dark() { Color::Rgb(108, 112, 134) } else { Color::Rgb(154, 157, 175) }
    }
    /// 青色（teal）
    pub fn teal() -> Color {
        if is_dark() { Color::Rgb(148, 226, 213) } else { Color::Rgb(1, 163, 150) }
    }
    /// 紫色（purple）
    pub fn purple() -> Color {
        if is_dark() { Color::Rgb(197, 160, 240) } else { Color::Rgb(136, 93, 228) }
    }
    /// 橙色（peach）
    pub fn orange() -> Color {
        if is_dark() { Color::Rgb(250, 179, 135) } else { Color::Rgb(214, 117, 20) }
    }
    /// 黄色（yellow）
    pub fn yellow() -> Color {
        if is_dark() { Color::Rgb(249, 226, 175) } else { Color::Rgb(179, 136, 20) }
    }
    /// 绿色（green）
    pub fn green() -> Color {
        if is_dark() { Color::Rgb(166, 227, 161) } else { Color::Rgb(40, 143, 61) }
    }
    /// 红色（red）
    pub fn red() -> Color {
        if is_dark() { Color::Rgb(243, 139, 168) } else { Color::Rgb(204, 36, 29) }
    }
    /// 文字色（text）
    pub fn text() -> Color {
        if is_dark() { Color::Rgb(198, 203, 235) } else { Color::Rgb(76, 79, 105) }
    }
    /// 索引色（16 色板）
    pub fn indexed(i: u8) -> Color {
        match i {
            0 => Color::Black,
            1 => Color::Red,
            2 => Color::Green,
            3 => Color::Yellow,
            4 => Color::Blue,
            5 => Color::Magenta,
            6 => Color::Cyan,
            7 => Color::White,
            8 => Color::DarkGray,
            9 => Color::LightRed,
            10 => Color::LightGreen,
            11 => Color::LightYellow,
            12 => Color::LightBlue,
            13 => Color::LightMagenta,
            14 => Color::LightCyan,
            15 => Color::Gray,
            _ => Color::Reset,
        }
    }
}

/// 跟随终端深浅色（默认深色）
pub fn is_dark() -> bool {
    true
}

/// RGB 颜色
pub fn color_rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

/// 工具状态色（运行中蓝 / 成功绿 / 失败红 / 待确认黄 / 已停止灰）
pub fn tool_status_color(running: bool, ok: bool, err: bool) -> Color {
    if err {
        Color::Red
    } else if ok {
        Color::Green
    } else if running {
        Color::Blue
    } else {
        Color::DarkGray
    }
}

pub const BOLD: Modifier = Modifier::BOLD;
