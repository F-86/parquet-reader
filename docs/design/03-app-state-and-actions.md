# 03. 应用状态与动作模型

## 实现目标

本阶段实现不依赖真实终端和真实 Parquet 文件的状态机。完成后，分页、筛选、视图切换和导航边界可以通过单元测试验证。

## 状态转移模型

TUI 事件不直接改数据源。所有输入先转换为领域动作，再由状态机处理。

```text
KeyEvent -> Action -> AppState transition -> optional DataCommand -> render
```

## 核心状态

```rust
pub struct AppState {
    pub tabs: Vec<FileTab>,
    pub active_tab: Option<usize>,
    pub root_dir: PathBuf,
    pub sidebar: FileSidebar,
    pub view: ViewMode,
    pub offset: usize,
    pub page_size: usize,
    pub total_rows: CountState,
    pub schema: Vec<ColumnInfo>,
    pub filter: Option<String>,
    pub rows: Vec<RowView>,
    pub selected_row: usize,
    pub scroll_x: u16,
    pub status: StatusLine,
}

pub struct FileSidebar {
    pub current_dir: PathBuf,
    pub entries: Vec<FileEntry>,
    pub selected: usize,
    pub focused: bool,
}

pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub kind: FileEntryKind,
}

pub enum FileEntryKind {
    ParentDir,
    Directory,
    ParquetFile,
    OtherFile,
}

pub struct FileTab {
    pub file_path: PathBuf,
    pub title: String,
    pub offset: usize,
    pub filter: Option<String>,
    pub selected_row: usize,
    pub scroll_x: u16,
}

pub enum ViewMode {
    Empty,
    Data,
    Schema,
    SidebarFocused,
    FilterPopup,
    HelpPopup,
}

pub enum CountState {
    Unknown,
    Loading,
    Known(u64),
    Failed(String),
}
```

## 建议动作

| Action | 前置条件 | 状态变化 | 数据命令 |
|--------|----------|----------|----------|
| `NextPage` | 数据视图，未到末页 | `offset += page_size` | 加载新页 |
| `PrevPage` | 数据视图，`offset > 0` | `offset -= page_size` | 加载新页 |
| `ToggleSchema` | 已打开文件 | 切换 `view` | 无或加载 Schema |
| `FocusSidebar` | 任意状态 | 侧边栏获得焦点 | 无或 ListDirectory |
| `UnfocusSidebar` | 侧边栏聚焦 | 主区域获得焦点 | 无 |
| `SidebarUp` | 侧边栏聚焦 | 选中上一项 | 无 |
| `SidebarDown` | 侧边栏聚焦 | 选中下一项 | 无 |
| `OpenSidebarEntry` | 侧边栏聚焦 | 目录则进入；Parquet 文件则打开 | ListDirectory 或 LoadSchema + CountAndLoadFirstPage |
| `SelectFile(path)` | 侧边栏选中文件 | 第一阶段替换当前 tab 或创建第一个 tab，清空 filter/offset/rows/error | LoadSchema + CountAndLoadFirstPage |
| `OpenFilterPopup` | 已打开文件 | 打开筛选输入弹窗 | 无 |
| `CancelFilterPopup` | 筛选弹窗打开 | 关闭弹窗，恢复原视图 | 无 |
| `ApplyFilter(expr)` | 筛选弹窗提交 | 保存筛选，关闭弹窗，`offset = 0` | count + 加载第一页 |
| `ResetFilter` | 数据视图 | 清空筛选，`offset = 0` | count + 加载第一页 |
| `CursorDown` | 当前页有下一行 | `selected_row += 1` | 无 |
| `CursorUp` | 当前页有上一行 | `selected_row -= 1` | 无 |
| `ScrollRight` | 有水平可滚动区域 | `scroll_x` 增加 | 无 |
| `ScrollLeft` | `scroll_x > 0`，由 `←` 触发 | `scroll_x` 减少 | 无 |
| `ToggleHelp` | 任意状态 | 打开或关闭帮助弹窗 | 无 |
| `NextTab` | 多 tab 后续能力 | 切换 active_tab | 加载目标 tab 状态 |
| `PrevTab` | 多 tab 后续能力 | 切换 active_tab | 加载目标 tab 状态 |
| `CloseTab` | 多 tab 后续能力 | 关闭当前 tab | 可能加载相邻 tab |
| `Quit` | 任意状态 | 退出 | 无 |




