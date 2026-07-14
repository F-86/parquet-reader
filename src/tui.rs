use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
        MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap},
};

use crate::{
    app::{AppState, SidebarOpenResult, ViewMode},
    cli::AppConfig,
    data::{ParquetFileDataSource, validate_parquet_readable},
    error::Result,
    file_browser::FileEntryKind,
    formatting::truncate_to_width,
};

pub fn run(config: AppConfig) -> Result<()> {
    let mut app = AppState::new(config)?;

    if let Some(path) = app.active_file.clone() {
        load_file(&mut app, path);
    }

    let mut terminal = TerminalGuard::enter()?;
    loop {
        terminal.terminal.draw(|frame| draw(frame, &mut app))?;
        if app.should_quit {
            break;
        }
        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) => handle_key(&mut app, key),
                Event::Mouse(mouse) => handle_mouse(&mut app, mouse),
                _ => {}
            }
        }
    }

    Ok(())
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = self.terminal.show_cursor();
    }
}

fn handle_mouse(app: &mut AppState, mouse: MouseEvent) {
    if app.show_help {
        return;
    }

    if app.show_cell_detail {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                app.cell_detail_scroll = app.cell_detail_scroll.saturating_sub(1)
            }
            MouseEventKind::ScrollDown => {
                app.cell_detail_scroll = app.cell_detail_scroll.saturating_add(1)
            }
            _ => {}
        }
        return;
    }

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if is_on_sidebar_resize_handle(app, mouse.column) {
                app.resizing_sidebar = true;
                app.sidebar.focused = true;
                return;
            }
            let is_double_click = is_double_click(app, mouse.column, mouse.row);
            handle_sidebar_click(app, mouse.column, mouse.row);
            if is_double_click
                && mouse.column >= app.sidebar_width
                && app.selected_cell_value().is_some()
            {
                app.open_cell_detail();
            }
            remember_mouse_click(app, mouse.column, mouse.row);
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if app.resizing_sidebar {
                resize_sidebar(app, mouse.column);
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            if app.resizing_sidebar {
                resize_sidebar(app, mouse.column);
                app.resizing_sidebar = false;
            }
        }
        MouseEventKind::ScrollUp => scroll_table_up(app),
        MouseEventKind::ScrollDown => scroll_table_down(app),
        MouseEventKind::ScrollLeft => app.select_col_previous(),
        MouseEventKind::ScrollRight => app.select_col_next(),
        _ => {}
    }
}

fn scroll_table_up(app: &mut AppState) {
    if app.show_filter_popup || app.sidebar.focused {
        return;
    }
    if app.selected_row > 0 {
        app.select_row_previous();
    } else {
        load_previous_page(app);
    }
}

fn scroll_table_down(app: &mut AppState) {
    if app.show_filter_popup || app.sidebar.focused {
        return;
    }
    if app.selected_row + 1 < app.rows.len() {
        app.select_row_next();
    } else {
        load_next_page(app);
    }
}

fn is_double_click(app: &AppState, column: u16, row: u16) -> bool {
    const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(450);
    app.last_mouse_click.as_ref().is_some_and(|last| {
        last.column == column && last.row == row && last.at.elapsed() <= DOUBLE_CLICK_WINDOW
    })
}

fn remember_mouse_click(app: &mut AppState, column: u16, row: u16) {
    app.last_mouse_click = Some(crate::app::MouseClickState {
        column,
        row,
        at: Instant::now(),
    });
}

fn is_on_sidebar_resize_handle(app: &AppState, column: u16) -> bool {
    column == app.sidebar_width.saturating_sub(1) || column == app.sidebar_width
}

fn resize_sidebar(app: &mut AppState, column: u16) {
    const MIN_WIDTH: u16 = 18;
    const MAX_WIDTH: u16 = 80;
    app.sidebar_width = column.saturating_add(1).clamp(MIN_WIDTH, MAX_WIDTH);
    app.status = format!("File sidebar width: {}", app.sidebar_width);
}

fn handle_sidebar_click(app: &mut AppState, column: u16, row: u16) {
    if column < app.sidebar_width {
        if row == 0 {
            return;
        }

        let entry_index = row.saturating_sub(1) as usize;
        app.sidebar.focused = true;
        app.sidebar.select(entry_index);
        match app.open_selected_sidebar_entry() {
            Ok(SidebarOpenResult::File(path)) => load_file(app, path),
            Ok(SidebarOpenResult::None) => {}
            Err(error) => app.set_error(error.to_string()),
        }
        return;
    }

    if row == 0 {
        handle_tab_click(app, column);
        return;
    }

    handle_table_click(app, column, row);
}

fn handle_tab_click(app: &mut AppState, column: u16) {
    let relative_column = column.saturating_sub(app.sidebar_width) as usize;
    let mut cursor = 0usize;
    for index in 0..app.tabs.len() {
        let title = format!(" {}:{} ", index + 1, app.tabs[index].title);
        let width = unicode_width::UnicodeWidthStr::width(title.as_str()).max(1);
        if relative_column >= cursor && relative_column < cursor + width {
            app.switch_to_tab(index);
            return;
        }
        cursor += width;
    }
}

