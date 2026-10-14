// 界面渲染：标题栏 / 侧边栏 / 对话区 / 右栏（Agent 树·终端·状态板·助手）/ 输入栏 / 状态栏。
use std::time::Duration;

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap};
use ratatui::Frame;

use sophon_core::models::*;
use sophon_core::terminal::Color as TermColor;

use crate::app::{App, Focus, RightTab};
use crate::markdown::render_markdown;
use crate::theme::Theme;

/// 入口
pub fn draw(f: &mut Frame, app: &App) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(f.area());
    render_title_bar(f, areas[0], app);
    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(28), Constraint::Min(0), Constraint::Length(42)])
        .split(areas[1]);
    render_sidebar(f, main[0], app);
    render_chat(f, main[1], app);
    render_right_panel(f, main[2], app);
    render_input_bar(f, areas[2], app);
    render_status_bar(f, areas[3], app);
    if app.show_help {
        render_help_overlay(f, app);
    }
    if app.confirm.is_some() {
        render_confirm_overlay(f, app);
    }
}

// ── 标题栏 ──────────────────────────────────────

fn render_title_bar(f: &mut Frame, area: Rect, app: &App) {
    let engine = {
        let c = app.core.lock();
        c.resolved_engine(&Conversation::new(None))
            .map(|e| e.title)
            .unwrap_or_else(|| "离线".to_string())
    };
    let mode = "自主";
    let left = Line::from(vec![
        Span::styled(" 智子 ", Style::default().fg(Theme::base()).bg(Theme::accent()).add_modifier(Modifier::BOLD)),
        Span::styled(" Sophon ", Style::default().fg(Theme::accent()).add_modifier(Modifier::BOLD)),
        Span::styled("· 全自主 TUI 智能体", Style::default().fg(Theme::subtext())),
    ]);
    let right = Line::from(vec![
        Span::styled(
            format!("v{} · 引擎:{} · 模式:{}", env!("CARGO_PKG_VERSION"), engine, mode),
            Style::default().fg(Theme::dim()),
        ),
        Span::raw(" "),
    ]);
    let block = Block::default().style(Style::default().bg(Theme::crust()));
    f.render_widget(block, area);
    let (w, _) = (area.width, area.height);
    let left_w = 34usize;
    let right_text = format!("v{} · 引擎:{} · 模式:{}", env!("CARGO_PKG_VERSION"), engine, mode);
    let _ = w;
    f.render_widget(
        Paragraph::new(left).style(Style::default().bg(Theme::crust())),
        Rect::new(area.x, area.y, left_w as u16, 1),
    );
    let rw = right_text.chars().count() as u16 + 2;
    f.render_widget(
        Paragraph::new(right).alignment(Alignment::Right).style(Style::default().bg(Theme::crust())),
        Rect::new(area.x + area.width.saturating_sub(rw), area.y, rw, 1),
    );
}

// ── 侧边栏 ──────────────────────────────────────

fn render_sidebar(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);
    // 搜索框
    let search_style = if app.searching {
        Style::default().fg(Theme::accent())
    } else {
        Style::default().fg(Theme::dim())
    };
    let search_text = if app.searching || !app.search.is_empty() {
        app.search.clone()
    } else {
        "搜索会话… (Ctrl+S)".to_string()
    };
    let search_block = Block::default()
        .borders(Borders::ALL)
        .border_style(app.focus_style(Focus::Sidebar))
        .title(Span::styled(" 会话 ", Style::default().fg(Theme::subtext())));
    f.render_widget(
        Paragraph::new(search_text).style(search_style).block(search_block),
        chunks[0],
    );
    // 会话列表
    let convs = app.conversation_list();
    let items: Vec<ListItem> = convs
        .iter()
        .map(|c| {
            let title = if c.pinned { format!("📌 {}", c.title) } else { c.title.clone() };
            let dir = c
                .working_directory
                .as_deref()
                .map(|d| {
                    let p = std::path::Path::new(d);
                    p.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| d.to_string())
                })
                .unwrap_or_else(|| "—".to_string());
            let time = c.updated_at.chars().skip(11).take(5).collect::<String>();
            let gen = {
                let core = app.core.lock();
                let gen = core.generating.lock();
                gen.get(&c.id).map(|g| g.is_generating).unwrap_or(false)
            };
            let dot = if gen {
                Span::styled(" ●", Style::default().fg(Theme::accent()))
            } else {
                Span::raw("")
            };
            ListItem::new(vec![
                Line::from(vec![Span::raw(truncate(&title, 24)), dot]),
                Line::styled(
                    format!("  {} · {}", dir, time),
                    Style::default().fg(Theme::dim()),
                ),
            ])
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(app.sidebar_scroll.min(convs.len().saturating_sub(1))));
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(app.focus_style(Focus::Sidebar))
                .title(Span::styled(
                    format!(" 会话 ({}) ", convs.len()),
                    Style::default().fg(Theme::subtext()),
                )),
        )
        .highlight_style(
            Style::default()
                .bg(Theme::accent())
                .fg(Theme::base())
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    f.render_stateful_widget(list, chunks[1], &mut state);
}

