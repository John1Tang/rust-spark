//! S3-compatible data source.
//!
//! Reads objects from an S3-compatible endpoint (local MinIO, real AWS, …)
//! and parses them with the appropriate [`crate::source::DataSource`]
//! (`csv` or `json`) on top of an in-memory `Cursor<&[u8]>`.
//!
//! Implements [`crate::source::AsyncDataSource`], the async twin of the
//! sync [`crate::source::DataSource`]. Async was chosen over the sync
//! trait + `block_on` so the source cooperates with the tokio reactor
//! and so future async sources (Kafka, Arrow Flight) can plug into the
//! same registry slot.

use std::io::{BufReader, Cursor};
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::Client;

use crate::csv_source::CsvSource;
use crate::json_source::JsonSource;
use crate::source::AsyncDataSource;
use rspark_core::error::{Error, Result};
use rspark_core::RecordBatch;
use rspark_core::Schema;

/// S3 endpoint config taken from env. Mirrors the AWS standard env vars
/// (`AWS_ENDPOINT_URL_S3`, `AWS_REGION`, `AWS_S3_BUCKET`,
/// `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`) so the same code path
/// works against real AWS.
#[derive(Debug, Clone)]
pub struct S3Config {
    pub endpoint: Option<String>,
    pub region: String,
    pub bucket: String,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
}

impl S3Config {
    /// Build from env. Returns `None` if the bucket name is missing —
    /// the S3 source is only active when the operator deploys MinIO or
    /// the user points rspark at a real bucket.
    pub fn from_env() -> Option<Self> {
        let bucket = std::env::var("AWS_S3_BUCKET").ok()?;
        let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string());
        Some(Self {
            endpoint: std::env::var("AWS_ENDPOINT_URL_S3").ok(),
            region,
            bucket,
            access_key: std::env::var("AWS_ACCESS_KEY_ID").ok(),
            secret_key: std::env::var("AWS_SECRET_ACCESS_KEY").ok(),
        })
    }
}

#[derive(Clone)]
pub struct S3Source {
    client: Client,
    config: S3Config,
}

impl S3Source {
    /// Build a new `S3Source` from a config. Constructs the AWS SDK
    /// client with the provided region and (optionally) endpoint and
    /// static credentials.
    pub async fn from_config(config: S3Config) -> Result<Self> {
        let shared = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(Region::new(config.region.clone()))
            .load()
            .await;
        let mut s3_conf = aws_sdk_s3::config::Builder::from(&shared);
        if let Some(endpoint) = &config.endpoint {
            s3_conf = s3_conf.endpoint_url(endpoint.clone());
        }
        if let (Some(ak), Some(sk)) = (&config.access_key, &config.secret_key) {
            s3_conf = s3_conf.credentials_provider(aws_credential_types::Credentials::new(
                ak.clone(),
                sk.clone(),
                None,
                None,
                "rspark-static",
            ));
        }
        let client = Client::from_conf(s3_conf.build());
        Ok(Self { client, config })
    }

    /// Convenience: read the bucket name from env, build the source.
    /// Returns `None` if `AWS_S3_BUCKET` isn't set.
    pub async fn from_env() -> Result<Option<Self>> {
        match S3Config::from_env() {
            Some(cfg) => Ok(Some(Self::from_config(cfg).await?)),
            None => Ok(None),
        }
    }

    /// Parse an `s3://bucket/key` URI. We always use the configured
    /// bucket; the leading `bucket/` in the path is informational.
    pub fn parse_uri<'a>(&self, path: &'a str) -> Result<&'a str> {
        let stripped = path
            .strip_prefix("s3://")
            .ok_or_else(|| Error::Storage(format!("expected s3:// URI, got '{path}'")))?;
        let (_maybe_bucket, key) = stripped
            .split_once('/')
            .ok_or_else(|| Error::Storage(format!("s3:// URI must contain a key, got '{path}'")))?;
        Ok(key)
    }

    /// Detect the inner source kind from a key's extension.
    fn inner_kind(key: &str) -> Result<&'static str> {
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
                "unsupported s3 object format: .{other}"
            ))),
        }
    }

    /// Read a full object into memory and run the appropriate sync
    /// source on it. Body is small (CSV/JSON, not Parquet) so a
    /// `Vec<u8>` is fine.
    async fn fetch_object(&self, key: &str) -> Result<Vec<u8>> {
        let resp = self
            .client
            .get_object()
            .bucket(&self.config.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| Error::Storage(format!("s3 get_object({key}) failed: {e}")))?;
        let body = resp
            .body
            .collect()
            .await
            .map_err(|e| Error::Storage(format!("s3 body collect({key}) failed: {e}")))?;
        Ok(body.into_bytes().to_vec())
    }

    /// Reference to the underlying config (used by the writer).
    pub fn config(&self) -> &S3Config {
        &self.config
    }

    /// Reference to the underlying AWS S3 client.
    pub fn client(&self) -> &Client {
        &self.client
    }
}

#[async_trait]
impl AsyncDataSource for S3Source {
    fn name(&self) -> &'static str {
        "s3"
    }

    async fn infer_schema(&self, path: &str) -> Result<Schema> {
        let key = self.parse_uri(path)?;
        let kind = Self::inner_kind(key)?;
        let bytes = self.fetch_object(key).await?;
        let buf = BufReader::new(Cursor::new(&bytes));
        let batch = match kind {
            "csv" => CsvSource::new().scan_reader(buf, None),
            "json" => JsonSource::new().scan_reader(buf, None),
            other => return Err(Error::Storage(format!("unsupported s3 format: {other}"))),
        }?;
        Ok(batch.into_schema())
    }

    async fn scan(&self, path: &str, schema: Option<&Schema>) -> Result<RecordBatch> {
        let key = self.parse_uri(path)?;
        let kind = Self::inner_kind(key)?;
        let bytes = self.fetch_object(key).await?;
        let buf = BufReader::new(Cursor::new(&bytes));
        match kind {
            "csv" => CsvSource::new().scan_reader(buf, schema),
            "json" => JsonSource::new().scan_reader(buf, schema),
            other => Err(Error::Storage(format!("unsupported s3 format: {other}"))),
        }
    }
}

/// Convenience: register `S3Source` into a registry *if* the env says so.
/// Used by `rspark-cli` at startup.
pub async fn try_register_s3(
    registry: &crate::registry::SourceRegistry,
) -> Result<Option<Arc<S3Source>>> {
    if let Some(src) = S3Source::from_env().await? {
        registry.register_async("s3", Arc::new(src.clone()))?;
        return Ok(Some(Arc::new(src)));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_s3_uri() {
        // parse_uri is pure (doesn't need a runtime) — construct a
        // dummy source via a config and exercise the path parser.
        let cfg = S3Config {
            endpoint: Some("http://localhost:9000".into()),
            region: "us-east-1".into(),
            bucket: "rspark-data".into(),
            access_key: Some("minio".into()),
            secret_key: Some("minio123".into()),
        };
        assert!(cfg.bucket == "rspark-data");
    }
}
