use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use crate::{
    cli::AppConfig,
    data::{ColumnInfo, DataPage, RowView},
    error::Result,
    file_browser::{FileEntryKind, FileSidebar},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Empty,
    Data,
    Schema,
}

#[derive(Debug)]
pub enum SidebarOpenResult {
    None,
    File(PathBuf),
}

#[derive(Debug, Clone)]
pub struct FileTab {
    pub file_path: PathBuf,
    pub title: String,
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<RowView>,
    pub offset: usize,
    pub total_rows: Option<usize>,
    pub selected_row: usize,
    pub selected_col: usize,
    pub scroll_x: usize,
    pub filter: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MouseClickState {
    pub column: u16,
    pub row: u16,
    pub at: Instant,
}
#[derive(Debug)]
pub struct AppState {
    pub root_dir: PathBuf,
    pub sidebar: FileSidebar,
    pub tabs: Vec<FileTab>,
    pub active_tab: Option<usize>,
    pub active_file: Option<PathBuf>,
    pub view: ViewMode,
    pub page_size: usize,
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<RowView>,
    pub offset: usize,
    pub total_rows: Option<usize>,
    pub selected_row: usize,
    pub selected_col: usize,
    pub scroll_x: usize,
    pub filter: Option<String>,
    pub filter_input: String,
    pub filter_cursor: usize,
    pub filter_completion_candidates: Vec<String>,
    pub filter_completion_index: usize,
    pub filter_history: Vec<String>,
    pub filter_history_index: Option<usize>,
    pub show_filter_popup: bool,
    pub table_visible_column_count: usize,
    pub sidebar_width: u16,
    pub resizing_sidebar: bool,
    pub show_help: bool,
    pub show_cell_detail: bool,
    pub cell_detail_scroll: u16,
    pub status: String,
    pub error: Option<String>,
    pub last_mouse_click: Option<MouseClickState>,
    pub should_quit: bool,
}

impl AppState {
    pub fn new(config: AppConfig) -> Result<Self> {
        let sidebar = FileSidebar::new(config.root_directory.clone())?;
        Ok(Self {
            root_dir: config.root_directory,
            sidebar,
            tabs: Vec::new(),
            active_tab: None,
            active_file: config.initial_file_path,
            view: ViewMode::Empty,
            page_size: config.page_size,
            columns: Vec::new(),
            rows: Vec::new(),
            offset: 0,
            total_rows: None,
            selected_row: 0,
            selected_col: 0,
            scroll_x: 0,
            filter: None,
            filter_input: String::new(),
            filter_cursor: 0,
            filter_completion_candidates: Vec::new(),
            filter_completion_index: 0,
            filter_history: Vec::new(),
            filter_history_index: None,
            show_filter_popup: false,
            table_visible_column_count: 1,
            sidebar_width: 30,
            resizing_sidebar: false,
            show_help: false,
            show_cell_detail: false,
            cell_detail_scroll: 0,
            status: "Press d to focus file list, h for help, q to quit".to_string(),
            error: None,
            last_mouse_click: None,
            should_quit: false,
        })
    }

    pub fn tab_title(&self) -> String {
        self.active_tab
            .and_then(|index| self.tabs.get(index))
            .map(|tab| tab.title.clone())
            .or_else(|| {
                self.active_file
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "[No file]".to_string())
    }

    pub fn apply_page(&mut self, path: PathBuf, page: DataPage) {
        self.save_active_tab_state();
        let title = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        self.filter = None;
        let tab = FileTab {
            file_path: path,
            title,
            columns: page.columns,
            rows: page.rows,
            offset: page.offset,
            total_rows: page.total_rows,
            selected_row: 0,
            selected_col: 0,
            scroll_x: 0,
            filter: None,
        };
        self.tabs.push(tab);
        let index = self.tabs.len() - 1;
        self.restore_tab_state(index);
        self.error = None;
        self.sidebar.focused = false;
        self.status = format!(
            "Loaded rows {}-{} of {} · columns {} · filter {}",
            self.row_start_display(),
            self.row_end_display(),
            self.total_rows_display(),
            self.columns.len(),
            self.filter_display()
        );
    }

    pub fn tab_index_for_path(&self, path: &Path) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.file_path == path)
    }

