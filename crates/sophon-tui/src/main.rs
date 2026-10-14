// 智子（Sophon）TUI 入口：终端初始化 / 恢复、事件循环、--dump-screen 截图模式。
mod app;
mod markdown;
mod theme;
mod ui;

use std::io::{self, stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use parking_lot::Mutex;
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::Terminal;

use sophon_core::store::AppCore;
use sophon_core::util::Paths;

use crate::app::App;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--dump-screen") {
        return dump_screen(&args);
    }
    // P5 自修改管道：--self-improve <仓库路径> <问题描述> <plan.json> <验证命令>
    // plan.json 是 FixStep 数组，例：[{"kind":"Edit","file":"src/a.rs","old":"...","new":"..."}]
    if args.iter().any(|a| a == "--self-improve") {
        let rest: Vec<&String> = args.iter().skip_while(|a| a.as_str() != "--self-improve").skip(1).collect();
        if rest.len() < 4 {
            eprintln!("用法：sophon-tui --self-improve <仓库路径> <问题描述> <plan.json> <验证命令>");
            std::process::exit(2);
        }
        let (repo, problem, plan_file, verify_cmd) =
            (rest[0].clone(), rest[1].clone(), rest[2].clone(), rest[3].clone());
        let plan_text = std::fs::read_to_string(&plan_file)?;
        let plan: Vec<sophon_core::selfimprove::FixStep> = serde_json::from_str(&plan_text)?;
        let lessons = sophon_core::learning::LessonStore::load(&sophon_core::util::Paths::resolve().data_dir);
        let mut lessons = lessons;
        let report = sophon_core::selfimprove::run_self_improve(
            &repo, &problem, &plan, &verify_cmd, &mut lessons,
        );
        println!("{}", serde_json::to_string_pretty(&report)?);
        std::process::exit(if report.merged { 0 } else { 1 });
    }
    run_tui()
}

fn run_tui() -> Result<()> {
    // 最小尺寸防御：极窄终端下 UI 会越界（之前在 30 列终端实测 panic）。
    // 实测 ≥80x24 全部安全（dump 模式验证过 80x24 / 100x30 / 120x30 / 120x36 / 120x38）。
    let (tw, th) = crossterm::terminal::size()?;
    if tw < 80 || th < 24 {
        let _ = disable_raw_mode();
        anyhow::bail!(
            "终端窗口太小（{}x{}）：至少需要 80x24。请拉大终端窗口后重试。",
            tw, th
        );
    }
    // 终端进入原始模式 + 备用屏
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // 核心
    let paths = Paths::resolve();
    let (main_tx, main_rx) = sophon_core::util::main_thread::channel();
    let core = Arc::new(Mutex::new(AppCore::load(paths, main_tx)?));
    core.lock().set_secret_reader();
    // 重绘信号：工作线程调 request_repaint → 置 dirty
    let dirty = Arc::new(AtomicBool::new(true));
    {
        let dirty = dirty.clone();
        core.lock().set_repaint(Arc::new(move || {
            dirty.store(true, Ordering::Relaxed);
        }));
    }
    // 连接本机 MCP 服务
    core.lock().mcp.lock().connect_all();

    let mut app = App::new(core.clone(), main_rx, dirty.clone());
    let result = app.run(&mut terminal);

    // 恢复终端
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    // 落盘
    if let Ok(c) = core.lock().save_all() {
        let _ = c;
    }
    result
}

