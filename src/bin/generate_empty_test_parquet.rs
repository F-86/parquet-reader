use std::{fs, fs::File, path::PathBuf, sync::Arc};

use arrow_array::{ArrayRef, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use parquet::arrow::arrow_writer::ArrowWriter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("parquet");
    fs::create_dir_all(&output_dir)?;

    let path = output_dir.join("empty.parquet");
    write_empty(&path)?;

    println!("generated:");
    println!("  {}", path.display());
    println!();
    println!("try:");
    println!("  cargo run -- parquet/empty.parquet");

    Ok(())
}

fn write_empty(path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("message", DataType::Utf8, true),
    ]));
    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(Int64Array::from(Vec::<i64>::new())) as ArrayRef,
            Arc::new(StringArray::from(Vec::<Option<&str>>::new())),
        ],
    )?;

    let file = File::create(path)?;
    let mut writer = ArrowWriter::try_new(file, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;
    Ok(())
}
