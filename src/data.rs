use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

use arrow_array::RecordBatch;
use parquet::{
    arrow::arrow_reader::ParquetRecordBatchReaderBuilder, file::reader::SerializedFileReader,
};

use crate::{
    error::{AppError, Result},
    formatting::{CellView, format_cell},
};

#[derive(Debug, Clone)]
pub struct ColumnInfo {
    pub index: usize,
    pub name: String,
    pub logical_type: String,
    pub physical_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RowView {
    pub cells: Vec<CellView>,
}

#[derive(Debug, Clone)]
pub struct DataPage {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<RowView>,
    pub offset: usize,
    pub total_rows: Option<usize>,
}

pub struct ParquetFileDataSource {
    path: PathBuf,
}

impl ParquetFileDataSource {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn read_first_page(&self, limit: usize) -> Result<DataPage> {
        self.read_page(0, limit)
    }

    pub fn read_page(&self, offset: usize, limit: usize) -> Result<DataPage> {
        self.read_page_with_filter(offset, limit, None)
    }

    pub fn read_page_with_filter(
        &self,
        offset: usize,
        limit: usize,
        filter: Option<&str>,
    ) -> Result<DataPage> {
        let file = File::open(&self.path).map_err(|source| AppError::FileMetadata {
            path: self.path.clone(),
            source,
        })?;
        let builder = ParquetRecordBatchReaderBuilder::try_new(file).map_err(|source| {
            AppError::OpenParquet {
                path: self.path.clone(),
                source,
            }
        })?;
        let total_rows = if filter.is_some() {
            None
        } else {
            Some(builder.metadata().file_metadata().num_rows().max(0) as usize)
        };
        let schema = builder.schema();
        let columns = schema
            .fields()
            .iter()
            .enumerate()
            .map(|(index, field)| ColumnInfo {
                index,
                name: field.name().clone(),
                logical_type: field.data_type().to_string(),
                physical_type: None,
            })
            .collect::<Vec<_>>();

        let filter = filter
            .map(|expr| parse_filter(expr, &columns))
            .transpose()?;
        let mut reader = builder.with_batch_size(limit.max(1)).build()?;
        let mut rows = Vec::new();
        let mut skipped = 0usize;
        while rows.len() < limit {
            let Some(batch) = reader.next().transpose()? else {
                break;
            };
            append_batch_rows(
                &batch,
                &mut rows,
                offset,
                &mut skipped,
                limit,
                filter.as_ref(),
            );
        }

        Ok(DataPage {
            columns,
            rows,
            offset,
            total_rows,
        })
    }

