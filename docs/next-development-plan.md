# parquet-reader 后续开发计划

本文档面向接手实现的 AI coding 模型，记录当前项目状态、后续阶段设计、建议实现顺序和验收命令。

## 当前状态

项目已经完成到 M3 的可用版本。

已完成能力：

- M1：最小可启动 TUI viewer
  - TUI 启动和终端恢复
  - 左侧文件树
  - 鼠标支持
  - 可拖动侧边栏
  - 多 tab
  - Parquet 首屏读取
  - Cell Detail
  - 复杂类型 pretty print
  - 测试数据生成 bin
  - Cargo 默认运行主程序
- M2：导航与分页
  - 行导航
  - 列导航
  - 水平滚动
  - 上一页 / 下一页
  - 分页状态栏
  - 分页测试数据生成器
  - 鼠标滚轮上下翻行 / 翻页
- M3：Schema 与基础筛选
  - Schema 视图
  - 筛选弹窗
  - 筛选输入光标编辑
  - 字段 Tab 补全
  - 筛选重置
  - 筛选错误恢复
  - 每个 tab 独立筛选条件
  - 状态栏 filter 高亮
  - 筛选后的分页 offset 语义修复

当前建议后续阶段：

```text
P1：补测试，锁定现有行为
P2：文档同步
P3：M3 筛选增强
P4：M4 健壮性和值显示体验
P5：性能优化与架构清理
```

推荐优先级：

```text
P1 > P2 > P3 > P4 > P5
```

原因：当前功能增长较快，但测试较少。后续继续加筛选、复杂类型、分页优化时，容易破坏已有行为。先补测试可以降低回归风险。

---

## P1：补测试，锁定现有行为

### 目标

为当前已经实现的核心行为补单元测试，尤其是：

- AppState 状态转移
- filter input 光标编辑
- tab filter 隔离
- 分页边界
- Schema/Data 切换
- filter parse 和匹配
- 文件树 root 边界

### P1.1 AppState 测试模块

建议先在 `src/app.rs` 末尾加：

```rust
#[cfg(test)]
mod tests {
    // ...
}
```

后续如果测试变多，再拆成：

```text
src/app/tests.rs
```

或：

```text
tests/app_state.rs
```

当前先放在模块内最简单。

### P1.2 filter input 光标编辑测试

当前相关方法：

```rust
insert_filter_char
backspace_filter_char
delete_filter_char
move_filter_cursor_left
move_filter_cursor_right
move_filter_cursor_home
move_filter_cursor_end
complete_filter_field
```

需要测试：

#### 插入字符在光标位置

初始：

```text
filter_input = "city  上海"
filter_cursor = 5
```

插入：

```text
'='
```

期望：

```text
filter_input = "city = 上海"
filter_cursor 在 '=' 后
```

#### Backspace 删除光标前字符

输入：

```text
city = 上海
```

光标在末尾。

执行：

```rust
backspace_filter_char()
```

期望删除 `海`，不能破坏 UTF-8。

#### Delete 删除光标处字符

输入：

```text
city = 上海
```

光标在 `上` 前。

执行：

```rust
delete_filter_char()
```

期望：

```text
city = 海
```

#### 左右移动光标按字符移动，不按字节移动

输入：

```text
a你b
```

从末尾连续左移，光标位置应落在合法 UTF-8 边界：

```text
a你|b
a|你b
|a你b
```

不能 panic。

#### Home/End

输入：

```text
score > 80
```

- Home 后 cursor = 0
- End 后 cursor = len

#### 空字符串边界

对空输入执行：

```rust
backspace_filter_char()
delete_filter_char()
move_filter_cursor_left()
move_filter_cursor_right()
```

都不能 panic，cursor 应保持 0。

### P1.3 字段 Tab 补全测试

准备一个带 columns 的 AppState，例如：

```rust
columns = vec![
    ColumnInfo { name: "city" },
    ColumnInfo { name: "score" },
    ColumnInfo { name: "status" },
]
```

测试点：

#### 单候选补全

输入：

```text
ci
```

光标末尾，执行：

```rust
complete_filter_field(false)
```

