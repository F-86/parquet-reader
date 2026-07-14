use std::sync::Arc;

use arrow_array::{Array, ArrayRef, cast::AsArray, types::*};
use arrow_buffer::i256;

use arrow_schema::{DataType, TimeUnit};
use serde_json;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// A rendered cell value: the short `display` shown in the table and the
/// fuller `detail` shown in the cell-detail popup.
#[derive(Debug, Clone)]
pub struct CellView {
    pub display: String,
    pub detail: String,
}

impl CellView {
    pub(crate) fn new(value: String) -> Self {
        Self {
            display: value.clone(),
            detail: value,
        }
    }

    pub(crate) fn complex(display: String, detail: String) -> Self {
        Self { display, detail }
    }
}

pub fn truncate_to_width(value: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(value) <= max_width {
        return value.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "\u{2026}".to_string();
    }

    let target = max_width - 1;
    let mut width = 0;
    let mut out = String::new();
    for ch in value.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if width + ch_width > target {
            break;
        }
        width += ch_width;
        out.push(ch);
    }
    out.push('\u{2026}');
    out
}

macro_rules! format_dict {
    ($array:expr, $row:expr, $kt:ty) => {{
        let dict = $array.as_dictionary::<$kt>();
        let raw = dict.keys().value($row) as i128;
        let index = if raw < 0 { 0 } else { raw as usize };
        format_scalar(Arc::clone(dict.values()), index)
    }};
}

pub fn format_cell(array: ArrayRef, row_index: usize) -> CellView {
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
        DataType::FixedSizeBinary(_) => {
            let bytes = array.as_fixed_size_binary().value(row_index);
            CellView::complex(binary_summary(bytes), binary_detail(bytes, 0))
        }
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
        DataType::Date32 => format_date32(array.as_primitive::<Date32Type>().value(row_index)),
        DataType::Date64 => format_date64(array.as_primitive::<Date64Type>().value(row_index)),
        DataType::Time32(unit) => match unit {
            TimeUnit::Second => {
                format_time32_second(array.as_primitive::<Time32SecondType>().value(row_index))
            }
            TimeUnit::Millisecond => format_time32_milli(
                array
                    .as_primitive::<Time32MillisecondType>()
                    .value(row_index),
            ),
            _ => format!("{:?}", array.slice(row_index, 1)),
        },
        DataType::Time64(unit) => match unit {
            TimeUnit::Microsecond => format_time64_micro(
                array
                    .as_primitive::<Time64MicrosecondType>()
                    .value(row_index),
            ),
            TimeUnit::Nanosecond => format_time64_nano(
                array
                    .as_primitive::<Time64NanosecondType>()
                    .value(row_index),
            ),
            _ => format!("{:?}", array.slice(row_index, 1)),
        },
        DataType::Timestamp(unit, tz) => {
            format_timestamp(timestamp_value(&array, row_index, unit), unit, tz)
        }
        DataType::Duration(unit) => format_duration(duration_value(&array, row_index, unit), unit),
        DataType::Decimal128(_, scale) => format_decimal_i128(
            array.as_primitive::<Decimal128Type>().value(row_index),
            *scale as u8,
        ),
        DataType::Decimal256(_, scale) => format_decimal_i256(
            array.as_primitive::<Decimal256Type>().value(row_index),
            *scale as u8,
        ),
        DataType::Dictionary(key_type, _) => match key_type.as_ref() {
            DataType::Int8 => format_dict!(array, row_index, Int8Type),
            DataType::Int16 => format_dict!(array, row_index, Int16Type),
            DataType::Int32 => format_dict!(array, row_index, Int32Type),
            DataType::Int64 => format_dict!(array, row_index, Int64Type),
            DataType::UInt8 => format_dict!(array, row_index, UInt8Type),
            DataType::UInt16 => format_dict!(array, row_index, UInt16Type),
            DataType::UInt32 => format_dict!(array, row_index, UInt32Type),
            DataType::UInt64 => format_dict!(array, row_index, UInt64Type),
            _ => "unsupported dictionary key".to_string(),
        },
        DataType::FixedSizeBinary(_) => {
            binary_summary(array.as_fixed_size_binary().value(row_index))
        }
        _ => format!("{:?}", array.slice(row_index, 1)),
    }
}

