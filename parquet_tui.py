"""Parquet TUI Viewer - k9s 风格的交互式 Parquet 文件查看器

用法:
  uv run --group dev parquet/parquet_tui.py --ds /path/to/file.parquet

快捷键:
  h/←        向左滚动      l/→        向右滚动
  j/↓        向下移动      k/↑        向上移动
  H          滚到最左列    L          滚到最右列
  J          跳到底部      K          跳到顶部
  n/PageDn   下一页        p/PageUp   上一页
  /          输入筛选条件  r          重置筛选
  s          切换 Schema   q          退出

数据加载策略:
  每页 _PAGE_SIZE 行，通过 DuckDB LIMIT/OFFSET 按需查询，不全量加载内存。
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from typing import Annotated
import typer
import duckdb
from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.widgets import DataTable, Footer, Input, Static, Label
from textual.containers import Horizontal
from textual import on

# 单个单元格最大显示宽度（字符数），超出截断显示 …
_MAX_CELL_WIDTH = 50
# 每页行数
_PAGE_SIZE = 100


class ParquetTUI(App):
    """k9s 风格的 Parquet 文件 TUI 查看器"""

    CSS = """
    #filter-container {
        height: 3;
        dock: top;
        background: $boost;
        display: none;
    }
    #filter-container.visible {
        display: block;
    }
    #filter-label {
        width: auto;
        color: $accent;
        text-style: bold;
        padding: 1 1 0 1;
    }
    #filter-input {
        width: 1fr;
        margin: 0 1;
    }
    #status-bar {
        height: 1;
        dock: bottom;
        background: $boost;
        padding: 0 1;
    }
    #data-table {
        height: 1fr;
    }
    """

    BINDINGS = [
        Binding("j,down", "cursor_down", "↓", show=False),
        Binding("k,up", "cursor_up", "↑", show=False),
        Binding("h,left", "scroll_left", "←"),
        Binding("l,right", "scroll_right", "→"),
        Binding("H", "scroll_leftmost", "⏮ col"),
        Binding("J", "cursor_bottom", "⏭ row"),
        Binding("K", "cursor_top", "⏮ row"),
        Binding("L", "scroll_rightmost", "⏭ col"),
        Binding("n,pagedown", "next_page", "Next Pg"),
        Binding("p,pageup", "prev_page", "Prev Pg"),
        Binding("/", "show_filter", "Filter"),
        Binding("r", "reset_filter", "Reset"),
        Binding("s", "toggle_schema", "Schema"),
        Binding("escape", "cancel_filter", "Cancel", show=False),
        Binding("q", "quit", "Quit"),
    ]

    def __init__(self, ds: str):
        super().__init__()
        self.ds = ds
        self.offset = 0
        self.filter_condition = ""
        self.show_schema = False
        self.total_count = 0
        self.schema_info: list[tuple[str, str]] = []
        self._con = duckdb.connect()

    def compose(self) -> ComposeResult:
        with Horizontal(id="filter-container"):
            yield Label("WHERE:", id="filter-label")
            yield Input(
                id="filter-input",
                placeholder="如: col_name > 100 AND col_name2 LIKE '%abc%'",
            )
        yield DataTable(id="data-table", cursor_type="row", zebra_stripes=True, fixed_rows=1)
        yield Static("", id="status-bar")
        yield Footer()

    def on_mount(self) -> None:
        self.title = f"📄 {os.path.basename(self.ds)}"
        self.sub_title = "Parquet Viewer"
        try:
            self._load_schema()
        except Exception as e:
            self._update_status(error=str(e))
            return
        self._refresh()
        self.query_one("#data-table").focus()

    def on_unmount(self) -> None:
        self._con.close()

    # ── 数据加载 ──

    @staticmethod
    def _truncate_cell(value: str) -> str:
        """截断超长单元格，超出 _MAX_CELL_WIDTH 显示 …"""
        if len(value) > _MAX_CELL_WIDTH:
            return value[: _MAX_CELL_WIDTH - 1] + "…"
        return value

    def _load_schema(self) -> None:
        result = self._con.sql(f"DESCRIBE SELECT * FROM '{self.ds}'").fetchall()
        self.schema_info = [(row[0], row[1]) for row in result]

    def _build_query(self) -> str:
        where = f"WHERE {self.filter_condition}" if self.filter_condition else ""
        return f"SELECT * FROM '{self.ds}' {where}"

    def _refresh(self) -> None:
        table = self.query_one("#data-table", DataTable)
        table.clear(columns=True)

        if self.show_schema:
            table.add_column("#")
            table.add_column("column_name")
            table.add_column("column_type")
            for idx, (name, dtype) in enumerate(self.schema_info):
                table.add_row(str(idx), name, dtype)
            self.total_count = len(self.schema_info)
            self._update_status()
            return

        try:
            self.total_count = self._con.sql(
                f"SELECT COUNT(*) FROM ({self._build_query()}) _t"
            ).fetchone()[0]

            for name, _ in self.schema_info:
                table.add_column(name, key=name)

            if self.total_count == 0:
                self._update_status()
                return

            rows = self._con.sql(
                f"{self._build_query()} LIMIT {_PAGE_SIZE} OFFSET {self.offset}"
            ).fetchall()

            for row in rows:
                table.add_row(
                    *[self._truncate_cell(str(v) if v is not None else "NULL") for v in row]
                )

        except Exception as e:
            self._update_status(error=str(e))
            return

        table.move_cursor(row=0)
        self._update_status()

    def _update_status(self, error: str | None = None) -> None:
        status = self.query_one("#status-bar", Static)
        if error:
            status.update(f"❌ {error}")
            return

        parts: list[str] = [os.path.basename(self.ds)]

        if self.show_schema:
            parts.append(f"schema ({len(self.schema_info)} cols)")
        else:
            if self.total_count == 0:
                label = "no data (filtered)" if self.filter_condition else "no data"
                parts.append(label)
            else:
                page_end = min(self.offset + _PAGE_SIZE, self.total_count)
                parts.append(f"rows {self.offset + 1}-{page_end}/{self.total_count}")
                page_num = self.offset // _PAGE_SIZE + 1
                total_pages = (self.total_count + _PAGE_SIZE - 1) // _PAGE_SIZE
                parts.append(f"pg {page_num}/{total_pages}")

            total_cols = len(self.schema_info)
            if total_cols > 0:
                parts.append(f"cols {total_cols}")

        if self.filter_condition:
            parts.append(f"🔍 {self.filter_condition}")

        status.update(" | ".join(parts))

    # ── 行导航 ──

    def action_cursor_down(self) -> None:
        self.query_one("#data-table", DataTable).action_cursor_down()

    def action_cursor_up(self) -> None:
        self.query_one("#data-table", DataTable).action_cursor_up()

    def action_cursor_top(self) -> None:
        table = self.query_one("#data-table", DataTable)
        if table.row_count > 0:
            table.move_cursor(row=0)

    def action_cursor_bottom(self) -> None:
        table = self.query_one("#data-table", DataTable)
        if table.row_count > 0:
            table.move_cursor(row=table.row_count - 1)

    # ── 翻页 ──

    def action_next_page(self) -> None:
        if self.show_schema:
            return
        if self.offset + _PAGE_SIZE < self.total_count:
            self.offset += _PAGE_SIZE
            self._refresh()

    def action_prev_page(self) -> None:
        if self.show_schema or self.offset == 0:
            return
        self.offset = max(0, self.offset - _PAGE_SIZE)
        self._refresh()

    # ── 水平滚动 ──

    def action_scroll_left(self) -> None:
        self.query_one("#data-table", DataTable).scroll_left()

    def action_scroll_right(self) -> None:
        self.query_one("#data-table", DataTable).scroll_right()

    def action_scroll_leftmost(self) -> None:
        self.query_one("#data-table", DataTable).scroll_x = 0

    def action_scroll_rightmost(self) -> None:
        table = self.query_one("#data-table", DataTable)
        table.scroll_x = table.max_scroll_x

    # ── 筛选 ──

    def action_show_filter(self) -> None:
        container = self.query_one("#filter-container")
        container.add_class("visible")
        input_widget = self.query_one("#filter-input", Input)
        input_widget.value = self.filter_condition
        input_widget.focus()

    def action_cancel_filter(self) -> None:
        container = self.query_one("#filter-container")
        if "visible" in container.classes:
            container.remove_class("visible")
            self.query_one("#data-table").focus()

    def action_reset_filter(self) -> None:
        self.filter_condition = ""
        self.offset = 0
        self._refresh()

    @on(Input.Submitted, "#filter-input")
    def _on_filter_submitted(self, event: Input.Submitted) -> None:
        self.filter_condition = event.value.strip()
        self.offset = 0
        container = self.query_one("#filter-container")
        container.remove_class("visible")
        self._refresh()
        self.query_one("#data-table").focus()

    # ── 视图切换 ──

    def action_toggle_schema(self) -> None:
        self.show_schema = not self.show_schema
        self._refresh()


app = typer.Typer(
    no_args_is_help=True,
    add_completion=False,
    pretty_exceptions_show_locals=False,
    pretty_exceptions_enable=False,
)

@app.command(help="k9s 风格的 Parquet TUI 查看器")
def main(
    ds: Annotated[str, typer.Option("--ds", help="Parquet 文件路径")] = None,
):
    if not ds:
        from rich import print as rprint
        rprint("[bold red]==> ERR: 请通过 --ds 指定 parquet 文件路径[/bold red]")
        raise typer.Exit(code=1)

    if not os.path.exists(ds):
        from rich import print as rprint
        rprint(f"[bold red]==> ERR: 文件不存在: {ds}[/bold red]")
        raise typer.Exit(code=1)

    viewer = ParquetTUI(ds=ds)
    viewer.run()

if __name__ == "__main__":
    app()