期望：

```text
city
```

#### 多候选循环

字段：

```text
score
status
```

输入：

```text
s
```

第一次 Tab 可能补全为：

```text
score
```

第二次 Tab 应循环到：

```text
status
```

再次回到：

```text
score
```

顺序按当前实现排序后的结果。

#### Shift-Tab 反向循环

输入当前 token：

```text
score
```

执行：

```rust
complete_filter_field(true)
```

期望切到上一个候选。

#### 只补全当前 token

输入：

```text
city = s
```

光标在 `s` 后。

执行 Tab。

期望只替换 `s`，不要影响：

```text
city =
```

#### 无匹配

输入：

```text
zzz
```

执行 Tab。

期望输入不变，status 显示：

```text
No column matches 'zzz'
```

### P1.4 tab filter 隔离测试

当前行为目标：每个 tab 独立保存：

```rust
filter
offset
rows
columns
selected_row
selected_col
scroll_x
```

测试流程：

1. 创建 app。
2. 构造两个 DataPage：file A、file B。
3. `apply_page(file_a, page_a)`。
4. 设置：

   ```rust
   app.filter = Some("score > 80".to_string())
   ```

5. `apply_page(file_b, page_b)`。
6. 设置：

   ```rust
   app.filter = Some("city = 上海".to_string())
   ```

7. `switch_to_tab(0)`。
8. 期望：

   ```rust
   app.filter == Some("score > 80")
   ```

9. `switch_to_tab(1)`。
10. 期望：

    ```rust
    app.filter == Some("city = 上海")
    ```

注意：当前 `apply_page` 新打开文件时会清空 filter，这是合理的。测试时给 tab 设置 filter 后要触发 `save_active_tab_state()`，可以通过 `switch_to_tab` 间接触发。

### P1.5 分页边界测试

#### 第一页不能上一页

```rust
offset = 0
previous_page_offset() == None
```

#### 下一页 offset

```rust
offset = 0
page_size = 50
total_rows = Some(120)
next_page_offset() == Some(50)
```

#### 最后一页不能下一页

```rust
offset = 100
page_size = 50
total_rows = Some(120)
next_page_offset() == None
```

#### 未知 total rows 下允许下一页

筛选后：

```rust
total_rows = None
rows 非空
```

期望：

```rust
next_page_offset() == Some(offset + page_size)
```

#### 空 rows 不能下一页

```rust
rows = []
next_page_offset() == None
```

### P1.6 Schema/Data 切换测试

目标：Schema/Data 切换不破坏：

- filter
- offset
- selected_row
- selected_col
- rows
- columns

测试流程：

1. 打开一个 page。
2. 设置：

   ```rust
   offset = 50
   selected_row = 3
   selected_col = 2
   filter = Some("score > 80")
   ```

3. 执行：

   ```rust
   toggle_schema_view()
   ```

4. 期望：

   ```rust
   view == Schema
   ```

5. 再执行：

   ```rust
   toggle_schema_view()
   ```

6. 期望：

   ```rust
   view == Data
   ```

7. 状态仍保持。

### P1.7 filter parse 与匹配测试

当前 filter parse 在 `src/data.rs` 内部，相关函数是私有的：

```rust
parse_filter
split_filter
unquote_filter_value
FilterExpr::matches
```

推荐先在 `src/data.rs` 内部加测试模块，直接测私有函数。

测试点：

- 支持操作符：`=`、`!=`、`>`、`>=`、`<`、`<=`、`contains`
- unknown column 返回 `AppError::InvalidFilter`
- 非法表达式返回错误，例如：`score`、`score >`、`= 1`
- 字符串引号剥离：`city = "上海"`、`city = '上海'`
- 数字比较：cell detail `98.5` 匹配 `score > 80`
- `contains` 大小写不敏感：`note contains TEST` 匹配 `test row`

### P1.8 文件树 root 边界测试

当前文件树逻辑在：

```text
src/file_browser.rs
```

测试点：不能越过 root。

构造临时目录：

```text
/tmp/root
/tmp/root/sub
```

进入 `sub` 后，尝试进入 root 之外路径应返回：

