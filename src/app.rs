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
        self.show_filter_popup = true;
    }

    pub fn cancel_filter_popup(&mut self) {
        self.show_filter_popup = false;
        self.filter_input.clear();
    }

    pub fn set_filter_from_input(&mut self) -> Option<String> {
        let filter = self.filter_input.trim().to_string();
        self.filter = if filter.is_empty() {
            None
        } else {
            Some(filter.clone())
        };
        self.offset = 0;
        self.selected_row = 0;
        self.show_filter_popup = false;
        self.filter_input.clear();
        self.filter.clone()
    }

    pub fn reset_filter(&mut self) {
        self.filter = None;
        self.offset = 0;
        self.selected_row = 0;
        self.show_filter_popup = false;
        self.filter_input.clear();
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
