use std::{fs, fs::File, path::PathBuf, sync::Arc};

use arrow_array::{ArrayRef, BooleanArray, Float64Array, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use parquet::arrow::arrow_writer::ArrowWriter;

const ROW_COUNT: i64 = 257;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("parquet");
    fs::create_dir_all(&output_dir)?;

    let path = output_dir.join("pagination.parquet");
    write_pagination_file(&path)?;

    println!("generated:");
    println!("  {}", path.display());
    println!("rows: {ROW_COUNT}");
    println!();
    println!("try:");
    println!("  cargo run -- --page-size 25 parquet/pagination.parquet");

    Ok(())
}

fn write_pagination_file(path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("row_id", DataType::Int64, false),
        Field::new("group", DataType::Utf8, false),
        Field::new("page_hint_25", DataType::Int64, false),
        Field::new("score", DataType::Float64, false),
        Field::new("is_boundary", DataType::Boolean, false),
        Field::new("note", DataType::Utf8, false),
    ]));

    let row_ids = Int64Array::from_iter_values(1..=ROW_COUNT);
    let groups = StringArray::from(
        (1..=ROW_COUNT)
            .map(|row| format!("group-{:02}", (row - 1) / 10))
            .collect::<Vec<_>>(),
    );
    let page_hints = Int64Array::from_iter_values((1..=ROW_COUNT).map(|row| (row - 1) / 25 + 1));
    let scores = Float64Array::from_iter_values((1..=ROW_COUNT).map(|row| row as f64 * 1.25));
    let is_boundary = BooleanArray::from(
        (1..=ROW_COUNT)
            .map(|row| row == 1 || row == ROW_COUNT || row % 25 == 0 || row % 25 == 1)
            .collect::<Vec<_>>(),
    );
    let notes = StringArray::from(
        (1..=ROW_COUNT)
            .map(|row| {
                if row == 1 {
                    "first row".to_string()
                } else if row == ROW_COUNT {
                    "last row, final partial page".to_string()
                } else if row % 25 == 0 {
                    format!("page boundary end at row {row}")
                } else if row % 25 == 1 {
                    format!("page boundary start at row {row}")
                } else {
                    format!("pagination test row {row}")
                }
            })
            .collect::<Vec<_>>(),
    );

    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(row_ids) as ArrayRef,
            Arc::new(groups) as ArrayRef,
            Arc::new(page_hints) as ArrayRef,
            Arc::new(scores) as ArrayRef,
            Arc::new(is_boundary) as ArrayRef,
            Arc::new(notes) as ArrayRef,
        ],
    )?;

    let file = File::create(path)?;
    let mut writer = ArrowWriter::try_new(file, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;

    Ok(())
}
