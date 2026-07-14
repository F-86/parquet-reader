use std::{fs, fs::File, path::PathBuf, sync::Arc};

use arrow_array::{
    Array, ArrayRef, BinaryArray, Int64Array, ListArray, RecordBatch, StringArray, StructArray,
    builder::{Int64Builder, MapBuilder, StringBuilder},
    types::Int64Type,
};
use arrow_schema::{DataType, Field, Fields, Schema};
use parquet::arrow::arrow_writer::ArrowWriter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("parquet");
    fs::create_dir_all(&output_dir)?;

    let path = output_dir.join("complex_types.parquet");
    write_complex_types(&path)?;

    println!("generated:");
    println!("  {}", path.display());
    println!();
    println!("try:");
    println!("  cargo run -- parquet/complex_types.parquet");

    Ok(())
}

fn write_complex_types(path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let ids = Int64Array::from_iter_values(1..=5);

    let int_list = ListArray::from_iter_primitive::<Int64Type, _, _>(vec![
        Some(vec![Some(1), Some(2), Some(3)]),
        Some(vec![Some(10), None, Some(30)]),
        None,
        Some(vec![Some(100), Some(200), Some(300), Some(400)]),
        Some(vec![]),
    ]);

    let names = StringArray::from(vec![
        Some("alpha"),
        Some("beta"),
        None,
        Some("delta"),
        Some("emoji 🚀"),
    ]);
    let levels = Int64Array::from(vec![Some(7), Some(42), None, Some(99), Some(5)]);
    let profile_fields = Fields::from(vec![
        Field::new("nickname", DataType::Utf8, true),
        Field::new("level", DataType::Int64, true),
    ]);
    let profile = StructArray::try_new(
        profile_fields.clone(),
        vec![Arc::new(names) as ArrayRef, Arc::new(levels) as ArrayRef],
        None,
    )?;

    let attributes = build_attributes_map()?;

    let json_text = StringArray::from(vec![
        Some(r#"{"kind":"alpha","flags":["fast","safe"],"nested":{"x":1}}"#),
        Some(r#"[1,2,{"ok":true,"message":"hello"}]"#),
        Some(r#"{"unicode":"你好，世界","emoji":"🚀📦🦀"}"#),
        Some("not json"),
        None,
    ]);

    let binary_blob = BinaryArray::from(vec![
        Some(&[0, 1, 2, 3, 4, 5, 254, 255][..]),
        Some(&b"hello complex parquet"[..]),
        None,
        Some(&[16, 32, 48, 64, 80, 96, 112, 128][..]),
        Some(&[255; 32][..]),
    ]);

    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("int_list", int_list.data_type().clone(), true),
        Field::new("profile", DataType::Struct(profile_fields), true),
        Field::new("attributes", attributes.data_type().clone(), true),
        Field::new("json_text", DataType::Utf8, true),
        Field::new("binary_blob", DataType::Binary, true),
    ]));

    let batch = RecordBatch::try_new(
        Arc::clone(&schema),
        vec![
            Arc::new(ids) as ArrayRef,
            Arc::new(int_list) as ArrayRef,
            Arc::new(profile) as ArrayRef,
            Arc::new(attributes) as ArrayRef,
            Arc::new(json_text) as ArrayRef,
            Arc::new(binary_blob) as ArrayRef,
        ],
    )?;

    let file = File::create(path)?;
    let mut writer = ArrowWriter::try_new(file, schema, None)?;
    writer.write(&batch)?;
    writer.close()?;

    Ok(())
}

fn build_attributes_map() -> Result<arrow_array::MapArray, Box<dyn std::error::Error>> {
    let mut builder = MapBuilder::new(None, StringBuilder::new(), Int64Builder::new());

    builder.keys().append_value("age");
    builder.values().append_value(30);
    builder.keys().append_value("score");
    builder.values().append_value(98);
    builder.append(true)?;

    builder.keys().append_value("age");
    builder.values().append_value(25);
    builder.append(true)?;

    builder.append(false)?;

    builder.keys().append_value("priority");
    builder.values().append_value(1);
    builder.keys().append_value("retries");
    builder.values().append_value(3);
    builder.keys().append_value("timeout");
    builder.values().append_value(60);
    builder.append(true)?;

    builder.append(true)?;

    Ok(builder.finish())
}
