# parquet-reader

`parquet-reader` 是一个用 Rust 编写的终端 Parquet TUI Viewer，目标是在终端内查看 Parquet 文件内容。默认按需分页读取，不会全量加载文件。

## 特性

- 终端内浏览、分页查看 Parquet 文件。
- 左侧文件树侧边栏选择文件，支持目录展开/折叠、拖动侧边栏、鼠标操作。
- 顶部多 tab 栏，每个文件独立保存分页、筛选、滚动状态。
- Schema 视图与 Data 视图切换。
- 受限 DSL 筛选：`column op value`，支持 `and`/`or`、字段 Tab 补全、筛选历史、手动 count。
- 复杂类型（List / Struct / Map / Binary）表格摘要 + Cell Detail 详情。
- 面向终端用户的错误信息短、明确、可行动；空结果与失败路径不崩溃。
- vim/k9s 风格快捷键，常用操作全部可键盘完成。

## 快速开始

### 运行 Rust 版本

直接打开一个文件：

```bash
cargo run -- path/to/file.parquet
```

不带文件启动，进入 TUI 后通过左侧文件树选择 `.parquet` 文件：

```bash
cargo run
```

### 生成测试数据

```bash
cargo run --bin generate-test-parquet
cargo run --bin generate-empty-test-parquet
cargo run --bin generate-complex-test-parquet
cargo run --bin generate-pagination-test-parquet
```

对应输出文件（位于仓库 `parquet/`）：

```text
parquet/people.parquet
parquet/empty.parquet
parquet/complex_types.parquet
parquet/pagination.parquet
```

## 快捷键

| 快捷键 | 行为 |
|---|---|
| `q` / `Ctrl-C` | 退出 |
| `h` | 帮助弹窗 |
| `d` | 聚焦文件树 |
| `s` | Schema / Data 切换 |
| `/` | 打开筛选弹窗 |
| `r` | 重置筛选 |
| `c` | 统计当前筛选匹配行数 |
| `n` / `PageDown` | 下一页 |
| `p` / `PageUp` | 上一页 |
| `j` / `k` / `↑` / `↓` | 行移动 |
| `J` / `K` | 当前页底部 / 顶部 |
| `←` / `→` / `l` | 列移动 |
| `H` / `L` | 第一列 / 最后一列 |
| `Enter` / `Space` | Cell Detail |
| `Tab` / `Shift-Tab` | tab 切换；筛选弹窗内字段补全 / 反向补全 |
| `↑` / `↓`（筛选弹窗内） | 切换筛选历史 |
| `y` | OSC52 复制当前 cell |

## 鼠标

- 点击文件树展开目录 / 打开 parquet。
- 拖动侧边栏右边框调整宽度。
- 点击表格选中 cell，双击 cell 打开 detail。
- 滚轮上下移动行 / 翻页，横向滚轮移动列。
- 点击 tab 切换文件。

## 筛选语法

当前是受限 DSL，语法为：

```text
column op value
```

操作符：

```text
= != > >= < <= contains
```

示例：

```text
score > 80
city = 上海
note contains test
row_id >= 100
city = 上海 or city = 東京
score > 80 and active = true
```

说明：

- `contains` 大小写不敏感（`note contains TEST` 匹配 `test row`）。
- 数字比较优先按数字解析，否则按字符串比较。
- 字符串可以不加引号，也可以使用 `'` 或 `"` 包裹（如 `city = "New York"`）。
- 支持 `and` / `or` 组合多个条件。
- 不支持 SQL，不支持括号分组，不支持 `and`/`or` 混排优先级。
- **这不是安全沙箱**：筛选表达式会被直接用于读取逻辑，UI 与文档都明确说明它不是受信环境。

## 当前限制

- 分页 offset 当前顺序跳过已读取行，超大 offset 后续优化（row group aware pagination）。
- 筛选基于格式化后的 cell 文本，而非原始 Arrow 值。
- 筛选后的总行数暂未知，状态栏显示 `?`（按 `c` 可手动 count）。
- Schema 视图暂不支持滚动 / 排序。
- 暂无列排序、导出功能。

## 文档

- 设计方案与里程碑：`docs/design/`
- 使用说明：`docs/usage.md`
- 后续开发计划：`docs/next-development-plan.md`
- AI agent 协作规则：`AGENTS.md`

## 开发

常用验证命令：

```bash
cargo fmt
cargo check
cargo test
```

只修改文档时，不需要强行运行 Rust 编译或测试命令。