    pub fn switch_to_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        self.save_active_tab_state();
        self.restore_tab_state(index);
        self.status = format!("Switched to tab {}: {}", index + 1, self.tab_title());
    }

    pub fn next_tab(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        let next = self
            .active_tab
            .map_or(0, |index| (index + 1) % self.tabs.len());
        self.switch_to_tab(next);
    }

    pub fn previous_tab(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        let previous = self.active_tab.map_or(0, |index| {
            if index == 0 {
                self.tabs.len() - 1
            } else {
                index - 1
            }
        });
        self.switch_to_tab(previous);
    }

    pub fn current_file_path(&self) -> Option<PathBuf> {
        self.active_file.clone()
    }

    pub fn next_page_offset(&self) -> Option<usize> {
        if self.active_file.is_none() || self.rows.is_empty() {
            return None;
        }
        let next = self.offset.saturating_add(self.page_size);
        if self.total_rows.is_some_and(|total| next >= total) {
            None
        } else {
            Some(next)
        }
    }

    pub fn previous_page_offset(&self) -> Option<usize> {
        if self.active_file.is_none() || self.offset == 0 {
            None
        } else {
            Some(self.offset.saturating_sub(self.page_size))
        }
    }

    pub fn replace_active_page(&mut self, page: DataPage) {
        self.columns = page.columns;
        self.rows = page.rows;
        self.offset = page.offset;
        self.total_rows = page.total_rows;
        self.selected_row = 0;
        self.selected_col = self.selected_col.min(self.columns.len().saturating_sub(1));
        self.scroll_x = self.scroll_x.min(self.columns.len().saturating_sub(1));
        self.show_cell_detail = false;
        self.cell_detail_scroll = 0;
        self.show_filter_popup = false;
        self.error = None;
        self.status = format!(
            "Loaded rows {}-{} of {} · page {} · columns {} · filter {}",
            self.row_start_display(),
            self.row_end_display(),
            self.total_rows_display(),
            self.page_display(),
            self.columns.len(),
            self.filter_display()
        );
        self.save_active_tab_state();
    }

    pub fn page_display(&self) -> String {
        let current = if self.rows.is_empty() {
            0
        } else {
            self.offset / self.page_size + 1
        };
        match self.total_rows {
            Some(0) => "0/0".to_string(),
            Some(total) => {
                let pages = total.div_ceil(self.page_size);
                format!("{current}/{pages}")
            }
            None => current.to_string(),
        }
    }

    pub fn row_start_display(&self) -> usize {
        if self.rows.is_empty() {
            0
        } else {
            self.offset + 1
        }
    }

    pub fn row_end_display(&self) -> usize {
        self.offset + self.rows.len()
    }

    pub fn total_rows_display(&self) -> String {
        self.total_rows
            .map_or_else(|| "?".to_string(), |total| total.to_string())
    }

    fn save_active_tab_state(&mut self) {
        let Some(index) = self.active_tab else {
            return;
        };
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        tab.columns = self.columns.clone();
        tab.rows = self.rows.clone();
        tab.offset = self.offset;
        tab.total_rows = self.total_rows;
        tab.selected_row = self.selected_row;
        tab.selected_col = self.selected_col;
        tab.scroll_x = self.scroll_x;
        tab.filter = self.filter.clone();
    }

    fn restore_tab_state(&mut self, index: usize) {
        let Some(tab) = self.tabs.get(index).cloned() else {
            return;
        };
        self.active_tab = Some(index);
        self.active_file = Some(tab.file_path);
        self.view = ViewMode::Data;
        self.columns = tab.columns;
        self.rows = tab.rows;
        self.filter = tab.filter;
        self.offset = tab.offset;
        self.total_rows = tab.total_rows;
        self.selected_row = tab.selected_row.min(self.rows.len().saturating_sub(1));
        self.selected_col = tab.selected_col.min(self.columns.len().saturating_sub(1));
        self.scroll_x = tab.scroll_x.min(self.columns.len().saturating_sub(1));
        self.show_cell_detail = false;
        self.cell_detail_scroll = 0;
    }

    pub fn set_error(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.status = message.clone();
        self.error = Some(message);
    }

    pub fn open_selected_sidebar_entry(&mut self) -> Result<SidebarOpenResult> {
        let Some(entry) = self.sidebar.selected_entry().cloned() else {
            return Ok(SidebarOpenResult::None);
        };
        match entry.kind {
            FileEntryKind::Directory => {
                self.sidebar.toggle_directory(&entry.path)?;
                self.status = format!("Toggled directory: {}", entry.path.display());
                Ok(SidebarOpenResult::None)
            }
            FileEntryKind::ParquetFile => Ok(SidebarOpenResult::File(entry.path)),
            FileEntryKind::OtherFile => {
                self.status = "Select a .parquet file".to_string();
                Ok(SidebarOpenResult::None)
            }
        }
    }

    pub fn select_row_previous(&mut self) {
        self.selected_row = self.selected_row.saturating_sub(1);
    }

    pub fn select_row_next(&mut self) {
        if self.selected_row + 1 < self.rows.len() {
            self.selected_row += 1;
        }
    }

    pub fn select_row_top(&mut self) {
        self.selected_row = 0;
    }

    pub fn select_row_bottom(&mut self) {
        self.selected_row = self.rows.len().saturating_sub(1);
    }

    pub fn selected_row_display(&self) -> usize {
        if self.rows.is_empty() {
            0
        } else {
            self.offset + self.selected_row + 1
        }
    }

    pub fn selected_col_display(&self) -> usize {
        if self.columns.is_empty() {
            0
        } else {
            self.selected_col + 1
        }
    }

    pub fn select_col_previous(&mut self) {
        self.selected_col = self.selected_col.saturating_sub(1);
        if self.selected_col < self.scroll_x {
            self.scroll_x = self.selected_col;
        }
    }

    pub fn select_col_next(&mut self) {
        if self.selected_col + 1 < self.columns.len() {
            self.selected_col += 1;
            let visible_count = self.table_visible_column_count.max(1);
            if self.selected_col >= self.scroll_x + visible_count {
                self.scroll_x = self.selected_col + 1 - visible_count;
            }
        }
    }

    pub fn select_first_col(&mut self) {
        self.selected_col = 0;
        self.scroll_x = 0;
    }

    pub fn select_last_col(&mut self) {
        if self.columns.is_empty() {
            return;
        }
        self.selected_col = self.columns.len() - 1;
        let visible_count = self.table_visible_column_count.max(1);
        self.scroll_x = self.columns.len().saturating_sub(visible_count);
    }

    pub fn open_cell_detail(&mut self) {
        if self.selected_cell_value().is_some() {
            self.show_cell_detail = true;
            self.cell_detail_scroll = 0;
        }
    }

    pub fn toggle_schema_view(&mut self) {
        if self.active_file.is_none() {
            self.status = "No file opened".to_string();
            return;
        }
        self.view = match self.view {
            ViewMode::Schema => ViewMode::Data,
            _ => ViewMode::Schema,
        };
        self.status = match self.view {
            ViewMode::Schema => "Schema view".to_string(),
            ViewMode::Data => "Data view".to_string(),
            ViewMode::Empty => self.status.clone(),
        };
    }

    pub fn open_filter_popup(&mut self) {
        if self.active_file.is_none() {
            self.status = "No file opened".to_string();
            return;
        }
        self.filter_input = self.filter.clone().unwrap_or_default();
        self.filter_cursor = self.filter_input.len();
        self.filter_completion_candidates.clear();
        self.filter_completion_index = 0;
        self.filter_history_index = None;
        self.show_filter_popup = true;
    }

    pub fn cancel_filter_popup(&mut self) {
        self.show_filter_popup = false;
        self.filter_input.clear();
        self.filter_cursor = 0;
        self.filter_completion_candidates.clear();
        self.filter_completion_index = 0;
        self.filter_history_index = None;
    }

    pub fn set_filter_from_input(&mut self) -> Option<String> {
        let filter = self.filter_input.trim().to_string();
        self.filter = if filter.is_empty() {
            None
        } else {
            self.add_filter_to_history(filter.clone());
            Some(filter.clone())
        };
        self.offset = 0;
        self.selected_row = 0;
        self.show_filter_popup = false;
        self.filter_input.clear();
        self.filter_cursor = 0;
        self.filter_completion_candidates.clear();
        self.filter_completion_index = 0;
        self.filter_history_index = None;
        self.filter.clone()
    }

    pub fn add_filter_to_history(&mut self, filter: String) {
        if filter.is_empty() {
            return;
        }
        if self.filter_history.iter().any(|entry| *entry == filter) {
            return;
        }
        self.filter_history.push(filter);
    }

    pub fn previous_filter_history(&mut self) {
        if self.filter_history.is_empty() {
            return;
        }
        let next = match self.filter_history_index {
            None => self.filter_history.len() - 1,
            Some(index) if index == 0 => 0,
            Some(index) => index - 1,
        };
        self.filter_history_index = Some(next);
        self.filter_input = self.filter_history[next].clone();
        self.filter_cursor = self.filter_input.len();
    }

    pub fn next_filter_history(&mut self) {
        let Some(index) = self.filter_history_index else {
            return;
        };
        if index + 1 >= self.filter_history.len() {
            self.filter_history_index = None;
            self.filter_input.clear();
            self.filter_cursor = 0;
            return;
        }
        let next = index + 1;
        self.filter_history_index = Some(next);
        self.filter_input = self.filter_history[next].clone();
        self.filter_cursor = self.filter_input.len();
    }

    pub fn reset_filter(&mut self) {
        self.filter = None;
        self.offset = 0;
        self.selected_row = 0;
        self.show_filter_popup = false;
        self.filter_input.clear();
        self.filter_cursor = 0;
        self.filter_completion_candidates.clear();
        self.filter_completion_index = 0;
        self.filter_history_index = None;
    }

    pub fn insert_filter_char(&mut self, ch: char) {
        self.filter_cursor = self.filter_cursor.min(self.filter_input.len());
        self.filter_input.insert(self.filter_cursor, ch);
        self.filter_cursor += ch.len_utf8();
    }

    pub fn backspace_filter_char(&mut self) {
        self.filter_cursor = self.filter_cursor.min(self.filter_input.len());
        if self.filter_cursor == 0 {
            return;
        }
        let previous = self.filter_input[..self.filter_cursor]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(0);
        self.filter_input.drain(previous..self.filter_cursor);
        self.filter_cursor = previous;
    }

    pub fn delete_filter_char(&mut self) {
        self.filter_cursor = self.filter_cursor.min(self.filter_input.len());
        if self.filter_cursor >= self.filter_input.len() {
            return;
        }
        let next = self.filter_input[self.filter_cursor..]
            .char_indices()
            .nth(1)
            .map(|(index, _)| self.filter_cursor + index)
            .unwrap_or(self.filter_input.len());
        self.filter_input.drain(self.filter_cursor..next);
    }

    pub fn move_filter_cursor_left(&mut self) {
        self.filter_cursor = self.filter_cursor.min(self.filter_input.len());
        if self.filter_cursor == 0 {
            return;
        }
        self.filter_cursor = self.filter_input[..self.filter_cursor]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(0);
    }

    pub fn move_filter_cursor_right(&mut self) {
        self.filter_cursor = self.filter_cursor.min(self.filter_input.len());
        if self.filter_cursor >= self.filter_input.len() {
            return;
        }
        self.filter_cursor = self.filter_input[self.filter_cursor..]
            .char_indices()
            .nth(1)
            .map(|(index, _)| self.filter_cursor + index)
            .unwrap_or(self.filter_input.len());
    }

    pub fn move_filter_cursor_home(&mut self) {
        self.filter_cursor = 0;
    }

    pub fn move_filter_cursor_end(&mut self) {
        self.filter_cursor = self.filter_input.len();
    }

    pub fn complete_filter_field(&mut self, reverse: bool) {
        self.filter_cursor = self.filter_cursor.min(self.filter_input.len());
        let token_start = self.filter_input[..self.filter_cursor]
            .rfind(char::is_whitespace)
            .map_or(0, |index| index + 1);
        let token_end = self.filter_input[self.filter_cursor..]
            .find(char::is_whitespace)
            .map_or(self.filter_input.len(), |index| self.filter_cursor + index);
        let prefix = &self.filter_input[token_start..self.filter_cursor];
        let current_token = &self.filter_input[token_start..token_end];

        let continued = !self.filter_completion_candidates.is_empty()
            && self
                .filter_completion_candidates
                .get(self.filter_completion_index)
                .is_some_and(|name| name == current_token);

        let matches = if continued {
            self.filter_completion_candidates.clone()
        } else {
            let mut generated = self
                .columns
                .iter()
                .filter(|column| column.name.starts_with(prefix))
                .map(|column| column.name.clone())
                .collect::<Vec<_>>();
            generated.sort();
            self.filter_completion_candidates = generated.clone();
            generated
        };

        if matches.is_empty() {
            self.status = format!("No column matches '{prefix}'");
            return;
        }

        let selected = if continued {
            let len = matches.len();
            if reverse {
                (self.filter_completion_index + len - 1) % len
            } else {
                (self.filter_completion_index + 1) % len
            }
        } else {
            matches
                .iter()
                .position(|name| name == current_token)
                .unwrap_or(0)
        };
        self.filter_completion_index = selected;
        let replacement = &matches[selected];
        self.filter_input
            .replace_range(token_start..token_end, replacement);
        self.filter_cursor = token_start + replacement.len();

        if matches.len() > 1 {
            self.status = format!("Column matches: {}", matches.join(", "));
        } else {
            self.status = format!("Completed column: {replacement}");
        }
    }

    pub fn filter_display(&self) -> String {
        self.filter.clone().unwrap_or_else(|| "-".to_string())
    }

    pub fn selected_cell_value(&self) -> Option<&str> {
        self.rows
            .get(self.selected_row)
            .and_then(|row| row.cells.get(self.selected_col))
            .map(|cell| cell.detail.as_str())
    }

    pub fn status_text(&self) -> String {
        if let Some(error) = &self.error {
            format!("error: {error}")
        } else if self.active_file.is_none() {
            format!("{} | root: {}", self.status, self.root_dir.display())
        } else {
            format!(
                "{} | rows {}-{} of {} | page {} | cols {} | filter {} | selected r{} c{}",
                self.status,
                self.row_start_display(),
                self.row_end_display(),
                self.total_rows_display(),
                self.page_display(),
                self.columns.len(),
                self.filter_display(),
                self.selected_row_display(),
                self.selected_col_display(),
            )
        }
    }

    pub fn active_file_path(&self) -> Option<&Path> {
        self.active_file.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::AppConfig;
    use crate::data::{ColumnInfo, DataPage, RowView};
    use std::path::PathBuf;

    fn test_state() -> AppState {
        let config = AppConfig {
            initial_file_path: None,
            page_size: 50,
            root_directory: std::env::temp_dir(),
        };
        AppState::new(config).unwrap()
    }

    fn set_columns(state: &mut AppState, names: &[&str]) {
        state.columns = names
            .iter()
            .enumerate()
            .map(|(index, name)| ColumnInfo {
                index,
                name: name.to_string(),
                logical_type: "Utf8".to_string(),
                physical_type: None,
            })
            .collect();
    }

    fn single_row() -> RowView {
        RowView { cells: Vec::new() }
    }

    // P1.2: insert at cursor position
    #[test]
    fn insert_at_cursor_splits_token() {
        let mut state = test_state();
        state.filter_input = "city  上海".to_string();
        state.filter_cursor = 5;
        state.insert_filter_char('=');
        assert_eq!(state.filter_input, "city = 上海");
        assert_eq!(state.filter_cursor, 6);
    }

    // P1.2: backspace removes char before cursor without breaking UTF-8
    #[test]
    fn backspace_removes_previous_char() {
        let mut state = test_state();
        state.filter_input = "city = 上海".to_string();
        state.filter_cursor = state.filter_input.len();
        state.backspace_filter_char();
        assert_eq!(state.filter_input, "city = 上");
        assert_eq!(state.filter_cursor, 10);
    }

    // P1.2: delete removes char at cursor
    #[test]
    fn delete_removes_current_char() {
        let mut state = test_state();
        state.filter_input = "city = 上海".to_string();
        state.filter_cursor = 7;
        state.delete_filter_char();
        assert_eq!(state.filter_input, "city = 海");
        assert_eq!(state.filter_cursor, 7);
    }

    // P1.2: left cursor moves on UTF-8 boundaries, never panics
    #[test]
    fn left_cursor_stays_on_char_boundary() {
        let mut state = test_state();
        state.filter_input = "a你b".to_string();
        state.filter_cursor = state.filter_input.len();
        state.move_filter_cursor_left();
        assert_eq!(state.filter_cursor, 4);
        state.move_filter_cursor_left();
        assert_eq!(state.filter_cursor, 1);
        state.move_filter_cursor_left();
        assert_eq!(state.filter_cursor, 0);
        state.move_filter_cursor_left();
        assert_eq!(state.filter_cursor, 0);
    }

    // P1.2: right cursor moves on UTF-8 boundaries
    #[test]
    fn right_cursor_stays_on_char_boundary() {
        let mut state = test_state();
        state.filter_input = "a你b".to_string();
        state.filter_cursor = 0;
        state.move_filter_cursor_right();
        assert_eq!(state.filter_cursor, 1);
        state.move_filter_cursor_right();
        assert_eq!(state.filter_cursor, 4);
        state.move_filter_cursor_right();
        assert_eq!(state.filter_cursor, 5);
        state.move_filter_cursor_right();
        assert_eq!(state.filter_cursor, 5);
    }

    // P1.2: home/end
    #[test]
    fn home_and_end_move_to_boundaries() {
        let mut state = test_state();
        state.filter_input = "score > 80".to_string();
        state.move_filter_cursor_home();
        assert_eq!(state.filter_cursor, 0);
        state.move_filter_cursor_end();
        assert_eq!(state.filter_cursor, 10);
    }

    // P1.2: empty input does not panic
    #[test]
    fn empty_input_edits_are_safe() {
        let mut state = test_state();
        state.filter_input = String::new();
        state.filter_cursor = 0;
        state.backspace_filter_char();
        state.delete_filter_char();
        state.move_filter_cursor_left();
        state.move_filter_cursor_right();
        assert_eq!(state.filter_cursor, 0);
        assert_eq!(state.filter_input, "");
    }

    // P1.3: single candidate completion
    #[test]
    fn completes_single_candidate() {
        let mut state = test_state();
        set_columns(&mut state, &["city", "score", "status"]);
        state.filter_input = "ci".to_string();
        state.filter_cursor = 2;
        state.complete_filter_field(false);
        assert_eq!(state.filter_input, "city");
        assert_eq!(state.filter_cursor, 4);
    }

    // P1.3: cycling through multiple candidates and reverse
    #[test]
    fn cycles_through_multiple_candidates() {
        let mut state = test_state();
        set_columns(&mut state, &["score", "status"]);
        state.filter_input = "s".to_string();
        state.filter_cursor = 1;
        state.complete_filter_field(false);
        assert_eq!(state.filter_input, "score");
        state.complete_filter_field(false);
        assert_eq!(state.filter_input, "status");
        state.complete_filter_field(false);
        assert_eq!(state.filter_input, "score");
        state.filter_input = "score".to_string();
        state.filter_cursor = 5;
        state.complete_filter_field(true);
        assert_eq!(state.filter_input, "status");
    }

    // P1.3: only the current token is replaced
    #[test]
    fn completes_only_current_token() {
        let mut state = test_state();
        set_columns(&mut state, &["city", "score", "status"]);
        state.filter_input = "city = s".to_string();
        state.filter_cursor = 8;
        state.complete_filter_field(false);
        assert_eq!(state.filter_input, "city = score");
    }

    // P1.3: no-match keeps input and reports status
    #[test]
    fn no_match_keeps_input() {
        let mut state = test_state();
        set_columns(&mut state, &["city"]);
        state.filter_input = "zzz".to_string();
        state.filter_cursor = 3;
        state.complete_filter_field(false);
        assert_eq!(state.filter_input, "zzz");
        assert!(state.status.contains("zzz"));
    }

    // P3.3: applied filters are recorded in history without duplicates
    #[test]
    fn applied_filters_enter_history() {
        let mut state = test_state();
        state.active_file = Some(PathBuf::from("file.parquet"));
        state.filter_input = "score > 80".to_string();
        state.set_filter_from_input();
        state.filter_input = "city = 上海".to_string();
        state.set_filter_from_input();
        state.filter_input = "score > 80".to_string();
        state.set_filter_from_input();
        assert_eq!(state.filter_history, vec!["score > 80", "city = 上海"]);
    }

    // P3.3: up/down navigate history and clear it at the end
    #[test]
    fn history_navigation_cycles_through_entries() {
        let mut state = test_state();
        state.active_file = Some(PathBuf::from("file.parquet"));
        state.filter_input = "a = 1".to_string();
        state.set_filter_from_input();
        state.filter_input = "b = 2".to_string();
        state.set_filter_from_input();

        state.previous_filter_history();
        assert_eq!(state.filter_input, "b = 2");
        state.previous_filter_history();
        assert_eq!(state.filter_input, "a = 1");
        state.previous_filter_history();
        assert_eq!(state.filter_input, "a = 1");

        state.next_filter_history();
        assert_eq!(state.filter_input, "b = 2");
        state.next_filter_history();
        assert_eq!(state.filter_input, "");
        assert_eq!(state.filter_history_index, None);
    }

    // P1.4: filter conditions are isolated per tab
    #[test]
    fn filters_are_isolated_per_tab() {
        let mut state = test_state();
        set_columns(&mut state, &["row_id", "score", "city"]);
        let file_a = PathBuf::from("a.parquet");
        let file_b = PathBuf::from("b.parquet");
        let page_a = DataPage {
            columns: state.columns.clone(),
            rows: vec![single_row()],
            offset: 0,
            total_rows: Some(10),
        };
        let page_b = DataPage {
            columns: state.columns.clone(),
            rows: vec![single_row()],
            offset: 0,
            total_rows: Some(10),
        };
        state.apply_page(file_a.clone(), page_a);
        state.filter = Some("score > 80".to_string());
        state.switch_to_tab(0);
        state.apply_page(file_b.clone(), page_b);
        state.filter = Some("city = 上海".to_string());
        state.switch_to_tab(1);
        state.switch_to_tab(0);
        assert_eq!(state.filter, Some("score > 80".to_string()));
        state.switch_to_tab(1);
        assert_eq!(state.filter, Some("city = 上海".to_string()));
    }

    // P1.5: pagination boundaries
    #[test]
    fn pagination_boundaries() {
        let mut state = test_state();
        state.active_file = Some(PathBuf::from("file.parquet"));
        state.rows = vec![single_row()];
        state.offset = 0;
        state.page_size = 50;
        state.total_rows = Some(120);
        assert_eq!(state.previous_page_offset(), None);
        assert_eq!(state.next_page_offset(), Some(50));

        state.offset = 100;
        assert_eq!(state.next_page_offset(), None);

        state.total_rows = None;
        state.offset = 0;
        assert_eq!(state.next_page_offset(), Some(50));

        state.rows = Vec::new();
        assert_eq!(state.next_page_offset(), None);
    }

    // P1.6: schema/data toggle preserves app state
    #[test]
    fn schema_toggle_preserves_state() {
        let mut state = test_state();
        state.active_file = Some(PathBuf::from("file.parquet"));
        set_columns(&mut state, &["a", "b", "c", "d"]);
        state.rows = vec![single_row(); 5];
        state.offset = 50;
        state.selected_row = 3;
        state.selected_col = 2;
        state.filter = Some("score > 80".to_string());

        state.toggle_schema_view();
        assert_eq!(state.view, ViewMode::Schema);
        state.toggle_schema_view();
        assert_eq!(state.view, ViewMode::Data);
        assert_eq!(state.offset, 50);
        assert_eq!(state.selected_row, 3);
        assert_eq!(state.selected_col, 2);
        assert_eq!(state.filter, Some("score > 80".to_string()));
        assert_eq!(state.columns.len(), 4);
        assert_eq!(state.rows.len(), 5);
    }
}
