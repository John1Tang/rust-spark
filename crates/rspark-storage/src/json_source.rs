use crate::source::DataSource;
use rspark_core::error::{Error, Result};
use rspark_core::schema::{DataType, Field, Schema};
use rspark_core::value::Value;
use rspark_core::{Record, RecordBatch};
use std::fs::File;
use std::io::{BufRead, BufReader};

pub struct JsonSource;

impl JsonSource {
    pub fn new() -> Self {
        Self
    }

    /// Read NDJSON from any `Read`. Used by the S3 source on a
    /// `Cursor<&[u8]>` after `get_object`, and by tests.
    pub fn scan_reader<R: BufRead>(
        &self,
        mut reader: R,
        schema: Option<&Schema>,
    ) -> Result<RecordBatch> {
        // If the caller didn't pass a schema, read the full body into
        // memory (NDJSON is small) and feed it twice — once for schema
        // inference, once for records.
        if schema.is_none() {
            let mut buf = Vec::new();
            let _ = std::io::Read::read_to_end(&mut reader, &mut buf)
                .map_err(|e| Error::Storage(format!("json read for schema: {e}")))?;
            let schema = self.infer_schema_from_reader(BufReader::new(&buf[..]))?;
            return self.scan_reader(BufReader::new(&buf[..]), Some(&schema));
        }
        let effective_schema = schema.unwrap().clone();
        let stream = serde_json::Deserializer::from_reader(reader).into_iter::<serde_json::Value>();
        let mut records = Vec::new();
        for value in stream {
            let value = value?;
            let mut row = Vec::with_capacity(effective_schema.field_count());
            match value {
                serde_json::Value::Object(map) => {
                    for field in effective_schema.fields() {
                        let v = map
                            .get(&field.name)
                            .cloned()
                            .unwrap_or(serde_json::Value::Null);
                        row.push(json_to_value(&v, &field.data_type));
                    }
                }
                _ => {
                    return Err(Error::Storage("each JSON line must be an object".into()));
                }
            }
            records.push(Record::new(row));
        }
        RecordBatch::from_records(effective_schema, records)
    }

    fn infer_schema_from_reader<R: BufRead>(&self, reader: R) -> Result<Schema> {
        let stream = serde_json::Deserializer::from_reader(reader).into_iter::<serde_json::Value>();
        let mut fields: Vec<Field> = Vec::new();
        for value in stream {
            let value = value?;
            if let serde_json::Value::Object(map) = value {
                for (k, v) in map {
                    if fields.iter().any(|f| f.name == k) {
                        continue;
                    }
                    fields.push(Field::new(k, json_type(&v)));
                }
            }
        }
        Ok(Schema::new(fields))
    }
}

impl Default for JsonSource {
    fn default() -> Self {
        Self::new()
    }
}

impl DataSource for JsonSource {
    fn name(&self) -> &'static str {
        "json"
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

fn json_type(v: &serde_json::Value) -> DataType {
    match v {
        serde_json::Value::Null => DataType::Null,
        serde_json::Value::Bool(_) => DataType::Boolean,
        serde_json::Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                DataType::Int64
            } else {
                DataType::Float64
            }
        }
        serde_json::Value::String(_) => DataType::String,
        _ => DataType::String,
    }
}

fn json_to_value(v: &serde_json::Value, target: &DataType) -> Value {
    match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Boolean(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                match target {
                    DataType::Float64 => Value::Float64(i as f64),
                    DataType::String => Value::String(i.to_string()),
                    _ => Value::Int64(i),
                }
            } else if let Some(f) = n.as_f64() {
                match target {
                    DataType::Int64 => Value::Float64(f),
                    DataType::String => Value::String(f.to_string()),
                    _ => Value::Float64(f),
                }
            } else {
                Value::Null
            }
        }
        serde_json::Value::String(s) => match target {
            DataType::Int64 => s
                .parse::<i64>()
                .map(Value::Int64)
                .unwrap_or(Value::String(s.clone())),
            DataType::Float64 => s
                .parse::<f64>()
                .map(Value::Float64)
                .unwrap_or(Value::String(s.clone())),
            DataType::Boolean => s
                .parse::<bool>()
                .map(Value::Boolean)
                .unwrap_or(Value::String(s.clone())),
            _ => Value::String(s.clone()),
        },
        _ => Value::String(v.to_string()),
    }
}