fn timestamp_value(array: &ArrayRef, row_index: usize, unit: &TimeUnit) -> i64 {
    match unit {
        TimeUnit::Second => array.as_primitive::<TimestampSecondType>().value(row_index),
        TimeUnit::Millisecond => array
            .as_primitive::<TimestampMillisecondType>()
            .value(row_index),
        TimeUnit::Microsecond => array
            .as_primitive::<TimestampMicrosecondType>()
            .value(row_index),
        TimeUnit::Nanosecond => array
            .as_primitive::<TimestampNanosecondType>()
            .value(row_index),
    }
}

fn duration_value(array: &ArrayRef, row_index: usize, unit: &TimeUnit) -> i64 {
    match unit {
        TimeUnit::Second => array.as_primitive::<DurationSecondType>().value(row_index),
        TimeUnit::Millisecond => array
            .as_primitive::<DurationMillisecondType>()
            .value(row_index),
        TimeUnit::Microsecond => array
            .as_primitive::<DurationMicrosecondType>()
            .value(row_index),
        TimeUnit::Nanosecond => array
            .as_primitive::<DurationNanosecondType>()
            .value(row_index),
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
        return "\u{2026}".to_string();
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
        return "[\u{2026}]".to_string();
    }
    let max_items = 6;
    let mut parts = Vec::new();
    for index in 0..values.len().min(max_items) {
        parts.push(format_value_inline(Arc::clone(&values), index, depth + 1));
    }
    if values.len() > max_items {
        parts.push("\u{2026}".to_string());
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
        hex.push('\u{2026}');
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
        DataType::FixedSizeBinary(_) => {
            binary_detail(array.as_fixed_size_binary().value(row_index), indent)
        }
        _ => format_scalar(array, row_index),
    }
}

