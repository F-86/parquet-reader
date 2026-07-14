# 04. 数据访问层

## 实现目标

本阶段实现 Parquet 数据访问抽象，并选择或预留底层查询实现。TUI 不应依赖 Arrow、DataFusion、DuckDB 或其他具体引擎。


## 推荐技术路线

第一阶段推荐使用 `datafusion` 作为默认 Parquet 查询后端，并把它完全封装在 Data Access Layer 内。

选择理由：

- 本项目需要 Schema、count、page、filter 这些查询能力。
- `datafusion` 提供 SQL/DataFrame 查询能力，并以内存中的 Arrow 格式作为执行基础。
- 相比直接使用 `parquet` crate，`datafusion` 可以更快获得筛选和分页查询能力。
- 相比 `duckdb` 绑定，`datafusion` 更符合纯 Rust 分发目标，避免第一阶段引入原生 DuckDB 分发复杂度。

约束：

- 不允许 TUI 层直接依赖 `datafusion` 类型。
- 不允许 App State 层直接拼 SQL。
- 如果后续发现 `datafusion` 依赖过重或异步模型影响 TUI，可以保留 trait，替换为 Arrow 原生或 DuckDB 后端。

## 候选库对比

| 路线 | 相关库 | 优点 | 代价 | 结论 |
|------|--------|------|------|------|
| DataFusion 查询后端 | `datafusion`、Arrow 相关类型 | SQL/filter 能力完整；支持 Parquet 扫描；适合 count、limit、offset 这类查询 | 依赖较重；异步执行需要封装；错误类型不能泄漏到 TUI | **推荐第一阶段默认路线** |
| Arrow / Parquet 原生读取 | `parquet`、`arrow-array`、`arrow-schema` | Rust 原生；类型和批读取控制力强；适合长期优化 | 筛选、表达式、精确分页需要更多自研 | 作为后续优化或轻量模式候选 |
| DuckDB 绑定 | `duckdb` | 最接近 Python 原型；SQL、count、limit/offset 简单直接 | 原生库分发和跨平台构建更复杂 | 作为快速复刻原型的备选路线 |

## 依赖引入策略

- M1 阶段如果只做 Schema + 第一页，可以先验证 `datafusion` 最小查询闭环。
- 不要同时引入 `datafusion` 和 `duckdb` 作为生产路径。
- 如果需要写临时 Parquet 测试文件，可以在 dev-dependencies 中使用 Arrow/Parquet 写入相关能力。
- 对外暴露的项目类型使用自定义 `ColumnInfo`、`Page`、`CellView`，不要直接暴露底层库类型。

## 数据访问 trait

```rust
pub trait ParquetDataSource {
    fn schema(&mut self) -> Result<Vec<ColumnInfo>>;
    fn count(&mut self, filter: Option<&str>) -> Result<u64>;
    fn page(&mut self, request: PageRequest<'_>) -> Result<Page>;
}

pub struct PageRequest<'a> {
    pub offset: usize,
    pub limit: usize,
    pub filter: Option<&'a str>,
}
```

## 核心数据类型

```rust
pub struct ColumnInfo {
    pub index: usize,
    pub name: String,
    pub logical_type: String,
    pub physical_type: Option<String>,
}

pub struct Page {
    pub rows: Vec<RowValue>,
}

pub struct RowValue {
    pub cells: Vec<Value>,
}
```

具体 `Value` 类型可由底层实现决定，但进入 TUI 前必须经过 Formatting Layer 转换。

## 分页语义

- `offset` 表示筛选后结果集中的逻辑行偏移。
- `limit` 表示当前页最多显示的行数。
- 数据访问层负责将逻辑分页映射到底层批读取、SQL 查询或执行计划。
- 如果底层无法高效随机跳页，可以先实现顺序分页，再在文档中标注限制。

## Count 语义

- `count(filter)` 用于状态栏总行数和页码。
- Count 失败不应阻塞用户查看第一页数据。
- Count 成本高时允许引入懒统计、缓存、后台任务或未知状态。

## 筛选表达式语义

第一阶段可以选择透传底层查询引擎表达式，但必须满足：

- UI 明确提示表达式语法来自底层引擎。
- 错误可恢复。
- 不宣称筛选表达式是安全沙箱。
- 后续可替换为受限 DSL 或表达式构造器。

## 历史候选路线说明

| 路线 | 优点 | 代价 | 适用判断 |
|------|------|------|----------|
| Arrow / parquet crate | Rust 原生，依赖边界清晰，类型和批读取可控 | 筛选和精确分页需要更多自研或组合 DataFusion | 长期可控、偏纯 Rust |
| DataFusion | SQL/filter 能力完整，执行计划和 Parquet 扫描成熟 | 依赖较重，异步模型需要封装 | 想要查询能力且接受较重依赖 |
| DuckDB 绑定 | 最接近 Python 原型，筛选、count、分页实现简单 | 原生库分发和跨平台构建更复杂 | 优先快速复刻原型 |

阶段性建议：

- 如果目标是尽快复刻 Python 原型，优先评估 DuckDB 绑定。
- 如果目标是长期维护和纯 Rust 分发，优先评估 Arrow + DataFusion。
- 无论选择哪条路线，都必须封装在 Data Access Layer 内，避免影响 TUI 和 App State。

## 集成测试重点

- 小型 Parquet 文件可以读取 Schema。
- 小型 Parquet 文件可以读取第一页。
- 空文件或空结果有稳定返回。
- 非法筛选表达式返回可恢复错误。
- Count 失败不会破坏 Page 读取接口。

## 验收标准

- TUI 不依赖具体查询引擎类型。
- App State 只接收 Page、Schema、Count 或错误。
- Data Access 层能替换底层实现而不影响 TUI。

## 下一步

继续实现 [`05-value-formatting.md`](./05-value-formatting.md)。
