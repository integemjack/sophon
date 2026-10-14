// Markdown → ratatui 文本：标题、粗斜体、删除线、行内代码、代码块、引用、有序/无序列表、表格、任务列表、链接、图片（本地路径）。
use ratatui::text::{Line, Span};

/// 渲染 Markdown（别名，ui.rs 用这个名）
pub fn render_markdown(text: &str, width: usize) -> Vec<Line<'static>> {
    render(text, width)
}

/// 渲染 Markdown 为多行（width 用于换行估算）
pub fn render(text: &str, _width: usize) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let lines: Vec<&str> = text.split('\n').collect();
    let mut i = 0;
    let mut in_code = false;
    let mut code_buf = String::new();

    while i < lines.len() {
        let line = lines[i];
        // 代码块
        if line.trim_start().starts_with("```") {
            if in_code {
                // 结束
                for cl in code_buf.lines() {
                    out.push(Line::from(Span::styled(
                        format!("  │ {}", cl),
                        ratatui::style::Style::default()
                            .fg(ratatui::style::Color::DarkGray)
                            .add_modifier(ratatui::style::Modifier::REVERSED),
                    )));
                }
                code_buf.clear();
                in_code = false;
            } else {
                in_code = true;
            }
            i += 1;
            continue;
        }
        if in_code {
            code_buf.push_str(line);
            code_buf.push('\n');
            i += 1;
            continue;
        }
        let trimmed = line.trim_start();
        // 表格
        if is_table_row(trimmed) && i + 1 < lines.len() && is_table_sep(lines[i + 1]) {
            let header = split_row(trimmed);
            i += 2;
            let mut rows = Vec::new();
            while i < lines.len() && is_table_row(lines[i].trim_start()) {
                rows.push(split_row(lines[i].trim_start()));
                i += 1;
            }
            out.extend(render_table(&header, &rows));
            continue;
        }
        // 标题
        if let Some((level, rest)) = parse_heading(trimmed) {
            let style = ratatui::style::Style::default()
                .fg(ratatui::style::Color::Cyan)
                .add_modifier(ratatui::style::Modifier::BOLD);
            let prefix = match level {
                1 => "# ",
                2 => "## ",
                3 => "### ",
                _ => "#### ",
            };
            out.push(Line::from(Span::styled(format!("{}{}", prefix, rest), style)));
            i += 1;
            continue;
        }
        // 引用
        if trimmed.starts_with('>') {
            let rest = trimmed[1..].trim_start();
            out.push(Line::from(vec![
                Span::styled("▎ ", ratatui::style::Style::default().fg(ratatui::style::Color::DarkGray)),
                Span::styled(rest.to_string(), ratatui::style::Style::default().fg(ratatui::style::Color::DarkGray).add_modifier(ratatui::style::Modifier::ITALIC)),
            ]));
            i += 1;
            continue;
        }
        // 任务列表
        if let Some((checked, rest)) = parse_task(trimmed) {
            let mark = if checked { "[x]" } else { "[ ]" };
            out.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(mark, ratatui::style::Style::default().fg(ratatui::style::Color::Green)),
                Span::raw(" "),
                Span::raw(inline(rest)),
            ]));
            i += 1;
            continue;
        }
        // 无序列表
        if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            out.push(Line::from(vec![
                Span::styled("  • ", ratatui::style::Style::default().fg(ratatui::style::Color::Cyan)),
                Span::raw(inline(rest)),
            ]));
            i += 1;
            continue;
        }
        // 有序列表
        if let Some((num, rest)) = parse_ordered(trimmed) {
            out.push(Line::from(vec![
                Span::styled(format!("  {}. ", num), ratatui::style::Style::default().fg(ratatui::style::Color::Cyan)),
                Span::raw(inline(rest)),
            ]));
            i += 1;
            continue;
        }
        // 分隔线
        if trimmed.chars().all(|c| c == '-' || c == '*') && trimmed.len() >= 3 {
            out.push(Line::from(Span::styled(
                "  ────────────────────",
                ratatui::style::Style::default().fg(ratatui::style::Color::DarkGray),
            )));
            i += 1;
            continue;
        }
        // 图片（本地路径）：显示占位（TUI 无图形，标出路径）
        if let Some((alt, url)) = parse_image(trimmed) {
            out.push(Line::from(Span::styled(
                format!("  [图片: {}] {}", alt, url),
                ratatui::style::Style::default().fg(ratatui::style::Color::Blue),
            )));
            i += 1;
            continue;
        }
        // 普通行（行内格式）
        if !trimmed.is_empty() {
            out.push(Line::from(Span::raw(inline(trimmed))));
        } else {
            out.push(Line::from(""));
        }
        i += 1;
    }
    if in_code && !code_buf.is_empty() {
        for cl in code_buf.lines() {
            out.push(Line::from(Span::styled(
                format!("  │ {}", cl),
                ratatui::style::Style::default().fg(ratatui::style::Color::DarkGray),
            )));
        }
    }
    out
}