fn format_struct_inline(
    array: &arrow_array::StructArray,
    row_index: usize,
    depth: usize,
) -> String {
    if depth > 4 {
        return "{...}".to_string();
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
        parts.push("\u{2026}".to_string());
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
        return "{...}".to_string();
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
        parts.push("\u{2026}".to_string());
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

/// Convert a count of days since 1970-01-01 into a (year, month, day) triple
/// using the standard proleptic Gregorian algorithm.
fn date_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

fn format_date32(days: i32) -> String {
    let (y, m, d) = date_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

fn format_date64(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let rem = (ms.rem_euclid(86_400_000)) as u64;
    let (y, m, d) = date_from_days(days);
    let h = (rem / 3_600_000) as u32;
    let rem = rem % 3_600_000;
    let min = (rem / 60_000) as u32;
    let rem = rem % 60_000;
    let s = (rem / 1000) as u32;
    let ms_part = (rem % 1000) as u32;
    format!("{y:04}-{m:02}-{d:02} {h:02}:{min:02}:{s:02}.{ms_part:03}")
}

fn time_of_day_string(rem: u64, width: usize) -> String {
    let h = (rem / 3_600_000_000_000) as u32;
    let rem = rem % 3_600_000_000_000;
    let min = (rem / 60_000_000_000) as u32;
    let rem = rem % 60_000_000_000;
    let s = (rem / 1_000_000_000) as u32;
    let frac = rem % 1_000_000_000;
    if width == 0 {
        format!("{h:02}:{min:02}:{s:02}")
    } else {
        format!("{h:02}:{min:02}:{s:02}.{:0width$}", frac, width = width)
    }
}

fn format_time32_second(sec: i32) -> String {
    time_of_day_string(sec.unsigned_abs() as u64 * 1_000_000_000, 0)
}

fn format_time32_milli(ms: i32) -> String {
    time_of_day_string(ms.unsigned_abs() as u64 * 1_000_000, 3)
}

fn format_time64_micro(us: i64) -> String {
    time_of_day_string(us.unsigned_abs() as u64 * 1000, 6)
}

fn format_time64_nano(ns: i64) -> String {
    time_of_day_string(ns.unsigned_abs() as u64, 9)
}

fn format_timestamp(epoch: i64, unit: &TimeUnit, tz: &Option<std::sync::Arc<str>>) -> String {
    let (secs, nanos) = match unit {
        TimeUnit::Second => (epoch, 0),
        TimeUnit::Millisecond => (
            epoch.div_euclid(1000),
            (epoch.rem_euclid(1000) as u32) * 1_000_000,
        ),
        TimeUnit::Microsecond => (
            epoch.div_euclid(1_000_000),
            (epoch.rem_euclid(1_000_000) as u32) * 1000,
        ),
        TimeUnit::Nanosecond => (
            epoch.div_euclid(1_000_000_000),
            epoch.rem_euclid(1_000_000_000) as u32,
        ),
    };
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400) as u64;
    let (y, m, d) = date_from_days(days);
    let h = (rem / 3600) as u32;
    let rem = rem % 3600;
    let min = (rem / 60) as u32;
    let s = (rem % 60) as u32;
    let tz_label = match tz {
        Some(name) if name.as_ref() != "UTC" && name.as_ref() != "+00:00" => {
            format!(" ({name})")
        }
        _ => String::new(),
    };
    format!("{y:04}-{m:02}-{d:02} {h:02}:{min:02}:{s:02}.{nanos:09}{tz_label}")
}

fn format_duration(value: i64, unit: &TimeUnit) -> String {
    let suffix = match unit {
        TimeUnit::Second => "s",
        TimeUnit::Millisecond => "ms",
        TimeUnit::Microsecond => "us",
        TimeUnit::Nanosecond => "ns",
    };
    format!("{value}{suffix}")
}

fn format_decimal_i128(value: i128, scale: u8) -> String {
    let scale = scale as usize;
    if scale == 0 {
        return value.to_string();
    }
    let s = value.to_string();
    if s.len() <= scale {
        let sign = if s.starts_with('-') { "-" } else { "" };
        let digits = if sign.is_empty() {
            s.clone()
        } else {
            s[1..].to_string()
        };
        let mut padded = digits;
        while padded.len() < scale {
            padded.insert(0, '0');
        }
        format!("{sign}0.{padded}")
    } else {
        let split = s.len() - scale;
        format!("{}.{}", &s[..split], &s[split..])
    }
}

fn format_decimal_i256(value: i256, scale: u8) -> String {
    let scale = scale as usize;
    if scale == 0 {
        return value.to_string();
    }
    let s = value.to_string();
    if s.len() <= scale {
        let sign = if s.starts_with('-') { "-" } else { "" };
        let digits = if sign.is_empty() {
            s.clone()
        } else {
            s[1..].to_string()
        };
        let mut padded = digits;
        while padded.len() < scale {
            padded.insert(0, '0');
        }
        format!("{sign}0.{padded}")
    } else {
        let split = s.len() - scale;
        format!("{}.{}", &s[..split], &s[split..])
    }
}

fn primitive_value<T>(array: &ArrayRef, row_index: usize) -> String
where
    T: ArrowPrimitiveType,
    T::Native: std::fmt::Display,
{
    array.as_primitive::<T>().value(row_index).to_string()
}

// ── Typed value extraction for filter comparison ──

/// A typed value extracted from an Arrow array for structured comparison.
///
/// Used by the filter layer to compare cell values by their native type
/// (number vs number, bool vs bool) rather than their formatted string.
#[derive(Debug, Clone)]
pub enum TypedValue {
    Null,
    Boolean(bool),
    Number(f64),
    Str(String),
    /// Fallback for complex types (List, Struct, Map, Binary, …): the
    /// formatted scalar string is used for comparison.
    Other(String),
}

macro_rules! typed_dict {
    ($array:expr, $row:expr, $kt:ty) => {{
        let dict = $array.as_dictionary::<$kt>();
        let raw = dict.keys().value($row) as i128;
        let index = if raw < 0 { 0 } else { raw as usize };
        let values = Arc::clone(dict.values());
        extract_typed_value(&values, index)
    }};
}

/// Extract a typed value from an Arrow array at `row_index` for filter
/// comparison.  This is the typed counterpart of [`format_cell`]: instead
/// of producing a display string it yields a value whose native Rust type
/// can be compared directly.
pub fn extract_typed_value(array: &ArrayRef, row_index: usize) -> TypedValue {
    if array.is_null(row_index) {
        return TypedValue::Null;
    }
    match array.data_type() {
        DataType::Boolean => TypedValue::Boolean(array.as_boolean().value(row_index)),
        DataType::Int8 => {
            TypedValue::Number(array.as_primitive::<Int8Type>().value(row_index) as f64)
        }
        DataType::Int16 => {
            TypedValue::Number(array.as_primitive::<Int16Type>().value(row_index) as f64)
        }
        DataType::Int32 => {
            TypedValue::Number(array.as_primitive::<Int32Type>().value(row_index) as f64)
        }
        DataType::Int64 => {
            TypedValue::Number(array.as_primitive::<Int64Type>().value(row_index) as f64)
        }
        DataType::UInt8 => {
            TypedValue::Number(array.as_primitive::<UInt8Type>().value(row_index) as f64)
        }
        DataType::UInt16 => {
            TypedValue::Number(array.as_primitive::<UInt16Type>().value(row_index) as f64)
        }
        DataType::UInt32 => {
            TypedValue::Number(array.as_primitive::<UInt32Type>().value(row_index) as f64)
        }
        DataType::UInt64 => {
            TypedValue::Number(array.as_primitive::<UInt64Type>().value(row_index) as f64)
        }
        DataType::Float32 => {
            TypedValue::Number(array.as_primitive::<Float32Type>().value(row_index) as f64)
        }
        DataType::Float64 => {
            TypedValue::Number(array.as_primitive::<Float64Type>().value(row_index) as f64)
        }
        DataType::Date32 => {
            TypedValue::Number(array.as_primitive::<Date32Type>().value(row_index) as f64)
        }
        DataType::Date64 => {
            TypedValue::Number(array.as_primitive::<Date64Type>().value(row_index) as f64)
        }
        DataType::Time32(unit) => match unit {
            TimeUnit::Second => {
                TypedValue::Number(array.as_primitive::<Time32SecondType>().value(row_index) as f64)
            }
            TimeUnit::Millisecond => TypedValue::Number(
                array
                    .as_primitive::<Time32MillisecondType>()
                    .value(row_index) as f64,
            ),
            _ => TypedValue::Other(format_scalar(Arc::clone(array), row_index)),
        },
        DataType::Time64(unit) => match unit {
            TimeUnit::Microsecond => TypedValue::Number(
                array
                    .as_primitive::<Time64MicrosecondType>()
                    .value(row_index) as f64,
            ),
            TimeUnit::Nanosecond => TypedValue::Number(
                array
                    .as_primitive::<Time64NanosecondType>()
                    .value(row_index) as f64,
            ),
            _ => TypedValue::Other(format_scalar(Arc::clone(array), row_index)),
        },
        DataType::Timestamp(unit, _) => {
            TypedValue::Number(timestamp_value(array, row_index, unit) as f64)
        }
        DataType::Duration(unit) => {
            TypedValue::Number(duration_value(array, row_index, unit) as f64)
        }
        DataType::Decimal128(_, scale) => {
            let raw = array.as_primitive::<Decimal128Type>().value(row_index);
            TypedValue::Number(decimal128_to_f64(raw, *scale as usize))
        }
        DataType::Decimal256(_, scale) => {
            let raw = array.as_primitive::<Decimal256Type>().value(row_index);
            TypedValue::Number(decimal256_to_f64(raw, *scale as usize))
        }
        DataType::Utf8 => TypedValue::Str(array.as_string::<i32>().value(row_index).to_string()),
        DataType::LargeUtf8 => {
            TypedValue::Str(array.as_string::<i64>().value(row_index).to_string())
        }
        DataType::Dictionary(key_type, _) => match key_type.as_ref() {
            DataType::Int8 => typed_dict!(array, row_index, Int8Type),
            DataType::Int16 => typed_dict!(array, row_index, Int16Type),
            DataType::Int32 => typed_dict!(array, row_index, Int32Type),
            DataType::Int64 => typed_dict!(array, row_index, Int64Type),
            DataType::UInt8 => typed_dict!(array, row_index, UInt8Type),
            DataType::UInt16 => typed_dict!(array, row_index, UInt16Type),
            DataType::UInt32 => typed_dict!(array, row_index, UInt32Type),
            DataType::UInt64 => typed_dict!(array, row_index, UInt64Type),
            _ => TypedValue::Other(format_scalar(Arc::clone(array), row_index)),
        },
        _ => TypedValue::Other(format_scalar(Arc::clone(array), row_index)),
    }
}

fn decimal128_to_f64(value: i128, scale: usize) -> f64 {
    if scale == 0 {
        value as f64
    } else {
        value as f64 / 10f64.powi(scale as i32)
    }
}

fn decimal256_to_f64(value: i256, scale: usize) -> f64 {
    let value_f64 = value.to_string().parse::<f64>().unwrap_or(0.0);
    if scale == 0 {
        value_f64
    } else {
        value_f64 / 10f64.powi(scale as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::{BooleanArray, Int32Array, StringArray};
    use std::sync::Arc;

    fn cell_from(array: &ArrayRef, row: usize) -> CellView {
        format_cell(Arc::clone(array), row)
    }

    #[test]
    fn null_renders_as_null() {
        let array: ArrayRef = Arc::new(Int32Array::from(vec![Some(1), None]));
        assert_eq!(cell_from(&array, 1).display, "NULL");
    }

    #[test]
    fn date32_renders_iso() {
        let array: ArrayRef = Arc::new(arrow_array::Date32Array::from(vec![0, 1, -1]));
        assert_eq!(cell_from(&array, 0).display, "1970-01-01");
        assert_eq!(cell_from(&array, 1).display, "1970-01-02");
        assert_eq!(cell_from(&array, 2).display, "1969-12-31");
    }

    #[test]
    fn time32_milli_renders_hms() {
        let array: ArrayRef = Arc::new(arrow_array::Time32MillisecondArray::from(vec![
            0,
            3_600_000 + 61_000 + 2000,
        ]));
        assert_eq!(cell_from(&array, 0).display, "00:00:00.000");
        assert_eq!(cell_from(&array, 1).display, "01:01:03.000");
    }

    #[test]
    fn timestamp_second_renders_with_nanos_zero() {
        let array: ArrayRef = Arc::new(arrow_array::TimestampSecondArray::from(vec![
            0,
            1_600_000_000,
        ]));
        assert_eq!(
            cell_from(&array, 0).display,
            "1970-01-01 00:00:00.000000000"
        );
        assert_eq!(
            cell_from(&array, 1).display,
            "2020-09-13 12:26:40.000000000"
        );
    }

    #[test]
    fn decimal128_applies_scale() {
        let array: ArrayRef = Arc::new(
            arrow_array::Decimal128Array::from(vec![12345, -500, 7])
                .with_precision_and_scale(38, 3)
                .unwrap(),
        );
        assert_eq!(cell_from(&array, 0).display, "12.345");
        assert_eq!(cell_from(&array, 1).display, "-.500");
        assert_eq!(cell_from(&array, 2).display, "0.007");
    }

    #[test]
    fn utf8_scalar_render_in_detail() {
        let array: ArrayRef = Arc::new(StringArray::from(vec!["hello"]));
        let cell = cell_from(&array, 0);
        assert_eq!(cell.display, "hello");
        assert_eq!(cell.detail, "hello");
    }

    #[test]
    fn wide_char_truncation_preserves_boundaries() {
        let value = "你你你";
        let truncated = truncate_to_width(value, 3);
        assert_eq!(UnicodeWidthStr::width(truncated.as_str()), 3);
        assert!(truncated.ends_with('…') || truncated == value);
    }

    #[test]
    fn fixed_size_binary_summarizes_length() {
        let values: Vec<Vec<u8>> = vec![vec![1u8, 2, 3], vec![4u8, 5, 6]];
        let array: ArrayRef = Arc::new(
            arrow_array::FixedSizeBinaryArray::try_from_iter(values.iter().cloned()).unwrap(),
        );
        let cell = cell_from(&array, 0);
        assert_eq!(cell.display, "<3 bytes>");
        assert!(cell.detail.contains("\"hex\""));
    }

    #[test]
    fn boolean_renders_true_false() {
        let array: ArrayRef = Arc::new(BooleanArray::from(vec![true, false]));
        assert_eq!(cell_from(&array, 0).display, "true");
        assert_eq!(cell_from(&array, 1).display, "false");
    }
}
