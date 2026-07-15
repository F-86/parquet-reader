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
    formatting::{CellView, TypedValue, extract_typed_value, format_cell},
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

        let mut rows = Vec::new();
        if offset == 0 {
            // Fast path: read from the start without row-group seeking.
            let mut reader = builder.with_batch_size(limit.max(1)).build()?;
            let mut skipped = 0usize;
            while rows.len() < limit {
                let Some(batch) = reader.next().transpose()? else {
                    break;
                };
                append_batch_rows(&batch, &mut rows, 0, &mut skipped, limit, filter.as_ref());
            }
        } else {
            // Row-group aware paging: skip whole row groups entirely before
            // `offset`, then read only the groups that the [offset, offset+limit)
            // window covers. Within the first covered group we skip the
            // remaining in-group rows. This avoids streaming and discarding
            // every row before the requested window.
            let metadata = builder.metadata();
            let row_group_rows = (0..metadata.num_row_groups())
                .map(|index| metadata.row_group(index).num_rows() as usize)
                .collect::<Vec<_>>();

            let mut remaining = offset;
            let mut start_group = 0;
            while start_group < row_group_rows.len() && remaining >= row_group_rows[start_group] {
                remaining -= row_group_rows[start_group];
                start_group += 1;
            }

            if start_group < row_group_rows.len() {
                // Expand the covered range until it can supply `limit` rows
                // (or we run out of row groups).
                let mut end_group = start_group;
                let mut covered = row_group_rows[start_group] - remaining;
                while covered < limit && end_group + 1 < row_group_rows.len() {
                    end_group += 1;
                    covered += row_group_rows[end_group];
                }

                let mut reader = builder
                    .with_row_groups((start_group..end_group + 1).collect())
                    .with_batch_size(limit.max(1))
                    .build()?;
                let mut skipped = 0usize;
                while rows.len() < limit {
                    let Some(batch) = reader.next().transpose()? else {
                        break;
                    };
                    append_batch_rows(
                        &batch,
                        &mut rows,
                        remaining,
                        &mut skipped,
                        limit,
                        filter.as_ref(),
                    );
                }
            }
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

        let referenced = filter.referenced_columns();
        let num_columns = schema.fields().len();

        let mut reader = builder.with_batch_size(1024).build()?;
        let mut count = 0usize;
        while let Some(batch) = reader.next().transpose()? {
            let columns = batch.columns();
            for row_index in 0..batch.num_rows() {
                let mut typed = vec![TypedValue::Null; num_columns];
                for &col_idx in &referenced {
                    typed[col_idx] = extract_typed_value(&columns[col_idx], row_index);
                }
                if filter.matches_typed(&typed) {
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
    let columns = batch.columns();
    let num_columns = columns.len();
    // Pre-compute which columns the filter actually references so we
    // only extract typed values for those columns, leaving the rest as
    // Null.  This avoids needless extract_typed_value calls for columns
    // that the filter never inspects.
    let referenced: Vec<usize> = filter.map(|f| f.referenced_columns()).unwrap_or_default();

    for row_index in 0..batch.num_rows() {
        if rows.len() >= limit {
            break;
        }
        if let Some(filter) = filter {
            let mut typed = vec![TypedValue::Null; num_columns];
            for &col_idx in &referenced {
                typed[col_idx] = extract_typed_value(&columns[col_idx], row_index);
            }
            if !filter.matches_typed(&typed) {
                continue;
            }
        }
        // Only format cells for rows that survive the filter (or when no
        // filter is active).  Rows that are filtered out skip formatting
        // entirely.
        let cells: Vec<CellView> = columns
            .iter()
            .map(|array| format_cell(Arc::clone(array), row_index))
            .collect();
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
    fn matches_typed(&self, values: &[TypedValue]) -> bool {
        let Some(value) = values.get(self.column_index) else {
            return false;
        };
        match self.op {
            FilterOp::Eq => self.compare_typed(value, |ord| ord == std::cmp::Ordering::Equal),
            FilterOp::NotEq => !self.compare_typed(value, |ord| ord == std::cmp::Ordering::Equal),
            FilterOp::Gt => self.compare_typed(value, |ord| ord == std::cmp::Ordering::Greater),
            FilterOp::Gte => self.compare_typed(value, |ord| {
                matches!(ord, std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
            }),
            FilterOp::Lt => self.compare_typed(value, |ord| ord == std::cmp::Ordering::Less),
            FilterOp::Lte => self.compare_typed(value, |ord| {
                matches!(ord, std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
            }),
            FilterOp::Contains => self.contains_typed(value),
        }
    }

    /// Compare a typed cell value against the filter's string value.
    ///
    /// Numbers are compared numerically, booleans as booleans, and strings
    /// fall back to numeric-then-lexical comparison for backward
    /// compatibility.  Null cells never match comparison operators.
    fn compare_typed(
        &self,
        cell: &TypedValue,
        predicate: impl FnOnce(std::cmp::Ordering) -> bool,
    ) -> bool {
        match cell {
            TypedValue::Null => false,
            TypedValue::Boolean(b) => match self.value.to_lowercase().as_str() {
                "true" => predicate(b.cmp(&true)),
                "false" => predicate(b.cmp(&false)),
                _ => false,
            },
            TypedValue::Number(n) => self
                .value
                .parse::<f64>()
                .ok()
                .and_then(|f| n.partial_cmp(&f))
                .is_some_and(predicate),
            TypedValue::Str(s) => match (s.parse::<f64>(), self.value.parse::<f64>()) {
                (Ok(l), Ok(r)) => l.partial_cmp(&r).is_some_and(predicate),
                _ => predicate(s.as_str().cmp(&self.value)),
            },
            TypedValue::Other(s) => match (s.parse::<f64>(), self.value.parse::<f64>()) {
                (Ok(l), Ok(r)) => l.partial_cmp(&r).is_some_and(predicate),
                _ => predicate(s.as_str().cmp(&self.value)),
            },
        }
    }

    /// `contains` is always string-based: the cell's textual representation
    /// must contain the filter value (case-insensitive).
    fn contains_typed(&self, cell: &TypedValue) -> bool {
        let text = match cell {
            TypedValue::Null => return false,
            TypedValue::Boolean(b) => b.to_string(),
            TypedValue::Number(n) => n.to_string(),
            TypedValue::Str(s) => s.clone(),
            TypedValue::Other(s) => s.clone(),
        };
        text.to_lowercase().contains(&self.value.to_lowercase())
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
    fn matches_typed(&self, values: &[TypedValue]) -> bool {
        match self {
            FilterAst::Predicate(predicate) => predicate.matches_typed(values),
            FilterAst::And(left, right) => {
                left.matches_typed(values) && right.matches_typed(values)
            }
            FilterAst::Or(left, right) => left.matches_typed(values) || right.matches_typed(values),
        }
    }

    /// Return the deduplicated set of column indices referenced by this
    /// filter expression.  Only these columns need to be extracted for
    /// matching; all others can be left as `TypedValue::Null`.
    fn referenced_columns(&self) -> Vec<usize> {
        match self {
            FilterAst::Predicate(p) => vec![p.column_index],
            FilterAst::And(left, right) | FilterAst::Or(left, right) => {
                let mut cols = left.referenced_columns();
                cols.extend(right.referenced_columns());
                cols.sort_unstable();
                cols.dedup();
                cols
            }
        }
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
/// Sort rows in place by a column using its formatted `detail` value.
///
/// Comparison prefers numeric ordering when both cells parse as numbers, so
/// `10` sorts after `2`; otherwise it falls back to a locale-independent
/// string comparison. `ascending` toggles direction.
pub fn sort_rows_by_column(rows: &mut [RowView], column_index: usize, ascending: bool) {
    rows.sort_by(|left, right| {
        let left_value = left
            .cells
            .get(column_index)
            .map(|cell| cell.detail.as_str())
            .unwrap_or("");
        let right_value = right
            .cells
            .get(column_index)
            .map(|cell| cell.detail.as_str())
            .unwrap_or("");

        let ordering = compare_sort_keys(left_value, right_value);
        if ascending {
            ordering
        } else {
            ordering.reverse()
        }
    });
}

fn compare_sort_keys(left: &str, right: &str) -> std::cmp::Ordering {
    match (left.parse::<f64>(), right.parse::<f64>()) {
        (Ok(left), Ok(right)) => left
            .partial_cmp(&right)
            .unwrap_or(std::cmp::Ordering::Equal),
        _ => left.cmp(right),
    }
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

    fn typed_str(s: &str) -> TypedValue {
        TypedValue::Str(s.to_string())
    }

    fn typed_num(n: f64) -> TypedValue {
        TypedValue::Number(n)
    }

    fn typed_bool(b: bool) -> TypedValue {
        TypedValue::Boolean(b)
    }

    fn row(cells: &[&str]) -> RowView {
        RowView {
            cells: cells.iter().map(|value| cell(value)).collect(),
        }
    }

    fn typed_row(values: &[TypedValue]) -> Vec<TypedValue> {
        values.to_vec()
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
        assert!(expr.matches_typed(&typed_row(&[
            typed_str(""),
            typed_num(98.5),
            typed_str(""),
            typed_str("")
        ])));
        assert!(!expr.matches_typed(&typed_row(&[
            typed_str(""),
            typed_num(80.0),
            typed_str(""),
            typed_str("")
        ])));
        assert!(!expr.matches_typed(&typed_row(&[
            typed_str(""),
            typed_num(50.0),
            typed_str(""),
            typed_str("")
        ])));
    }

    #[test]
    fn matches_contains_is_case_insensitive() {
        let cols = columns(&["row_id", "score", "city", "note"]);
        let expr = parse_filter("note contains TEST", &cols).unwrap();
        assert!(expr.matches_typed(&typed_row(&[
            typed_str(""),
            typed_num(0.0),
            typed_str(""),
            typed_str("test row")
        ])));
        assert!(!expr.matches_typed(&typed_row(&[
            typed_str(""),
            typed_num(0.0),
            typed_str(""),
            typed_str("other")
        ])));
    }

    #[test]
    fn matches_string_equality() {
        let cols = columns(&["row_id", "score", "city", "note"]);
        let expr = parse_filter("city = 上海", &cols).unwrap();
        assert!(expr.matches_typed(&typed_row(&[
            typed_str(""),
            typed_num(0.0),
            typed_str("上海"),
            typed_str("")
        ])));
        assert!(!expr.matches_typed(&typed_row(&[
            typed_str(""),
            typed_num(0.0),
            typed_str("东京"),
            typed_str("")
        ])));
    }

    #[test]
    fn matches_and_combines_predicates() {
        let cols = columns(&["row_id", "score", "city", "active"]);
        let expr = parse_filter("score > 80 and active = true", &cols).unwrap();
        assert!(expr.matches_typed(&typed_row(&[
            typed_str(""),
            typed_num(90.0),
            typed_str(""),
            typed_bool(true)
        ])));
        assert!(!expr.matches_typed(&typed_row(&[
            typed_str(""),
            typed_num(90.0),
            typed_str(""),
            typed_bool(false)
        ])));
        assert!(!expr.matches_typed(&typed_row(&[
            typed_str(""),
            typed_num(50.0),
            typed_str(""),
            typed_bool(true)
        ])));
    }

    #[test]
    fn matches_or_combines_predicates() {
        let cols = columns(&["row_id", "score", "city", "active"]);
        let expr = parse_filter("city = 上海 or city = 東京", &cols).unwrap();
        assert!(expr.matches_typed(&typed_row(&[
            typed_str(""),
            typed_num(0.0),
            typed_str("上海"),
            typed_bool(false)
        ])));
        assert!(expr.matches_typed(&typed_row(&[
            typed_str(""),
            typed_num(0.0),
            typed_str("東京"),
            typed_bool(false)
        ])));
        assert!(!expr.matches_typed(&typed_row(&[
            typed_str(""),
            typed_num(0.0),
            typed_str("北京"),
            typed_bool(false)
        ])));
    }

    #[test]
    fn and_binds_tighter_than_or() {
        let cols = columns(&["row_id", "score", "city", "active"]);
        // (score > 90 or score < 10) and active = true
        let expr = parse_filter("score > 90 or score < 10 and active = true", &cols).unwrap();
        // high score alone should match (it is on the or side, unconstrained)
        assert!(expr.matches_typed(&typed_row(&[
            typed_str(""),
            typed_num(95.0),
            typed_str(""),
            typed_bool(false)
        ])));
        // low score requires active = true
        assert!(expr.matches_typed(&typed_row(&[
            typed_str(""),
            typed_num(5.0),
            typed_str(""),
            typed_bool(true)
        ])));
        assert!(!expr.matches_typed(&typed_row(&[
            typed_str(""),
            typed_num(5.0),
            typed_str(""),
            typed_bool(false)
        ])));
    }

    #[test]
    fn quoted_operator_text_is_not_split() {
        let cols = columns(&["row_id", "score", "city", "note"]);
        let expr = parse_filter("note contains \"A and B\"", &cols).unwrap();
        assert!(expr.matches_typed(&typed_row(&[
            typed_str(""),
            typed_num(0.0),
            typed_str(""),
            typed_str("x A and B y")
        ])));
        assert!(!expr.matches_typed(&typed_row(&[
            typed_str(""),
            typed_num(0.0),
            typed_str(""),
            typed_str("other")
        ])));
    }

    #[test]
    fn row_group_aware_pagination_matches_sequential_read() {
        use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
        use arrow_schema::{DataType, Field, Schema};
        use parquet::arrow::ArrowWriter;
        use std::sync::Arc;

        // Build a file with many small row groups so pagination must skip groups.
        let schema = Arc::new(Schema::new(vec![
            Field::new("row_id", DataType::Int64, false),
            Field::new("group", DataType::Utf8, false),
        ]));
        let total = 200usize;
        let row_ids = Int64Array::from_iter_values(1..=total as i64);
        let groups = StringArray::from(
            (1..=total)
                .map(|row| format!("group-{:02}", (row - 1) / 10))
                .collect::<Vec<_>>(),
        );
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(row_ids) as ArrayRef, Arc::new(groups) as ArrayRef],
        )
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rg.parquet");
        let file = File::create(&path).unwrap();
        let props = parquet::file::properties::WriterProperties::builder()
            .set_max_row_group_size(10)
            .build();
        let options =
            parquet::arrow::arrow_writer::ArrowWriterOptions::new().with_properties(props);
        let mut writer = ArrowWriter::try_new_with_options(file, schema, options).unwrap();
        writer.write(&batch).unwrap();
        writer.close().unwrap();

        let source = ParquetFileDataSource::new(path);
        let page_size = 25;
        let mut scanned = 0usize;
        while scanned < total {
            let page = source.read_page(scanned, page_size).unwrap();
            let expected: Vec<i64> =
                ((scanned + 1) as i64..=(scanned + page.rows.len()) as i64).collect();
            let got = page
                .rows
                .iter()
                .map(|row| row.cells[0].display.parse::<i64>().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(got, expected, "offset {scanned} page mismatch");
            assert_eq!(page.offset, scanned);
            scanned += page.rows.len();
            if page.rows.len() < page_size {
                break;
            }
        }
        assert_eq!(scanned, total);
    }

    #[test]
    fn sort_rows_by_column_orders_numerically_and_toggle_direction() {
        let mut rows = vec![
            RowView {
                cells: vec![CellView::new("3".to_string())],
            },
            RowView {
                cells: vec![CellView::new("10".to_string())],
            },
            RowView {
                cells: vec![CellView::new("2".to_string())],
            },
        ];
        sort_rows_by_column(&mut rows, 0, true);
        let order: Vec<&str> = rows.iter().map(|r| r.cells[0].detail.as_str()).collect();
        assert_eq!(order, vec!["2", "3", "10"]);

        sort_rows_by_column(&mut rows, 0, false);
        let order: Vec<&str> = rows.iter().map(|r| r.cells[0].detail.as_str()).collect();
        assert_eq!(order, vec!["10", "3", "2"]);
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
        let short = vec![typed_str("y")];
        assert!(!expr.matches_typed(&short));
    }

    #[test]
    fn typed_boolean_equality() {
        let cols = columns(&["active"]);
        let expr = parse_filter("active = true", &cols).unwrap();
        assert!(expr.matches_typed(&typed_row(&[typed_bool(true)])));
        assert!(!expr.matches_typed(&typed_row(&[typed_bool(false)])));
    }

    #[test]
    fn typed_null_never_matches_comparison() {
        let cols = columns(&["score"]);
        let expr = parse_filter("score > 50", &cols).unwrap();
        assert!(!expr.matches_typed(&typed_row(&[TypedValue::Null])));
    }

    #[test]
    fn typed_number_vs_non_numeric_value() {
        let cols = columns(&["score"]);
        let expr = parse_filter("score > abc", &cols).unwrap();
        // Non-numeric filter value against a number column: no match.
        assert!(!expr.matches_typed(&typed_row(&[typed_num(100.0)])));
    }

    #[test]
    fn typed_string_not_equal_to_number() {
        let cols = columns(&["name"]);
        let expr = parse_filter("name = 42", &cols).unwrap();
        // String column value "42" should still match numeric "42" via
        // the string fallback path that tries numeric comparison.
        assert!(expr.matches_typed(&typed_row(&[typed_str("42")])));
    }

    #[test]
    fn typed_contains_on_number() {
        let cols = columns(&["score"]);
        let expr = parse_filter("score contains 9", &cols).unwrap();
        assert!(expr.matches_typed(&typed_row(&[typed_num(98.5)])));
        assert!(!expr.matches_typed(&typed_row(&[typed_num(50.0)])));
    }

    #[test]
    fn referenced_columns_single_predicate() {
        let cols = columns(&["row_id", "score", "city"]);
        let expr = parse_filter("score > 80", &cols).unwrap();
        assert_eq!(expr.referenced_columns(), vec![1]);
    }

    #[test]
    fn referenced_columns_and_combines_and_deduplicates() {
        let cols = columns(&["row_id", "score", "city", "active"]);
        let expr = parse_filter("score > 80 and city = 上海", &cols).unwrap();
        assert_eq!(expr.referenced_columns(), vec![1, 2]);
    }

    #[test]
    fn referenced_columns_same_column_in_or_deduplicates() {
        let cols = columns(&["row_id", "score", "city"]);
        let expr = parse_filter("score > 80 or score < 10", &cols).unwrap();
        // Both predicates reference column index 1 ("score"); result should
        // be deduplicated to a single entry.
        assert_eq!(expr.referenced_columns(), vec![1]);
    }

    #[test]
    fn minimal_matching_skips_unreferenced_columns() {
        // When a filter only references column 1 ("score"), columns 0 and 2
        // are left as Null.  The filter should still match correctly because
        // it only inspects the referenced column.
        let cols = columns(&["row_id", "score", "city"]);
        let expr = parse_filter("score > 80", &cols).unwrap();
        let referenced = expr.referenced_columns();
        // Simulate the minimal extraction path: only extract column 1.
        let mut typed = vec![TypedValue::Null; 3];
        for &col_idx in &referenced {
            typed[col_idx] = typed_num(95.0);
        }
        assert!(expr.matches_typed(&typed));

        // Non-matching value
        for &col_idx in &referenced {
            typed[col_idx] = typed_num(50.0);
        }
        assert!(!expr.matches_typed(&typed));
    }
}