fn handle_table_click(app: &mut AppState, column: u16, row: u16) {
    const DEFAULT_COLUMN_WIDTH: u16 = 24;

    let right_start = app.sidebar_width;
    if column < right_start || app.rows.is_empty() {
        return;
    }

    // Right pane layout:
    // row 0: tab bar
    // data area starts at row 1, table border at row 1, table header at row 2,
    // first data row at row 3. Data cells start after the table left border.
    let first_data_row = 3u16;
    if row < first_data_row {
        return;
    }

    let row_index = row.saturating_sub(first_data_row) as usize;
    if row_index >= app.rows.len() {
        return;
    }

    let relative_column = column.saturating_sub(right_start).saturating_sub(1);
    let column_index = app.scroll_x + (relative_column / DEFAULT_COLUMN_WIDTH) as usize;
    if column_index >= app.columns.len() {
        return;
    }

    app.sidebar.focused = false;
    app.selected_row = row_index;
    app.selected_col = column_index;
    app.status = format!(
        "Selected row {}, column {}",
        row_index + 1,
        column_index + 1
    );
}

fn handle_key(app: &mut AppState, key: KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
        app.should_quit = true;
        return;
    }

    if app.show_cell_detail {
        if app.detail_search_active {
            match key.code {
                KeyCode::Esc => app.cancel_detail_search(),
                KeyCode::Enter => app.execute_detail_search(),
                KeyCode::Backspace => app.backspace_detail_search_char(),
                KeyCode::Char(ch) => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        match ch {
                            'u' => {
                                app.detail_search_input.clear();
                                app.detail_search_cursor = 0;
                            }
                            'c' => app.should_quit = true,
                            _ => {}
                        }
                    } else {
                        app.insert_detail_search_char(ch);
                    }
                }
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char(' ') => {
                app.show_cell_detail = false;
                app.reset_detail_search();
            }
            KeyCode::Char('q') => app.should_quit = true,
            KeyCode::Char('/') => app.start_detail_search(),
            KeyCode::Char('n') => app.next_detail_search_match(),
            KeyCode::Char('N') => app.previous_detail_search_match(),
            KeyCode::Up | KeyCode::Char('k') => {
                app.cell_detail_scroll = app.cell_detail_scroll.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.cell_detail_scroll = app.cell_detail_scroll.saturating_add(1)
            }
            KeyCode::PageUp => app.cell_detail_scroll = app.cell_detail_scroll.saturating_sub(8),
            KeyCode::PageDown => app.cell_detail_scroll = app.cell_detail_scroll.saturating_add(8),
            KeyCode::Home => app.cell_detail_scroll = 0,
            KeyCode::Char('y') => copy_selected_cell(app),
            KeyCode::Char('Y') => copy_selected_row(app),
            _ => {}
        }
        return;
    }

    if app.show_help {
        match key.code {
            KeyCode::Esc | KeyCode::Char('h') => app.show_help = false,
            KeyCode::Char('q') => app.should_quit = true,
            _ => {}
        }
        return;
    }

    if app.show_filter_popup {
        match key.code {
            KeyCode::Esc => app.cancel_filter_popup(),
            KeyCode::Enter => apply_filter(app),
            KeyCode::Backspace => app.backspace_filter_char(),
            KeyCode::Delete => app.delete_filter_char(),
            KeyCode::Left => app.move_filter_cursor_left(),
            KeyCode::Right => app.move_filter_cursor_right(),
            KeyCode::Home => app.move_filter_cursor_home(),
            KeyCode::End => app.move_filter_cursor_end(),
            KeyCode::Tab => app.complete_filter_field(false),
            KeyCode::BackTab => app.complete_filter_field(true),
            KeyCode::Up => app.previous_filter_history(),
            KeyCode::Down => app.next_filter_history(),
            KeyCode::Char(ch) => app.insert_filter_char(ch),
            _ => {}
        }
        return;
    }

    if app.view == ViewMode::Schema {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => app.should_quit = true,
            KeyCode::Char('h') => app.show_help = true,
            KeyCode::Char('s') => app.toggle_schema_view(),
            KeyCode::Up | KeyCode::Char('k') => app.select_schema_row_previous(),
            KeyCode::Down | KeyCode::Char('j') => app.select_schema_row_next(),
            KeyCode::Char('K') => app.select_schema_row_top(),
            KeyCode::Char('J') => app.select_schema_row_bottom(),
            KeyCode::Char('y') => copy_selected_cell(app),
            KeyCode::Char('Y') => copy_selected_row(app),
            _ => {}
        }
        return;
    }

    if app.sidebar.focused {
        match key.code {
            KeyCode::Esc => app.sidebar.focused = false,
            KeyCode::Char('q') => app.should_quit = true,
            KeyCode::Char('h') => app.show_help = true,
            KeyCode::Up | KeyCode::Char('k') => app.sidebar.select_previous(),
            KeyCode::Down | KeyCode::Char('j') => app.sidebar.select_next(),
            KeyCode::Enter => match app.open_selected_sidebar_entry() {
                Ok(SidebarOpenResult::File(path)) => load_file(app, path),
                Ok(SidebarOpenResult::None) => {}
                Err(error) => app.set_error(error.to_string()),
            },
            _ => {}
        }
        return;
    }

    if app.view == ViewMode::Schema {
        match key.code {
            KeyCode::Char('q') => app.should_quit = true,
            KeyCode::Char('h') => app.show_help = true,
            KeyCode::Char('s') => app.toggle_schema_view(),
            KeyCode::Up | KeyCode::Char('k') => app.select_schema_row_previous(),
            KeyCode::Down | KeyCode::Char('j') => app.select_schema_row_next(),
            KeyCode::Char('K') => app.select_schema_row_top(),
            KeyCode::Char('J') => app.select_schema_row_bottom(),
            KeyCode::Char('y') => copy_schema_field(app),
            KeyCode::Char('Y') => copy_schema_field(app),
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('h') => app.show_help = true,
        KeyCode::Char('d') => {
            app.sidebar.focused = true;
            if let Err(error) = app.sidebar.refresh() {
                app.set_error(error.to_string());
            }
        }
        KeyCode::Char('s') => app.toggle_schema_view(),
        KeyCode::Char('/') => app.open_filter_popup(),
        KeyCode::Char('r') => reset_filter(app),
        KeyCode::Char('c') => app.count_current_filter(),
        KeyCode::Char('e') => app.export_current_page_csv(),
        KeyCode::Char('o') => app.sort_by_column(app.selected_col),
        KeyCode::Char('O') => app.toggle_sort_direction(),
        KeyCode::Up | KeyCode::Char('k') => app.select_row_previous(),
        KeyCode::Down | KeyCode::Char('j') => app.select_row_next(),
        KeyCode::Char('K') => app.select_row_top(),
        KeyCode::Char('J') => app.select_row_bottom(),
        KeyCode::Left => app.select_col_previous(),
        KeyCode::Right | KeyCode::Char('l') => app.select_col_next(),
        KeyCode::Char('H') => app.select_first_col(),
        KeyCode::Char('L') => app.select_last_col(),
        KeyCode::Tab => app.next_tab(),
        KeyCode::BackTab => app.previous_tab(),
        KeyCode::Char('n') | KeyCode::PageDown => load_next_page(app),
        KeyCode::Char('p') | KeyCode::PageUp => load_previous_page(app),
        KeyCode::Enter | KeyCode::Char(' ') => app.open_cell_detail(),
        KeyCode::Char('y') => copy_selected_cell(app),
        KeyCode::Char('Y') => copy_selected_row(app),
        _ => {}
    }
}