// ── 对话区 ──────────────────────────────────────

fn render_chat(f: &mut Frame, area: Rect, app: &App) {
    let conv = app.selected_conversation();
    let messages = app.visible_messages();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(app.focus_style(Focus::Chat))
        .title(Span::styled(
            format!(
                " 对话 · {} ",
                conv.as_ref().map(|c| truncate(&c.title, 20)).unwrap_or_else(|| "未选择".to_string())
            ),
            Style::default().fg(Theme::subtext()),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if messages.is_empty() {
        render_welcome(f, inner, app);
        return;
    }
    // 全部消息渲染成行
    let mut lines: Vec<Line> = Vec::new();
    for m in &messages {
        render_message(&mut lines, m, app);
        lines.push(Line::raw(""));
    }
    let total = lines.len();
    let view_h = inner.height as usize;
    // chat_scroll = 从底部向上滚的行数
    let bottom_offset = total.saturating_sub(view_h);
    let scroll_y = bottom_offset.saturating_sub(app.chat_scroll);
    let para = Paragraph::new(lines).wrap(Wrap { trim: true }).scroll((0, scroll_y as u16));
    f.render_widget(para, inner);
}

fn render_welcome(f: &mut Frame, area: Rect, _app: &App) {
    let lines = vec![
        Line::raw(""),
        Line::from(vec![Span::styled(
            "智子 · Sophon",
            Style::default().fg(Theme::accent()).add_modifier(Modifier::BOLD),
        )]).alignment(Alignment::Center),
        Line::styled(
            "全自主 TUI 智能体",
            Style::default().fg(Theme::subtext()),
        ).alignment(Alignment::Center),
        Line::raw(""),
        Line::styled(
            "遇到问题自己想办法解决，直到成功交付。",
            Style::default().fg(Theme::dim()),
        ).alignment(Alignment::Center),
        Line::raw(""),
        Line::styled("快速开始：", Style::default().fg(Theme::teal())).alignment(Alignment::Center),
        Line::styled("  Ctrl+N   新建会话", Style::default().fg(Theme::dim())).alignment(Alignment::Center),
        Line::styled("  Enter    发送消息（生成中 = 插话补充）", Style::default().fg(Theme::dim())).alignment(Alignment::Center),
        Line::styled("  Ctrl+J   输入框内换行", Style::default().fg(Theme::dim())).alignment(Alignment::Center),
        Line::styled("  Tab      切换焦点区", Style::default().fg(Theme::dim())).alignment(Alignment::Center),
        Line::styled("  Ctrl+1/2/3/4/5  右栏：Agent 树 / 终端 / 状态板 / 助手 / 设置   ·   Ctrl+G 自主级别   ·   Ctrl+D 交付自检", Style::default().fg(Theme::dim())).alignment(Alignment::Center),
        Line::styled("  Ctrl+T   打开终端", Style::default().fg(Theme::dim())).alignment(Alignment::Center),
        Line::styled("  F1       完整快捷键", Style::default().fg(Theme::dim())).alignment(Alignment::Center),
        Line::raw(""),
        Line::styled(
            "大脑：内置离线引擎（默认）· 可指向本机 127.0.0.1 OpenAI 兼容服务",
            Style::default().fg(Theme::dim()),
        ).alignment(Alignment::Center),
    ];
    let h = lines.len() as u16;
    let y = area.y + area.height.saturating_sub(h) / 2;
    f.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center),
        Rect::new(area.x, y, area.width, h),
    );
}