```rust
AppError::OutsideRoot
```

需要临时目录。可以引入 dev-dependency：

```toml
[dev-dependencies]
tempfile = "3"
```

如果不想新增依赖，也可以用 `std::env::temp_dir()` 手动创建唯一目录，但 `tempfile` 更稳。

---

## P2：文档同步

### 目标

当前 README 仍然偏早期阶段，应更新为当前可用功能说明。

### P2.1 README 更新

需要包含：

#### 项目简介

说明这是 Rust Parquet TUI Viewer。

#### 快速运行

```bash
cargo run -- path/to/file.parquet
```

不带文件：

```bash
cargo run
```

#### 生成测试数据

```bash
cargo run --bin generate-test-parquet
cargo run --bin generate-empty-test-parquet
cargo run --bin generate-complex-test-parquet
cargo run --bin generate-pagination-test-parquet
```

对应文件：

```text
parquet/people.parquet
parquet/empty.parquet
parquet/complex_types.parquet
parquet/pagination.parquet
```

#### 快捷键

| 快捷键 | 行为 |
|---|---|
| `q` | 退出 |
| `h` | 帮助 |
| `d` | 聚焦文件树 |
| `s` | Schema/Data 切换 |
| `/` | 筛选 |
| `r` | 重置筛选 |
| `n` / `PageDown` | 下一页 |
| `p` / `PageUp` | 上一页 |
| `j` / `k` / `↑` / `↓` | 行移动 |
| `J` / `K` | 当前页底部 / 顶部 |
| `←` / `→` / `l` | 列移动 |
| `H` / `L` | 第一列 / 最后一列 |
| `Enter` / `Space` | Cell Detail |
| `Tab` / `Shift-Tab` | tab 切换；筛选弹窗内字段补全 |
| `y` | OSC52 复制当前 cell |

#### 鼠标

说明：

- 点击文件树展开目录 / 打开 parquet
- 拖动侧边栏右边框调整宽度
- 点击表格选中 cell
- 双击 cell 打开 detail
- 滚轮上下移动行 / 翻页
- 横向滚轮移动列
- 点击 tab 切换文件

#### 筛选语法

当前是受限语法：

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
```

说明：

- `contains` 大小写不敏感
- 数字比较优先按数字解析
- 字符串可以不加引号，也可以使用 `'` 或 `"`
- 不支持 SQL
- 不支持 and/or
- 不是安全沙箱
- 筛选后总数目前显示 `?`

#### 当前限制

- 分页 offset 当前顺序跳过，超大 offset 后续优化
- 筛选基于格式化后的 cell 文本
- 筛选后的 count 暂未知
- Schema 视图暂不支持滚动 / 排序
- 暂无列排序
- 暂无导出

### P2.2 docs/design/09 更新

更新：

```text
docs/design/09-milestones-and-decisions.md
```

标记：

- M1 已实现
- M2 已实现
- M3 第一版已实现

并记录当前筛选决策：

- 使用受限内置 filter 语法
- 暂未使用 DataFusion
- filter count 暂未知
- 后续再考虑 DataFusion / DSL

### P2.3 新增 docs/usage.md

如果 README 不想太长，可以新增：

```text
docs/usage.md
```

内容包括：

- 启动方式
- 快捷键
- 筛选语法
- 测试数据
- 限制

README 中链接过去。

---

## P3：筛选增强

### P3.1 支持 and/or

目标语法：

```text
score > 80 and active = true
city = 上海 or city = 東京
```

推荐实现：不要直接上 parser generator。可以做简单 AST：

```rust
enum FilterAst {
    Predicate(FilterExpr),
    And(Box<FilterAst>, Box<FilterAst>),
    Or(Box<FilterAst>, Box<FilterAst>),
}
```

解析策略：

1. 按不在引号内的 ` or ` 分割。
2. 每段按不在引号内的 ` and ` 分割。
3. 每个 leaf 用现有 `parse_filter`。

注意：需要处理引号内的字符串：

```text
note contains "A and B"
```

不能被拆开。

### P3.2 筛选补全候选弹窗

当前行为：Tab 补全字段名，但没有可视候选列表。