fn copy_schema_field(app: &mut AppState) {
    let Some(column) = app.columns.get(app.selected_schema_row) else {
        app.status = "No field selected".to_string();
        return;
    };
    let value = format!(
        "{}\nname: {}\ntype: {}\nphysical: {}",
        column.index,
        column.name,
        column.logical_type,
        column
            .physical_type
            .clone()
            .unwrap_or_else(|| "-".to_string()),
    );
    let encoded = general_purpose::STANDARD.encode(value.as_bytes());
    let sequence = format!("\x1b]52;c;{encoded}\x07");
    match io::stdout()
        .write_all(sequence.as_bytes())
        .and_then(|_| io::stdout().flush())
    {
        Ok(()) => {
            app.status = format!("Copied field '{}' to clipboard", column.name);
        }
        Err(error) => app.set_error(format!("failed to copy field: {error}")),
    }
}

fn copy_selected_cell(app: &mut AppState) {
    let Some(value) = app.selected_cell_value() else {
        app.status = "No cell selected".to_string();
        return;
    };

    let encoded = general_purpose::STANDARD.encode(value.as_bytes());
    let sequence = format!("\x1b]52;c;{encoded}\x07");
    match io::stdout()
        .write_all(sequence.as_bytes())
        .and_then(|_| io::stdout().flush())
    {
        Ok(()) => {
            app.status = format!(
                "Copied row {}, column {} to clipboard",
                app.selected_row + 1,
                app.selected_col + 1
            );
        }
        Err(error) => app.set_error(format!("failed to copy cell: {error}")),
    }
}

fn copy_selected_row(app: &mut AppState) {
    let Some(value) = app.selected_row_detail_json() else {
        app.status = "No row selected".to_string();
        return;
    };

    let encoded = general_purpose::STANDARD.encode(value.as_bytes());
    let sequence = format!("]52;c;{encoded}");
    match io::stdout()
        .write_all(sequence.as_bytes())
        .and_then(|_| io::stdout().flush())
    {
        Ok(()) => {
            app.status = format!(
                "Copied row {} ({} fields) to clipboard",
                app.selected_row + 1,
                app.columns.len()
            );
        }
        Err(error) => app.set_error(format!("failed to copy row: {error}")),
    }
}

pub fn display_cell_detail_value(value: &str) -> String {
    let trimmed = value.trim();
    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return value.to_string();
    }

    match serde_json::from_str::<serde_json::Value>(trimmed)
        .and_then(|json| serde_json::to_string_pretty(&json))
    {
        Ok(pretty) => pretty,
        Err(_) => value.to_string(),
    }
}

/// Render pretty-printed JSON with lightweight syntax highlighting.
///
/// Colors follow the plan: keys yellow, strings green, numbers cyan, null
/// gray, booleans magenta. Non-JSON detail falls back to a single plain line.
pub fn highlight_json_detail(raw: &str) -> Vec<Line<'static>> {
    let trimmed = raw.trim();
    let pretty = if trimmed.starts_with('{') || trimmed.starts_with('[') {
        serde_json::from_str::<serde_json::Value>(trimmed)
            .ok()
            .and_then(|json| serde_json::to_string_pretty(&json).ok())
    } else {
        None
    };

    let Some(text) = pretty.or_else(|| Some(raw.to_string())) else {
        return Vec::new();
    };

    text.lines()
        .map(|line| {
            let (indent, rest) =
                line.split_at(line.find(|c: char| !c.is_whitespace()).unwrap_or(0));
            let mut rendered = Line::from(Span::raw(indent.to_string()));
            rendered.spans.extend(highlight_line(rest));
            rendered
        })
        .collect()
}