fn render_message(lines: &mut Vec<Line>, m: &ChatMessage, app: &App) {
    match m.role {
        Role::User => {
            lines.push(Line::from(vec![
                Span::styled("▶ 用户", Style::default().fg(Theme::teal()).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!("  {}", m.created_at.chars().skip(11).take(8).collect::<String>()),
                    Style::default().fg(Theme::dim()),
                ),
            ]));
            for b in &m.blocks {
                if let BlockKind::Text { text } = &b.kind {
                    for l in render_markdown(text, 80) {
                        lines.push(indent_line(l, 2));
                    }
                }
            }
        }
        Role::Assistant => {
            lines.push(Line::from(vec![
                Span::styled("● 智子", Style::default().fg(Theme::accent()).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!("  {}", m.created_at.chars().skip(11).take(8).collect::<String>()),
                    Style::default().fg(Theme::dim()),
                ),
                if m.stopped_by_user {
                    Span::styled("  （已停止）", Style::default().fg(Theme::orange()))
                } else {
                    Span::raw("")
                },
            ]));
            for b in &m.blocks {
                render_block(lines, b, app);
            }
            if let Some(stats) = &m.stats {
                let mut spans = vec![Span::styled(
                    "  ─ ",
                    Style::default().fg(Theme::dim()),
                )];
                if stats.token_count > 0 {
                    spans.push(Span::styled(
                        format!("{} tokens", stats.token_count),
                        Style::default().fg(Theme::dim()),
                    ));
                    if stats.tokens_per_second > 0.0 {
                        spans.push(Span::styled(
                            format!(" · {:.1} tok/s", stats.tokens_per_second),
                            Style::default().fg(Theme::dim()),
                        ));
                    }
                }
                if let Some(eng) = &stats.engine_name {
                    spans.push(Span::styled(
                        format!(" · {}", eng),
                        Style::default().fg(Theme::dim()),
                    ));
                }
                if spans.len() > 1 {
                    lines.push(Line::from(spans));
                }
            }
        }
        Role::Tool => {
            for b in &m.blocks {
                render_block(lines, b, app);
            }
        }
    }
}

/// 给一行加前导空格（缩进）
fn indent_line(line: Line, n: usize) -> Line {
    let pad = " ".repeat(n);
    let mut spans = vec![Span::raw(pad)];
    spans.extend(line.spans);
    Line::from(spans)
}

fn render_block(lines: &mut Vec<Line>, b: &MessageBlock, _app: &App) {
    match &b.kind {
        BlockKind::Text { text } => {
            for l in render_markdown(text, 80) {
                lines.push(indent_line(l, 2));
            }
        }
        BlockKind::Thinking { text } => {
            lines.push(Line::from(vec![
                Span::styled("  ◌ 思考 ", Style::default().fg(Theme::purple())),
                Span::styled(
                    truncate(text, 60),
                    Style::default().fg(Theme::dim()).add_modifier(Modifier::DIM),
                ),
            ]));
        }
        BlockKind::Tool { tool } => {
            render_tool_card(lines, tool);
        }
        BlockKind::Media { media } => {
            lines.push(Line::from(vec![
                Span::styled("  📎 ", Style::default().fg(Theme::yellow())),
                Span::styled(
                    format!("{} ({})", media.file_name, media.mime_type),
                    Style::default().fg(Theme::subtext()),
                ),
            ]));
        }
    }
}

fn render_tool_card(lines: &mut Vec<Line>, tool: &ToolEvent) {
    let (icon, color) = match tool.status {
        ToolStatus::Running => ("⟳", Theme::accent()),
        ToolStatus::Succeeded => ("✓", Theme::green()),
        ToolStatus::Failed => ("✗", Theme::red()),
        ToolStatus::Denied => ("⊘", Theme::red()),
        ToolStatus::AwaitingApproval => ("!", Theme::yellow()),
        ToolStatus::Stopped => ("■", Theme::dim()),
        ToolStatus::Queued => ("…", Theme::dim()),
    };
    let args = if tool.arguments.is_null() {
        String::new()
    } else {
        let s = serde_json::to_string(&tool.arguments).unwrap_or_default();
        truncate(&s, 56)
    };
    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(icon, Style::default().fg(color)),
        Span::styled(
            format!(" {} ", tool.name),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            tool.status.label().to_string(),
            Style::default().fg(color),
        ),
        if args.is_empty() { Span::raw("") } else { Span::styled(format!(" {}", args), Style::default().fg(Theme::dim())) },
    ]));
    if let Some(result) = &tool.result {
        let preview = truncate(result, 100);
        for (i, l) in preview.split('\n').enumerate() {
            if i >= 3 {
                lines.push(Line::styled(
                    "    …",
                    Style::default().fg(Theme::dim()),
                ));
                break;
            }
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(l.to_string(), Style::default().fg(Theme::subtext())),
            ]));
        }
    }
}

// ── 右栏 ────────────────────────────────────────

