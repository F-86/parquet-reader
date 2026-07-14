# Parquet TUI Viewer 设计文档索引

本文是 Rust 版 Parquet TUI Viewer 的设计入口。详细设计已按实现顺序拆分到 `docs/design/`，建议从 `00` 开始逐个实现。

当前文档描述的是拟议方案，不表示所有能力已经实现。已实现状态以代码和测试为准。

## 推荐实现顺序

| 顺序 | 文档 | 实现目标 |
|------|------|----------|
| 00 | [`00-overview.md`](./design/00-overview.md) | 理解项目目标、整体架构和边界 |
| 01 | [`01-scope-and-requirements.md`](./design/01-scope-and-requirements.md) | 固定第一阶段目标、非目标和用户体验需求 |
| 02 | [`02-cli-and-error-handling.md`](./design/02-cli-and-error-handling.md) | 实现 CLI 参数、启动校验和错误模型 |
| 03 | [`03-app-state-and-actions.md`](./design/03-app-state-and-actions.md) | 实现应用状态、动作和状态转移测试 |
| 04 | [`04-data-access.md`](./design/04-data-access.md) | 实现 Parquet 数据访问 trait、Schema、count 和 page |
| 05 | [`05-value-formatting.md`](./design/05-value-formatting.md) | 实现单元格格式化、截断和复杂值显示策略 |
| 06 | [`06-tui-layout-and-navigation.md`](./design/06-tui-layout-and-navigation.md) | 实现 TUI 布局、事件循环、表格、导航和文件选择入口 |
| 07 | [`07-filtering-and-schema-view.md`](./design/07-filtering-and-schema-view.md) | 实现筛选输入、筛选重置和 Schema 视图 |
| 08 | [`08-testing-strategy.md`](./design/08-testing-strategy.md) | 补齐单元、集成和手动验收测试 |
| 09 | [`09-milestones-and-decisions.md`](./design/09-milestones-and-decisions.md) | 跟踪里程碑、风险和待决策项 |


## 技术选型总览

第一阶段推荐的默认库组合：

| 领域 | 推荐库 | 候选库 | 选择理由 | 记录位置 |
|------|--------|--------|----------|----------|
| CLI 参数解析 | `clap` | `pico-args`、`argh` | 需要稳定 help、错误提示和 derive 模式；二进制体积不是第一优先级 | [`02-cli-and-error-handling.md`](./design/02-cli-and-error-handling.md) |
| 错误类型 | `thiserror` | `anyhow`、`color-eyre` | 需要跨层传递结构化错误；面向用户的错误信息必须可控 | [`02-cli-and-error-handling.md`](./design/02-cli-and-error-handling.md) |
| TUI | `ratatui` + `crossterm` | `termion`、`termwiz`、`cursive` | 需要可控事件循环、表格布局、跨平台终端输入输出 | [`06-tui-layout-and-navigation.md`](./design/06-tui-layout-and-navigation.md) |
| Parquet 查询 | `datafusion` | `parquet`/Arrow 原生、`duckdb` | 需要 SQL/filter、count、limit/offset，且优先保持纯 Rust 分发 | [`04-data-access.md`](./design/04-data-access.md) |
| 值宽度计算 | `unicode-width` | `unicode-display-width`、手写逻辑 | 需要按终端显示宽度截断，避免按字节截断破坏布局 | [`05-value-formatting.md`](./design/05-value-formatting.md) |
| CLI 集成测试 | `assert_cmd` | 手写 `std::process::Command` | 需要稳定断言退出码、stdout、stderr | [`08-testing-strategy.md`](./design/08-testing-strategy.md) |
| 临时文件 | `tempfile` | 手写临时目录、`temp-file` | 需要跨平台临时目录和自动清理 | [`08-testing-strategy.md`](./design/08-testing-strategy.md) |
| 快照测试 | `insta`（可选） | 普通字符串断言 | 适合后续稳定 TUI 渲染片段和状态栏输出 | [`08-testing-strategy.md`](./design/08-testing-strategy.md) |

这些是设计建议，不要求一次性全部加入 `Cargo.toml`。实现到对应阶段时再引入对应依赖，并在 PR 或提交说明中写明理由。

## 实现原则

- 每完成一个编号文档，对应代码和测试应能独立验证。
- 不要跳过架构边界直接实现 TUI 与数据访问混合逻辑。
- 数据读取默认按需分页，不得为了简化实现而默认全量加载 Parquet 文件。
- 如果某个文档中的能力暂不实现，应在 `09-milestones-and-decisions.md` 中记录为延期或待决策。

## 与其他文档的关系

- 面向用户的项目介绍和快速开始见 [`../README.md`](../README.md)。
- AI agent 协作规则见 [`../AGENTS.md`](../AGENTS.md)。
- 本目录只讲设计、实现顺序、架构边界和验收标准。