/// Render pretty-printed JSON with syntax highlighting and search match
/// highlighting. Current match is shown in reverse-video; other matches are
/// underlined. Non-JSON text is also searched.
fn highlight_json_detail_with_search(raw: &str, app: &AppState) -> Vec<Line<'static>> {
    let trimmed = raw.trim();
    let pretty = if trimmed.starts_with('{') || trimmed.starts_with('[') {
        serde_json::from_str::<serde_json::Value>(trimmed)
            .ok()
            .and_then(|json| serde_json::to_string_pretty(&json).ok())
    } else {
        None
    };
    let text = pretty.unwrap_or_else(|| raw.to_string());

    let current_match = app
        .detail_search_index
        .and_then(|i| app.detail_search_matches.get(i).copied());

    // Build a set of byte ranges to highlight (non-current matches).
    let other_matches: Vec<(usize, usize)> = app
        .detail_search_matches
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != app.detail_search_index)
        .map(|(_, &(s, e, _))| (s, e))
        .collect();

    // Walk line by line, tracking byte offset in the full text.
    let mut byte_offset = 0usize;
    text.lines()
        .map(|line| {
            let line_start = byte_offset;
            let line_end = line_start + line.len();
            byte_offset = line_end + 1; // +1 for '\n'

            let (indent, rest) =
                line.split_at(line.find(|c: char| !c.is_whitespace()).unwrap_or(0));
            let mut rendered = Line::from(Span::raw(indent.to_string()));

            // Apply JSON syntax highlighting to the rest, then overlay search
            // matches by post-processing the spans.
            let syntax_spans = highlight_line(rest);
            let rest_start = line_start + indent.len();

            // Convert syntax spans to (byte_start, byte_end, content, style)
            // relative to the full text, then split at match boundaries.
            let mut char_spans: Vec<(usize, usize, String, Option<Style>)> = Vec::new();
            let mut rel_pos = 0usize;
            for span in &syntax_spans {
                let s_start = rest_start + rel_pos;
                let s_end = s_start + span.content.len();
                char_spans.push((s_start, s_end, span.content.to_string(), Some(span.style)));
                rel_pos += span.content.len();
            }

            // Now split char_spans by match boundaries and apply highlight.
            let mut all_ranges: Vec<(usize, usize, bool)> = Vec::new();
            // (start, end, is_current)
            for (s, e) in &other_matches {
                if *s < line_end && *e > line_start {
                    let clamped_s = (*s).max(line_start);
                    let clamped_e = (*e).min(line_end);
                    all_ranges.push((clamped_s, clamped_e, false));
                }
            }
            if let Some((cs, ce, _)) = current_match {
                if cs < line_end && ce > line_start {
                    let clamped_s = cs.max(line_start);
                    let clamped_e = ce.min(line_end);
                    all_ranges.push((clamped_s, clamped_e, true));
                }
            }
            all_ranges.sort_by_key(|r| r.0);

            // Merge overlapping ranges, preferring current match style.
            all_ranges.dedup_by(|a, b| {
                if a.0 == b.0 && a.1 == b.1 {
                    true
                } else {
                    false
                }
            });

            // Build final spans by walking char_spans and splitting at ranges.
            let match_style = Style::default().add_modifier(Modifier::UNDERLINED);
            let current_style = Style::default().add_modifier(Modifier::REVERSED);

            for (s_start, s_end, content, style) in &char_spans {
                let mut pos = *s_start;
                for &(rs, re, is_current) in &all_ranges {
                    if re <= pos || rs >= *s_end {
                        continue;
                    }
                    // Part before the match.
                    if rs > pos {
                        let before_len = rs - pos;
                        let before: String =
                            content[pos - s_start..pos - s_start + before_len].to_string();
                        rendered
                            .spans
                            .push(Span::styled(before, style.unwrap_or_default()));
                    }
                    // The match itself.
                    let match_start = rs.max(pos);
                    let match_end = re.min(*s_end);
                    let match_len = match_end - match_start;
                    let match_text: String = content
                        [match_start - s_start..match_start - s_start + match_len]
                        .to_string();
                    let ms = if is_current {
                        current_style
                    } else {
                        match_style
                    };
                    // Merge with existing style if any.
                    let final_style = style.map(|s| s.patch(ms)).unwrap_or(ms);
                    rendered.spans.push(Span::styled(match_text, final_style));
                    pos = match_end;
                }
                // Remaining part after all matches in this span.
                if pos < *s_end {
                    let remaining: String = content[pos - s_start..].to_string();
                    rendered
                        .spans
                        .push(Span::styled(remaining, style.unwrap_or_default()));
                }
            }

            rendered
        })
        .collect()
}

/// Highlight a single (already de-indented) JSON line into colored spans.
fn highlight_line(line: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let bytes = line.as_bytes();
    let mut index = 0;
    let key_style = Style::default().fg(Color::Yellow);
    let string_style = Style::default().fg(Color::Green);
    let number_style = Style::default().fg(Color::Cyan);
    let null_style = Style::default().fg(Color::DarkGray);
    let bool_style = Style::default().fg(Color::Magenta);

    while index < bytes.len() {
        let ch = bytes[index] as char;
        if ch == '"' {
            // Read a full string token, then check if it is a "key": prefix.
            let Some(end) = line[index + 1..].find('"') else {
                spans.push(Span::raw(line[index..].to_string()));
                break;
            };
            let end = index + 1 + end;
            let token = &line[index..=end];
            let after = line[end + 1..].trim_start();
            if after.starts_with(':') {
                spans.push(Span::styled(token.to_string(), key_style));
            } else {
                spans.push(Span::styled(token.to_string(), string_style));
            }
            index = end + 1;
            continue;
        }
        if ch.is_ascii_digit() || ch == '-' {
            let end = line[index..]
                .find(|c: char| {
                    !(c.is_ascii_digit()
                        || c == '.'
                        || c == '-'
                        || c == 'e'
                        || c == 'E'
                        || c == '+')
                })
                .unwrap_or(line.len() - index);
            spans.push(Span::styled(
                line[index..index + end].to_string(),
                number_style,
            ));
            index += end;
            continue;
        }
        if line[index..].starts_with("true") || line[index..].starts_with("false") {
            let word = if line[index..].starts_with("true") {
                "true"
            } else {
                "false"
            };
            spans.push(Span::styled(word.to_string(), bool_style));
            index += word.len();
            continue;
        }
        if line[index..].starts_with("null") {
            spans.push(Span::styled("null".to_string(), null_style));
            index += 4;
            continue;
        }
        // Punctuation / whitespace: render plain and advance by a UTF-8 char.
        let ch_len = line[index..].chars().next().map_or(1, |c| c.len_utf8());
        spans.push(Span::raw(line[index..index + ch_len].to_string()));
        index += ch_len;
    }
    spans
}