fn render_right_panel(f: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = RightTab::all()
        .iter()
        .map(|t| Line::styled(t.label(), Style::default()))
        .collect();
    let idx = RightTab::all().iter().position(|t| *t == app.right_tab).unwrap_or(0);
    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(app.focus_style(Focus::RightPanel)),
        )
        .select(idx)
        .style(Style::default().fg(Theme::dim()))
        .highlight_style(Style::default().fg(Theme::accent()).add_modifier(Modifier::BOLD));
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);
    f.render_widget(tabs, chunks[0]);
    let inner = Block::default().borders(Borders::ALL).border_style(app.focus_style(Focus::RightPanel)).inner(chunks[1]);
    match app.right_tab {
        RightTab::AgentTree => render_agent_tree(f, inner, app),
        RightTab::Terminal => render_terminal_panel(f, inner, app),
        RightTab::Board => render_board(f, inner, app),
        RightTab::Assistant => render_assistant_panel(f, inner, app),
        RightTab::Settings => render_settings(f, inner, app),
    }
}

// ── 设置页（P3/P4：自主级别、交付自检、经验库）─────────

fn render_settings(f: &mut Frame, area: Rect, app: &App) {
    let (mode_label, delivery_check, lesson_count, pattern_count, recent_lessons) = {
        let c = app.core.lock();
        let lessons = c.lessons.lock();
        let mut recent: Vec<String> = lessons
            .lessons
            .iter()
            .rev()
            .take(6)
            .map(|l| format!("· {} → {}", truncate(&l.problem, 38), truncate(&l.lesson, 52)))
            .collect();
        if recent.is_empty() {
            recent.push("·（还没有经验。每次交付后会自动总结写进经验库）".to_string());
        }
        (
            c.settings.autonomy_mode.label().to_string(),
            c.settings.delivery_check,
            lessons.lessons.len(),
            lessons.error_patterns.len(),
            recent,
        )
    };
    let mut lines: Vec<Line> = vec![];
    lines.push(Line::from(vec![
        Span::styled("自主级别  ", Style::default().fg(Theme::dim())),
        Span::styled(mode_label.clone(), Style::default().fg(Theme::accent()).add_modifier(Modifier::BOLD)),
        Span::styled("   [Ctrl+G 切换：自主 → 谨慎 → 全自动]", Style::default().fg(Theme::dim())),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("自主模式说明：", Style::default().fg(Theme::dim()))));
    lines.push(Line::from(Span::styled("  自主（默认）：安全操作直接执行；破坏性命令自动改写为可逆替代（移回收站 / 先 stash），无替代才请求确认", Style::default().fg(Theme::subtext()))));
    lines.push(Line::from(Span::styled("  谨慎：写操作与命令都需要确认", Style::default().fg(Theme::subtext()))));
    lines.push(Line::from(Span::styled("  全自动：破坏性也自动执行（硬拒绝列表仍然拒绝：格式化 / 关机 / 全盘删除等）", Style::default().fg(Theme::subtext()))));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("交付自检  ", Style::default().fg(Theme::dim())),
        Span::styled(if delivery_check { "开" } else { "关" }, Style::default().fg(if delivery_check { Theme::green() } else { Theme::red() }).add_modifier(Modifier::BOLD)),
        Span::styled("   [Ctrl+D 切换：改过代码后交付前自动 构建+测试，不绿就继续修]", Style::default().fg(Theme::dim())),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("经验库（P4 自我学习）  ", Style::default().fg(Theme::dim())),
        Span::styled(format!("{} 条经验 · {} 个错误模式", lesson_count, pattern_count), Style::default().fg(Theme::accent())),
    ]));
    lines.push(Line::from(Span::styled("  每次交付后自动总结；新任务开始前检索相关教训拼进上下文；同类错误第 2 次直接给已知修法", Style::default().fg(Theme::dim()))));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("最近经验：", Style::default().fg(Theme::dim()))));
    for l in recent_lessons {
        lines.push(Line::from(Span::styled(format!("  {}", l), Style::default().fg(Theme::subtext()))));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("自修改管道（P5）：发现自身缺陷 → git 分支修改 → 构建测试全绿才合并，否则回滚（--self-improve CLI）", Style::default().fg(Theme::dim()))));
    let para = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

