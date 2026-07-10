//! Pipeline YAML spec. Loaded with `serde_yaml` from a file or a string.
//!
//! Example:
//! ```yaml
//! pipeline: wordcount_demo
//! flows:
//!   - name: lines
//!     kind: streaming_table
//!     source: { tail_dir: /var/spool/rspark/lines }
//!     query: "SELECT * FROM lines"
//!     destination: { s3: s3://rspark-data/wordcount/lines.csv }
//!   - name: counts
//!     kind: materialized_view
//!     depends_on: [lines]
//!     source: { sql: "SELECT 'placeholder' AS k, 0 AS v" }
//!     query: |
//!       SELECT k, v FROM (SELECT 1)
//!     refresh: full
//!     destination: { s3: s3://rspark-data/wordcount/counts.csv }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    pub pipeline: String,
    #[serde(default)]
    pub flows: Vec<Flow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flow {
    pub name: String,
    pub kind: FlowKind,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub source: SourceSpec,
    pub query: String,
    #[serde(default = "default_refresh")]
    pub refresh: Refresh,
    pub destination: Destination,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowKind {
    #[default]
    MaterializedView,
    StreamingTable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Refresh {
    #[default]
    Full,
    Incremental,
}

fn default_refresh() -> Refresh {
    Refresh::Full
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceSpec {
    /// Tail all NDJSON files in a directory, like a micro-batch source.
    TailDir { tail_dir: PathBuf },
    /// One-shot CSV file read.
    Csv { path: PathBuf },
    /// One-shot NDJSON file read.
    Json { path: PathBuf },
    /// S3 object — bucket is read from `AWS_S3_BUCKET`, key from `key`.
    /// `bucket` overrides the env if set.
    S3 { key: String, bucket: Option<String> },
    /// Kafka — declared but requires a Kafka broker in the cluster.
    /// Without one, [`crate::source::KafkaSource::poll_batch`] returns
    /// an empty batch with a logged warning.
    Kafka {
        topic: String,
        brokers: String,
        group_id: String,
    },
    /// Inline SQL literal — useful for materialized views that derive
    /// purely from upstream flows without a physical source.
    Sql,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Destination {
    /// `s3://bucket/key` URI. Bucket comes from `AWS_S3_BUCKET` unless
    /// overridden.
    S3 { key: String, bucket: Option<String> },
    /// Local filesystem path (mostly for testing).
    File { path: PathBuf },
}

impl Pipeline {
    pub fn from_yaml(yaml: &str) -> Result<Self, serde_yaml::Error> {
        serde_yaml::from_str(yaml)
    }

    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self, SpecError> {
        let s = std::fs::read_to_string(path.as_ref()).map_err(SpecError::Io)?;
        Self::from_yaml(&s).map_err(SpecError::Yaml)
    }

    /// Index flows by name for fast lookup.
    pub fn index(&self) -> HashMap<&str, &Flow> {
        self.flows.iter().map(|f| (f.name.as_str(), f)).collect()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SpecError {
    #[error("io error reading spec: {0}")]
    Io(#[from] std::io::Error),
    #[error("yaml parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("flow not found: {0}")]
    UnknownFlow(String),
    #[error("cycle in pipeline: {0}")]
    Cycle(String),
}

// Bridge into rspark_core::Error so callers don't have to hand-convert.
impl From<SpecError> for rspark_core::error::Error {
    fn from(e: SpecError) -> Self {
        rspark_core::error::Error::Parse(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_pipeline() {
        let yaml = r#"
pipeline: p
flows:
  - name: a
    kind: materialized_view
    source: { kind: sql }
    query: "SELECT 1"
    destination: { kind: file, path: /tmp/a.csv }
"#;
        let p = Pipeline::from_yaml(yaml).unwrap();
        assert_eq!(p.pipeline, "p");
        assert_eq!(p.flows.len(), 1);
        assert_eq!(p.flows[0].name, "a");
    }

    #[test]
    fn default_refresh_is_full() {
        let yaml = r#"
pipeline: p
flows:
  - name: a
    kind: materialized_view
    source: { kind: sql }
    query: "SELECT 1"
    destination: { kind: file, path: /tmp/a.csv }
"#;
        let p = Pipeline::from_yaml(yaml).unwrap();
        assert_eq!(p.flows[0].refresh, Refresh::Full);
    }
}
