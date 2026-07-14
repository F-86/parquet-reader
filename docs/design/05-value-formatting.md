# 05. 值格式化与显示策略

## 实现目标

本阶段实现把数据访问层返回的原始值转换为 TUI 可显示文本的逻辑。目标是让任何单元格值都不能破坏表格布局。


## 技术选型

| 候选方案 | 优点 | 代价 | 结论 |
|----------|------|------|------|
| `unicode-width` | 按 Unicode 显示宽度计算 `char` / `str`；适合终端表格截断 | 对某些 grapheme cluster 场景仍需测试验证 | **推荐第一阶段使用** |
| `unicode-display-width` | 关注完整字符串显示列宽和 grapheme cluster | 生态使用面需要再评估 | 可作为宽字符边界问题的备选 |
| 手写字节或字符计数 | 无额外依赖 | 容易破坏 Unicode 边界；中文、emoji、组合字符会错位 | 不选 |

第一阶段建议使用 `unicode-width` 实现显示宽度计算，并为中文、emoji、长字符串补单元测试。

## 输出类型

```rust
pub struct CellView {
    pub text: String,
    pub truncated: bool,
    pub kind: CellKind,
}

pub enum CellKind {
    Null,
    Scalar,
    Text,
    Binary,
    List,
    Map,
    Struct,
    Unsupported,
}
```

## 显示策略

| 值类型 | 显示策略 |
|--------|----------|
| NULL | 显示固定文本，例如 `NULL` |
| 长字符串 | 按显示宽度截断，追加省略标记 |
| 宽字符 | 按终端显示宽度计算，不按字节数截断 |
| 二进制 | 显示十六进制摘要或 `<binary N bytes>` |
| List / Map | 显示紧凑摘要，必要时截断 |
| Struct | 显示紧凑结构摘要，必要时截断 |
| 时间/日期 | 使用稳定、可读的文本格式 |
| 不支持类型 | 显示 `<unsupported>` 或带类型名摘要 |

## 截断规则

- 截断基于显示宽度，不基于字节长度。
- 截断不得破坏 Unicode 边界。
- 超出宽度时追加省略标记。
- 最大单元格宽度应可配置，第一阶段可使用固定值。

## 错误处理

- 单个值格式化失败时，不应导致整页渲染失败。
- 格式化失败可以显示 `<format error>`，并记录内部错误上下文。
- 不默认把完整原始值写入日志。

## 后续扩展

后续可以增加单元格详情弹窗，用于查看完整值。第一阶段只要求表格中有稳定摘要。

## 单元测试重点

- NULL。
- 短字符串。
- 长字符串。
- 中文或 emoji 等宽字符。
- 二进制值。
- List、Map、Struct 摘要。
- 不支持类型。
- 截断边界。

## 验收标准

- 任意单元格值都能转换为 `CellView`。
- 超长值不会破坏表格布局。
- 宽字符截断后不会出现乱码。

## 下一步

继续实现 [`06-tui-layout-and-navigation.md`](./06-tui-layout-and-navigation.md)。