fn render_agent_tree(f: &mut Frame, area: Rect, app: &App) {
    let runs: Vec<sophon_core::agent::team::AgentRun> = {
        let c = app.core.lock();
        let team = c.team.lock();
        team.all_runs().into_iter().cloned().collect()
    };
    if runs.is_empty() {
        f.render_widget(
            Paragraph::new("暂无子 Agent。\n\nAgent 派活（delegate_task / run_parallel_tasks）\n会实时显示在这里：任务、日志、结果。"),
            area,
        );
        return;
    }
    let mut lines: Vec<Line> = Vec::new();
    for (i, run) in runs.iter().enumerate() {
        let selected = i == app.right_scroll;
        let (icon, color) = match run.status {
            sophon_core::agent::team::AgentStatus::Running => ("⟳", Theme::accent()),
            sophon_core::agent::team::AgentStatus::WaitingApproval => ("!", Theme::yellow()),
            sophon_core::agent::team::AgentStatus::Queued => ("…", Theme::dim()),
            sophon_core::agent::team::AgentStatus::Stopping => ("■", Theme::orange()),
            sophon_core::agent::team::AgentStatus::Stopped => ("■", Theme::dim()),
            sophon_core::agent::team::AgentStatus::Done => ("✓", Theme::green()),
            sophon_core::agent::team::AgentStatus::Failed => ("✗", Theme::red()),
        };
        let indent = "  ".repeat(run.depth as usize);
        let style = if selected {
            Style::default().fg(Theme::base()).bg(Theme::accent())
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![
            Span::raw(indent.clone()),
            Span::styled(format!("{} ", icon), Style::default().fg(color)),
            Span::styled(
                format!("{} ", run.name),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(run.status.label(), Style::default().fg(color)),
            if run.background { Span::styled(" · 后台", Style::default().fg(Theme::dim())) } else { Span::raw("") },
        ]).patch_style(style));
        let task = truncate(&run.task, 38);
        lines.push(Line::from(vec![
            Span::raw(indent),
            Span::styled(format!("  {}", task), Style::default().fg(Theme::subtext())),
        ]).patch_style(style));
    }
    let para = Paragraph::new(lines).scroll((0, 0));
    f.render_widget(para, area);
    // 选中项详情
    if let Some(run) = runs.get(app.right_scroll) {
        let mut detail = vec![
            Line::styled("─ 任务 ─", Style::default().fg(Theme::dim())),
            Line::raw(truncate(&run.task, 40)),
        ];
        if let Some(result) = &run.result {
            detail.push(Line::styled("─ 结果 ─", Style::default().fg(Theme::dim())));
            for l in truncate(result, 200).split('\n').take(6) {
                detail.push(Line::raw(l.to_string()));
            }
        }
        if !run.log.is_empty() {
            detail.push(Line::styled(
                format!("─ 日志 (最近 {} 条) ─", run.log.len().min(5)),
                Style::default().fg(Theme::dim()),
            ));
            for entry in run.log.iter().rev().take(5) {
                let l = match &entry.kind {
                    sophon_core::agent::team::AgentLogKind::Tool { name, status, detail } => {
                        format!("  ⚙ {} [{}] {}", name, status.label(), truncate(detail, 30))
                    }
                    sophon_core::agent::team::AgentLogKind::Text { text } => format!("  {}", truncate(text, 36)),
                    sophon_core::agent::team::AgentLogKind::Note { text } => format!("  ◌ {}", truncate(text, 36)),
                };
                detail.push(Line::raw(l));
            }
        }
        let h = detail.len() as u16;
        let y = area.y + area.height.saturating_sub(h);
        f.render_widget(
            Paragraph::new(detail).block(Block::default().style(Style::default().bg(Theme::crust()))),
            Rect::new(area.x, y, area.width, h),
        );
    }
}

fn render_terminal_panel(f: &mut Frame, area: Rect, app: &App) {
    // 确保有终端
    let term_id = app.term_id.clone().or_else(|| {
        let c = app.core.lock();
        let tm = c.terminal_manager.lock();
        tm.list().first().map(|t| t.id.clone())
    });
    let session = term_id.as_deref().and_then(|id| {
        let c = app.core.lock();
        let tm = c.terminal_manager.lock();
        tm.get(id)
    });
    if let Some(session) = session {
        let all_lines = {
            let s = session.lock();
            let emu = s.emulator().lock();
            emu.render_lines(app.term_scroll)
        };
        let mut lines: Vec<Line> = Vec::new();
        for row in all_lines.iter() {
            let mut spans: Vec<Span> = Vec::new();
            for span in row {
                let fg = term_color(span.fg);
                let bg = term_color(span.bg);
                let mut style = Style::default().fg(fg).bg(bg);
                if span.bold {
                    style = style.add_modifier(Modifier::BOLD);
                }
                spans.push(Span::styled(span.text.clone(), style));
            }
            lines.push(Line::from(spans));
        }
        f.render_widget(Paragraph::new(lines), area);
    } else {
        f.render_widget(
            Paragraph::new("暂无终端。\n\nCtrl+T 打开终端（在本会话工作目录里起 shell）。\nAgent 的 run_in_terminal 也会自动开终端。"),
            area,
        );
    }
}

