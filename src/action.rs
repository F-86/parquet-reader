use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Represents which input context the user is currently in.  The TUI layer
/// derives this from `AppState` and passes it to [`key_to_action`] so that the
/// key-mapping function remains a pure, easily-testable transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Cell-detail popup with the search input actively accepting characters.
    DetailSearch,
    /// Cell-detail popup (not in search-input mode).
    CellDetail,
    /// Help overlay is open.
    Help,
    /// Filter-expression input popup is open.
    FilterPopup,
    /// Schema view is active (takes priority over sidebar focus, matching
    /// the pre-refactoring `handle_key` ordering).
    SchemaView,
    /// File sidebar has keyboard focus.
    SidebarFocused,
    /// Normal data-table view.
    Data,
}

/// A user intention, decoupled from the specific key event that triggered it.
///
/// The TUI layer maps keyboard events to `Action`s; `AppState::handle_action`
/// processes each `Action` and optionally returns a [`DataCommand`] when data
/// access or I/O is required.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    // ── Global ──
    Quit,
    ToggleHelp,

    // ── Navigation: data view ──
    SelectRowPrevious,
    SelectRowNext,
    SelectRowTop,
    SelectRowBottom,
    SelectColPrevious,
    SelectColNext,
    SelectFirstCol,
    SelectLastCol,
    NextPage,
    PreviousPage,

    // ── Navigation: schema view ──
    SelectSchemaRowPrevious,
    SelectSchemaRowNext,
    SelectSchemaRowTop,
    SelectSchemaRowBottom,

    // ── View-mode switches ──
    ToggleSchemaView,
    FocusSidebar,
    OpenCellDetail,
    CloseCellDetail,

    // ── Tabs ──
    NextTab,
    PreviousTab,

    // ── Data operations ──
    OpenFilterPopup,
    ResetFilter,
    CountFilter,
    ExportPage,
    SortByColumn,
    ToggleSortDirection,

    // ── Copy (OSC 52) ──
    CopyCell,
    CopyRow,

    // ── Sidebar ──
    SidebarSelectPrevious,
    SidebarSelectNext,
    SidebarOpenSelected,
    SidebarUnfocus,

    // ── Cell-detail scrolling ──
    DetailScrollUp,
    DetailScrollDown,
    DetailScrollPageUp,
    DetailScrollPageDown,
    DetailScrollHome,

    // ── Cell-detail search ──
    StartDetailSearch,
    DetailSearchCancel,
    DetailSearchExecute,
    DetailSearchBackspace,
    DetailSearchInsertChar(char),
    DetailSearchClearInput,
    NextDetailSearchMatch,
    PreviousDetailSearchMatch,

    // ── Filter popup ──
    FilterCancel,
    FilterApply,
    FilterBackspace,
    FilterDelete,
    FilterCursorLeft,
    FilterCursorRight,
    FilterCursorHome,
    FilterCursorEnd,
    FilterComplete,
    FilterCompleteReverse,
    FilterHistoryPrevious,
    FilterHistoryNext,
    FilterInsertChar(char),
}

/// A command for the data-access / I/O layer, returned by
/// `AppState::handle_action` when an action cannot be fulfilled by a pure
/// state change alone.  The TUI run-loop executes the command and writes
/// results back into `AppState`.
#[derive(Debug)]
pub enum DataCommand {
    /// Open (or switch to) a Parquet file.
    LoadFile(std::path::PathBuf),
    /// Read a single page at `offset`.
    LoadPage { offset: usize },
    /// Apply the filter expression just entered in the popup and reload page 0.
    /// `previous_filter` is carried so the executor can roll back on error.
    ApplyFilterAndLoad {
        path: std::path::PathBuf,
        previous_filter: Option<String>,
    },
    /// Count rows matching the active filter.
    CountFilter(std::path::PathBuf),
    /// Write a value to the clipboard via OSC 52.
    CopyToClipboard { value: String, message: String },
}