目标：筛选弹窗中显示候选列表：

```text
> ci

columns:
  city
```

多候选：

```text
columns:
  score
  status
```

AppState 新增：

```rust
filter_completion_candidates: Vec<String>
filter_completion_index: usize
```

行为：

- 输入变化后清空 candidates
- 按 Tab 生成 candidates
- 多次 Tab 切换 index
- 绘制 popup 时展示 candidates

### P3.3 筛选历史

目标：在筛选弹窗中支持：

```text
↑ / ↓
```

切换历史筛选条件。

AppState 新增：

```rust
filter_history: Vec<String>
filter_history_index: Option<usize>
```

行为：

- Apply 成功后加入 history
- 重复 filter 不重复加入
- ↑ 上一条
- ↓ 下一条
- Esc 不改变 history

### P3.4 筛选 count

当前筛选后：

```text
total_rows = None
```

状态栏显示：

```text
?
```

推荐先做手动 count。

新增快捷键：

```text
c
```

表示 count current filter。

Data Access 新增：

```rust
count_with_filter(filter: Option<&str>) -> Result<usize>
```

实现：

- 无 filter：metadata num_rows
- 有 filter：扫描匹配数
- 后续优化

长期建议将 `Option<usize>` 改成：

```rust
CountState {
    Unknown,
    Known(usize),
    Failed(String),
}
```

---

## P4：M4 健壮性和值显示体验

### P4.1 Formatting 层拆分

当前大量格式化逻辑在：

```text
src/data.rs
```

建议拆到：

```text
src/formatting.rs
```

当前 `formatting.rs` 只有：

```rust
truncate_to_width
```

建议迁移：

- `CellView`
- `format_cell`
- `format_scalar`
- `format_list_*`
- `format_struct_*`
- `format_map_*`
- `binary_detail`
- JSON pretty 相关

建议边界：

- `src/data.rs`：ParquetFileDataSource、read_page、schema extraction、RecordBatch -> RowView 调用 formatting
- `src/formatting.rs`：Arrow value -> CellView、truncation、detail formatting
- `src/tui.rs`：只使用 CellView display/detail，不感知 Arrow 类型

### P4.2 更多 Arrow 类型支持

当前重点支持：

- Bool
- Int
- Float
- Utf8
- Binary
- List
- Struct
- Map

需要补：

#### Date / Time / Timestamp

类型：

```rust
DataType::Date32
DataType::Date64
DataType::Time32(_)
DataType::Time64(_)
DataType::Timestamp(unit, timezone)
DataType::Duration(_)
```

需要格式化为人类可读值。

#### Decimal

```rust
DataType::Decimal128(_, _)
DataType::Decimal256(_, _)
```

需要显示 scale。

#### Dictionary

```rust
DataType::Dictionary(_, _)
```

需要解析实际 value，而不是 debug。

#### FixedSizeBinary

表格显示：

```text
<N bytes>
```

detail 显示 hex。

### P4.3 Cell Detail 增强

#### 语法高亮

JSON-like 详情中：

- key 黄色
- string 绿色
- number cyan
- null gray
- bool magenta

实现成本中等，因为当前 detail 是纯 String。要高亮需要改为：

```rust
Vec<Line<'static>>
```

或者在 TUI 层解析。建议等 formatting 稳定后再做。

#### 复制行

新增快捷键：

```text
Y
```

复制当前整行 JSON-like。

需要：

```rust
selected_row_detail_json()
```

#### 详情弹窗搜索

快捷键：

```text
/
```

但 `/` 已用于 filter。详情弹窗内可以复用 `/` 做 detail search。不建议马上做。

#### 滚动边界

当前 detail scroll 可以无限加。

建议 draw 时 clamp：

```rust
let scroll = app.cell_detail_scroll.min(max_scroll)
```

需要根据 popup 高度和 lines 数计算 `max_scroll`。

### P4.4 表格体验

#### 动态列宽

当前列宽固定 24。可以改成：

- 最小 12
- 最大 40
- 根据 header 和当前页内容估算
- 剩余空间分配给最后一列

注意避免宽字符错位。