fn apply_filter(app: &mut AppState) {
    let previous_filter = app.filter.clone();
    let _ = app.set_filter_from_input();
    let Some(path) = app.current_file_path() else {
        app.status = "No file opened".to_string();
        return;
    };
    let data_source = ParquetFileDataSource::new(path);
    match data_source.read_page_with_filter(0, app.page_size, app.filter.as_deref()) {
        Ok(page) => app.replace_active_page(page),
        Err(error) => {
            app.filter = previous_filter;
            app.set_error(error.to_string());
        }
    }
}

fn reset_filter(app: &mut AppState) {
    if app.filter.is_none() {
        app.status = "No filter to reset".to_string();
        return;
    }
    app.reset_filter();
    load_page(app, 0);
}

fn load_file(app: &mut AppState, path: PathBuf) {
    if let Some(index) = app.tab_index_for_path(&path) {
        app.switch_to_tab(index);
        return;
    }

    if let Err(error) = validate_file_path(&path) {
        app.set_error(error);
        return;
    }
    let data_source = ParquetFileDataSource::new(path.clone());
    match data_source.read_first_page(app.page_size) {
        Ok(page) => app.apply_page(path, page),
        Err(error) => app.set_error(error.to_string()),
    }
}

fn load_next_page(app: &mut AppState) {
    let Some(offset) = app.next_page_offset() else {
        app.status = "Already at last page".to_string();
        return;
    };
    load_page(app, offset);
}

fn load_previous_page(app: &mut AppState) {
    let Some(offset) = app.previous_page_offset() else {
        app.status = "Already at first page".to_string();
        return;
    };
    load_page(app, offset);
}

fn load_page(app: &mut AppState, offset: usize) {
    let Some(path) = app.current_file_path() else {
        app.status = "No file opened".to_string();
        return;
    };
    let data_source = ParquetFileDataSource::new(path);
    match data_source.read_page_with_filter(offset, app.page_size, app.filter.as_deref()) {
        Ok(page) => app.replace_active_page(page),
        Err(error) => app.set_error(error.to_string()),
    }
}

fn validate_file_path(path: &Path) -> std::result::Result<(), String> {
    if !path.exists() {
        return Err(format!("path does not exist: {}", path.display()));
    }
    if !path.is_file() {
        return Err(format!("path is not a file: {}", path.display()));
    }
    validate_parquet_readable(path).map_err(|error| error.to_string())
}

fn draw(frame: &mut Frame<'_>, app: &mut AppState) {
    let root = frame.area();
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(app.sidebar_width), Constraint::Min(20)])
        .split(root);

    draw_sidebar(frame, app, columns[0]);
    draw_right(frame, app, columns[1]);

    if app.show_help {
        draw_help(frame, root);
    } else if app.show_cell_detail {
        draw_cell_detail(frame, app, root);
    } else if app.show_filter_popup {
        draw_filter_popup(frame, app, root);
    }
}

fn draw_sidebar(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let title = if app.resizing_sidebar {
        "Files ↔"
    } else if app.sidebar.focused {
        "Files *"
    } else {
        "Files"
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(if app.resizing_sidebar {
            Style::default().fg(Color::Yellow)
        } else if app.sidebar.focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        });

    let items = app
        .sidebar
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let indent = "  ".repeat(entry.depth);
            let marker = match entry.kind {
                FileEntryKind::Directory if entry.expanded => "▾ ",
                FileEntryKind::Directory => "▸ ",
                FileEntryKind::ParquetFile => "◆ ",
                FileEntryKind::OtherFile => "  ",
            };
            let style = match entry.kind {
                FileEntryKind::Directory => Style::default().fg(Color::Blue),
                FileEntryKind::ParquetFile => Style::default().fg(Color::Green),
                FileEntryKind::OtherFile => Style::default().fg(Color::DarkGray),
            };
            let mut name = format!("{indent}{marker}{}", entry.name);
            name = truncate_to_width(&name, area.width.saturating_sub(4) as usize);
            if index == app.sidebar.selected && app.sidebar.focused {
                Line::from(Span::styled(name, style.add_modifier(Modifier::REVERSED)))
            } else if index == app.sidebar.selected {
                Line::from(Span::styled(name, style.add_modifier(Modifier::UNDERLINED)))
            } else {
                Line::from(Span::styled(name, style))
            }
        })
        .collect::<Vec<_>>();

    let paragraph = Paragraph::new(items).block(block);
    frame.render_widget(paragraph, area);
}

fn draw_right(frame: &mut Frame<'_>, app: &mut AppState, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);

    draw_tab(frame, app, chunks[0]);
    match app.view {
        ViewMode::Empty => draw_empty(frame, app, chunks[1]),
        ViewMode::Data => draw_table(frame, app, chunks[1]),
        ViewMode::Schema => draw_schema(frame, app, chunks[1]),
    }
    draw_status(frame, app, chunks[2]);
}

