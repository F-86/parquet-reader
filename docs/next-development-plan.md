# parquet-reader 剩余开发计划

> 最后更新：2026-07-14
> 说明：本文件现在只保留**截至当前仍未完成**的事项；原先已经完成的 P1-P5 内容已不再重复展开。

本文档面向继续接手实现的 AI coding agent，用来回答两件事：

1. 这个项目现在还剩什么没做；
2. 接下来应按什么顺序继续推进。

---

## 当前状态

原计划中以下内容已经落地，不再作为待办：

- P1：测试补齐
  - AppState 状态测试
  - filter 输入编辑测试
  - tab 间 filter 隔离测试
  - 分页边界测试
  - Schema/Data 切换测试
  - filter parser / matcher 测试
  - 文件树 root 边界测试
- P2：文档同步
  - `README.md`
  - `docs/usage.md`
  - `docs/design/09-milestones-and-decisions.md`
- P3：筛选增强
  - `and` / `or`
  - 筛选补全候选列表
  - 筛选历史
  - 手动 count（`c`）
  - `CountState`
- P4：健壮性与值显示体验
  - Formatting 层拆分到 `src/formatting.rs`
  - 更多 Arrow 类型支持
  - Cell Detail JSON 高亮
  - 复制整行（`Y`）
  - detail scroll clamp
  - 动态列宽
  - 行号列
  - Schema 视图滚动与独立导航
- P5 已完成部分
  - Row-group-aware pagination
  - 当前页排序（`o` / `O`）
  - 当前页导出（`e`）

当前实现仍遵守以下长期约束：

- 默认按需分页读取，不做隐式全量加载；
- 排序与导出都只针对**当前页**；
- TUI / AppState / Data Access / Formatting 边界保持分离；
- 错误走状态栏短消息，不因空结果或失败路径崩溃。

---

## 仍未完成的事项

## R1：Cell Detail 内搜索

### 目标

在 Cell Detail 弹窗中支持内容内搜索，减少长 JSON / 长文本手工滚动成本。

### 当前情况

- 详情弹窗已经支持：
  - JSON 高亮
  - 滚动
  - `y` 复制当前 cell
  - `Y` 复制当前整行 JSON
- 但**还没有**弹窗内搜索。

### 建议实现

- 仅在 Cell Detail 打开时复用 `/` 进入 detail search 模式；
- 搜索状态不要污染主表格的 filter 输入；
- 至少支持：
  - 输入搜索词
  - 下一个匹配
  - 上一个匹配
  - 清空搜索
  - 搜索结果高亮
- 搜索滚动应自动把当前匹配滚动到可见区域；
- 无匹配时给出短状态提示，不报错退出。

### 建议拆分

1. 给 detail view 增加独立搜索状态：

   ```rust
   detail_search_input
   detail_search_matches
   detail_search_index
   ```

2. 新增匹配与跳转逻辑。
3. 渲染层高亮当前匹配。
4. 补测试：
   - 空搜索
   - 单结果
   - 多结果前后跳转
   - 搜索后滚动 clamp

### 验收

```bash
cargo fmt
cargo test --offline
```

### 建议提交信息

```text
Add search within cell detail popup
```

---

## R2：筛选下推 / 筛选执行模型升级（原 P5.2）

### 目标

改进当前筛选执行方式，减少“先格式化再匹配文本”的局限，为后续更强过滤能力留演进空间。

### 当前情况

当前筛选能力已经可用，但仍有这些限制：

- 筛选主要基于格式化后的 cell 文本；
- 不能利用 Parquet predicate pushdown；
- 不支持括号和更复杂优先级；
- 排序和筛选仍以当前自定义 DSL 为主；
- 还没有引入 DataFusion。

### 建议路线

短期仍优先保留内置 DSL，不急着切到 DataFusion。

优先顺序建议：

1. **先把匹配逻辑从“显示文本”向“原始类型值”推进**
   - 数字按数字比较
   - 布尔按布尔比较
   - 日期/时间类型按结构化值比较
   - `contains` 仍可保留字符串语义
2. **按列裁剪读取/比较成本**
   - 只对筛选实际涉及的列做匹配准备
3. **保留 DataFusion 评估为后续分支，不默认引入**
   - 仅当需要更强表达式能力或谓词下推收益明显时再评估

### 不要做的事

- 不要把 DataFusion 错误、表达式语法或异步模型直接泄漏到 TUI；
- 不要为了“支持复杂筛选”而默认全量加载整个文件；
- 不要把当前 DSL 描述成安全沙箱。