#### 显示行号列

左侧增加固定 row number：

```text
# | col1 | col2
51| ...
```

帮助分页阅读。

#### Schema 视图滚动

当前 Schema 视图不滚动。

后续可复用：

```rust
selected_schema_row
schema_scroll
```

或者简单使用 selected_row。

---

## P5：性能优化与架构清理

### P5.1 Row group aware pagination

当前实现：

```text
读取并跳过 offset 行
```

大 offset 会慢。

优化目标：利用 Parquet metadata row groups。

思路：

1. 计算 offset 落在哪个 row group。
2. 跳过前面的 row group。
3. 从目标 row group 开始读取。

如果 parquet arrow reader 支持：

```rust
with_row_groups(...)
```

可以构造需要的 row groups。

注意：如果从中间 row group 开始，仍需在目标 row group 内跳过 partial offset。

### P5.2 筛选下推

当前筛选基于格式化后文本。

后续路线：

#### 路线 A：继续内置 DSL

优点：

- 轻
- 可控
- 不泄露 DataFusion

缺点：

- 无法利用 Parquet predicate pushdown
- 表达能力有限

#### 路线 B：DataFusion

优点：

- 表达能力强
- 可 count/filter/page
- 未来支持 SQL-like

缺点：

- 依赖重
- 异步/执行模型复杂
- 表达式语义和错误类型需要封装

推荐：短期继续内置 DSL。等功能稳定后再评估 DataFusion。

### P5.3 状态机与 TUI 解耦

当前 TUI 中还有较多动作处理：

```rust
handle_key
handle_mouse
load_page
apply_filter
```

长期建议引入：

```rust
enum Action
enum DataCommand
```

分步改法：

1. 定义 `Action`。
2. 把 key -> action 单独函数化。
3. AppState 添加 `handle_action`。
4. 返回 `Option<DataCommand>`。
5. TUI 执行 DataCommand。

好处：

- AppState 可测试
- TUI 只负责事件转 action
- Data Access 由外层执行 command

---

## 推荐执行顺序

### Step 1：补 AppState 测试

优先做：

```text
filter input 光标编辑
tab filter 隔离
分页边界
Schema/Data 切换
```

验收：

```bash
cargo test --offline
```

### Step 2：补 data filter 测试

在 `src/data.rs` 内部测试：

```text
parse_filter
split_filter
unquote_filter_value
FilterExpr::matches
```

验收：

```bash
cargo test --offline
```

### Step 3：更新 README

让 README 反映当前真实能力。

验收：

- README 中有启动方式
- 有生成测试数据命令
- 有快捷键表
- 有筛选语法
- 有限制说明

### Step 4：更新设计状态

更新：

```text
docs/design/09-milestones-and-decisions.md
```

标记：

- M1 已实现
- M2 已实现
- M3 第一版已实现
- 当前筛选选择：内置受限 DSL

### Step 5：筛选增强

先做：

```text
and/or
```

再做：

```text
候选弹窗
筛选历史
手动 count
```

### Step 6：M4 格式化拆分

把 `src/data.rs` 里的格式化逻辑迁移到 `src/formatting.rs`。

要求：

- TUI 不感知 Arrow 类型
- Data Access 不承载复杂显示策略
- 保持测试通过

---

## 每一步建议提交

建议每个小步骤独立提交。

示例提交信息：

```text
Add filter input state tests
Add filter parser tests
Document current TUI usage
Record implemented milestones
Support and/or filter expressions
Show filter completion candidates
Move cell formatting into formatting layer
```

---

## 验证命令

每次代码改动至少跑：

```bash
cargo fmt
cargo check --offline
cargo test --offline
```

涉及生成器时跑：

```bash
cargo check --bin generate-test-parquet --offline
cargo check --bin generate-empty-test-parquet --offline
cargo check --bin generate-complex-test-parquet --offline
cargo check --bin generate-pagination-test-parquet --offline
```

只改文档时，不需要强制运行 Rust 编译或测试命令。

---

## 当前最新提交参考

低级模型接手前建议先执行：

```bash
git log --oneline -8
cargo test --offline
```

确认基线正常。
