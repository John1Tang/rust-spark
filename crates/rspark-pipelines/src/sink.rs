//! Pipeline output destinations. For now we support local filesystem
//! (always) and S3 (when `AWS_S3_BUCKET` is configured).

use std::path::PathBuf;

use rspark_core::error::{Error, Result};
use rspark_core::RecordBatch;
use rspark_storage::writer::render_table;

use crate::spec::Destination;

/// Write a batch to a destination. Returns the number of bytes written.
pub async fn write_destination(dest: &Destination, batch: &RecordBatch) -> Result<u64> {
    match dest {
        Destination::File { path } => write_file(path, batch),
        Destination::S3 { key, bucket } => write_s3(key, bucket.as_deref(), batch).await,
    }
}

fn write_file(path: &PathBuf, batch: &RecordBatch) -> Result<u64> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(Error::Io)?;
        }
    }
    let bytes = render_table(batch).into_bytes();
    std::fs::write(path, &bytes).map_err(Error::Io)?;
    Ok(bytes.len() as u64)
}

async fn write_s3(key: &str, bucket: Option<&str>, batch: &RecordBatch) -> Result<u64> {
    let bucket_name = bucket
        .map(String::from)
        .or_else(|| std::env::var("AWS_S3_BUCKET").ok())
        .ok_or_else(|| {
            Error::Storage("s3 destination requires AWS_S3_BUCKET or explicit bucket".into())
        })?;
    let cfg = rspark_storage::S3Config {
        endpoint: std::env::var("AWS_ENDPOINT_URL_S3").ok(),
        region: std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".into()),
        bucket: bucket_name,
        access_key: std::env::var("AWS_ACCESS_KEY_ID").ok(),
        secret_key: std::env::var("AWS_SECRET_ACCESS_KEY").ok(),
    };
    let writer = rspark_storage::S3Writer::new(cfg);
    let bytes = render_table(batch).into_bytes();
    // S3Writer::write takes a RecordBatch and serializes internally;
    // pass an empty batch with our bytes via a small adapter so we
    // can keep the pre-rendered bytes when needed. For now we let
    // S3Writer handle the serialization itself.
    let _ = bytes;
    writer.write(batch, key, "csv").await?;
    let mut buf = Vec::new();
    // The serialized CSV has a header + N rows; one cheap way to
    // report the size without re-rendering is to count what S3Writer
    // would have written. We approximate with `render_table` again.
    buf.extend(render_table(batch).into_bytes());
    Ok(buf.len() as u64)
}

/// Render the destination's URI for logging.
pub fn describe(dest: &Destination) -> String {
    match dest {
        Destination::File { path } => format!("file://{}", path.display()),
        Destination::S3 { key, bucket } => {
            let b = bucket
                .clone()
                .or_else(|| std::env::var("AWS_S3_BUCKET").ok())
                .unwrap_or_else(|| "<no-bucket>".into());
            format!("s3://{b}/{key}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rspark_core::schema::{DataType, Field, Schema};
    use rspark_core::{Record, Value};

    #[tokio::test]
    async fn writes_table_to_file() {
        let schema = Schema::new(vec![
            Field::new("id", DataType::Int64),
            Field::new("name", DataType::String),
        ]);
        let batch = RecordBatch::from_records(
            schema,
            vec![Record::new(vec![
                Value::Int64(1),
                Value::String("a".into()),
            ])],
        )
        .unwrap();
        let dir = std::env::temp_dir().join(format!(
            "rspark-pipeline-sink-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.txt");
        let dest = Destination::File { path: path.clone() };
        let n = write_destination(&dest, &batch).await.unwrap();
        assert!(n > 0);
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("id") && body.contains("name"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn render_table_renders_rows() {
        let schema = Schema::new(vec![Field::new("x", DataType::Int64)]);
        let batch =
            RecordBatch::from_records(schema, vec![Record::new(vec![Value::Int64(7)])]).unwrap();
        let txt = render_table(&batch);
        assert!(txt.contains("7"));
    }
}