/// 行内格式：`code` **bold** *italic* ~~strike~~ [link](url)
fn inline(input: &str) -> String {
    let mut out = String::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut buf = String::new();
    while i < bytes.len() {
        // 行内代码
        if bytes[i] == b'`' {
            if let Some(end) = find_byte(bytes, i + 1, b'`') {
                if !buf.is_empty() {
                    out.push_str(&buf);
                    buf.clear();
                }
                out.push_str(&format!("`{}`", String::from_utf8_lossy(&bytes[i + 1..end])));
                i = end + 1;
                continue;
            }
        }
        // 加粗
        if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            if let Some(end) = find_seq(bytes, i + 2, b"**") {
                if !buf.is_empty() {
                    out.push_str(&buf);
                    buf.clear();
                }
                out.push_str(&String::from_utf8_lossy(&bytes[i + 2..end]));
                i = end + 2;
                continue;
            }
        }
        // 斜体
        if bytes[i] == b'*' {
            if let Some(end) = find_byte(bytes, i + 1, b'*') {
                if !buf.is_empty() {
                    out.push_str(&buf);
                    buf.clear();
                }
                out.push_str(&String::from_utf8_lossy(&bytes[i + 1..end]));
                i = end + 1;
                continue;
            }
        }
        // 删除线 ~~text~~
        if bytes[i] == b'~' && i + 1 < bytes.len() && bytes[i + 1] == b'~' {
            if let Some(end) = find_seq(bytes, i + 2, b"~~") {
                if !buf.is_empty() {
                    out.push_str(&buf);
                    buf.clear();
                }
                out.push_str(&String::from_utf8_lossy(&bytes[i + 2..end]));
                i = end + 2;
                continue;
            }
        }
        // 链接 [text](url)
        if bytes[i] == b'[' {
            if let Some(close) = find_byte(bytes, i + 1, b']') {
                if close + 1 < bytes.len() && bytes[close + 1] == b'(' {
                    if let Some(paren) = find_byte(bytes, close + 2, b')') {
                        if !buf.is_empty() {
                            out.push_str(&buf);
                            buf.clear();
                        }
                        let text = String::from_utf8_lossy(&bytes[i + 1..close]);
                        let url = String::from_utf8_lossy(&bytes[close + 2..paren]);
                        out.push_str(&format!("{} ({})", text, url));
                        i = paren + 1;
                        continue;
                    }
                }
            }
        }
        let ch = input[i..].chars().next().unwrap();
        buf.push(ch);
        i += ch.len_utf8();
    }
    if !buf.is_empty() {
        out.push_str(&buf);
    }
    out
}

fn parse_heading(line: &str) -> Option<(u8, &str)> {
    let mut level = 0u8;
    for c in line.chars() {
        if c == '#' {
            level += 1;
        } else {
            break;
        }
    }
    if level >= 1 && level <= 6 {
        let rest = line[level as usize..].trim_start();
        if !rest.is_empty() {
            return Some((level, rest));
        }
    }
    None
}

