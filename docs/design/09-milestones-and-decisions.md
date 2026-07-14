# 09. 里程碑、风险与待决策项

## 当前里程碑状态

| 里程碑 | 状态 | 说明 |
|--------|------|------|
| M1：最小可启动查看器 | 已实现 | 启动、文件树、tab 栏、帮助、终端恢复均已落地 |
| M2：导航与分页 | 已实现 | 行/列导航、水平滚动、上一页/下一页、分页状态栏、鼠标滚轮 |
| M3：Schema 与筛选（第一版） | 已实现 | Schema 视图、筛选弹窗、字段 Tab 补全、筛选错误恢复、每 tab 独立筛选、筛选后分页 offset 归零 |
| M4：健壮性与体验 | 进行中 | 见 `docs/next-development-plan.md` 的 P4 |
| M5：性能优化与架构清理 | 规划 | 见 `docs/next-development-plan.md` 的 P5 |

## M1：最小可启动查看器

交付能力：

- CLI 接收可选文件路径。
- 不带文件路径时进入 TUI 空状态。
- TUI 左侧显示贯穿全高的文件列表侧边栏，根目录为程序运行目录。
- TUI 中按 `d` 聚焦左侧文件列表。
- 右侧顶部 tab 栏显示当前文件或 `[No file]`。
- 不常驻显示 key hints，按 `h` 打开帮助弹窗。
- 读取 Schema。
- 显示第一页数据。
- 支持退出。
- 多 tab、Cell Detail、复杂类型 pretty print、测试数据生成 bin。

验收标准：

- 有效 Parquet 文件能通过 CLI 直接进入 TUI。
- 不带文件启动时能进入空状态。
- 空状态可以通过左侧文件列表选择本地 `.parquet` 文件。
- 打开文件后右侧顶部 tab 栏显示当前文件名。
- 左侧文件侧边栏不被底部 key hints 或其他区域挤压。
- 按 `h` 能打开帮助弹窗，`Esc` 或 `h` 能关闭。
- 无效路径能给出明确错误。
- 退出后终端恢复正常。

对应文档：

- [`02-cli-and-error-handling.md`](./02-cli-and-error-handling.md)
- [`03-app-state-and-actions.md`](./03-app-state-and-actions.md)
- [`04-data-access.md`](./04-data-access.md)
- [`06-tui-layout-and-navigation.md`](./06-tui-layout-and-navigation.md)

## M2：导航与分页

交付能力：

- 行导航。
- 水平滚动。
- 上一页和下一页。
- 状态栏显示行范围、页码和列数。
- 分页测试数据生成器。
- 鼠标滚轮上下翻行 / 翻页。

验收标准：

- 翻页不改变筛选条件。
- 上一页不会越过第一页。
- 最后一页不会越界。

对应文档：

- [`03-app-state-and-actions.md`](./03-app-state-and-actions.md)
- [`06-tui-layout-and-navigation.md`](./06-tui-layout-and-navigation.md)

## M3：Schema 与筛选

交付能力：

- Schema 视图。
- 筛选输入弹窗。
- 筛选输入光标编辑（插入 / 删除 / Home / End / 左右移动，保持 UTF-8 边界）。
- 字段 Tab 补全（单候选 / 多候选循环 / Shift-Tab 反向 / 只补全当前 token / 无匹配提示）。
- 筛选重置。
- 查询错误展示（非法表达式不崩溃，保留上下文并展示可行动错误）。
- 每个 tab 独立保存筛选条件。
- 状态栏 filter 高亮。
- 筛选后的分页 offset 语义修复（提交筛选后 offset 归零）。

验收标准：

- 切换 Schema 不破坏数据视图状态。
- 提交筛选后 offset 归零。
- 提交筛选后 status 栏显示当前筛选条件。
- 非法筛选表达式不导致崩溃。

对应文档：

- [`07-filtering-and-schema-view.md`](./07-filtering-and-schema-view.md)

## M4：健壮性与体验

交付能力（规划，见 P4）：

- 类型显示策略完善（Date/Time/Timestamp/Decimal/Dictionary/FixedSizeBinary）。
- 宽字符和截断处理。
- Formatting 层从 `data.rs` 拆分到 `formatting.rs`，TUI 不感知 Arrow 类型。
- 更完整测试，失败路径与空数据路径作为一等场景。
- Cell Detail 增强（滚动边界 clamp、复制整行、语法高亮）。

验收标准：

- 复杂类型和异常值不会破坏表格布局。
- 空数据和错误路径有稳定展示。
- 核心状态转移有测试覆盖。