    /// Count rows matching the optional filter.
    ///
    /// Without a filter this is the metadata `num_rows` (cheap). With a filter
    /// every row is scanned and matched against the formatted cell values, so
    /// it can be expensive for large files; callers should surface this as an
    /// on-demand action rather than on every page load.
    pub fn count_with_filter(&self, filter: Option<&str>) -> Result<usize> {
        let file = File::open(&self.path).map_err(|source| AppError::FileMetadata {
            path: self.path.clone(),
            source,
        })?;
        let builder = ParquetRecordBatchReaderBuilder::try_new(file).map_err(|source| {
            AppError::OpenParquet {
                path: self.path.clone(),
                source,
            }
        })?;
        let schema = builder.schema();
        let columns = schema
            .fields()
            .iter()
            .enumerate()
            .map(|(index, field)| ColumnInfo {
                index,
                name: field.name().clone(),
                logical_type: field.data_type().to_string(),
                physical_type: None,
            })
            .collect::<Vec<_>>();

        let Some(filter) = filter
            .map(|expr| parse_filter(expr, &columns))
            .transpose()?
        else {
            return Ok(builder.metadata().file_metadata().num_rows().max(0) as usize);
        };

        let mut reader = builder.with_batch_size(1024).build()?;
        let mut count = 0usize;
        while let Some(batch) = reader.next().transpose()? {
            for row_index in 0..batch.num_rows() {
                let cells = batch
                    .columns()
                    .iter()
                    .map(|array| format_cell(Arc::clone(array), row_index))
                    .collect::<Vec<_>>();
                if filter.matches(&cells) {
                    count += 1;
                }
            }
        }
        Ok(count)
    }

    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn validate_parquet_readable(path: &Path) -> Result<()> {
    let file = File::open(path).map_err(|source| AppError::FileMetadata {
        path: path.to_path_buf(),
        source,
    })?;
    let _ = SerializedFileReader::new(file).map_err(|source| AppError::OpenParquet {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

fn append_batch_rows(
    batch: &RecordBatch,
    rows: &mut Vec<RowView>,
    offset: usize,
    skipped: &mut usize,
    limit: usize,
    filter: Option<&FilterAst>,
) {
    for row_index in 0..batch.num_rows() {
        if rows.len() >= limit {
            break;
        }
        let cells = batch
            .columns()
            .iter()
            .map(|array| format_cell(Arc::clone(array), row_index))
            .collect::<Vec<_>>();
        if filter.is_some_and(|filter| !filter.matches(&cells)) {
            continue;
        }
        if *skipped < offset {
            *skipped += 1;
            continue;
        }
        rows.push(RowView { cells });
    }
}

#[derive(Debug, Clone, Copy)]
enum FilterOp {
    Eq,
    NotEq,
    Gt,
    Gte,
    Lt,
    Lte,
    Contains,
}

#[derive(Debug, Clone)]
struct FilterExpr {
    column_index: usize,
    op: FilterOp,
    value: String,
}

impl FilterExpr {
    fn matches(&self, cells: &[CellView]) -> bool {
        let Some(cell) = cells.get(self.column_index) else {
            return false;
        };
        let left = cell.detail.as_str();
        match self.op {
            FilterOp::Eq => {
                compare_string_or_number(left, &self.value, |ord| ord == std::cmp::Ordering::Equal)
            }
            FilterOp::NotEq => {
                !compare_string_or_number(left, &self.value, |ord| ord == std::cmp::Ordering::Equal)
            }
            FilterOp::Gt => compare_string_or_number(left, &self.value, |ord| {
                ord == std::cmp::Ordering::Greater
            }),
            FilterOp::Gte => compare_string_or_number(left, &self.value, |ord| {
                matches!(ord, std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
            }),
            FilterOp::Lt => {
                compare_string_or_number(left, &self.value, |ord| ord == std::cmp::Ordering::Less)
            }
            FilterOp::Lte => compare_string_or_number(left, &self.value, |ord| {
                matches!(ord, std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
            }),
            FilterOp::Contains => left.to_lowercase().contains(&self.value.to_lowercase()),
        }
    }
}

/// Boolean combination of predicates. `Or` binds looser than `And`, matching
/// the precedence of the textual operators.
enum FilterAst {
    Predicate(FilterExpr),
    And(Box<FilterAst>, Box<FilterAst>),
    Or(Box<FilterAst>, Box<FilterAst>),
}

impl FilterAst {
    fn matches(&self, cells: &[CellView]) -> bool {
        match self {
            FilterAst::Predicate(predicate) => predicate.matches(cells),
            FilterAst::And(left, right) => left.matches(cells) && right.matches(cells),
            FilterAst::Or(left, right) => left.matches(cells) || right.matches(cells),
        }
    }
}

fn compare_string_or_number(
    left: &str,
    right: &str,
    predicate: impl FnOnce(std::cmp::Ordering) -> bool,
) -> bool {
    match (left.parse::<f64>(), right.parse::<f64>()) {
        (Ok(left), Ok(right)) => left.partial_cmp(&right).is_some_and(predicate),
        _ => predicate(left.cmp(right)),
    }
}

/// Split `expr` on `or` / `and` (whitespace surrounded) without breaking
/// quoted substrings such as `note contains "A and B"`.
fn parse_filter(expr: &str, columns: &[ColumnInfo]) -> Result<FilterAst> {
    let expr = expr.trim();
    if expr.is_empty() {
        return Err(AppError::InvalidFilter("empty expression".to_string()));
    }
    parse_or(expr, columns)
}

fn parse_or(expr: &str, columns: &[ColumnInfo]) -> Result<FilterAst> {
    let parts = split_top_level(expr, " or ");
    let mut ast = parse_and(parts[0].trim(), columns)?;
    for part in &parts[1..] {
        let right = parse_and(part.trim(), columns)?;
        ast = FilterAst::Or(Box::new(ast), Box::new(right));
    }
    Ok(ast)
}

fn parse_and(expr: &str, columns: &[ColumnInfo]) -> Result<FilterAst> {
    let parts = split_top_level(expr, " and ");
    let mut ast = parse_predicate(parts[0].trim(), columns)?;
    for part in &parts[1..] {
        let right = parse_predicate(part.trim(), columns)?;
        ast = FilterAst::And(Box::new(ast), Box::new(right));
    }
    Ok(ast)
}

fn parse_predicate(expr: &str, columns: &[ColumnInfo]) -> Result<FilterAst> {
    let (column, op, value) = split_filter(expr)?;
    let column_index = columns
        .iter()
        .position(|info| info.name == column)
        .ok_or_else(|| AppError::InvalidFilter(format!("unknown column '{column}'")))?;
    Ok(FilterAst::Predicate(FilterExpr {
        column_index,
        op,
        value: unquote_filter_value(value.trim()),
    }))
}

/// Split on `delimiter` (e.g. `" or "`) only at positions outside of single
/// or double quotes.
fn split_top_level(expr: &str, delimiter: &str) -> Vec<String> {
    let chars: Vec<(usize, char)> = expr.char_indices().collect();
    let mut parts = Vec::new();
    let mut start = 0;
    let mut in_quote: Option<char> = None;
    let mut index = 0;
    while index < chars.len() {
        let (byte_index, ch) = chars[index];
        match in_quote {
            None => {
                if ch == '\'' || ch == '"' {
                    in_quote = Some(ch);
                    index += 1;
                    continue;
                }
                if expr[byte_index..].starts_with(delimiter) {
                    parts.push(expr[start..byte_index].to_string());
                    start = byte_index + delimiter.len();
                    index += delimiter.chars().count();
                    continue;
                }
                index += 1;
            }
            Some(quote) => {
                if ch == quote {
                    in_quote = None;
                }
                index += 1;
            }
        }
    }
    parts.push(expr[start..].to_string());
    parts
}

fn split_filter(expr: &str) -> Result<(&str, FilterOp, &str)> {
    for op_text in [" contains ", " >= ", " <= ", " != ", " = ", " > ", " < "] {
        if let Some(index) = expr.find(op_text) {
            let column = expr[..index].trim();
            let value = expr[index + op_text.len()..].trim();
            if column.is_empty() || value.is_empty() {
                return Err(AppError::InvalidFilter(
                    "expected: column op value".to_string(),
                ));
            }
            let op = match op_text.trim() {
                "=" => FilterOp::Eq,
                "!=" => FilterOp::NotEq,
                ">" => FilterOp::Gt,
                ">=" => FilterOp::Gte,
                "<" => FilterOp::Lt,
                "<=" => FilterOp::Lte,
                "contains" => FilterOp::Contains,
                _ => unreachable!(),
            };
            return Ok((column, op, value));
        }
    }
    Err(AppError::InvalidFilter(
        "expected: column op value; operators: = != > >= < <= contains".to_string(),
    ))
}

fn unquote_filter_value(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
            || (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
        {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}
#[cfg(test)]
mod tests {
    use super::*;

    fn columns(names: &[&str]) -> Vec<ColumnInfo> {
        names
            .iter()
            .enumerate()
            .map(|(index, name)| ColumnInfo {
                index,
                name: name.to_string(),
                logical_type: "Utf8".to_string(),
                physical_type: None,
            })
            .collect()
    }

    fn cell(detail: &str) -> CellView {
        CellView::new(detail.to_string())
    }

    fn row(cells: &[&str]) -> RowView {
        RowView {
            cells: cells.iter().map(|value| cell(value)).collect(),
        }
    }

    #[test]
    fn split_basic_expression() {
        let (column, op, value) = split_filter("score > 80").unwrap();
        assert_eq!(column, "score");
        assert_eq!(value, "80");
        assert!(matches!(op, FilterOp::Gt));
    }

    #[test]
    fn split_recognizes_all_operators() {
        assert!(matches!(split_filter("a = 1").unwrap().1, FilterOp::Eq));
        assert!(matches!(split_filter("a != 1").unwrap().1, FilterOp::NotEq));
        assert!(matches!(split_filter("a > 1").unwrap().1, FilterOp::Gt));
        assert!(matches!(split_filter("a >= 1").unwrap().1, FilterOp::Gte));
        assert!(matches!(split_filter("a < 1").unwrap().1, FilterOp::Lt));
        assert!(matches!(split_filter("a <= 1").unwrap().1, FilterOp::Lte));
        assert!(matches!(
            split_filter("a contains x").unwrap().1,
            FilterOp::Contains
        ));
    }

    #[test]
    fn split_requires_column_and_value() {
        assert!(split_filter("score >").is_err());
        assert!(split_filter("score").is_err());
        assert!(split_filter("= 1").is_err());
        assert!(split_filter("").is_err());
    }

    #[test]
    fn unquote_strips_surrounding_quotes() {
        assert_eq!(unquote_filter_value("\"上海\""), "上海");
        assert_eq!(unquote_filter_value("'东京'"), "东京");
        assert_eq!(unquote_filter_value("上海"), "上海");
        assert_eq!(unquote_filter_value("ab"), "ab");
    }

    #[test]
    fn parse_unknown_column_errors() {
        let cols = columns(&["city", "score"]);
        assert!(parse_filter("score > 80", &cols).is_ok());
        assert!(parse_filter("missing > 80", &cols).is_err());
        assert!(parse_filter("score", &cols).is_err());
        assert!(parse_filter("score >", &cols).is_err());
    }

    #[test]
    fn matches_numeric_comparison() {
        let cols = columns(&["row_id", "score", "city", "note"]);
        let expr = parse_filter("score > 80", &cols).unwrap();
        assert!(expr.matches(&row(&["", "98.5", "", ""]).cells));
        assert!(!expr.matches(&row(&["", "80", "", ""]).cells));
        assert!(!expr.matches(&row(&["", "50", "", ""]).cells));
    }

    #[test]
    fn matches_contains_is_case_insensitive() {
        let cols = columns(&["row_id", "score", "city", "note"]);
        let expr = parse_filter("note contains TEST", &cols).unwrap();
        assert!(expr.matches(&row(&["", "", "", "test row"]).cells));
        assert!(!expr.matches(&row(&["", "", "", "other"]).cells));
    }

    #[test]
    fn matches_string_equality() {
        let cols = columns(&["row_id", "score", "city", "note"]);
        let expr = parse_filter("city = 上海", &cols).unwrap();
        assert!(expr.matches(&row(&["", "", "上海", ""]).cells));
        assert!(!expr.matches(&row(&["", "", "东京", ""]).cells));
    }

    #[test]
    fn matches_and_combines_predicates() {
        let cols = columns(&["row_id", "score", "city", "active"]);
        let expr = parse_filter("score > 80 and active = true", &cols).unwrap();
        assert!(expr.matches(&row(&["", "90", "", "true"]).cells));
        assert!(!expr.matches(&row(&["", "90", "", "false"]).cells));
        assert!(!expr.matches(&row(&["", "50", "", "true"]).cells));
    }

    #[test]
    fn matches_or_combines_predicates() {
        let cols = columns(&["row_id", "score", "city", "active"]);
        let expr = parse_filter("city = 上海 or city = 東京", &cols).unwrap();
        assert!(expr.matches(&row(&["", "", "上海", ""]).cells));
        assert!(expr.matches(&row(&["", "", "東京", ""]).cells));
        assert!(!expr.matches(&row(&["", "", "北京", ""]).cells));
    }

    #[test]
    fn and_binds_tighter_than_or() {
        let cols = columns(&["row_id", "score", "city", "active"]);
        // (score > 90 or score < 10) and active = true
        let expr = parse_filter("score > 90 or score < 10 and active = true", &cols).unwrap();
        // high score alone should match (it is on the or side, unconstrained)
        assert!(expr.matches(&row(&["", "95", "", "false"]).cells));
        // low score requires active = true
        assert!(expr.matches(&row(&["", "5", "", "true"]).cells));
        assert!(!expr.matches(&row(&["", "5", "", "false"]).cells));
    }

    #[test]
    fn quoted_operator_text_is_not_split() {
        let cols = columns(&["row_id", "score", "city", "note"]);
        let expr = parse_filter("note contains \"A and B\"", &cols).unwrap();
        assert!(expr.matches(&row(&["", "", "", "x A and B y"]).cells));
        assert!(!expr.matches(&row(&["", "", "", "other"]).cells));
    }

    #[test]
    fn complex_types_render_without_panicking() {
        // Exercises Date/Time/Timestamp/Decimal/Dictionary/FixedSizeBinary for
        // every row and column; any formatting panic surfaces here.
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("parquet/complex_types.parquet");
        if !path.exists() {
            return;
        }
        let source = ParquetFileDataSource::new(path);
        let page = source.read_first_page(100).unwrap();
        assert!(!page.columns.is_empty());
        assert!(!page.rows.is_empty());
        for row in &page.rows {
            for cell in &row.cells {
                assert!(!cell.display.contains("panic"));
            }
        }
    }

    #[test]
    fn count_with_filter_counts_matching_rows() {
        use arrow_array::{Int32Array, RecordBatch, StringArray};
        use arrow_schema::{DataType, Field, Schema};
        use parquet::arrow::ArrowWriter;
        use std::sync::Arc;

        let schema = Arc::new(Schema::new(vec![
            Field::new("score", DataType::Int32, false),
            Field::new("city", DataType::Utf8, false),
        ]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(Int32Array::from(vec![90, 40, 85, 10])),
                Arc::new(StringArray::from(vec!["a", "b", "a", "c"])),
            ],
        )
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("count.parquet");
        let file = File::create(&path).unwrap();
        let mut writer = ArrowWriter::try_new(file, schema, None).unwrap();
        writer.write(&batch).unwrap();
        writer.close().unwrap();

        let source = ParquetFileDataSource::new(path);
        assert_eq!(source.count_with_filter(None).unwrap(), 4);
        assert_eq!(source.count_with_filter(Some("score > 80")).unwrap(), 2);
        assert_eq!(source.count_with_filter(Some("city = a")).unwrap(), 2);
        assert_eq!(source.count_with_filter(Some("score > 999")).unwrap(), 0);
    }

    #[test]
    fn matches_missing_cell_is_false() {
        let cols = columns(&["a", "b"]);
        let expr = parse_filter("a = x", &cols).unwrap();
        let short = RowView {
            cells: vec![cell("y")],
        };
        assert!(!expr.matches(&short.cells));
    }
}
