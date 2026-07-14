use std::{fs, fs::File, path::PathBuf, sync::Arc};

use arrow_array::{
    ArrayRef, BinaryArray, BooleanArray, Float64Array, Int64Array, RecordBatch, StringArray,
};
use arrow_schema::{DataType, Field, Schema};
use parquet::arrow::arrow_writer::ArrowWriter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("parquet");
    fs::create_dir_all(&output_dir)?;

    write_people(&output_dir.join("people.parquet"))?;

    println!("generated:");
    println!("  {}", output_dir.join("people.parquet").display());
    println!();
    println!("try:");
    println!("  cargo run -- parquet/people.parquet");

    Ok(())
}

fn write_people(path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("name", DataType::Utf8, true),
        Field::new("city", DataType::Utf8, true),
        Field::new("score", DataType::Float64, true),
        Field::new("active", DataType::Boolean, true),
        Field::new("note", DataType::Utf8, true),
        Field::new("payload", DataType::Binary, true),
    ]));

    let ids = Int64Array::from_iter_values(1..=12);
    let names = StringArray::from(vec![
        Some("Alice"),
        Some("Bob"),
        Some("陈小龙"),
        Some("Dana"),
        None,
        Some("Élodie"),
        Some("Fatima"),
        Some("Георгий"),
        Some("Hana"),
        Some("伊藤"),
        Some("José"),
        Some("Kira"),
    ]);
    let cities = StringArray::from(vec![
        Some("New York"),
        Some("San Francisco"),
        Some("上海"),
        Some("Berlin"),
        Some("São Paulo"),
        Some("Paris"),
        Some("Dubai"),
        Some("Москва"),
        Some("서울"),
        Some("東京"),
        None,
        Some("Sydney"),
    ]);
    let scores = Float64Array::from(vec![
        Some(98.5),
        Some(72.25),
        None,
        Some(88.0),
        Some(91.75),
        Some(64.0),
        Some(100.0),
        Some(79.5),
        Some(83.25),
        Some(95.0),
        Some(70.0),
        Some(86.5),
    ]);
    let active = BooleanArray::from(vec![
        Some(true),
        Some(false),
        Some(true),
        Some(true),
        None,
        Some(false),
        Some(true),
        Some(false),
        Some(true),
        Some(true),
        Some(false),
        Some(true),
    ]);
    let notes = StringArray::from(vec![
        Some("short"),
        Some("contains ASCII text"),
        Some("宽字符测试：你好，世界，终端宽度应该稳定"),
        Some(
            "a very very very very very very long cell that should be truncated safely by the TUI",
        ),
        None,
        Some("emoji test 🚀📦🦀"),
        Some("binary payload is shown as byte count"),
        Some("cyrillic text"),
        Some("한국어 테스트: 안녕하세요"),
        Some("日本語テスト：こんにちは"),
        Some("NULL city on this row"),
        Some("last row"),
    ]);
    let payload = BinaryArray::from(vec![
        Some(&b"abc"[..]),
        Some(&b"hello"[..]),
        None,
        Some(&[0, 1, 2, 3, 4, 5][..]),
        Some(&b"null-name"[..]),
        Some(&b"emoji"[..]),
        Some(&[255, 254, 253, 252][..]),
        Some(&b"ru"[..]),
        Some(&b"kr"[..]),
        Some(&b"jp"[..]),
        Some(&b"city-null"[..]),
        Some(&b"end"[..]),
    ]);

    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(ids) as ArrayRef,
            Arc::new(names),
            Arc::new(cities),
            Arc::new(scores),
            Arc::new(active),
            Arc::new(notes),
            Arc::new(payload),
        ],
    )?;

    write_batch(path, schema, &batch)
}

fn write_batch(
    path: &PathBuf,
    schema: Arc<Schema>,
    batch: &RecordBatch,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::create(path)?;
    let mut writer = ArrowWriter::try_new(file, schema, None)?;
    writer.write(batch)?;
    writer.close()?;
    Ok(())
}
