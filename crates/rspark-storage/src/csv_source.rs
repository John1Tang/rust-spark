use crate::source::DataSource;
use rspark_core::error::{Error, Result};
use rspark_core::schema::{DataType, Field, Schema};
use rspark_core::value::Value;
use rspark_core::{Record, RecordBatch};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub struct CsvSource;

impl CsvSource {
    pub fn new() -> Self {
        Self
    }

    /// Read a CSV from any `BufRead`. Used by the S3 source against a
    /// `BufReader<Cursor<&[u8]>>` after `get_object`, and by tests.
    pub fn scan_reader<R: BufRead>(
        &self,
        mut reader: R,
        schema: Option<&Schema>,
    ) -> Result<RecordBatch> {
        // If the caller didn't pass a schema, we have to read the file
        // twice (once to infer, once to scan). The reader might not be
        // seekable, so buffer into memory — CSVs are small.
        let effective_schema = match schema {
            Some(s) => s.clone(),
            None => {
                let mut buf = Vec::new();
                let _ = std::io::Read::read_to_end(&mut reader, &mut buf)
                    .map_err(|e| Error::Storage(format!("csv read for schema: {e}")))?;
                let schema = self.infer_schema_from_reader(BufReader::new(&buf[..]))?;
                let mut rdr = csv::ReaderBuilder::new()
                    .has_headers(true)
                    .from_reader(BufReader::new(&buf[..]));
                let mut records = Vec::new();
                for result in rdr.records() {
                    let row = result.map_err(|e| {
                        rspark_core::error::Error::from(rspark_core::error::CsvError::from(
                            e.to_string(),
                        ))
                    })?;
                    let values = row
                        .iter()
                        .zip(schema.fields().iter())
                        .map(|(raw, field)| coerce_value(raw, &field.data_type))
                        .collect();
                    records.push(Record::new(values));
                }
                return RecordBatch::from_records(schema, records);
            }
        };
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(true)
            .from_reader(reader);
        let mut records = Vec::new();
        for result in rdr.records() {
            let row = result.map_err(|e| {
                rspark_core::error::Error::from(rspark_core::error::CsvError::from(e.to_string()))
            })?;
            let values = row
                .iter()
                .zip(effective_schema.fields().iter())
                .map(|(raw, field)| coerce_value(raw, &field.data_type))
                .collect();
            records.push(Record::new(values));
        }
        RecordBatch::from_records(effective_schema, records)
    }

    fn infer_schema_from_reader<R: BufRead>(&self, reader: R) -> Result<Schema> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .from_reader(reader);
        let headers = reader
            .headers()
            .map_err(|e| Error::Storage(format!("failed to read csv headers: {e}")))?
            .clone();
        let mut schema = Schema::new(
            headers
                .iter()
                .map(|h| Field::new(h.to_string(), DataType::String))
                .collect(),
        );
        let mut record = csv::StringRecord::new();
        let mut converted = Vec::new();
        while reader.read_record(&mut record).unwrap_or(false) {
            let row: Vec<Value> = record.iter().map(parse_csv_value).collect();
            converted.push(row);
        }
        if !converted.is_empty() {
            let detected = infer_columns(&converted, &headers);
            let fields = headers
                .iter()
                .zip(detected.iter())
                .map(|(h, t)| Field::new(h.to_string(), t.clone()))
                .collect();
            schema = Schema::new(fields);
        }
        Ok(schema)
    }
}

impl Default for CsvSource {
    fn default() -> Self {
        Self::new()
    }
}

impl DataSource for CsvSource {
    fn name(&self) -> &'static str {
        "csv"
    }

    fn infer_schema(&self, path: &str) -> Result<Schema> {
        let file = File::open(path)?;
        self.infer_schema_from_reader(BufReader::new(file))
    }

    fn scan(&self, path: &str, schema: Option<&Schema>) -> Result<RecordBatch> {
        let file = File::open(path)?;
        self.scan_reader(BufReader::new(file), schema)
    }
}

fn parse_csv_value(raw: &str) -> Value {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("null")
        || trimmed.eq_ignore_ascii_case("none")
    {
        return Value::Null;
    }
    if trimmed.eq_ignore_ascii_case("true") {
        return Value::Boolean(true);
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return Value::Boolean(false);
    }
    if let Ok(v) = trimmed.parse::<i64>() {
        return Value::Int64(v);
    }
    if let Ok(v) = trimmed.parse::<f64>() {
        return Value::Float64(v);
    }
    Value::String(trimmed.to_string())
}

fn infer_columns(rows: &[Vec<Value>], headers: &csv::StringRecord) -> Vec<DataType> {
    let mut types = Vec::with_capacity(headers.len());
    for col_idx in 0..headers.len() {
        let mut all_int = true;
        let mut all_float = true;
        let mut all_bool = true;
        let mut any_value = false;
        for row in rows {
            match row.get(col_idx) {
                Some(Value::Null) | None => continue,
                Some(_) => any_value = true,
            }
            let v = &row[col_idx];
            match v {
                Value::Int64(_) => {}
                Value::Boolean(_) => {
                    all_int = false;
                    all_float = false;
                }
                Value::Float64(_) => {
                    all_int = false;
                }
                _ => {
                    all_int = false;
                    all_float = false;
                    all_bool = false;
                }
            }
        }
        types.push(if !any_value {
            DataType::String
        } else if all_int {
            DataType::Int64
        } else if all_float {
            DataType::Float64
        } else if all_bool {
            DataType::Boolean
        } else {
            DataType::String
        });
    }
    types
}

fn coerce_value(raw: &str, data_type: &DataType) -> Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null") {
        return Value::Null;
    }
    match data_type {
        DataType::Boolean => match trimmed.to_ascii_lowercase().as_str() {
            "true" | "t" | "1" | "yes" | "y" => Value::Boolean(true),
            "false" | "f" | "0" | "no" | "n" => Value::Boolean(false),
            _ => Value::String(trimmed.to_string()),
        },
        DataType::Int64 => trimmed
            .parse::<i64>()
            .map(Value::Int64)
            .unwrap_or_else(|_| Value::String(trimmed.to_string())),
        DataType::Int32 => trimmed
            .parse::<i32>()
            .map(Value::Int32)
            .unwrap_or_else(|_| Value::String(trimmed.to_string())),
        DataType::Float64 => trimmed
            .parse::<f64>()
            .map(Value::Float64)
            .unwrap_or_else(|_| Value::String(trimmed.to_string())),
        DataType::Float32 => trimmed
            .parse::<f32>()
            .map(Value::Float32)
            .unwrap_or_else(|_| Value::String(trimmed.to_string())),
        _ => Value::String(trimmed.to_string()),
    }
}

pub fn ensure_extension(path: &str, extension: &str) -> Result<()> {
    let p = Path::new(path);
    let got = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if got == extension.to_ascii_lowercase() {
        Ok(())
    } else {
        Err(Error::Storage(format!(
            "expected .{extension} file, got '{path}'"
        )))
    }
}