fn draw_tab(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    if app.tabs.is_empty() {
        let title = truncate_to_width(&format!(" {} ", app.tab_title()), area.width as usize);
        let line = Line::from(Span::styled(
            title,
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        frame.render_widget(Paragraph::new(line), area);
        return;
    }

    let mut spans = Vec::new();
    let mut used_width = 0usize;
    let max_width = area.width as usize;
    for (index, tab) in app.tabs.iter().enumerate() {
        let raw_title = format!(" {}:{} ", index + 1, tab.title);
        let remaining = max_width.saturating_sub(used_width);
        if remaining == 0 {
            break;
        }
        let title = truncate_to_width(&raw_title, remaining);
        used_width += unicode_width::UnicodeWidthStr::width(title.as_str());
        let is_active = app.active_tab == Some(index);
        let style = if is_active {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White).bg(Color::DarkGray)
        };
        spans.push(Span::styled(title, style));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_empty(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let message = if let Some(path) = app.active_file_path() {
        format!("No rows loaded from {}", path.display())
    } else {
        "No file opened. Press d to focus the file list, choose a .parquet file, then press Enter."
            .to_string()
    };
    let paragraph = Paragraph::new(message)
        .block(Block::default().borders(Borders::ALL).title("Data"))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn draw_schema(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let header = Row::new([
        Cell::from("#").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("name").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("logical type").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("physical type").style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    let rows = app.columns.iter().map(|column| {
        Row::new([
            Cell::from(column.index.to_string()),
            Cell::from(column.name.clone()),
            Cell::from(column.logical_type.clone()),
            Cell::from(
                column
                    .physical_type
                    .clone()
                    .unwrap_or_else(|| "-".to_string()),
            ),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Length(32),
            Constraint::Min(24),
            Constraint::Length(18),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title("Schema"))
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    // Keep the selected schema row visible: clamp scroll to a window around it.
    let visible_rows = area.height.saturating_sub(2) as usize;
    let selected = app.selected_schema_row;
    let scroll = if visible_rows == 0 {
        0
    } else if selected < app.schema_scroll {
        selected
    } else if selected >= app.schema_scroll + visible_rows {
        selected + 1 - visible_rows
    } else {
        app.schema_scroll
    };
    let mut state = TableState::default()
        .with_selected(Some(app.selected_schema_row))
        .with_offset(scroll);
    frame.render_stateful_widget(table, area, &mut state);
}

/// Build the table header label for a column, appending a sort marker
/// (`▲` ascending / `▼` descending) when this is the active sort column.
fn column_header_label(
    column: &crate::data::ColumnInfo,
    sort_column: Option<usize>,
    sort_ascending: bool,
) -> String {
    let mut label = format!("{}:{}", column.name, column.logical_type);
    if let Some(physical_type) = &column.physical_type {
        label.push_str(&format!("/{physical_type}"));
    }
    if sort_column == Some(column.index) {
        label.push_str(if sort_ascending { " ▲" } else { " ▼" });
    }
    label
}

fn draw_table(frame: &mut Frame<'_>, app: &mut AppState, area: Rect) {
    const MIN_COLUMN_WIDTH: u16 = 12;
    const MAX_COLUMN_WIDTH: u16 = 40;
    const ROW_NUMBER_WIDTH: u16 = 6;

    let inner_width = area.width.saturating_sub(2).max(1);
    let available = inner_width.saturating_sub(ROW_NUMBER_WIDTH).max(1);
    // Show as many columns as fit between the min and max widths.
    let visible_column_count = ((available / MIN_COLUMN_WIDTH).max(1) as usize)
        .min((available / MAX_COLUMN_WIDTH).max(1) as usize);
    app.table_visible_column_count = visible_column_count;
    let visible_columns = app
        .columns
        .iter()
        .skip(app.scroll_x)
        .take(visible_column_count)
        .collect::<Vec<_>>();
    if visible_columns.is_empty() {
        draw_empty(frame, app, area);
        return;
    }

    // Estimate a width per column from the longest header / cell value currently
    // visible, clamped to [MIN, MAX]. A single visible column takes all space.
    let column_widths = if visible_columns.len() == 1 {
        vec![available]
    } else {
        visible_columns
            .iter()
            .map(|column| {
                let mut width = unicode_width::UnicodeWidthStr::width(column.name.as_str()).max(
                    unicode_width::UnicodeWidthStr::width(column.logical_type.as_str()),
                ) + 1;
                for row in &app.rows {
                    if let Some(cell) = row.cells.get(column.index) {
                        width =
                            width.max(unicode_width::UnicodeWidthStr::width(cell.display.as_str()));
                    }
                }
                width = width
                    .min(MAX_COLUMN_WIDTH as usize)
                    .max(MIN_COLUMN_WIDTH as usize);
                width as u16
            })
            .collect::<Vec<_>>()
    };
    let cell_display_widths = column_widths
        .iter()
        .map(|width| (*width).saturating_sub(1) as usize)
        .collect::<Vec<_>>();

    let header_cells = visible_columns.iter().enumerate().map(|(i, column)| {
        let label = column_header_label(column, app.sort_column, app.sort_ascending);
        let sorted = app.sort_column == Some(column.index);
        let style = if sorted {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        };
        Cell::from(truncate_to_width(&label, cell_display_widths[i])).style(style)
    });
    let header = Row::new(
        std::iter::once(Cell::from(Span::styled(
            "#",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )))
        .chain(header_cells),
    );

    let rows = app.rows.iter().enumerate().map(|(row_index, row)| {
        let row_number = (app.offset + row_index + 1).to_string();
        let number_cell = Cell::from(Span::raw(truncate_to_width(
            &row_number,
            ROW_NUMBER_WIDTH as usize - 1,
        )));
        let body_cells = visible_columns.iter().enumerate().map(|(i, column)| {
            let value = row
                .cells
                .get(column.index)
                .map(|cell| cell.display.as_str())
                .unwrap_or_default();
            let cell = Cell::from(truncate_to_width(value, cell_display_widths[i]));
            if row_index == app.selected_row && column.index == app.selected_col {
                cell.style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                cell
            }
        });
        Row::new(std::iter::once(number_cell).chain(body_cells))
    });

    let mut widths = vec![Constraint::Length(ROW_NUMBER_WIDTH)];
    widths.extend(column_widths.iter().map(|width| Constraint::Length(*width)));
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title("Data"))
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut state = TableState::default().with_selected(Some(app.selected_row));
    frame.render_stateful_widget(table, area, &mut state);
}

fn draw_status(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let status = truncate_to_width(&app.status_text(), area.width as usize);
    if app.error.is_some() {
        frame.render_widget(
            Paragraph::new(status).style(Style::default().fg(Color::Red)),
            area,
        );
        return;
    }

    let base_style = Style::default().fg(Color::White).bg(Color::DarkGray);
    let filter_style = Style::default()
        .fg(Color::Green)
        .bg(Color::DarkGray)
        .add_modifier(Modifier::BOLD);

    let Some(filter) = app.filter.as_deref() else {
        frame.render_widget(Paragraph::new(status).style(base_style), area);
        return;
    };

    let filter_segment = format!("filter {filter}");
    if let Some(index) = status.find(&filter_segment) {
        let before = &status[..index];
        let after_start = index + filter_segment.len();
        let after = status.get(after_start..).unwrap_or_default();
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(before.to_string(), base_style),
                Span::styled(filter_segment, filter_style),
                Span::styled(after.to_string(), base_style),
            ])),
            area,
        );
    } else {
        frame.render_widget(Paragraph::new(status).style(base_style), area);
    }
}

fn draw_cell_detail(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let popup = centered_rect(72, 62, area);
    let column = app.columns.get(app.selected_col);
    let value = display_cell_detail_value(app.selected_cell_value().unwrap_or(""));

    let title = if app.detail_search_active {
        "Cell Detail (search)".to_string()
    } else if app.detail_search_query.is_some() {
        let total = app.detail_search_matches.len();
        let current = app.detail_search_index.map(|i| i + 1).unwrap_or(0);
        format!(
            "Cell Detail: {}  [{}/{}]",
            column.map(|c| c.name.as_str()).unwrap_or(""),
            current,
            total
        )
    } else {
        column
            .map(|column| format!("Cell Detail: {}", column.name))
            .unwrap_or_else(|| "Cell Detail".to_string())
    };

    let detail_lines = if app.detail_search_query.is_some() {
        highlight_json_detail_with_search(&value, app)
    } else {
        highlight_json_detail(&value)
    };

    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("row: ", Style::default().fg(Color::Yellow)),
        Span::raw((app.selected_row + 1).to_string()),
        Span::raw("  "),
        Span::styled("column: ", Style::default().fg(Color::Yellow)),
        Span::raw((app.selected_col + 1).to_string()),
    ]));
    if let Some(column) = column {
        lines.push(Line::from(vec![
            Span::styled("name: ", Style::default().fg(Color::Yellow)),
            Span::raw(column.name.clone()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("type: ", Style::default().fg(Color::Yellow)),
            Span::raw(column.logical_type.clone()),
        ]));
    }
    lines.push(Line::from(""));
    lines.extend(detail_lines);
    lines.push(Line::from(""));

    if app.detail_search_active {
        let input = &app.detail_search_input;
        let cursor = app.detail_search_cursor.min(input.len());
        let before: String = input[..cursor].to_string();
        let cursor_char: String = input[cursor..]
            .chars()
            .next()
            .map(|c| c.to_string())
            .unwrap_or_default();
        let after: String = if cursor < input.len() {
            input[cursor + cursor_char.len()..].to_string()
        } else {
            String::new()
        };
        lines.push(Line::from(vec![
            Span::styled("search: ", Style::default().fg(Color::Cyan)),
            Span::raw(before),
            Span::styled(
                cursor_char,
                Style::default().add_modifier(Modifier::REVERSED),
            ),
            Span::raw(after),
        ]));
    } else {
        let match_info = app
            .detail_search_query
            .as_ref()
            .map(|q| {
                let total = app.detail_search_matches.len();
                let current = app.detail_search_index.map(|i| i + 1).unwrap_or(0);
                format!("  ·  search '{q}': {current}/{total}  (n/N navigate, / new)")
            })
            .unwrap_or_default();
        lines.push(Line::from(Span::styled(
            format!(
                "j/k: scroll  ·  y: copy cell  ·  Y: copy row  ·  /: search{match_info}  ·  Esc: close"
            ),
            Style::default().fg(Color::DarkGray),
        )));
    }

    // Clamp scroll so the popup never scrolls past its content.
    let inner_height = popup.height.saturating_sub(2) as usize;
    let max_scroll = lines.len().saturating_sub(inner_height);
    let scroll = app.cell_detail_scroll.min(max_scroll as u16);

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title(title).borders(Borders::ALL))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        popup,
    );
}