/// 截图模式：把当前界面渲染到 TestBackend 缓冲，导出 JSON（由 tools/dump_to_png.py 转 PNG）。
/// 用法：sophon.exe --dump-screen [WxH] [out.json]
fn dump_screen(args: &[String]) -> Result<()> {
    let mut size = (120usize, 35usize);
    let mut out_path = "sophon_screen.json".to_string();
    let mut skip_next = false;
    for a in args.iter().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }
        if a == "--dump-screen" || a == "--settings" {
            continue;
        }
        if a == "--data-dir" {
            skip_next = true;
            continue;
        }
        if a.contains('x') {
            let parts: Vec<&str> = a.split('x').collect();
            if parts.len() == 2 {
                if let (Ok(w), Ok(h)) = (parts[0].parse::<usize>(), parts[1].parse::<usize>()) {
                    size = (w.max(40).min(300), h.max(10).min(100));
                }
            }
        } else {
            out_path = a.clone();
        }
    }
    // 用临时数据目录，不碰真实数据（--data-dir 指定则用指定的，便于验证经验库 / 自主级别显示）
    let tmp_override = args
        .iter()
        .position(|a| a == "--data-dir")
        .and_then(|i| args.get(i + 1))
        .map(|s| std::path::PathBuf::from(s));
    let tmp = match &tmp_override {
        Some(d) => d.clone(),
        None => {
            let t = std::env::temp_dir().join(format!("sophon_dump_{}", std::process::id()));
            std::env::set_var("FRIDAY_DATA_DIR", t.join("data"));
            std::env::set_var("FRIDAY_CACHE_DIR", t.join("cache"));
            t
        }
    };
    if let Some(d) = &tmp_override {
        eprintln!("[dump] create data dir: {:?}", d.join("data"));
        std::fs::create_dir_all(d.join("data"))?;
        eprintln!("[dump] data dir ok");
        std::env::set_var("FRIDAY_DATA_DIR", d.join("data"));
        std::env::set_var("FRIDAY_CACHE_DIR", d.join("cache"));
    } else {
        std::fs::create_dir_all(&tmp)?;
    }

    let paths = Paths::resolve();
    eprintln!("[dump] paths ok: {:?}", paths.data_dir);
    let (main_tx, main_rx) = sophon_core::util::main_thread::channel();
    let core = Arc::new(Mutex::new(AppCore::load(paths, main_tx)?));
    eprintln!("[dump] core loaded");
    core.lock().set_secret_reader();
    let dirty = Arc::new(AtomicBool::new(true));
    let mut app = App::new(core, main_rx, dirty);
    // --settings：初始右栏切到设置页（截图验证 P3/P4）
    if args.iter().any(|a| a == "--settings") {
        app.right_tab = crate::app::RightTab::Settings;
        app.focus = crate::app::Focus::RightPanel;
    }

    let backend = TestBackend::new(size.0 as u16, size.1 as u16);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|f| crate::ui::draw(f, &app))?;

    // 导出缓冲
    let buffer = terminal.backend().buffer();
    let mut cells: Vec<Vec<serde_json::Value>> = Vec::new();
    for y in 0..size.1 {
        let mut row = Vec::new();
        for x in 0..size.0 {
            let cell = &buffer.content[y * size.0 + x];
            row.push(serde_json::json!({
                "s": cell.symbol(),
                "fg": color_json(&cell.fg),
                "bg": color_json(&cell.bg),
                "b": cell.modifier.contains(ratatui::style::Modifier::BOLD),
            }));
        }
        cells.push(row);
    }
    let json = serde_json::json!({
        "width": size.0,
        "height": size.1,
        "cells": cells,
    });
    std::fs::write(&out_path, serde_json::to_string(&json)?)?;
    // 清理临时目录（--data-dir 指定的目录不清理）
    if tmp_override.is_none() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    println!("dumped {} -> {} ({}x{})", args[0], out_path, size.0, size.1);
    Ok(())
}

fn color_json(c: &ratatui::style::Color) -> serde_json::Value {
    match c {
        ratatui::style::Color::Rgb(r, g, b) => serde_json::json!([r, g, b]),
        ratatui::style::Color::Indexed(i) => {
            let c = crate::theme::Theme::indexed(*i);
            if let ratatui::style::Color::Rgb(r, g, b) = c {
                serde_json::json!([r, g, b])
            } else {
                serde_json::Value::Null
            }
        }
        _ => serde_json::Value::Null,
    }
}