fn term_color(c: TermColor) -> ratatui::style::Color {
    let (idx, rgb) = c.index();
    if let Some((r, g, b)) = rgb {
        ratatui::style::Color::Rgb(r, g, b)
    } else {
        Theme::indexed(idx)
    }
}

fn render_board(f: &mut Frame, area: Rect, app: &App) {
    let items = {
        let c = app.core.lock();
        c.assistant.board(&c)
    };
    if items.is_empty() {
        f.render_widget(Paragraph::new("全部空闲。"), area);
        return;
    }
    let mut lines = Vec::new();
    for item in &items {
        let (icon, color) = match item.kind {
            sophon_core::assistant::BoardKind::Generating => ("⟳", Theme::accent()),
            sophon_core::assistant::BoardKind::Stopping => ("■", Theme::orange()),
            sophon_core::assistant::BoardKind::PendingApproval => ("!", Theme::yellow()),
            sophon_core::assistant::BoardKind::BackgroundAgent => ("⚙", Theme::teal()),
            sophon_core::assistant::BoardKind::Idle => ("·", Theme::dim()),
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", icon), Style::default().fg(color)),
            Span::styled(truncate(&item.title, 36), Style::default().fg(color)),
        ]));
        if let Some(detail) = &item.detail {
            lines.push(Line::styled(
                format!("  {}", truncate(detail, 36)),
                Style::default().fg(Theme::dim()),
            ));
        }
    }
    f.render_widget(Paragraph::new(lines), area);
}

fn render_assistant_panel(f: &mut Frame, area: Rect, app: &App) {
    let (timeline, memories, summary) = {
        let c = app.core.lock();
        let timeline = c.assistant.timeline();
        let memories = c.assistant.memories();
        let summary = app
            .selected_conv
            .as_deref()
            .and_then(|id| c.store.get(id))
            .map(|conv| c.assistant.summarize(conv));
        (timeline, memories, summary)
    };
    let mut lines: Vec<Line> = Vec::new();
    if let Some(s) = &summary {
        lines.push(Line::styled("─ 话题摘要 ─", Style::default().fg(Theme::dim())));
        for l in truncate(&s.summary, 120).split('\n').take(4) {
            lines.push(Line::raw(l.to_string()));
        }
        lines.push(Line::raw(""));
    }
    lines.push(Line::styled(
        format!("─ 记忆 ({}) ─", memories.len()),
        Style::default().fg(Theme::dim()),
    ));
    for m in memories.iter().rev().take(4) {
        lines.push(Line::styled(
            format!("  💡 {}", truncate(&m.content, 36)),
            Style::default().fg(Theme::subtext()),
        ));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled("─ 时间线 ─", Style::default().fg(Theme::dim())));
    for ev in timeline.iter().take(12) {
        let icon = match ev.kind {
            sophon_core::assistant::TimelineKind::UserMessage => "▶",
            sophon_core::assistant::TimelineKind::AgentMessage => "●",
            sophon_core::assistant::TimelineKind::ToolCall => "⚙",
            sophon_core::assistant::TimelineKind::AgentRun => "⚡",
            sophon_core::assistant::TimelineKind::Approval => "!",
            sophon_core::assistant::TimelineKind::System => "·",
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", ev.at.chars().skip(11).take(5).collect::<String>()),
                Style::default().fg(Theme::dim()),
            ),
            Span::styled(format!("{} ", icon), Style::default().fg(Theme::accent())),
            Span::styled(truncate(&ev.title, 30), Style::default().fg(Theme::subtext())),
        ]));
    }
    f.render_widget(Paragraph::new(lines), area);
}

// ── 输入栏 ──────────────────────────────────────