fn filter_input_before_cursor(app: &AppState) -> String {
    let cursor = app.filter_cursor.min(app.filter_input.len());
    app.filter_input[..cursor].to_string()
}

fn filter_cursor_char(app: &AppState) -> String {
    let cursor = app.filter_cursor.min(app.filter_input.len());
    app.filter_input[cursor..]
        .chars()
        .next()
        .map(|ch| ch.to_string())
        .unwrap_or_else(|| " ".to_string())
}

fn filter_input_after_cursor(app: &AppState) -> String {
    let cursor = app.filter_cursor.min(app.filter_input.len());
    let mut chars = app.filter_input[cursor..].chars();
    let _ = chars.next();
    chars.collect()
}

fn draw_filter_popup(frame: &mut Frame<'_>, app: &AppState, area: Rect) {
    let popup = centered_rect(72, 22, area);
    let mut lines = vec![
        Line::from("Filter expression (simple syntax):"),
        Line::from(Span::styled(
            "  column op value    op: = != > >= < <= contains    and / or combine",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Yellow)),
            Span::raw(filter_input_before_cursor(app)),
            Span::styled(
                filter_cursor_char(app),
                Style::default().fg(Color::Black).bg(Color::Yellow),
            ),
            Span::raw(filter_input_after_cursor(app)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Tab: complete column  ·  ↑/↓ history  ·  ←/→ Home/End Del/Bksp: edit  ·  Enter: apply",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    if !app.filter_completion_candidates.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "columns:",
            Style::default().fg(Color::Cyan),
        )));
        for (index, candidate) in app.filter_completion_candidates.iter().enumerate() {
            let style = if index == app.filter_completion_index {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let marker = if index == app.filter_completion_index {
                "> "
            } else {
                "  "
            };
            lines.push(Line::from(Span::styled(
                format!("{marker}{candidate}"),
                style,
            )));
        }
    }

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title("Filter").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn draw_help(frame: &mut Frame<'_>, area: Rect) {
    let popup = centered_rect(66, 58, area);
    let lines = vec![
        Line::from(vec![Span::styled(
            "Global",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("  q / Ctrl-C        Quit"),
        Line::from("  h                 Toggle this help"),
        Line::from("  d                 Focus file sidebar"),
        Line::from("  s                 Toggle Schema view"),
        Line::from("  /                 Open filter popup"),
        Line::from("  Tab in filter     Complete/cycle column name"),
        Line::from("  r                 Reset filter"),
        Line::from("  c                 Count rows for current filter"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "File sidebar",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("  j/k or ↑/↓        Move selection"),
        Line::from("  Enter             Enter directory or open .parquet file"),
        Line::from("  Esc               Return focus to data area"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Table",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("  j/k or ↑/↓        Move row selection"),
        Line::from("  J / K             Jump to page bottom / top"),
        Line::from("  y / Y             Copy cell / row via OSC52"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Schema view",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("  j/k or ↑/↓        Move selected field"),
        Line::from("  J / K             Jump to first / last field"),
        Line::from("  y / Y             Copy field cell / row via OSC52"),
        Line::from("  ← / l / →         Move selected column"),
        Line::from("  H / L             Jump to first / last column"),
        Line::from("  n / PageDown      Next page"),
        Line::from("  p / PageUp        Previous page"),
        Line::from("  o / O             Sort by selected column / toggle direction"),
        Line::from("  e                 Export current page to CSV"),
        Line::from("  Enter / Space     Open selected cell detail"),
        Line::from("  Double click      Open clicked cell detail"),
        Line::from("  y                 Copy selected cell via OSC52"),
        Line::from("  Y                 Copy selected row (JSON) via OSC52"),
        Line::from("  mouse wheel ←/→   Move selected column"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Tabs",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("  Tab / Shift-Tab   Next / previous tab"),
        Line::from("  mouse click tab   Switch tab"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Cell detail popup",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("  j/k or ↑/↓        Scroll content"),
        Line::from("  y / Y             Copy cell / row via OSC52"),
        Line::from("  /                 Search within detail"),
        Line::from("  n / N             Next / previous search match"),
        Line::from("  Esc/Enter/Space   Close popup"),
        Line::from(""),
        Line::from(
            "M1 implements startup, file picker, tab bar, help, schema and first page display.",
        ),
    ];
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title("Help").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_produces_colored_spans_without_panicking() {
        let value = r#"{"name":"上海","count":42,"active":true,"note":null}"#;
        let lines = highlight_json_detail(value);
        // pretty JSON spans multiple lines; at least one line produced.
        assert!(!lines.is_empty());
        // The aggregate spans must reproduce the original value text.
        let mut text = String::new();
        for line in &lines {
            for span in &line.spans {
                text.push_str(&span.content);
            }
        }
        assert!(text.contains("上海"));
        assert!(text.contains("42"));
        assert!(text.contains("null"));
    }

    #[test]
    fn highlight_falls_back_for_plain_text() {
        let lines = highlight_json_detail("just a plain string cell");
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn column_header_label_marks_sorted_column() {
        let column = crate::data::ColumnInfo {
            index: 2,
            name: "score".to_string(),
            logical_type: "Float64".to_string(),
            physical_type: None,
        };
        // Unsorted: no marker.
        assert_eq!(column_header_label(&column, None, true), "score:Float64");
        // Ascending marker on the active sort column.
        assert_eq!(
            column_header_label(&column, Some(2), true),
            "score:Float64 ▲"
        );
        // Descending marker.
        assert_eq!(
            column_header_label(&column, Some(2), false),
            "score:Float64 ▼"
        );
        // A different sort column does not mark this one.
        assert_eq!(column_header_label(&column, Some(0), true), "score:Float64");
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