## 筛选弹窗状态

筛选输入是覆盖式弹窗，不参与常规布局。

- `/` 打开筛选弹窗。
- 弹窗打开时保存进入弹窗前的视图和焦点。
- `Esc` 关闭弹窗，不改变当前筛选条件。
- `Enter` 提交表达式，关闭弹窗，重置 offset 并触发重新加载。
- 筛选错误写入状态栏或错误状态，不重新打开弹窗。

## 帮助弹窗状态

帮助弹窗是纯 UI 状态，不触发数据访问。

- `h` 打开帮助弹窗。
- 帮助弹窗打开后，`Esc` 或再次按 `h` 关闭弹窗。
- 弹窗关闭后恢复进入弹窗前的视图和焦点。
- 帮助内容由静态快捷键分组生成，不从数据源读取。

## 侧边栏状态模型

- `root_dir` 是程序启动时的当前工作目录。
- `sidebar.current_dir` 表示侧边栏当前浏览目录。
- 侧边栏可以进入子目录，但不能越过 `root_dir`。
- `entries` 保存目录项，第一阶段至少包含目录和 `.parquet` 文件。
- `focused` 表示键盘事件当前是否优先给侧边栏处理。
- 打开文件后，主区域显示数据视图，侧边栏仍保留当前目录和选中项。

## Tab 状态模型

第一阶段只需要单 tab，但状态模型预留多 tab 扩展空间。

- `tabs` 保存已打开文件列表。
- `active_tab` 指向当前显示的 tab；未打开文件时为 `None`。
- 每个 tab 应保存该文件自己的分页、筛选、选中行和水平滚动状态。
- 第一阶段可以只允许 0 或 1 个 tab。
- 后续支持多个 tab 时，切换 tab 不应丢失各自的分页和筛选状态。

## 数据命令

状态机可以返回数据命令，让外层协调 Data Access：

```rust
pub enum DataCommand {
    ListDirectory { path: PathBuf },
    OpenFile { path: PathBuf },
    LoadSchema,
    LoadPage { offset: usize, limit: usize, filter: Option<String> },
    Count { filter: Option<String> },
    CountAndLoadFirstPage { filter: Option<String> },
}
```

## 状态栏

状态栏应由 `AppState` 派生，避免 TUI 层拼接业务状态。

状态栏包含：

- 顶部 tab 栏显示当前文件名；未打开文件时显示空状态提示。
- 当前视图。
- 行范围和总数。
- 页码。
- 列数。
- 当前筛选条件；筛选过长时使用统一截断策略。
- 最近错误。

## 单元测试重点

- 第一页不能再上一页。
- 最后一页不能越界。
- 筛选弹窗提交后 offset 归零。
- 筛选成功后 status 栏包含当前 filter 文本。
- 重置筛选后 status 栏不再显示旧 filter。
- 筛选弹窗按 `Esc` 取消时不改变 filter 和 offset。
- 重置筛选后 offset 归零。
- Schema/Data 互切不破坏筛选条件。
- 空状态按 `d` 后聚焦左侧文件列表。
- 按 `h` 打开帮助弹窗，不改变当前数据、tab 或侧边栏状态。
- 帮助弹窗打开后按 `Esc` 或 `h` 可以关闭并恢复原焦点。
- 侧边栏取消焦点后恢复主区域快捷键。
- 选择新文件后清空旧 filter、offset、rows 和错误状态。
- 打开文件后 tab 栏显示当前文件 basename。
- 未打开文件时 tab 栏显示 `[No file]` 或等价提示。
- 当前页行导航不会越界。
- 只有需要读数据的动作返回 `DataCommand`。

## 验收标准

- 状态机单元测试不需要真实终端。
- 状态机单元测试不需要真实 Parquet 文件。
- 所有核心动作都有边界测试。
- 文件侧边栏相关动作不需要真实终端即可测试。
- 侧边栏不能越过 root_dir 的边界测试必须覆盖。

## 下一步

继续实现 [`04-data-access.md`](./04-data-access.md)。
