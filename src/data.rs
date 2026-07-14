use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};

use arrow_array::{Array, ArrayRef, RecordBatch, cast::AsArray, types::*};
use arrow_schema::DataType;
use parquet::{
    arrow::arrow_reader::ParquetRecordBatchReaderBuilder, file::reader::SerializedFileReader,
};

use crate::{
    error::{AppError, Result},
    formatting::truncate_to_width,
};

#[derive(Debug, Clone)]
pub struct ColumnInfo {
    pub index: usize,
    pub name: String,
    pub logical_type: String,
    pub physical_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DataPage {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<RowView>,
    pub offset: usize,
    pub total_rows: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct RowView {
    pub cells: Vec<CellView>,
}

#[derive(Debug, Clone)]
pub struct CellView {
    pub display: String,
    pub detail: String,
}

impl CellView {
    fn new(value: String) -> Self {
        Self {
            display: value.clone(),
            detail: value,
        }
    }

    fn complex(display: String, detail: String) -> Self {
        Self { display, detail }
    }
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
        let total_rows = Some(builder.metadata().file_metadata().num_rows().max(0) as usize);
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

        let mut reader = builder.with_batch_size(limit.max(1)).build()?;
        let mut rows = Vec::new();
        let mut skipped = 0usize;
        while rows.len() < limit {
            let Some(batch) = reader.next().transpose()? else {
                break;
            };
            append_batch_rows(&batch, &mut rows, offset, &mut skipped, limit);
        }

        Ok(DataPage {
            columns,
            rows,
            offset,
            total_rows,
        })
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
) {
    for row_index in 0..batch.num_rows() {
        if rows.len() >= limit {
            break;
        }
        if *skipped < offset {
            *skipped += 1;
            continue;
        }
        let cells = batch
            .columns()
            .iter()
            .map(|array| format_cell(Arc::clone(array), row_index))
            .collect();
        rows.push(RowView { cells });
    }
}

fn format_cell(array: ArrayRef, row_index: usize) -> CellView {
    if array.is_null(row_index) {
        return CellView::new("NULL".to_string());
    }

    match array.data_type() {
        DataType::List(_) => format_list_cell(array.as_list::<i32>().value(row_index)),
        DataType::LargeList(_) => format_list_cell(array.as_list::<i64>().value(row_index)),
        DataType::FixedSizeList(_, _) => {
            format_list_cell(array.as_fixed_size_list().value(row_index))
        }
        DataType::Struct(_) => format_struct_cell(array.as_struct(), row_index),
        DataType::Map(_, _) => format_map_cell(array.as_map().value(row_index)),
        _ => CellView::new(format_scalar(array, row_index)),
    }
}

fn format_scalar(array: ArrayRef, row_index: usize) -> String {
    if array.is_null(row_index) {
        return "null".to_string();
    }

    match array.data_type() {
        DataType::Boolean => array.as_boolean().value(row_index).to_string(),
        DataType::Int8 => primitive_value::<Int8Type>(&array, row_index),
        DataType::Int16 => primitive_value::<Int16Type>(&array, row_index),
        DataType::Int32 => primitive_value::<Int32Type>(&array, row_index),
        DataType::Int64 => primitive_value::<Int64Type>(&array, row_index),
        DataType::UInt8 => primitive_value::<UInt8Type>(&array, row_index),
        DataType::UInt16 => primitive_value::<UInt16Type>(&array, row_index),
        DataType::UInt32 => primitive_value::<UInt32Type>(&array, row_index),
        DataType::UInt64 => primitive_value::<UInt64Type>(&array, row_index),
        DataType::Float32 => primitive_value::<Float32Type>(&array, row_index),
        DataType::Float64 => primitive_value::<Float64Type>(&array, row_index),
        DataType::Utf8 => array.as_string::<i32>().value(row_index).to_string(),
        DataType::LargeUtf8 => array.as_string::<i64>().value(row_index).to_string(),
        DataType::Binary => binary_summary(array.as_binary::<i32>().value(row_index)),
        DataType::LargeBinary => binary_summary(array.as_binary::<i64>().value(row_index)),
        DataType::List(_) => format_list_inline(array.as_list::<i32>().value(row_index), 0),
        DataType::LargeList(_) => format_list_inline(array.as_list::<i64>().value(row_index), 0),
        DataType::FixedSizeList(_, _) => {
            format_list_inline(array.as_fixed_size_list().value(row_index), 0)
        }
        DataType::Struct(_) => format_struct_inline(array.as_struct(), row_index, 0),
        DataType::Map(_, _) => format_map_inline(array.as_map().value(row_index), 0),
        _ => format!("{:?}", array.slice(row_index, 1)),
    }
}

fn format_list_cell(values: ArrayRef) -> CellView {
    let display = truncate_to_width(&format_list_inline(Arc::clone(&values), 0), 80);
    let detail = format_list_pretty(values, 0);
    CellView::complex(display, detail)
}

fn format_struct_cell(array: &arrow_array::StructArray, row_index: usize) -> CellView {
    let display = truncate_to_width(&format_struct_inline(array, row_index, 0), 80);
    let detail = format_struct_pretty(array, row_index, 0);
    CellView::complex(display, detail)
}

fn format_map_cell(entries: arrow_array::StructArray) -> CellView {
    let display = truncate_to_width(&format_map_inline(entries.clone(), 0), 80);
    let detail = format_map_pretty(entries, 0);
    CellView::complex(display, detail)
}

fn format_value_inline(array: ArrayRef, row_index: usize, depth: usize) -> String {
    if array.is_null(row_index) {
        return "NULL".to_string();
    }
    if depth > 4 {
        return "…".to_string();
    }

    match array.data_type() {
        DataType::List(_) => format_list_inline(array.as_list::<i32>().value(row_index), depth),
        DataType::LargeList(_) => {
            format_list_inline(array.as_list::<i64>().value(row_index), depth)
        }
        DataType::FixedSizeList(_, _) => {
            format_list_inline(array.as_fixed_size_list().value(row_index), depth)
        }
        DataType::Struct(_) => format_struct_inline(array.as_struct(), row_index, depth),
        DataType::Map(_, _) => format_map_inline(array.as_map().value(row_index), depth),
        DataType::Utf8 => quote_string(array.as_string::<i32>().value(row_index)),
        DataType::LargeUtf8 => quote_string(array.as_string::<i64>().value(row_index)),
        _ => format_scalar(array, row_index),
    }
}

fn format_list_inline(values: ArrayRef, depth: usize) -> String {
    if depth > 4 {
        return "[…]".to_string();
    }
    let max_items = 6;
    let mut parts = Vec::new();
    for index in 0..values.len().min(max_items) {
        parts.push(format_value_inline(Arc::clone(&values), index, depth + 1));
    }
    if values.len() > max_items {
        parts.push("…".to_string());
    }
    format!("[{}]", parts.join(", "))
}

fn format_list_pretty(values: ArrayRef, indent: usize) -> String {
    if values.is_empty() {
        return "[]".to_string();
    }
    let pad = "  ".repeat(indent);
    let child_pad = "  ".repeat(indent + 1);
    let mut out = String::from("[\n");
    for index in 0..values.len() {
        out.push_str(&child_pad);
        out.push_str(&format_value_pretty(Arc::clone(&values), index, indent + 1));
        if index + 1 < values.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str(&pad);
    out.push(']');
    out
}

fn quote_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| format!("\"{value}\""))
}

fn quote_field_name(value: &str) -> String {
    quote_string(value)
}

fn binary_summary(bytes: &[u8]) -> String {
    format!("<{} bytes>", bytes.len())
}

fn binary_detail(bytes: &[u8], indent: usize) -> String {
    let pad = "  ".repeat(indent);
    let mut hex = String::new();
    for byte in bytes.iter().take(128) {
        hex.push_str(&format!("{byte:02x}"));
    }
    if bytes.len() > 128 {
        hex.push('…');
    }

    format!(
        "{{\n{pad}  \"type\": \"binary\",\n{pad}  \"length\": {},\n{pad}  \"hex\": {}\n{pad}}}",
        bytes.len(),
        quote_string(&hex),
    )
}

fn format_scalar_detail(array: ArrayRef, row_index: usize, indent: usize) -> String {
    if array.is_null(row_index) {
        return "null".to_string();
    }

    match array.data_type() {
        DataType::Utf8 => quote_string(array.as_string::<i32>().value(row_index)),
        DataType::LargeUtf8 => quote_string(array.as_string::<i64>().value(row_index)),
        DataType::Binary => binary_detail(array.as_binary::<i32>().value(row_index), indent),
        DataType::LargeBinary => binary_detail(array.as_binary::<i64>().value(row_index), indent),
        _ => format_scalar(array, row_index),
    }
}

fn format_struct_inline(
    array: &arrow_array::StructArray,
    row_index: usize,
    depth: usize,
) -> String {
    if depth > 4 {
        return "{…}".to_string();
    }
    let fields = array.fields();
    let mut parts = Vec::new();
    for (index, field) in fields.iter().enumerate().take(6) {
        parts.push(format!(
            "{}: {}",
            quote_field_name(field.name()),
            format_value_inline(Arc::clone(array.column(index)), row_index, depth + 1)
        ));
    }
    if fields.len() > 6 {
        parts.push("…".to_string());
    }
    format!("{{{}}}", parts.join(", "))
}

fn format_struct_pretty(
    array: &arrow_array::StructArray,
    row_index: usize,
    indent: usize,
) -> String {
    let fields = array.fields();
    if fields.is_empty() {
        return "{}".to_string();
    }
    let pad = "  ".repeat(indent);
    let child_pad = "  ".repeat(indent + 1);
    let mut out = String::from("{\n");
    for (index, field) in fields.iter().enumerate() {
        out.push_str(&child_pad);
        out.push_str(&quote_field_name(field.name()));
        out.push_str(": ");
        out.push_str(&format_value_pretty(
            Arc::clone(array.column(index)),
            row_index,
            indent + 1,
        ));
        if index + 1 < fields.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str(&pad);
    out.push('}');
    out
}

fn format_map_inline(entries: arrow_array::StructArray, depth: usize) -> String {
    if depth > 4 {
        return "{…}".to_string();
    }
    if entries.num_columns() < 2 {
        return "{}".to_string();
    }
    let keys = Arc::clone(entries.column(0));
    let values = Arc::clone(entries.column(1));
    let max_items = 6;
    let mut parts = Vec::new();
    for index in 0..entries.len().min(max_items) {
        parts.push(format!(
            "{}: {}",
            format_scalar_detail(Arc::clone(&keys), index, depth + 1),
            format_value_inline(Arc::clone(&values), index, depth + 1)
        ));
    }
    if entries.len() > max_items {
        parts.push("…".to_string());
    }
    format!("{{{}}}", parts.join(", "))
}

fn format_map_pretty(entries: arrow_array::StructArray, indent: usize) -> String {
    if entries.num_columns() < 2 || entries.is_empty() {
        return "{}".to_string();
    }
    let keys = Arc::clone(entries.column(0));
    let values = Arc::clone(entries.column(1));
    let pad = "  ".repeat(indent);
    let child_pad = "  ".repeat(indent + 1);
    let mut out = String::from("{\n");
    for index in 0..entries.len() {
        out.push_str(&child_pad);
        out.push_str(&format_scalar_detail(Arc::clone(&keys), index, indent + 1));
        out.push_str(": ");
        out.push_str(&format_value_pretty(Arc::clone(&values), index, indent + 1));
        if index + 1 < entries.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str(&pad);
    out.push('}');
    out
}

fn format_value_pretty(array: ArrayRef, row_index: usize, indent: usize) -> String {
    if array.is_null(row_index) {
        return "null".to_string();
    }
    match array.data_type() {
        DataType::List(_) => format_list_pretty(array.as_list::<i32>().value(row_index), indent),
        DataType::LargeList(_) => {
            format_list_pretty(array.as_list::<i64>().value(row_index), indent)
        }
        DataType::FixedSizeList(_, _) => {
            format_list_pretty(array.as_fixed_size_list().value(row_index), indent)
        }
        DataType::Struct(_) => format_struct_pretty(array.as_struct(), row_index, indent),
        DataType::Map(_, _) => format_map_pretty(array.as_map().value(row_index), indent),
        _ => format_scalar_detail(array, row_index, indent),
    }
}

fn primitive_value<T>(array: &ArrayRef, row_index: usize) -> String
where
    T: ArrowPrimitiveType,
    T::Native: std::fmt::Display,
{
    array.as_primitive::<T>().value(row_index).to_string()
}