fn render_input_bar(f: &mut Frame, area: Rect, app: &App) {
    let generating = app.is_generating();
    let approval = app.pending_approval();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(if approval {
            Style::default().fg(Theme::yellow())
        } else {
            app.focus_style(Focus::Input)
        })
        .title(Span::styled(
            if generating {
                " 输入（工作中 — Enter = 插话补充） "
            } else if approval {
                " 输入（有待确认调用 — 右栏/对话区批准） "
            } else {
                " 输入 "
            },
            Style::default().fg(Theme::subtext()),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);
    // 输入文本（多行，显示最后几行）
    let input_lines: Vec<String> = if app.input.is_empty() {
        vec!["".to_string()]
    } else {
        app.input.split('\n').map(|s| s.to_string()).collect()
    };
    let max_lines = inner.height as usize;
    let start = input_lines.len().saturating_sub(max_lines);
    let visible: Vec<&String> = input_lines.iter().skip(start).collect();
    // 光标位置
    let cursor_line = app.input.chars().take(app.input_cursor).filter(|c| *c == '\n').count();
    let cursor_col = {
        let mut col = 0usize;
        for (i, c) in app.input.chars().enumerate() {
            if i == app.input_cursor {
                break;
            }
            if c == '\n' {
                col = 0;
            } else {
                col += 1;
            }
        }
        col
    };
    let mut lines: Vec<Line> = Vec::new();
    for (i, l) in visible.iter().enumerate() {
        let is_cursor_line = start + i == cursor_line;
        if is_cursor_line && app.focus == Focus::Input {
            let mut spans = vec![Span::styled("❯ ", Style::default().fg(Theme::accent()))];
            let before: String = l.chars().take(cursor_col).collect();
            let after: String = l.chars().skip(cursor_col).collect();
            spans.push(Span::raw(before));
            spans.push(Span::styled(
                after.chars().next().unwrap_or(' ').to_string(),
                Style::default().fg(Theme::base()).bg(Theme::text()),
            ));
            let rest: String = after.chars().skip(1).collect();
            spans.push(Span::raw(rest));
            lines.push(Line::from(spans));
        } else {
            lines.push(Line::from(vec![
                Span::styled("❯ ", Style::default().fg(Theme::dim())),
                Span::raw(l.to_string()),
            ]));
        }
    }
    // 右侧提示
    let hint = if generating {
        "Ctrl+C 停止"
    } else {
        "Enter 发送 · Ctrl+J 换行 · ↑↓ 历史"
    };
    let hint_line = Line::styled(hint, Style::default().fg(Theme::dim())).alignment(Alignment::Right);
    while lines.len() < max_lines {
        lines.push(Line::raw(""));
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
    }
    let last = lines.len() - 1;
    lines[last] = lines[last].clone().patch_style(Style::default());
    let _ = &hint_line;
    f.render_widget(Paragraph::new(lines), inner);
    // hint 画在右下角
    let hw = hint.chars().count() as u16;
    f.render_widget(
        Paragraph::new(hint_line).style(Style::default().bg(Theme::base())),
        Rect::new(
            inner.x + inner.width.saturating_sub(hw),
            inner.y + inner.height.saturating_sub(1),
            hw,
            1,
        ),
    );
}

// ── 状态栏 ──────────────────────────────────────

fn render_status_bar(f: &mut Frame, area: Rect, app: &App) {
    let left = if !app.status.is_empty() {
        app.status.clone()
    } else {
        let running = {
            let c = app.core.lock();
            let rs = c.runner_state.lock();
            rs.live_agents
        };
        let gen = app.is_generating();
        let parts = vec![
            if gen { "⟳ 生成中".to_string() } else { "○ 空闲".to_string() },
            format!("⚡ {} 子 Agent", running),
            format!("📁 {}", app.selected_conv.as_deref().map(|s| truncate(s, 8)).unwrap_or("—".to_string())),
        ];
        parts.join("  ")
    };
    let right = "Tab 切换焦点 · F1 帮助 · Ctrl+Q 退出";
    let block = Block::default().style(Style::default().bg(Theme::crust()));
    f.render_widget(block, area);
    f.render_widget(
        Paragraph::new(Line::styled(left, Style::default().fg(Theme::subtext()))),
        Rect::new(area.x + 1, area.y, area.width / 2, 1),
    );
    f.render_widget(
        Paragraph::new(Line::styled(right, Style::default().fg(Theme::dim())).alignment(Alignment::Right)),
        Rect::new(area.x + area.width / 2, area.y, area.width / 2, 1),
    );
}

// ── 覆盖层 ──────────────────────────────────────

fn render_help_overlay(f: &mut Frame, app: &App) {
    let area = centered_rect(56, 20, f.area());
    f.render_widget(Clear, area);
    let items = App::help_lines();
    let lines: Vec<Line> = items
        .iter()
        .map(|(k, v)| {
            Line::from(vec![
                Span::styled(format!("{:<16}", k), Style::default().fg(Theme::accent())),
                Span::raw("  "),
                Span::raw(v.clone()),
            ])
        })
        .collect();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Theme::accent()))
        .title(" 快捷键（F1 / Esc 关闭）");
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_confirm_overlay(f: &mut Frame, app: &App) {
    let area = centered_rect(48, 7, f.area());
    f.render_widget(Clear, area);
    let (title, body) = match &app.confirm {
        Some(crate::app::Confirm::Quit) => (
            "退出",
            "确定退出智子？有任务在跑会一起停止。\n[y] 确认退出 · [其他] 取消".to_string(),
        ),
        Some(crate::app::Confirm::DeleteConversation(_)) => (
            "删除会话",
            "确定删除这段会话？此操作不可恢复。\n[y] 确认删除 · [其他] 取消".to_string(),
        ),
        None => return,
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Theme::yellow()))
        .title(format!(" {} ", title));
    f.render_widget(Paragraph::new(body).block(block), area);
}