/// Map a keyboard event to an [`Action`], given the current [`InputMode`].
///
/// This is a **pure function** — it reads no mutable state and performs no
/// side effects, making it trivially testable.
///
/// `Ctrl+C` always maps to [`Action::Quit`] regardless of mode.
pub fn key_to_action(key: KeyEvent, mode: InputMode) -> Option<Action> {
    // Ctrl+C is a hard quit in every mode.
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
        return Some(Action::Quit);
    }

    match mode {
        InputMode::DetailSearch => key_to_action_detail_search(key),
        InputMode::CellDetail => key_to_action_cell_detail(key),
        InputMode::Help => key_to_action_help(key),
        InputMode::FilterPopup => key_to_action_filter_popup(key),
        InputMode::SchemaView => key_to_action_schema(key),
        InputMode::SidebarFocused => key_to_action_sidebar(key),
        InputMode::Data => key_to_action_data(key),
    }
}

// ── Per-mode key maps ──────────────────────────────────────────────────

fn key_to_action_detail_search(key: KeyEvent) -> Option<Action> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('u') => Some(Action::DetailSearchClearInput),
            _ => None,
        };
    }
    match key.code {
        KeyCode::Esc => Some(Action::DetailSearchCancel),
        KeyCode::Enter => Some(Action::DetailSearchExecute),
        KeyCode::Backspace => Some(Action::DetailSearchBackspace),
        KeyCode::Char(ch) => Some(Action::DetailSearchInsertChar(ch)),
        _ => None,
    }
}

fn key_to_action_cell_detail(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::Char(' ') => Some(Action::CloseCellDetail),
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char('/') => Some(Action::StartDetailSearch),
        KeyCode::Char('n') => Some(Action::NextDetailSearchMatch),
        KeyCode::Char('N') => Some(Action::PreviousDetailSearchMatch),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::DetailScrollUp),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::DetailScrollDown),
        KeyCode::PageUp => Some(Action::DetailScrollPageUp),
        KeyCode::PageDown => Some(Action::DetailScrollPageDown),
        KeyCode::Home => Some(Action::DetailScrollHome),
        KeyCode::Char('y') => Some(Action::CopyCell),
        KeyCode::Char('Y') => Some(Action::CopyRow),
        _ => None,
    }
}

fn key_to_action_help(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('h') => Some(Action::ToggleHelp),
        KeyCode::Char('q') => Some(Action::Quit),
        _ => None,
    }
}

fn key_to_action_filter_popup(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Esc => Some(Action::FilterCancel),
        KeyCode::Enter => Some(Action::FilterApply),
        KeyCode::Backspace => Some(Action::FilterBackspace),
        KeyCode::Delete => Some(Action::FilterDelete),
        KeyCode::Left => Some(Action::FilterCursorLeft),
        KeyCode::Right => Some(Action::FilterCursorRight),
        KeyCode::Home => Some(Action::FilterCursorHome),
        KeyCode::End => Some(Action::FilterCursorEnd),
        KeyCode::Tab => Some(Action::FilterComplete),
        KeyCode::BackTab => Some(Action::FilterCompleteReverse),
        KeyCode::Up => Some(Action::FilterHistoryPrevious),
        KeyCode::Down => Some(Action::FilterHistoryNext),
        KeyCode::Char(ch) => Some(Action::FilterInsertChar(ch)),
        _ => None,
    }
}

fn key_to_action_schema(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char('h') => Some(Action::ToggleHelp),
        KeyCode::Char('s') => Some(Action::ToggleSchemaView),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::SelectSchemaRowPrevious),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::SelectSchemaRowNext),
        KeyCode::Char('K') => Some(Action::SelectSchemaRowTop),
        KeyCode::Char('J') => Some(Action::SelectSchemaRowBottom),
        KeyCode::Char('y') => Some(Action::CopyCell),
        KeyCode::Char('Y') => Some(Action::CopyRow),
        _ => None,
    }
}

fn key_to_action_sidebar(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Esc => Some(Action::SidebarUnfocus),
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char('h') => Some(Action::ToggleHelp),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::SidebarSelectPrevious),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::SidebarSelectNext),
        KeyCode::Enter => Some(Action::SidebarOpenSelected),
        _ => None,
    }
}