对应文档：

- [`05-value-formatting.md`](./05-value-formatting.md)
- [`08-testing-strategy.md`](./08-testing-strategy.md)

## 当前推荐技术栈

| 领域 | 当前推荐 | 状态 | 说明 |
|------|----------|------|------|
| CLI | `clap` | 已采用 | 面向用户的 CLI 需要稳定 help 和错误提示 |
| 错误类型 | `thiserror` | 已采用 | 保持跨层错误结构化 |
| TUI | `ratatui` + `crossterm` | 已采用 | 兼顾布局能力、事件控制和跨平台终端支持 |
| Parquet 读取 | `parquet`（arrow 特性） | 已采用 | 直接基于 Arrow/Parquet，跳过 DataFusion |
| 值宽度 | `unicode-width` | 已采用 | 避免按字节截断导致终端错位 |
| CLI 测试 | `assert_cmd` | 未采用 | 暂未写 CLI 集成测试 |
| 临时文件 | `tempfile` | 已采用 | 文件树 root 边界测试 |
| 快照测试 | `insta` | 未采用 | 等 TUI 输出稳定后再引入 |

## 技术选型变更记录

- 数据访问路线：原设计建议默认采用 `datafusion`，实际 M1-M3 阶段改为直接基于 `parquet` + Arrow 原生读取。理由：当前功能（分页读取、受限 DSL 筛选、类型格式化）不需要 SQL 引擎，避免重依赖与异步/执行模型复杂化。查询引擎表达式、异步模型、错误类型不向 TUI 层泄漏。后续若需复杂查询 / 谓词下推，再评估 DataFusion（见 P5.2）。

## 风险与缓解

| 风险 | 影响 | 缓解策略 |
|------|------|----------|
| Count 代价过高 | 打开或筛选后卡顿 | 懒统计、缓存、后台任务、允许显示未知；按 `c` 手动 count |
| 查询引擎依赖过重 | 构建慢、分发复杂 | 用 Data Access 边界隔离；暂用原生 Arrow 读取 |
| 复杂类型显示不可控 | 表格布局破坏 | 统一 Formatting Layer，先摘要再详情 |
| 宽字符截断错误 | UI 错位 | 使用显示宽度计算，不按字节截断 |
| 筛选表达式语义不清 | 用户误解能力或安全边界 | UI 和文档明确表达式来源与限制，标注非安全沙箱 |
| 查询阻塞 UI | TUI 无响应 | 后续引入异步查询、取消任务或进度状态 |
| 终端恢复失败 | 用户 shell 状态异常 | 统一终端生命周期管理，错误路径也执行恢复 |
| 文件侧边栏状态复杂 | 空状态、已有文件、焦点切换、目录切换和选择文件之间状态混乱 | 侧边栏动作进入 App State 状态机，并为焦点切换、目录边界和切换文件写单元测试 |
| 多 tab 状态膨胀 | 每个文件都有分页、筛选和滚动状态，容易互相污染 | 每个 tab 保存独立状态，切换时 save/restore |
| 帮助信息挤占布局 | 常驻 key hints 会压缩文件侧边栏和数据表 | 不常驻显示 key hints；按 `h` 使用帮助弹窗 |

## 已决策项

- 第一阶段数据访问路线：暂缓 `datafusion`，采用 `parquet` + Arrow 原生读取（受限内置 DSL 筛选）。
- 筛选表达式是受限 DSL，不是安全沙箱；UI 与文档均明确标注。
- 筛选后 count 暂未知（状态栏显示 `?`），先提供手动 count 快捷键 `c`，长期再演进为 `CountState`。
- 筛选基于格式化后的 cell 文本，而非原始 Arrow 值。

## 待决策项

- 是否默认计算精确总行数。
- 是否需要异步查询与取消。
- 左侧文件侧边栏是否需要模糊搜索、最近文件、目录书签、隐藏非 Parquet 文件等增强能力。
- 帮助弹窗是否需要搜索、分页或按上下文只显示当前模式快捷键。
- 是否支持列选择、排序、单元格详情弹窗、导出当前页。
- 是否需要为大型样例建立单独的性能测试集。
- 后续是否引入 DataFusion 以获得谓词下推与更强查询能力（见 `docs/next-development-plan.md` P5.2）。

## 决策记录规则

- 做出技术选型后，在本文档中更新待决策项状态。
- 影响较大的单个决策应新增 ADR，而不是把完整争论写进本文档。
- 已延期能力应写明延期原因和预计进入的里程碑。
