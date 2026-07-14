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
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char(' ') => app.show_cell_detail = false,
            KeyCode::Char('q') => app.should_quit = true,
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
        _ => {}
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

fn display_cell_detail_value(value: &str) -> String {
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
    .block(Block::default().borders(Borders::ALL).title("Schema"));
    frame.render_widget(table, area);
}

fn draw_table(frame: &mut Frame<'_>, app: &mut AppState, area: Rect) {
    const DEFAULT_COLUMN_WIDTH: u16 = 24;

    let inner_width = area.width.saturating_sub(2).max(1);
    let visible_column_count = (inner_width / DEFAULT_COLUMN_WIDTH).max(1) as usize;
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

    let column_width = if visible_columns.len() == 1 {
        inner_width
    } else {
        DEFAULT_COLUMN_WIDTH
    };
    let cell_display_width = column_width.saturating_sub(1) as usize;

    let header = Row::new(visible_columns.iter().map(|column| {
        let mut label = format!("{}:{}", column.name, column.logical_type);
        if let Some(physical_type) = &column.physical_type {
            label.push_str(&format!("/{physical_type}"));
        }
        Cell::from(truncate_to_width(&label, cell_display_width)).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    }));

    let rows = app.rows.iter().enumerate().map(|(row_index, row)| {
        Row::new(visible_columns.iter().map(|column| {
            let value = row
                .cells
                .get(column.index)
                .map(|cell| cell.display.as_str())
                .unwrap_or_default();
            let cell = Cell::from(truncate_to_width(value, cell_display_width));
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
        }))
    });

    let widths = visible_columns
        .iter()
        .map(|_| Constraint::Length(column_width))
        .collect::<Vec<_>>();
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
    let title = column
        .map(|column| format!("Cell Detail: {}", column.name))
        .unwrap_or_else(|| "Cell Detail".to_string());

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
    lines.extend(value.lines().map(|line| Line::from(line.to_string())));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "j/k or mouse wheel: scroll  ·  y: copy cell  ·  Esc/Enter/Space: close",
        Style::default().fg(Color::DarkGray),
    )));

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title(title).borders(Borders::ALL))
            .wrap(Wrap { trim: false })
            .scroll((app.cell_detail_scroll, 0)),
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
        Line::from("  ← / l / →         Move selected column"),
        Line::from("  H / L             Jump to first / last column"),
        Line::from("  n / PageDown      Next page"),
        Line::from("  p / PageUp        Previous page"),
        Line::from("  Enter / Space     Open selected cell detail"),
        Line::from("  Double click      Open clicked cell detail"),
        Line::from("  y                 Copy selected cell via OSC52"),
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