fn key_to_action_data(key: KeyEvent) -> Option<Action> {
    match key.code {
        KeyCode::Char('q') => Some(Action::Quit),
        KeyCode::Char('h') => Some(Action::ToggleHelp),
        KeyCode::Char('d') => Some(Action::FocusSidebar),
        KeyCode::Char('s') => Some(Action::ToggleSchemaView),
        KeyCode::Char('/') => Some(Action::OpenFilterPopup),
        KeyCode::Char('r') => Some(Action::ResetFilter),
        KeyCode::Char('c') => Some(Action::CountFilter),
        KeyCode::Char('e') => Some(Action::ExportPage),
        KeyCode::Char('o') => Some(Action::SortByColumn),
        KeyCode::Char('O') => Some(Action::ToggleSortDirection),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::SelectRowPrevious),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::SelectRowNext),
        KeyCode::Char('K') => Some(Action::SelectRowTop),
        KeyCode::Char('J') => Some(Action::SelectRowBottom),
        // ← is the only left-scroll key; `h` stays bound to Help.
        KeyCode::Left => Some(Action::SelectColPrevious),
        KeyCode::Right | KeyCode::Char('l') => Some(Action::SelectColNext),
        KeyCode::Char('H') => Some(Action::SelectFirstCol),
        KeyCode::Char('L') => Some(Action::SelectLastCol),
        KeyCode::Tab => Some(Action::NextTab),
        KeyCode::BackTab => Some(Action::PreviousTab),
        KeyCode::Char('n') | KeyCode::PageDown => Some(Action::NextPage),
        KeyCode::Char('p') | KeyCode::PageUp => Some(Action::PreviousPage),
        KeyCode::Enter | KeyCode::Char(' ') => Some(Action::OpenCellDetail),
        KeyCode::Char('y') => Some(Action::CopyCell),
        KeyCode::Char('Y') => Some(Action::CopyRow),
        _ => None,
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn shift_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::SHIFT)
    }

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    // ── Ctrl+C quits in every mode ──

    #[test]
    fn ctrl_c_quits_in_data_mode() {
        assert_eq!(
            key_to_action(ctrl(KeyCode::Char('c')), InputMode::Data),
            Some(Action::Quit)
        );
    }

    #[test]
    fn ctrl_c_quits_in_filter_popup() {
        assert_eq!(
            key_to_action(ctrl(KeyCode::Char('c')), InputMode::FilterPopup),
            Some(Action::Quit)
        );
    }

    #[test]
    fn ctrl_c_quits_in_detail_search() {
        assert_eq!(
            key_to_action(ctrl(KeyCode::Char('c')), InputMode::DetailSearch),
            Some(Action::Quit)
        );
    }

    // ── Data mode ──

    #[test]
    fn data_q_quits() {
        assert_eq!(
            key_to_action(key(KeyCode::Char('q')), InputMode::Data),
            Some(Action::Quit)
        );
    }

    #[test]
    fn data_h_is_help_not_left() {
        // `h` must toggle help, NOT move left (AGENTS.md rule).
        assert_eq!(
            key_to_action(key(KeyCode::Char('h')), InputMode::Data),
            Some(Action::ToggleHelp)
        );
    }

    #[test]
    fn data_left_arrow_is_col_previous() {
        assert_eq!(
            key_to_action(key(KeyCode::Left), InputMode::Data),
            Some(Action::SelectColPrevious)
        );
    }

    #[test]
    fn data_j_moves_row_down() {
        assert_eq!(
            key_to_action(key(KeyCode::Char('j')), InputMode::Data),
            Some(Action::SelectRowNext)
        );
    }

    #[test]
    fn data_n_is_next_page() {
        assert_eq!(
            key_to_action(key(KeyCode::Char('n')), InputMode::Data),
            Some(Action::NextPage)
        );
    }

    #[test]
    fn data_pagedown_is_next_page() {
        assert_eq!(
            key_to_action(key(KeyCode::PageDown), InputMode::Data),
            Some(Action::NextPage)
        );
    }

    #[test]
    fn data_o_sorts_by_column() {
        assert_eq!(
            key_to_action(key(KeyCode::Char('o')), InputMode::Data),
            Some(Action::SortByColumn)
        );
    }

    #[test]
    fn data_shift_o_toggles_sort_direction() {
        assert_eq!(
            key_to_action(shift_key(KeyCode::Char('O')), InputMode::Data),
            Some(Action::ToggleSortDirection)
        );
    }

    #[test]
    fn data_e_exports_page() {
        assert_eq!(
            key_to_action(key(KeyCode::Char('e')), InputMode::Data),
            Some(Action::ExportPage)
        );
    }

    #[test]
    fn data_unhandled_key_returns_none() {
        assert_eq!(key_to_action(key(KeyCode::Insert), InputMode::Data), None);
    }

    // ── Cell detail mode ──

    #[test]
    fn cell_detail_esc_closes() {
        assert_eq!(
            key_to_action(key(KeyCode::Esc), InputMode::CellDetail),
            Some(Action::CloseCellDetail)
        );
    }

    #[test]
    fn cell_detail_j_scrolls_down() {
        // In cell detail, `j` scrolls — it does NOT move the data row.
        assert_eq!(
            key_to_action(key(KeyCode::Char('j')), InputMode::CellDetail),
            Some(Action::DetailScrollDown)
        );
    }

    #[test]
    fn cell_detail_slash_starts_search() {
        assert_eq!(
            key_to_action(key(KeyCode::Char('/')), InputMode::CellDetail),
            Some(Action::StartDetailSearch)
        );
    }

    // ── Detail search input mode ──

    #[test]
    fn detail_search_ctrl_u_clears_input() {
        assert_eq!(
            key_to_action(ctrl(KeyCode::Char('u')), InputMode::DetailSearch),
            Some(Action::DetailSearchClearInput)
        );
    }

    #[test]
    fn detail_search_char_inserts() {
        assert_eq!(
            key_to_action(key(KeyCode::Char('x')), InputMode::DetailSearch),
            Some(Action::DetailSearchInsertChar('x'))
        );
    }

    // ── Filter popup mode ──

    #[test]
    fn filter_popup_tab_completes() {
        assert_eq!(
            key_to_action(key(KeyCode::Tab), InputMode::FilterPopup),
            Some(Action::FilterComplete)
        );
    }

    #[test]
    fn filter_popup_backtab_completes_reverse() {
        assert_eq!(
            key_to_action(key(KeyCode::BackTab), InputMode::FilterPopup),
            Some(Action::FilterCompleteReverse)
        );
    }

    // ── Schema mode ──

    #[test]
    fn schema_esc_quits() {
        // Preserves pre-refactoring behaviour: Esc quits in schema view.
        assert_eq!(
            key_to_action(key(KeyCode::Esc), InputMode::SchemaView),
            Some(Action::Quit)
        );
    }

    #[test]
    fn schema_j_moves_schema_row() {
        assert_eq!(
            key_to_action(key(KeyCode::Char('j')), InputMode::SchemaView),
            Some(Action::SelectSchemaRowNext)
        );
    }

    // ── Sidebar mode ──

    #[test]
    fn sidebar_esc_unfocuses() {
        assert_eq!(
            key_to_action(key(KeyCode::Esc), InputMode::SidebarFocused),
            Some(Action::SidebarUnfocus)
        );
    }

    #[test]
    fn sidebar_enter_opens_selected() {
        assert_eq!(
            key_to_action(key(KeyCode::Enter), InputMode::SidebarFocused),
            Some(Action::SidebarOpenSelected)
        );
    }

    // ── Help mode ──

    #[test]
    fn help_h_closes_help() {
        assert_eq!(
            key_to_action(key(KeyCode::Char('h')), InputMode::Help),
            Some(Action::ToggleHelp)
        );
    }
}
