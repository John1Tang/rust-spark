use rspark_core::error::{Error, Result};
use rspark_core::schema::{DataType, Schema};
use rspark_core::value::Value;
use rspark_core::RecordBatch;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

pub struct OutputWriter;

impl OutputWriter {
    pub fn write(batch: &RecordBatch, path: &str) -> Result<()> {
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "csv" => Self::write_csv(batch, path),
            "json" | "jsonl" | "ndjson" => Self::write_json(batch, path),
            _ => Err(Error::Storage(format!(
                "unsupported output format: .{ext}"
            ))),
        }
    }

    pub fn write_csv(batch: &RecordBatch, path: &str) -> Result<()> {
        let file = File::create(path)?;
        let mut wtr = csv::Writer::from_writer(BufWriter::new(file));
        let header: Vec<&str> = batch.schema().field_names();
        wtr.write_record(header)
            .map_err(|e| rspark_core::error::CsvError::from(e.to_string()))?;
        for record in batch.records() {
            let row: Vec<String> = record.values().iter().map(value_to_csv_string).collect();
            wtr.write_record(&row)
                .map_err(|e| rspark_core::error::CsvError::from(e.to_string()))?;
        }
        wtr.flush()?;
        Ok(())
    }

    pub fn write_json(batch: &RecordBatch, path: &str) -> Result<()> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        for record in batch.records() {
            let mut obj = serde_json::Map::new();
            for (field, value) in batch.schema().fields().iter().zip(record.values().iter()) {
                obj.insert(field.name.clone(), value_to_json(value));
            }
            serde_json::to_writer(&mut writer, &serde_json::Value::Object(obj))?;
            writeln!(writer)?;
        }
        writer.flush()?;
        Ok(())
    }
}

fn value_to_csv_string(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.cast_to_string(),
    }
}

fn value_to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Null => serde_json::Value::Null,
        Value::Boolean(b) => serde_json::Value::Bool(*b),
        Value::Int32(i) => serde_json::Value::from(*i),
        Value::Int64(i) => serde_json::Value::from(*i),
        Value::Float32(f) => serde_json::Value::from(*f),
        Value::Float64(f) => serde_json::Value::from(*f),
        Value::String(s) => serde_json::Value::String(s.clone()),
    }
}

/// Render a [`RecordBatch`] as a pretty text table for CLI output.
pub fn render_table(batch: &RecordBatch) -> String {
    if batch.is_empty() {
        return "(0 rows)\n".to_string();
    }
    let schema = batch.schema();
    let mut widths: Vec<usize> = schema.fields().iter().map(|f| f.name.len()).collect();
    let mut rendered_rows: Vec<Vec<String>> = Vec::with_capacity(batch.len());
    for record in batch.records() {
        let row: Vec<String> = record
            .values()
            .iter()
            .map(|v| match v {
                Value::Null => "NULL".to_string(),
                Value::Boolean(b) => b.to_string(),
                Value::Int32(i) => i.to_string(),
                Value::Int64(i) => i.to_string(),
                Value::Float32(f) => format!("{f}"),
                Value::Float64(f) => format!("{f}"),
                Value::String(s) => s.clone(),
            })
            .collect();
        for (idx, cell) in row.iter().enumerate() {
            widths[idx] = widths[idx].max(cell.len());
        }
        rendered_rows.push(row);
    }
    let mut out = String::new();
    let header: Vec<String> = schema
        .fields()
        .iter()
        .enumerate()
        .map(|(i, f)| format!("{:>width$}", f.name, width = widths[i]))
        .collect();
    out.push_str(&header.join(" | "));
    out.push('\n');
    out.push_str(&widths.iter().map(|w| "-".repeat(*w)).collect::<Vec<_>>().join("-+-"));
    out.push('\n');
    for row in rendered_rows {
        let line: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(i, cell)| format!("{:<width$}", cell, width = widths[i]))
            .collect();
        out.push_str(&line.join(" | "));
        out.push('\n');
    }
    out.push_str(&format!("({} rows)\n", batch.len()));
    out
}

/// Convenience wrapper around a [`Schema`] used when writing.
pub struct SchemaView<'a>(pub &'a Schema);

impl SchemaView<'_> {
    pub fn field_types(&self) -> Vec<DataType> {
        self.0.fields().iter().map(|f| f.data_type.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csv_source::CsvSource;
    use crate::source::DataSource;
    use std::io::Write;

    #[test]
    fn round_trip_csv() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.csv");
        let mut tmp = File::create(&path).unwrap();
        writeln!(tmp, "id,name,score").unwrap();
        writeln!(tmp, "1,alice,90.0").unwrap();
        writeln!(tmp, "2,bob,85.5").unwrap();
        drop(tmp);
        let source = CsvSource::new();
        let batch = source.scan(path.to_str().unwrap(), None).unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch.schema().field_names(), vec!["id", "name", "score"]);
    }
}
