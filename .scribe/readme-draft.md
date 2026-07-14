# parquet-reader

`parquet-reader` 是一个用 Rust 编写的 Parquet TUI Viewer，目标是在终端内查看 Parquet 文件内容。

项目当前处于设计与早期实现阶段。根目录的 `parquet_tui.py` 是 Python 行为原型，Rust 版本会参考它的交互体验，但实现架构会重新设计。

## 特性

- 在终端内浏览 Parquet 文件。
- 默认分页读取，避免默认全量加载文件内容。
- 数据表视图展示当前页数据。
- Schema 视图展示字段名和类型信息。
- 支持 vim/k9s 风格快捷键导航。
- 支持筛选输入、筛选重置和状态栏反馈。

## 项目状态

当前项目仍在早期阶段：

- Python 原型：`parquet_tui.py`
- Rust 入口：基础项目骨架
- 设计方案：`docs/design.md`

<!-- TODO: Rust TUI 实现完成后补充实际运行截图或录屏。 -->

## 快速开始

### 运行 Python 原型

如果你想先体验交互原型，可以运行 Python 版本：

```bash
uv run --group dev parquet_tui.py --ds path/to/file.parquet
```

### 运行 Rust 版本

Rust 版本的 TUI 功能仍在实现中。当前可以先验证项目能否编译：

```bash
cargo check
```

<!-- TODO: Rust CLI 实现后补充正式运行命令，例如 cargo run -- --ds path/to/file.parquet。 -->

## 文档

- 设计方案：`docs/design.md`
- AI agent 协作规则：`AGENTS.md`

## 开发

常用验证命令：

```bash
cargo fmt
cargo check
cargo test
```

只修改文档时，不需要强行运行 Rust 编译或测试命令。

## 许可证

<!-- TODO: 请补充许可证信息。 -->