### 建议拆分

#### R2.1 类型化比较

- 把 filter predicate 的比较尽量下沉到结构化值层；
- 保持 UI 语义不变；
- 先覆盖：
  - 数字
  - 布尔
  - 日期 / 时间 / 时间戳

#### R2.2 列级最小化匹配

- 解析 filter AST 后，提取被引用列；
- 匹配阶段只访问必要列数据；
- 不改变当前分页接口语义。

#### R2.3 DataFusion 可行性调研（可选）

- 单独形成设计结论，不直接落代码也可以；
- 只回答：
  - 是否值得引入；
  - 会带来哪些收益；
  - 会破坏哪些当前架构边界；
  - 如果引入，边界应如何封装。

### 验收

```bash
cargo fmt
cargo check --offline
cargo test --offline
```

### 建议提交信息

```text
Improve typed filter evaluation
```

或：

```text
Evaluate DataFusion as an optional filter backend
```

---

## R3：状态机与 TUI 解耦（原 P5.3）

### 目标

进一步把“事件解释”和“状态变更”从 TUI 层抽离，让：

- TUI 只负责事件采集、布局和渲染；
- AppState 负责状态迁移；
- Data Access 负责执行读取命令。

### 当前情况

虽然当前分层已经比早期清晰很多，但 TUI 里仍然有较多按键分支直接驱动应用行为。长期看，这会增加：

- 行为测试难度；
- 快捷键改动成本；
- 视图模式分支复杂度；
- 新增交互时的回归风险。

### 目标形态

建议逐步收敛到：

```rust
enum Action
enum DataCommand
```

方向：

- TUI：`event -> Action`
- AppState：`handle_action(action) -> Option<DataCommand>`
- 外层协调：执行 `DataCommand` 并把结果回写 AppState

### 建议拆分

#### R3.1 提取 Action

- 先把键盘事件映射提取成纯函数；
- 不要一开始就重写所有分支；
- 先覆盖最稳定的主路径：
  - 翻页
  - 行列移动
  - schema/data 切换
  - 排序
  - 导出

#### R3.2 AppState handle_action

- 把纯状态变更移动到 AppState；
- 对需要读取数据的动作返回 `DataCommand`；
- 保持错误信息仍由 AppState 持有。

#### R3.3 为 Action 层补测试

- 每个 Action 至少验证：
  - 前置条件
  - 状态变化
  - 是否发出正确的 `DataCommand`

### 注意事项

- 不要一次性重写全部 TUI 事件分支；
- 不要把 Arrow / parquet 具体类型引进 TUI；
- 不要破坏现有快捷键语义，尤其：
  - `h` 继续保留给帮助
  - `←` 继续是左移，不把 `h` 绑定回左移

### 验收

```bash
cargo fmt
cargo check --offline
cargo test --offline
```

### 建议提交信息

```text
Decouple actions from the TUI event layer
```

---

## 可继续但不属于原始 P1-P5 主线的增强项

这些不是当前必须项，但如果用户继续要求“再往前推进”，优先从这里挑小步工作。

### E1：导出路径可配置

当前 `e` 会把当前页导出到系统临时目录，文件名形如：

```text
<stem>.page.csv
```

可继续增强为：

- 导出到当前工作目录；
- 支持 CLI 参数指定导出目录；
- 或增加导出路径输入弹窗。

前提不变：**仍然只导出当前页，不做隐式全量导出。**

### E2：状态栏更明确显示排序 / count 状态

当前功能已经可用，但还可以进一步明确：

- 当前是否处于页内排序；
- 当前排序列和方向；
- `CountState::Failed` 的可读提示；
- 未知总数与已知总数的差异提示。

---

## 推荐实现顺序

建议按以下顺序继续：

```text
R1 > R3 > R2
```

原因：

- **R1** 改动面最小，用户感知最直接；
- **R3** 是长期维护性收益最大的重构；
- **R2** 风险最高，适合在行为和分层更稳定后推进。

如果要先做一个最小可交付增强，优先选：

```text
R1：Cell Detail 内搜索
```

---

## 最小验收命令

代码改动后至少执行其一：

```bash
cargo fmt
cargo check --offline
cargo test --offline
```

若改动涉及测试数据生成器，再额外执行：

```bash
cargo check --bin generate-test-parquet --offline
cargo check --bin generate-empty-test-parquet --offline
cargo check --bin generate-complex-test-parquet --offline
cargo check --bin generate-pagination-test-parquet --offline
```
