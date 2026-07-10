//! S3-compatible writer.
//!
//! Serializes a [`RecordBatch`] to CSV or JSON bytes in memory, then
//! `put_object` to the configured bucket. Used by the pipeline runner
//! to land materialised-view output.
//!
//! Lives in the same module as [`S3Source`] so the writer and the
//! reader share a config.

use std::io::{BufWriter, Write};
use std::path::Path;

use aws_sdk_s3::primitives::ByteStream;
use rspark_core::error::{Error, Result};
use rspark_core::value::Value;
use rspark_core::RecordBatch;

use crate::s3_source::S3Config;

/// S3 writer. Cheap to construct (it just holds a config; the actual
/// S3 client is built per-write because the cluster worker may have
/// refreshed credentials between runs).
pub struct S3Writer {
    config: S3Config,
}

impl S3Writer {
    pub fn new(config: S3Config) -> Self {
        Self { config }
    }

    /// Convenience: read bucket/region/creds from env.
    pub fn from_env() -> Option<Self> {
        S3Config::from_env().map(Self::new)
    }

    /// Serialize the batch to bytes in `format` ("csv" or "json"),
    /// then `put_object` to `<bucket>/<key>`.
    pub async fn write(&self, batch: &RecordBatch, key: &str, format: &str) -> Result<()> {
        let bytes = match format.to_ascii_lowercase().as_str() {
            "csv" => render_csv(batch)?,
            "json" | "jsonl" | "ndjson" => render_json(batch)?,
            other => {
                return Err(Error::Storage(format!(
                    "unsupported s3 output format: {other}"
                )))
            }
        };

        let shared = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_sdk_s3::config::Region::new(self.config.region.clone()))
            .load()
            .await;
        let mut s3_conf = aws_sdk_s3::config::Builder::from(&shared);
        if let Some(endpoint) = &self.config.endpoint {
            s3_conf = s3_conf.endpoint_url(endpoint.clone());
        }
        if let (Some(ak), Some(sk)) = (&self.config.access_key, &self.config.secret_key) {
            s3_conf = s3_conf.credentials_provider(aws_credential_types::Credentials::new(
                ak.clone(),
                sk.clone(),
                None,
                None,
                "rspark-static",
            ));
        }
        let client = aws_sdk_s3::Client::from_conf(s3_conf.build());

        // Parent dirs on the key (e.g. `tables/wordcount_top/data.csv`)
        // are implicit in S3 — the put below creates them.

        let key = normalize_key(key);
        let body = ByteStream::from(bytes);
        client
            .put_object()
            .bucket(&self.config.bucket)
            .key(&key)
            .body(body)
            .send()
            .await
            .map_err(|e| Error::Storage(format!("s3 put_object({key}) failed: {e}")))?;
        Ok(())
    }
}

fn normalize_key(key: &str) -> String {
    // Strip a leading `s3://bucket/` if the caller passed the full URI.
    if let Some(stripped) = key.strip_prefix("s3://") {
        if let Some((_bucket, k)) = stripped.split_once('/') {
            return k.to_string();
        }
    }
    key.to_string()
}

fn render_csv(batch: &RecordBatch) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    {
        let mut wtr = csv::Writer::from_writer(BufWriter::new(&mut buf));
        wtr.write_record(batch.schema().field_names())
            .map_err(|e| rspark_core::error::CsvError::from(e.to_string()))?;
        for record in batch.records() {
            let row: Vec<String> = record.values().iter().map(value_to_csv_string).collect();
            wtr.write_record(&row)
                .map_err(|e| rspark_core::error::CsvError::from(e.to_string()))?;
        }
        wtr.flush()
            .map_err(|e| Error::Storage(format!("csv flush: {e}")))?;
    }
    Ok(buf)
}

fn render_json(batch: &RecordBatch) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    {
        let mut writer = BufWriter::new(&mut buf);
        for record in batch.records() {
            let mut obj = serde_json::Map::new();
            for (field, value) in batch.schema().fields().iter().zip(record.values().iter()) {
                obj.insert(field.name.clone(), value_to_json(value));
            }
            serde_json::to_writer(&mut writer, &serde_json::Value::Object(obj))?;
            writer.write_all(b"\n")?;
        }
        writer
            .flush()
            .map_err(|e| Error::Storage(format!("json flush: {e}")))?;
    }
    Ok(buf)
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

/// Detect the output format from a path's extension.
pub fn format_from_key(key: &str) -> Result<&'static str> {
    match Path::new(key)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "csv" => Ok("csv"),
        "json" | "jsonl" | "ndjson" => Ok("json"),
        other => Err(Error::Storage(format!(
            "unsupported s3 output format: .{other}"
        ))),
    }
}