fn parse_ordered(line: &str) -> Option<(u32, &str)> {
    let mut digits = String::new();
    for c in line.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else {
            break;
        }
    }
    if !digits.is_empty() {
        if let Some(rest) = line[digits.len()..].strip_prefix(". ") {
            if let Ok(n) = digits.parse::<u32>() {
                return Some((n, rest));
            }
        }
    }
    None
}

fn parse_task(line: &str) -> Option<(bool, &str)> {
    for prefix in ["- [ ] ", "- [x] ", "* [ ] ", "* [x] "] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return Some((prefix.contains('x'), rest));
        }
    }
    None
}

fn parse_image(line: &str) -> Option<(String, String)> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'!' && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            if let Some(close) = find_byte(bytes, i + 2, b']') {
                if close + 1 < bytes.len() && bytes[close + 1] == b'(' {
                    if let Some(paren) = find_byte(bytes, close + 2, b')') {
                        let alt = String::from_utf8_lossy(&bytes[i + 2..close]).to_string();
                        let url = String::from_utf8_lossy(&bytes[close + 2..paren]).to_string();
                        return Some((alt, url));
                    }
                }
            }
        }
        i += 1;
    }
    None
}

// ── 表格 ──────────────────────────────────────────

fn is_table_row(line: &str) -> bool {
    let t = line.trim();
    t.contains('|') && t.chars().filter(|&c| c == '|').count() >= 2
}

fn is_table_sep(line: &str) -> bool {
    let t = line.trim();
    let inner = t.strip_prefix('|').unwrap_or(t);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let cells: Vec<&str> = inner.split('|').collect();
    !cells.is_empty()
        && cells.iter().all(|c| {
            let c = c.trim();
            !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' '))
        })
}

fn split_row(line: &str) -> Vec<String> {
    let t = line.trim();
    let inner = t.strip_prefix('|').unwrap_or(t);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    inner.split('|').map(|c| c.trim().to_string()).collect()
}

fn render_table(header: &[String], rows: &[Vec<String>]) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let ncols = header.len().max(rows.iter().map(|r| r.len()).max().unwrap_or(0));
    if ncols == 0 {
        return out;
    }
    // 列宽
    let mut col_chars = vec![0usize; ncols];
    for (j, cell) in header.iter().enumerate() {
        col_chars[j] = col_chars[j].max(display_width(cell));
    }
    for row in rows {
        for (j, cell) in row.iter().enumerate() {
            if j < ncols {
                col_chars[j] = col_chars[j].max(display_width(cell));
            }
        }
    }
    let col_widths: Vec<usize> = col_chars.iter().map(|&c| c + 2).collect();
    // 表头
    let mut header_line = String::from("  ");
    for (j, cell) in header.iter().enumerate() {
        header_line.push_str(&format!("{:<w$}", cell, w = col_widths[j]));
    }
    out.push(Line::from(Span::styled(
        header_line,
        ratatui::style::Style::default()
            .fg(ratatui::style::Color::Cyan)
            .add_modifier(ratatui::style::Modifier::BOLD),
    )));
    // 分隔线
    let mut sep = String::from("  ");
    for w in &col_widths {
        sep.push_str(&"─".repeat(*w));
    }
    out.push(Line::from(Span::styled(
        sep,
        ratatui::style::Style::default().fg(ratatui::style::Color::DarkGray),
    )));
    // 数据行
    for row in rows {
        let mut line = String::from("  ");
        for (j, cell) in row.iter().enumerate() {
            if j < col_widths.len() {
                line.push_str(&format!("{:<w$}", cell, w = col_widths[j]));
            }
        }
        out.push(Line::from(Span::raw(line)));
    }
    out
}

fn display_width(s: &str) -> usize {
    s.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum()
}

fn find_byte(bytes: &[u8], from: usize, target: u8) -> Option<usize> {
    let mut i = from;
    while i < bytes.len() {
        if bytes[i] == target {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn find_seq(bytes: &[u8], from: usize, seq: &[u8]) -> Option<usize> {
    let mut i = from;
    while i + seq.len() <= bytes.len() {
        if &bytes[i..i + seq.len()] == seq {
            return Some(i);
        }
        i += 1;
    }
    None
}
