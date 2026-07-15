use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use crate::{
    action::{Action, DataCommand, InputMode},
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

/// Status of the matched-row count shown in the status bar.
///
/// `Unknown` is the default for filtered reads (a full scan can be expensive).
/// `Known` is used when the count is cheaply available: an unfiltered file's
/// metadata row count, or a filtered result that fits within a single page.
/// `Failed` preserves the error message instead of crashing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CountState {
    Unknown,
    Known(usize),
    Failed(String),
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
    pub selected_schema_row: usize,
    pub filter: Option<String>,
    pub sort_column: Option<usize>,
    pub sort_ascending: bool,
    pub count_state: CountState,
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
    pub export_dir: PathBuf,
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
    pub sort_column: Option<usize>,
    pub sort_ascending: bool,
    pub count_state: CountState,
    pub show_filter_popup: bool,
    pub table_visible_column_count: usize,
    pub sidebar_width: u16,
    pub resizing_sidebar: bool,
    pub selected_schema_row: usize,
    pub schema_scroll: usize,
    pub show_help: bool,
    pub show_cell_detail: bool,
    pub cell_detail_scroll: u16,
    pub detail_search_input: String,
    pub detail_search_cursor: usize,
    pub detail_search_active: bool,
    pub detail_search_query: Option<String>,
    pub detail_search_matches: Vec<(usize, usize, usize)>,
    pub detail_search_index: Option<usize>,
    pub status: String,
    pub error: Option<String>,
    pub last_mouse_click: Option<MouseClickState>,
    pub should_quit: bool,
}