fn centered_rect(w: u16, h: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    Rect::new(x, y, w.min(area.width), h.min(area.height))
}

/// 按显示宽度截断（CJK 算 2）
pub fn truncate(s: &str, max: usize) -> String {
    let mut width = 0usize;
    let mut out = String::new();
    for c in s.chars() {
        let w = if c.is_cjk_char() { 2 } else { 1 };
        if width + w > max {
            out.push('…');
            break;
        }
        out.push(c);
        width += w;
    }
    out
}

trait CJKChar {
    fn is_cjk_char(self) -> bool;
}
impl CJKChar for char {
    fn is_cjk_char(self) -> bool {
        matches!(self,
            '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}'
            | '\u{3000}'..='\u{303F}' | '\u{FF00}'..='\u{FFEF}' | '\u{3040}'..='\u{30FF}'
            | '\u{AC00}'..='\u{D7AF}'
        )
    }
}

// 避免未使用导入警告
#[allow(dead_code)]
fn _unused(_d: Duration) {}

// ── 单元测试：工具卡七态渲染 ──────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sophon_core::models::{MessageBlock, ToolEvent, ToolStatus};

    fn tool(status: ToolStatus) -> ToolEvent {
        ToolEvent {
            id: "t".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "main.rs"}),
            status,
            result: Some("fn main() {}".to_string()),
            title: None,
            symbol: None,
            preview: None,
            transcript: vec![],
            agent_name: None,
            queued: false,
            model_result: None,
            usage: Default::default(),
            started_at: None,
            finished_at: None,
        }
    }

    #[test]
    fn test_tool_card_all_seven_statuses() {
        // 七态各渲染一行，图标/状态标签不同
        let statuses = [
            (ToolStatus::Running, "⟳"),
            (ToolStatus::Succeeded, "✓"),
            (ToolStatus::Failed, "✗"),
            (ToolStatus::Denied, "⊘"),
            (ToolStatus::AwaitingApproval, "!"),
            (ToolStatus::Stopped, "■"),
            (ToolStatus::Queued, "…"),
        ];
        for (status, icon) in statuses {
            let mut lines: Vec<Line> = Vec::new();
            render_tool_card(&mut lines, &tool(status));
            assert!(!lines.is_empty(), "{:?} 应渲染出内容", status);
            // 第一行含工具名和状态标签
            let first = lines[0].spans.iter().map(|s| s.content.clone()).collect::<String>();
            assert!(first.contains("read_file"), "{:?} 应含工具名: {}", status, first);
            assert!(first.contains(icon), "{:?} 应含图标 {}: {}", status, icon, first);
        }
    }

    #[test]
    fn test_tool_card_result_preview() {
        // 有结果时渲染结果预览（多行）
        let mut t = tool(ToolStatus::Succeeded);
        t.result = Some("line1\nline2\nline3\nline4".to_string());
        let mut lines: Vec<Line> = Vec::new();
        render_tool_card(&mut lines, &t);
        // 至少 2 行（工具行 + 结果预览行）
        assert!(lines.len() >= 2, "有结果时应渲染结果预览");
        let all = lines.iter().map(|l| l.spans.iter().map(|s| s.content.clone()).collect::<String>()).collect::<String>();
        assert!(all.contains("line1"), "应含结果内容");
    }

    #[test]
    fn test_render_block_tool() {
        // render_block 路由 Tool 块到 render_tool_card
        let b = MessageBlock::tool(tool(ToolStatus::Running));
        let mut lines: Vec<Line> = Vec::new();
        render_block(&mut lines, &b, &App::dummy());
        assert!(!lines.is_empty(), "Tool 块应渲染");
    }
}
