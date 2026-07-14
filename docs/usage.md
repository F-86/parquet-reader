# 使用说明

本文件面向人类用户，补充 README 中的快捷键、筛选语法与限制说明。

## 启动方式

```bash
# 直接打开一个文件
cargo run -- path/to/file.parquet

# 不带文件启动，在左侧文件树中选择 .parquet 文件
cargo run
```

不带文件路径启动时，左侧文件树以程序运行目录为根目录；点击目录可展开/折叠，点击 `.parquet` 文件即打开。

## 快捷键

### 全局

| 快捷键 | 行为 |
|---|---|
| `q` / `Ctrl-C` | 退出 |
| `h` | 打开 / 关闭帮助弹窗 |
| `d` | 聚焦文件树侧边栏 |
| `s` | Schema / Data 视图切换 |
| `/` | 打开筛选弹窗 |
| `r` | 重置筛选 |
| `c` | 统计当前筛选匹配行数 |

### 文件树

| 快捷键 | 行为 |
|---|---|
| `j` / `k` / `↑` / `↓` | 移动选择 |
| `Enter` | 进入目录或打开 `.parquet` 文件 |
| `Esc` | 退出文件树聚焦，回到数据区 |

### 表格

| 快捷键 | 行为 |
|---|---|
| `j` / `k` / `↑` / `↓` | 行移动 |
| `J` / `K` | 当前页底部 / 顶部 |
| `←` / `→` / `l` | 列移动 |
| `H` / `L` | 第一列 / 最后一列 |
| `n` / `PageDown` | 下一页 |
| `p` / `PageUp` | 上一页 |
| `Enter` / `Space` | 打开选中 cell 详情 |
| `y` | OSC52 复制当前 cell 到系统剪贴板 |
| `Y` | OSC52 复制当前整行 (JSON) 到系统剪贴板 |

### Tab

| 快捷键 | 行为 |
|---|---|
| `Tab` / `Shift-Tab` | 下一个 / 上一个 tab |
| 鼠标点击 tab | 切换文件 |

### Cell Detail

| 快捷键 | 行为 |
|---|---|
| `↑` / `↓` / `k` / `j` | 滚动详情 |
| `PageUp` / `PageDown` | 大步滚动 |
| `Home` | 回到顶部 |
| `y` | 复制当前 cell |
| `Y` | 复制当前整行 (JSON) |
| `Esc` / `Enter` / `Space` | 关闭详情 |

### Schema 视图

| 快捷键 | 行为 |
|---|---|
| `s` | 切换回 Data 视图 |
| `j` / `k` / `↑` / `↓` | 选择字段 |
| `J` / `K` | 第一个 / 最后一个字段 |
| `y` | 复制当前字段 cell |
| `Y` | 复制当前整行 (JSON) |

### 筛选弹窗

| 快捷键 | 行为 |
|---|---|
| `Tab` / `Shift-Tab` | 字段补全 / 反向循环 |
| `↑` / `↓` | 上一条 / 下一条筛选历史 |
| `←` / `→` / `Home` / `End` | 移动光标 |
| `Backspace` / `Delete` | 删除光标前 / 处字符 |
| `Enter` | 应用筛选 |
| `Esc` | 取消 |

## 筛选语法

受限 DSL，语法：

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

规则：

- `contains` 大小写不敏感。
- 数字比较优先按数字解析，否则按字符串比较。
- 字符串可加 `'` 或 `"` 引号，也可省略。
- 支持 `and` / `or` 组合多个条件；`or` 优先级低于 `and`。
- 不支持 SQL、括号、内层 `and`/`or` 混排优先级。
- **不是安全沙箱**：表达式直接作用于读取逻辑。

## 测试数据

```bash
cargo run --bin generate-test-parquet
cargo run --bin generate-empty-test-parquet
cargo run --bin generate-complex-test-parquet
cargo run --bin generate-pagination-test-parquet
```

## 限制

- 分页 offset 顺序跳过已读行，超大 offset 后续优化。
- 筛选基于格式化后的 cell 文本。
- 筛选后总数未知（状态栏显示 `?`），可按 `c` 手动统计。
- Schema 视图暂不支持排序（支持字段上下选择与自动滚动）。
- 暂无列排序、导出。