/// Escape a single CSV field: wrap in double quotes and double any embedded quote
/// when the value contains a comma, quote, newline or carriage return.
fn csv_field(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        let escaped = value.replace('"', "\"\"");
        format!("\"{escaped}\"")
    } else {
        value.to_string()
    }
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
            export_dir: config.export_dir,
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
            sort_column: None,
            sort_ascending: true,
            count_state: CountState::Unknown,
            show_filter_popup: false,
            table_visible_column_count: 1,
            sidebar_width: 30,
            resizing_sidebar: false,
            selected_schema_row: 0,
            schema_scroll: 0,
            show_help: false,
            show_cell_detail: false,
            cell_detail_scroll: 0,
            detail_search_input: String::new(),
            detail_search_cursor: 0,
            detail_search_active: false,
            detail_search_query: None,
            detail_search_matches: Vec::new(),
            detail_search_index: None,
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
            selected_schema_row: 0,
            filter: None,
            sort_column: None,
            sort_ascending: true,
            count_state: CountState::Unknown,
        };
        self.tabs.push(tab);
        let index = self.tabs.len() - 1;
        self.restore_tab_state(index);
        self.update_count_state();
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
        self.reset_detail_search();
        self.show_filter_popup = false;
        self.error = None;
        self.update_count_state();
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

    /// Derive the matched-row count state from the just-loaded page.
    ///
    /// Unfiltered reads carry the metadata `num_rows` in `total_rows`, so the
    /// count is `Known`. A filtered read that returned fewer than `page_size`
    /// rows is the entire result, so its length is `Known`; otherwise it is
    /// `Unknown` (the user can run an explicit count with `c`).
    fn update_count_state(&mut self) {
        if self.filter.is_none() {
            self.count_state = match self.total_rows {
                Some(total) => CountState::Known(total),
                None => CountState::Unknown,
            };
            return;
        }
        if self.rows.len() < self.page_size {
            self.count_state = CountState::Known(self.offset + self.rows.len());
        } else {
            self.count_state = CountState::Unknown;
        }
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
        match &self.count_state {
            CountState::Known(total) => total.to_string(),
            CountState::Failed(_) => "count failed".to_string(),
            CountState::Unknown => self
                .total_rows
                .map_or_else(|| "unknown".to_string(), |total| total.to_string()),
        }
    }

    /// Build a display string for the current sort state.
    ///
    /// Returns `None` when no sort is active, or a string like `"name (asc)"`
    /// showing the sorted column name and direction.
    pub fn sort_display(&self) -> Option<String> {
        self.sort_column.map(|col| {
            let name = self
                .columns
                .get(col)
                .map(|c| c.name.as_str())
                .unwrap_or("?");
            let direction = if self.sort_ascending { "asc" } else { "desc" };
            format!("{name} ({direction})")
        })
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
        tab.selected_schema_row = self.selected_schema_row;
        tab.sort_column = self.sort_column;
        tab.sort_ascending = self.sort_ascending;
        tab.count_state = self.count_state.clone();
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
        self.selected_schema_row = tab
            .selected_schema_row
            .min(self.columns.len().saturating_sub(1));
        self.sort_column = tab.sort_column;
        self.sort_ascending = tab.sort_ascending;
        self.count_state = tab.count_state.clone();
        self.schema_scroll = 0;
        self.show_cell_detail = false;
        self.cell_detail_scroll = 0;
        self.reset_detail_search();
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

    pub fn select_schema_row_next(&mut self) {
        if self.selected_schema_row + 1 < self.columns.len() {
            self.selected_schema_row += 1;
        }
    }

    pub fn select_schema_row_previous(&mut self) {
        self.selected_schema_row = self.selected_schema_row.saturating_sub(1);
    }

    pub fn select_schema_row_top(&mut self) {
        self.selected_schema_row = 0;
    }

    pub fn select_schema_row_bottom(&mut self) {
        self.selected_schema_row = self.columns.len().saturating_sub(1);
    }

    pub fn sort_by_column(&mut self, column_index: usize) {
        if column_index >= self.columns.len() {
            return;
        }
        if self.sort_column == Some(column_index) {
            self.sort_ascending = !self.sort_ascending;
        } else {
            self.sort_column = Some(column_index);
            self.sort_ascending = true;
        }
        crate::data::sort_rows_by_column(&mut self.rows, column_index, self.sort_ascending);
        self.selected_row = self.selected_row.min(self.rows.len().saturating_sub(1));
        let column = self
            .columns
            .get(column_index)
            .map(|c| c.name.clone())
            .unwrap_or_default();
        self.status = format!(
            "Sorted by {} ({})",
            column,
            if self.sort_ascending { "asc" } else { "desc" }
        );
    }

    pub fn toggle_sort_direction(&mut self) {
        let Some(column) = self.sort_column else {
            self.status = "No sort column selected".to_string();
            return;
        };
        self.sort_ascending = !self.sort_ascending;
        crate::data::sort_rows_by_column(&mut self.rows, column, self.sort_ascending);
        let name = self
            .columns
            .get(column)
            .map(|c| c.name.clone())
            .unwrap_or_default();
        self.status = format!(
            "Sorted by {} ({})",
            name,
            if self.sort_ascending { "asc" } else { "desc" }
        );
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
            self.reset_detail_search();
        }
    }

    pub fn reset_detail_search(&mut self) {
        self.detail_search_input.clear();
        self.detail_search_cursor = 0;
        self.detail_search_active = false;
        self.detail_search_query = None;
        self.detail_search_matches.clear();
        self.detail_search_index = None;
    }

    pub fn start_detail_search(&mut self) {
        self.detail_search_active = true;
        self.detail_search_input.clear();
        self.detail_search_cursor = 0;
    }

    pub fn cancel_detail_search(&mut self) {
        self.detail_search_active = false;
        self.detail_search_input.clear();
        self.detail_search_cursor = 0;
    }

    pub fn insert_detail_search_char(&mut self, ch: char) {
        self.detail_search_cursor = self
            .detail_search_cursor
            .min(self.detail_search_input.len());
        self.detail_search_input
            .insert(self.detail_search_cursor, ch);
        self.detail_search_cursor += ch.len_utf8();
    }

    pub fn backspace_detail_search_char(&mut self) {
        self.detail_search_cursor = self
            .detail_search_cursor
            .min(self.detail_search_input.len());
        if self.detail_search_cursor == 0 {
            return;
        }
        let previous = self.detail_search_input[..self.detail_search_cursor]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(0);
        self.detail_search_input
            .drain(previous..self.detail_search_cursor);
        self.detail_search_cursor = previous;
    }

    /// Execute the detail search: find all byte ranges of the query in the
    /// pretty-printed cell value and store them for navigation and highlighting.
    pub fn execute_detail_search(&mut self) {
        let query = self.detail_search_input.trim().to_string();
        if query.is_empty() {
            self.detail_search_query = None;
            self.detail_search_matches.clear();
            self.detail_search_index = None;
            self.detail_search_active = false;
            return;
        }

        let value = self
            .selected_cell_value()
            .map(crate::tui::display_cell_detail_value)
            .unwrap_or_default();

        let query_len = query.len();
        let mut matches = Vec::new();
        let mut search_from = 0usize;
        while search_from <= value.len().saturating_sub(query_len) {
            if let Some(pos) = value[search_from..].find(&query) {
                let abs_pos = search_from + pos;
                matches.push((abs_pos, abs_pos + query_len, query_len));
                search_from = abs_pos + query_len;
            } else {
                break;
            }
        }

        if matches.is_empty() {
            self.status = format!("No matches for '{query}'");
        } else {
            self.status = format!("Found {} matches for '{query}'", matches.len());
        }
        self.detail_search_query = Some(query);
        self.detail_search_matches = matches;
        self.detail_search_index = if self.detail_search_matches.is_empty() {
            None
        } else {
            Some(0)
        };
        self.detail_search_active = false;
        self.scroll_to_current_detail_match();
    }

    pub fn next_detail_search_match(&mut self) {
        if self.detail_search_matches.is_empty() {
            return;
        }
        let next = match self.detail_search_index {
            None => 0,
            Some(i) => (i + 1) % self.detail_search_matches.len(),
        };
        self.detail_search_index = Some(next);
        self.scroll_to_current_detail_match();
        self.status = format!("Match {}/{}", next + 1, self.detail_search_matches.len());
    }

    pub fn previous_detail_search_match(&mut self) {
        if self.detail_search_matches.is_empty() {
            return;
        }
        let prev = match self.detail_search_index {
            None => 0,
            Some(0) => self.detail_search_matches.len() - 1,
            Some(i) => i - 1,
        };
        self.detail_search_index = Some(prev);
        self.scroll_to_current_detail_match();
        self.status = format!("Match {}/{}", prev + 1, self.detail_search_matches.len());
    }

    /// Scroll the detail popup so the current match is visible.
    fn scroll_to_current_detail_match(&mut self) {
        let Some(index) = self.detail_search_index else {
            return;
        };
        let Some(&(start, _, _)) = self.detail_search_matches.get(index) else {
            return;
        };
        let value = self
            .selected_cell_value()
            .map(crate::tui::display_cell_detail_value)
            .unwrap_or_default();
        let line_num = value[..start.min(value.len())].matches('\n').count();
        self.cell_detail_scroll = line_num as u16;
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

    /// Write the current page (header + rows) to a CSV file under the
    /// configured export directory and report the path in the status bar.
    /// Failures surface as actionable errors. Only the currently loaded page
    /// is exported, since data is read on demand, page by page.
    pub fn export_current_page_csv(&mut self) {
        if self.active_file.is_none() || self.rows.is_empty() {
            self.status = "No rows to export".to_string();
            return;
        }
        let stem = self
            .active_file
            .as_ref()
            .and_then(|path| path.file_stem())
            .and_then(|stem| stem.to_str())
            .unwrap_or("export");
        let path = self.export_dir.join(format!("{stem}.page.csv"));

        let mut writer = match std::fs::File::create(&path) {
            Ok(writer) => writer,
            Err(error) => {
                self.set_error(format!("failed to create export file: {error}"));
                return;
            }
        };

        use std::io::Write;
        let mut header = String::from("#");
        for column in &self.columns {
            header.push(',');
            header.push_str(&csv_field(&column.name));
        }
        if writeln!(writer, "{header}").is_err() {
            self.set_error("failed to write export header".to_string());
            return;
        }

        for (row_index, row) in self.rows.iter().enumerate() {
            let mut line = (self.offset + row_index + 1).to_string();
            for column in &self.columns {
                line.push(',');
                let value = row
                    .cells
                    .get(column.index)
                    .map(|cell| cell.detail.as_str())
                    .unwrap_or("");
                line.push_str(&csv_field(value));
            }
            if writeln!(writer, "{line}").is_err() {
                self.set_error("failed to write export row".to_string());
                return;
            }
        }

        self.status = format!("Exported page to {}", path.display());
    }

    /// Count rows matching the active filter by scanning the file. The result
    /// is shown in the status bar; failures are surfaced as actionable errors.
    #[allow(dead_code)]
    pub fn count_current_filter(&mut self) {
        let Some(path) = self.current_file_path() else {
            self.status = "No file opened".to_string();
            return;
        };
        let data_source = crate::data::ParquetFileDataSource::new(path);
        match data_source.count_with_filter(self.filter.as_deref()) {
            Ok(count) => {
                self.count_state = CountState::Known(count);
                let filter = self.filter_display();
                self.status = format!("Count for filter '{filter}': {count} rows");
            }
            Err(error) => {
                self.count_state = CountState::Failed(error.to_string());
                self.set_error(error.to_string());
            }
        }
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

    /// Build a pretty JSON object for the selected row, keyed by column name,
    /// using each cell's `detail` value. Used by the copy-row (`Y`) action.
    pub fn selected_row_detail_json(&self) -> Option<String> {
        let row = self.rows.get(self.selected_row)?;
        let mut fields = Vec::new();
        for column in &self.columns {
            if let Some(cell) = row.cells.get(column.index) {
                let value = cell.detail.trim();
                let rendered = if value.starts_with('{') || value.starts_with('[') {
                    serde_json::from_str::<serde_json::Value>(value)
                        .ok()
                        .and_then(|json| serde_json::to_string_pretty(&json).ok())
                        .unwrap_or_else(|| cell.detail.clone())
                } else {
                    cell.detail.clone()
                };
                fields.push(format!(
                    "  {}: {}",
                    serde_json::to_string(&column.name).unwrap_or_default(),
                    rendered
                ));
            }
        }
        Some(format!(
            "{{
{}
}}",
            fields.join(
                ",
"
            )
        ))
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
            let sort_part = self
                .sort_display()
                .map(|s| format!(" | sort: {s}"))
                .unwrap_or_default();
            format!(
                "{} | rows {}-{} of {} | page {} | cols {} | filter {} | selected r{} c{}{}",
                self.status,
                self.row_start_display(),
                self.row_end_display(),
                self.total_rows_display(),
                self.page_display(),
                self.columns.len(),
                self.filter_display(),
                self.selected_row_display(),
                self.selected_col_display(),
                sort_part,
            )
        }
    }

    pub fn active_file_path(&self) -> Option<&Path> {
        self.active_file.as_deref()
    }

    /// Derive the current [`InputMode`] from application state.
    ///
    /// This mirrors the priority order of the pre-refactoring `handle_key`:
    /// cell-detail search > cell-detail > help > filter-popup > schema view
    /// > sidebar focus > data.
    pub fn input_mode(&self) -> InputMode {
        if self.show_cell_detail {
            if self.detail_search_active {
                InputMode::DetailSearch
            } else {
                InputMode::CellDetail
            }
        } else if self.show_help {
            InputMode::Help
        } else if self.show_filter_popup {
            InputMode::FilterPopup
        } else if self.view == ViewMode::Schema {
            InputMode::SchemaView
        } else if self.sidebar.focused {
            InputMode::SidebarFocused
        } else {
            InputMode::Data
        }
    }

    /// Process an [`Action`] and optionally produce a [`DataCommand`] for the
    /// I/O layer to execute.
    ///
    /// Pure state transitions are handled here.  Actions requiring file I/O,
    /// clipboard writes, or Parquet reads return a `DataCommand` that the
    /// TUI run-loop executes and then writes results back.
    pub fn handle_action(&mut self, action: &Action) -> Option<DataCommand> {
        match action {
            // ── Global ──
            Action::Quit => {
                self.should_quit = true;
                None
            }
            Action::ToggleHelp => {
                self.show_help = !self.show_help;
                None
            }

            // ── Data view navigation ──
            Action::SelectRowPrevious => {
                self.select_row_previous();
                None
            }
            Action::SelectRowNext => {
                self.select_row_next();
                None
            }
            Action::SelectRowTop => {
                self.select_row_top();
                None
            }
            Action::SelectRowBottom => {
                self.select_row_bottom();
                None
            }
            Action::SelectColPrevious => {
                self.select_col_previous();
                None
            }
            Action::SelectColNext => {
                self.select_col_next();
                None
            }
            Action::SelectFirstCol => {
                self.select_first_col();
                None
            }
            Action::SelectLastCol => {
                self.select_last_col();
                None
            }
            Action::NextPage => self
                .next_page_offset()
                .map(|offset| DataCommand::LoadPage { offset }),
            Action::PreviousPage => self
                .previous_page_offset()
                .map(|offset| DataCommand::LoadPage { offset }),

            // ── Schema view navigation ──
            Action::SelectSchemaRowPrevious => {
                self.select_schema_row_previous();
                None
            }
            Action::SelectSchemaRowNext => {
                self.select_schema_row_next();
                None
            }
            Action::SelectSchemaRowTop => {
                self.select_schema_row_top();
                None
            }
            Action::SelectSchemaRowBottom => {
                self.select_schema_row_bottom();
                None
            }

            // ── View-mode switches ──
            Action::ToggleSchemaView => {
                self.toggle_schema_view();
                None
            }
            Action::FocusSidebar => {
                self.sidebar.focused = true;
                if let Err(error) = self.sidebar.refresh() {
                    self.set_error(error.to_string());
                }
                None
            }
            Action::OpenCellDetail => {
                self.open_cell_detail();
                None
            }
            Action::CloseCellDetail => {
                self.show_cell_detail = false;
                self.reset_detail_search();
                None
            }

            // ── Tabs ──
            Action::NextTab => {
                self.next_tab();
                None
            }
            Action::PreviousTab => {
                self.previous_tab();
                None
            }

            // ── Data operations ──
            Action::OpenFilterPopup => {
                self.open_filter_popup();
                None
            }
            Action::ResetFilter => {
                if self.filter.is_none() {
                    self.status = "No filter to reset".to_string();
                    None
                } else {
                    self.reset_filter();
                    Some(DataCommand::LoadPage { offset: 0 })
                }
            }
            Action::CountFilter => self.current_file_path().map(DataCommand::CountFilter),
            Action::ExportPage => {
                self.export_current_page_csv();
                None
            }
            Action::SortByColumn => {
                self.sort_by_column(self.selected_col);
                None
            }
            Action::ToggleSortDirection => {
                self.toggle_sort_direction();
                None
            }

            // ── Copy (OSC 52) ──
            // In schema view, both y and Y copy the selected schema field.
            Action::CopyCell | Action::CopyRow if self.view == ViewMode::Schema => {
                if let Some((value, message)) = self.schema_field_clipboard_value() {
                    Some(DataCommand::CopyToClipboard { value, message })
                } else {
                    self.status = "No field selected".to_string();
                    None
                }
            }
            Action::CopyCell => {
                if let Some(value) = self.selected_cell_value().map(|s| s.to_string()) {
                    let row = self.selected_row + 1;
                    let col = self.selected_col + 1;
                    Some(DataCommand::CopyToClipboard {
                        value,
                        message: format!("Copied row {row}, column {col} to clipboard"),
                    })
                } else {
                    self.status = "No cell selected".to_string();
                    None
                }
            }
            Action::CopyRow => {
                if let Some(value) = self.selected_row_detail_json() {
                    let row = self.selected_row + 1;
                    let cols = self.columns.len();
                    Some(DataCommand::CopyToClipboard {
                        value,
                        message: format!("Copied row {row} ({cols} fields) to clipboard"),
                    })
                } else {
                    self.status = "No row selected".to_string();
                    None
                }
            }

            // ── Sidebar ──
            Action::SidebarSelectPrevious => {
                self.sidebar.select_previous();
                None
            }
            Action::SidebarSelectNext => {
                self.sidebar.select_next();
                None
            }
            Action::SidebarOpenSelected => match self.open_selected_sidebar_entry() {
                Ok(SidebarOpenResult::File(path)) => Some(DataCommand::LoadFile(path)),
                Ok(SidebarOpenResult::None) => None,
                Err(error) => {
                    self.set_error(error.to_string());
                    None
                }
            },
            Action::SidebarUnfocus => {
                self.sidebar.focused = false;
                None
            }

            // ── Cell-detail scrolling ──
            Action::DetailScrollUp => {
                self.cell_detail_scroll = self.cell_detail_scroll.saturating_sub(1);
                None
            }
            Action::DetailScrollDown => {
                self.cell_detail_scroll = self.cell_detail_scroll.saturating_add(1);
                None
            }
            Action::DetailScrollPageUp => {
                self.cell_detail_scroll = self.cell_detail_scroll.saturating_sub(8);
                None
            }
            Action::DetailScrollPageDown => {
                self.cell_detail_scroll = self.cell_detail_scroll.saturating_add(8);
                None
            }
            Action::DetailScrollHome => {
                self.cell_detail_scroll = 0;
                None
            }

            // ── Cell-detail search ──
            Action::StartDetailSearch => {
                self.start_detail_search();
                None
            }
            Action::DetailSearchCancel => {
                self.cancel_detail_search();
                None
            }
            Action::DetailSearchExecute => {
                self.execute_detail_search();
                None
            }
            Action::DetailSearchBackspace => {
                self.backspace_detail_search_char();
                None
            }
            Action::DetailSearchInsertChar(ch) => {
                self.insert_detail_search_char(*ch);
                None
            }
            Action::DetailSearchClearInput => {
                self.detail_search_input.clear();
                self.detail_search_cursor = 0;
                None
            }
            Action::NextDetailSearchMatch => {
                self.next_detail_search_match();
                None
            }
            Action::PreviousDetailSearchMatch => {
                self.previous_detail_search_match();
                None
            }

            // ── Filter popup ──
            Action::FilterCancel => {
                self.cancel_filter_popup();
                None
            }
            Action::FilterApply => {
                let previous_filter = self.filter.clone();
                let _ = self.set_filter_from_input();
                if let Some(path) = self.current_file_path() {
                    Some(DataCommand::ApplyFilterAndLoad {
                        path,
                        previous_filter,
                    })
                } else {
                    self.status = "No file opened".to_string();
                    None
                }
            }
            Action::FilterBackspace => {
                self.backspace_filter_char();
                None
            }
            Action::FilterDelete => {
                self.delete_filter_char();
                None
            }
            Action::FilterCursorLeft => {
                self.move_filter_cursor_left();
                None
            }
            Action::FilterCursorRight => {
                self.move_filter_cursor_right();
                None
            }
            Action::FilterCursorHome => {
                self.move_filter_cursor_home();
                None
            }
            Action::FilterCursorEnd => {
                self.move_filter_cursor_end();
                None
            }
            Action::FilterComplete => {
                self.complete_filter_field(false);
                None
            }
            Action::FilterCompleteReverse => {
                self.complete_filter_field(true);
                None
            }
            Action::FilterHistoryPrevious => {
                self.previous_filter_history();
                None
            }
            Action::FilterHistoryNext => {
                self.next_filter_history();
                None
            }
            Action::FilterInsertChar(ch) => {
                self.insert_filter_char(*ch);
                None
            }
        }
    }

    /// Expose the schema-field copy logic used in schema view.
    ///
    /// The pre-refactoring code had `copy_schema_field` in tui.rs that
    /// duplicated the OSC 52 logic.  We centralize it here so that
    /// `handle_action(CopyCell)` can reuse it in schema mode.
    pub fn schema_field_clipboard_value(&self) -> Option<(String, String)> {
        let column = self.columns.get(self.selected_schema_row)?;
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
        let message = format!("Copied field '{}' to clipboard", column.name);
        Some((value, message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::AppConfig;
    use crate::{
        data::{ColumnInfo, DataPage, RowView},
        formatting::CellView,
    };
    use std::path::PathBuf;

    fn test_state() -> AppState {
        let config = AppConfig {
            initial_file_path: None,
            page_size: 50,
            root_directory: std::env::temp_dir(),
            export_dir: std::env::temp_dir(),
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

    #[test]
    fn selected_row_detail_json_builds_object() {
        let mut state = test_state();
        state.active_file = Some(PathBuf::from("file.parquet"));
        set_columns(&mut state, &["id", "name"]);
        state.rows = vec![RowView {
            cells: vec![
                CellView::new("7".to_string()),
                CellView::new("\"alpha\"".to_string()),
            ],
        }];
        state.selected_row = 0;
        let json = state.selected_row_detail_json().unwrap();
        assert!(json.starts_with('{'));
        assert!(json.contains("\"id\""));
        assert!(json.contains("\"name\""));
        assert!(json.contains("alpha"));
    }

    #[test]
    fn schema_view_navigates_schema_rows_independently() {
        let mut state = test_state();
        state.active_file = Some(PathBuf::from("file.parquet"));
        set_columns(&mut state, &["a", "b", "c", "d", "e"]);
        state.rows = vec![single_row(); 3];
        state.selected_row = 2;
        state.toggle_schema_view();
        assert_eq!(state.view, ViewMode::Schema);

        // Schema navigation must not move the data-row selection.
        state.select_schema_row_next();
        assert_eq!(state.selected_schema_row, 1);
        assert_eq!(state.selected_row, 2);

        state.select_schema_row_bottom();
        assert_eq!(state.selected_schema_row, 4);
        assert_eq!(state.selected_row, 2);

        // Returning to Data view keeps the data-row selection intact.
        state.toggle_schema_view();
        assert_eq!(state.view, ViewMode::Data);
        assert_eq!(state.selected_row, 2);
    }

    #[test]
    fn schema_row_selection_stays_in_bounds() {
        let mut state = test_state();
        state.active_file = Some(PathBuf::from("file.parquet"));
        set_columns(&mut state, &["a", "b", "c", "d", "e"]);
        state.select_schema_row_bottom();
        assert_eq!(state.selected_schema_row, 4);
        state.select_schema_row_next();
        assert_eq!(state.selected_schema_row, 4);
        state.select_schema_row_top();
        assert_eq!(state.selected_schema_row, 0);
        state.select_schema_row_previous();
        assert_eq!(state.selected_schema_row, 0);
    }

    #[test]
    fn export_current_page_csv_writes_header_and_rows() {
        let mut state = test_state();
        state.active_file = Some(PathBuf::from("people.parquet"));
        set_columns(&mut state, &["id", "name"]);
        state.rows = vec![
            RowView {
                cells: vec![
                    CellView::new("1".to_string()),
                    CellView::new("a".to_string()),
                ],
            },
            RowView {
                cells: vec![
                    CellView::new("2".to_string()),
                    CellView::new("b,c".to_string()),
                ],
            },
        ];
        state.offset = 0;
        state.export_current_page_csv();
        let path = state.export_dir.join("people.page.csv");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("#,id,name"));
        assert!(content.contains("1,a"));
        assert!(content.contains("2,\"b,c\""));
        let _ = std::fs::remove_file(&path);
    }

    // E1: export respects custom export_dir
    #[test]
    fn export_uses_custom_export_dir() {
        let mut state = test_state();
        let custom_dir = std::env::temp_dir().join("parquet_reader_export_test");
        std::fs::create_dir_all(&custom_dir).unwrap();
        state.export_dir = custom_dir.clone();
        state.active_file = Some(PathBuf::from("custom.parquet"));
        set_columns(&mut state, &["id"]);
        state.rows = vec![RowView {
            cells: vec![CellView::new("1".to_string())],
        }];
        state.export_current_page_csv();
        let path = custom_dir.join("custom.page.csv");
        assert!(path.exists(), "export file should exist in custom dir");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("#,id"));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&custom_dir);
    }

    // E2: sort_display shows column name and direction
    #[test]
    fn sort_display_shows_column_and_direction() {
        let mut state = test_state();
        set_columns(&mut state, &["name", "score"]);
        assert_eq!(state.sort_display(), None);

        state.sort_column = Some(1);
        state.sort_ascending = true;
        assert_eq!(state.sort_display().as_deref(), Some("score (asc)"));

        state.sort_ascending = false;
        assert_eq!(state.sort_display().as_deref(), Some("score (desc)"));
    }

    // E2: status_text includes sort info when active
    #[test]
    fn status_text_includes_sort_when_active() {
        let mut state = test_state();
        state.active_file = Some(PathBuf::from("file.parquet"));
        set_columns(&mut state, &["name", "score"]);
        state.rows = vec![single_row()];
        state.sort_column = Some(1);
        state.sort_ascending = false;
        let text = state.status_text();
        assert!(
            text.contains("sort: score (desc)"),
            "status should contain sort info: {text}"
        );
    }

    // E2: total_rows_display shows readable messages
    #[test]
    fn total_rows_display_readable_states() {
        let mut state = test_state();
        state.count_state = CountState::Known(42);
        assert_eq!(state.total_rows_display(), "42");

        state.count_state = CountState::Unknown;
        state.total_rows = None;
        assert_eq!(state.total_rows_display(), "unknown");

        state.total_rows = Some(100);
        assert_eq!(state.total_rows_display(), "100");

        state.count_state = CountState::Failed("scan error".to_string());
        assert_eq!(state.total_rows_display(), "count failed");
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

    // R1: empty search produces no matches and clears query
    #[test]
    fn detail_search_empty_clears_query() {
        let mut state = test_state();
        state.active_file = Some(PathBuf::from("file.parquet"));
        set_columns(&mut state, &["data"]);
        state.rows = vec![RowView {
            cells: vec![CellView::new("hello world".to_string())],
        }];
        state.selected_row = 0;
        state.selected_col = 0;
        state.open_cell_detail();
        state.start_detail_search();
        state.execute_detail_search();
        assert!(state.detail_search_query.is_none());
        assert!(state.detail_search_matches.is_empty());
        assert!(state.detail_search_index.is_none());
        assert!(!state.detail_search_active);
    }

    // R1: single match is found and indexed
    #[test]
    fn detail_search_single_match() {
        let mut state = test_state();
        state.active_file = Some(PathBuf::from("file.parquet"));
        set_columns(&mut state, &["data"]);
        let value = r#"{"name":"alpha","count":42}"#;
        state.rows = vec![RowView {
            cells: vec![CellView::new(value.to_string())],
        }];
        state.selected_row = 0;
        state.selected_col = 0;
        state.open_cell_detail();
        state.start_detail_search();
        for ch in "alpha".chars() {
            state.insert_detail_search_char(ch);
        }
        state.execute_detail_search();
        assert_eq!(state.detail_search_query.as_deref(), Some("alpha"));
        assert_eq!(state.detail_search_matches.len(), 1);
        assert_eq!(state.detail_search_index, Some(0));
    }

    // R1: multiple matches can be navigated forward and backward
    #[test]
    fn detail_search_multiple_matches_navigate() {
        let mut state = test_state();
        state.active_file = Some(PathBuf::from("file.parquet"));
        set_columns(&mut state, &["data"]);
        let value = r#"{"a":"foo","b":"foo","c":"bar"}"#;
        state.rows = vec![RowView {
            cells: vec![CellView::new(value.to_string())],
        }];
        state.selected_row = 0;
        state.selected_col = 0;
        state.open_cell_detail();
        state.start_detail_search();
        for ch in "foo".chars() {
            state.insert_detail_search_char(ch);
        }
        state.execute_detail_search();
        assert_eq!(state.detail_search_matches.len(), 2);
        assert_eq!(state.detail_search_index, Some(0));

        state.next_detail_search_match();
        assert_eq!(state.detail_search_index, Some(1));

        state.next_detail_search_match();
        assert_eq!(state.detail_search_index, Some(0));

        state.previous_detail_search_match();
        assert_eq!(state.detail_search_index, Some(1));
    }

    // R1: no match reports status without crashing
    #[test]
    fn detail_search_no_match_reports_status() {
        let mut state = test_state();
        state.active_file = Some(PathBuf::from("file.parquet"));
        set_columns(&mut state, &["data"]);
        state.rows = vec![RowView {
            cells: vec![CellView::new("hello world".to_string())],
        }];
        state.selected_row = 0;
        state.selected_col = 0;
        state.open_cell_detail();
        state.start_detail_search();
        for ch in "xyz".chars() {
            state.insert_detail_search_char(ch);
        }
        state.execute_detail_search();
        assert!(state.detail_search_matches.is_empty());
        assert!(state.detail_search_index.is_none());
        assert!(state.status.contains("No matches"));
    }

    // R1: search state is reset when reopening cell detail
    #[test]
    fn detail_search_resets_on_reopen() {
        let mut state = test_state();
        state.active_file = Some(PathBuf::from("file.parquet"));
        set_columns(&mut state, &["data"]);
        state.rows = vec![RowView {
            cells: vec![CellView::new("hello world".to_string())],
        }];
        state.selected_row = 0;
        state.selected_col = 0;
        state.open_cell_detail();
        state.start_detail_search();
        for ch in "hello".chars() {
            state.insert_detail_search_char(ch);
        }
        state.execute_detail_search();
        assert!(state.detail_search_query.is_some());

        // Close and reopen
        state.show_cell_detail = false;
        state.reset_detail_search();
        state.open_cell_detail();
        assert!(state.detail_search_query.is_none());
        assert!(state.detail_search_matches.is_empty());
        assert!(!state.detail_search_active);
    }

    // R1: backspace in search input removes last char
    #[test]
    fn detail_search_backspace_removes_char() {
        let mut state = test_state();
        state.active_file = Some(PathBuf::from("file.parquet"));
        set_columns(&mut state, &["data"]);
        state.rows = vec![RowView {
            cells: vec![CellView::new("hello".to_string())],
        }];
        state.selected_row = 0;
        state.selected_col = 0;
        state.open_cell_detail();
        state.start_detail_search();
        state.insert_detail_search_char('a');
        state.insert_detail_search_char('b');
        state.backspace_detail_search_char();
        assert_eq!(state.detail_search_input, "a");
        assert_eq!(state.detail_search_cursor, 1);
    }

    // R1: scroll clamps to current match line
    #[test]
    fn detail_search_scroll_to_match() {
        let mut state = test_state();
        state.active_file = Some(PathBuf::from("file.parquet"));
        set_columns(&mut state, &["data"]);
        // Multi-line JSON: the match "target" appears on line 3
        let value = r#"{"a":1,"b":2,"c":"target","d":4}"#;
        state.rows = vec![RowView {
            cells: vec![CellView::new(value.to_string())],
        }];
        state.selected_row = 0;
        state.selected_col = 0;
        state.open_cell_detail();
        state.start_detail_search();
        for ch in "target".chars() {
            state.insert_detail_search_char(ch);
        }
        state.execute_detail_search();
        // After search, scroll should point to the line containing the match.
        // In pretty-printed JSON, "target" is on its own line.
        assert!(state.cell_detail_scroll > 0);
    }
}
