//! Streaming source trait + reference implementations.
//!
//! A [`StreamingSource`] is anything that can produce a [`RecordBatch`]
//! of rows on demand: a file-tail reader, a Kafka consumer, etc. The
//! pipeline runner calls [`poll_batch`] once per micro-batch, runs the
//! flow's SQL, and writes the output to the flow's destination.
//!
//! [`poll_batch`]: StreamingSource::poll_batch

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
use rspark_core::error::{Error, Result};
use rspark_core::schema::{DataType, Field, Schema};
use rspark_core::{Record, RecordBatch};
use serde_json::Value as JsonValue;
use tracing::warn;

/// Streaming source. `poll_batch` returns 0 rows when there's nothing
/// new since the last call (micro-batch source semantics).
#[async_trait]
pub trait StreamingSource: Send {
    async fn poll_batch(&mut self) -> Result<RecordBatch>;

    /// Optional commit hook for sources that need to persist offsets
    /// (Kafka offsets, file-tail positions). Defaults to no-op.
    async fn commit(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Always-empty source. Useful as a placeholder in YAML specs.
pub struct NullSource;

#[async_trait]
impl StreamingSource for NullSource {
    async fn poll_batch(&mut self) -> Result<RecordBatch> {
        Ok(RecordBatch::new(Schema::empty()))
    }
}

/// Tails NDJSON files in a directory. Each call to `poll_batch` reads
/// one full file from the directory (sorted by name), appending to a
/// returned `RecordBatch`. File positions are remembered per filename
/// so a single file is never re-read after being committed.
///
/// For simplicity this implementation reads the whole file in one go
/// (no incremental offset tracking) — a learning-project scale is fine
/// with that.
pub struct FileTailSource {
    dir: PathBuf,
    state: Mutex<FileTailState>,
}

struct FileTailState {
    /// Filenames we've already emitted, in sorted order.
    seen: std::collections::BTreeSet<String>,
}

impl FileTailSource {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            state: Mutex::new(FileTailState {
                seen: Default::default(),
            }),
        }
    }

    fn next_unseen(&self) -> Result<Option<PathBuf>> {
        let entries = std::fs::read_dir(&self.dir).map_err(Error::Io)?;
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file())
            .collect();
        paths.sort();
        let state = self.state.lock().unwrap();
        for p in paths {
            let name = p
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            if !state.seen.contains(&name) {
                return Ok(Some(p));
            }
        }
        Ok(None)
    }

    fn read_ndjson(path: &Path) -> Result<RecordBatch> {
        let file = File::open(path).map_err(Error::Io)?;
        let reader = BufReader::new(file);
        let mut rows: Vec<HashMap<String, JsonValue>> = Vec::new();
        for (i, line) in reader.lines().enumerate() {
            let line = line.map_err(Error::Io)?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let v: JsonValue = serde_json::from_str(trimmed).map_err(|e| {
                Error::Parse(format!("ndjson line {} in {}: {e}", i + 1, path.display()))
            })?;
            if let JsonValue::Object(map) = v {
                rows.push(map.into_iter().collect::<HashMap<String, JsonValue>>());
            }
        }
        ndjson_rows_to_batch(&rows)
    }
}

#[async_trait]
impl StreamingSource for FileTailSource {
    async fn poll_batch(&mut self) -> Result<RecordBatch> {
        let Some(path) = self.next_unseen()? else {
            return Ok(RecordBatch::new(Schema::empty()));
        };
        let batch = Self::read_ndjson(&path)?;
        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            self.state.lock().unwrap().seen.insert(name.to_string());
        }
        Ok(batch)
    }

    async fn commit(&mut self) -> Result<()> {
        // In a real implementation we'd persist `seen` to a sidecar
        // `.watermark` file. For now, memory only — a restart will
        // re-emit. That's fine for a learning project.
        Ok(())
    }
}

/// Kafka source — consumes one micro-batch per `poll_batch` call.
/// Each batch is decoded as NDJSON (one event per line) and exposed
/// as a `RecordBatch` with a union schema over all keys observed.
///
/// The underlying [`rdkafka::consumer::StreamConsumer`] is async; we
/// wrap it with a 1s poll timeout so `poll_batch` returns quickly when
/// the topic is idle (the runner treats an empty batch as "nothing
/// new" rather than an error).
pub struct KafkaSource {
    pub topic: String,
    pub brokers: String,
    pub group_id: String,
    consumer: rdkafka::consumer::StreamConsumer,
}

impl KafkaSource {
    pub fn new(
        topic: impl Into<String>,
        brokers: impl Into<String>,
        group_id: impl Into<String>,
    ) -> Result<Self> {
        use rdkafka::config::ClientConfig;
        use rdkafka::consumer::Consumer;
        let topic = topic.into();
        let brokers = brokers.into();
        let group_id = group_id.into();
        let consumer: rdkafka::consumer::StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", &brokers)
            .set("group.id", &group_id)
            .set("enable.auto.commit", "true")
            .set("auto.offset.reset", "earliest")
            .set("session.timeout.ms", "6000")
            .create()
            .map_err(|e| Error::Storage(format!("kafka producer: {e}")))?;
        consumer
            .subscribe(&[&topic])
            .map_err(|e| Error::Storage(format!("kafka subscribe {topic}: {e}")))?;
        Ok(Self {
            topic,
            brokers,
            group_id,
            consumer,
        })
    }
}

#[async_trait]
impl StreamingSource for KafkaSource {
    async fn poll_batch(&mut self) -> Result<RecordBatch> {
        use rdkafka::Message;
        let mut rows: Vec<HashMap<String, JsonValue>> = Vec::new();
        // Drain up to 100 messages with a short per-message timeout.
        // rdkafka's recv is async; we race it against a tokio sleep.
        for _ in 0..100 {
            let recv = self.consumer.recv();
            let timeout = tokio::time::sleep(std::time::Duration::from_millis(50));
            tokio::pin!(timeout);
            tokio::select! {
                msg = recv => {
                    let Ok(msg) = msg else { break };
                    let Some(payload) = msg.payload() else { continue };
                    let text = match std::str::from_utf8(payload) {
                        Ok(s) => s,
                        Err(e) => {
                            warn!("non-utf8 payload skipped: {e}");
                            continue;
                        }
                    };
                    // Each Kafka value is a single JSON object.
                    let v: JsonValue = match serde_json::from_str(text) {
                        Ok(v) => v,
                        Err(e) => {
                            warn!("bad json skipped: {e}");
                            continue;
                        }
                    };
                    if let JsonValue::Object(map) = v {
                        rows.push(map.into_iter().collect());
                    }
                }
                _ = &mut timeout => break,
            }
        }
        if rows.is_empty() {
            return Ok(RecordBatch::new(Schema::empty()));
        }
        ndjson_rows_to_batch(&rows)
    }

    async fn commit(&mut self) -> Result<()> {
        // auto.commit=true handles offset commits for us. A future
        // "exactly once" implementation would call commit_consumer_state
        // here.
        Ok(())
    }
}

/// Convert a list of JSON-object rows to a `RecordBatch` with a union
/// schema (all keys observed across rows, in insertion order).
fn json_value_to_rspark(v: &JsonValue) -> rspark_core::value::Value {
    match v {
        JsonValue::String(s) => rspark_core::value::Value::String(s.clone()),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                rspark_core::value::Value::Int64(i)
            } else if let Some(f) = n.as_f64() {
                rspark_core::value::Value::Float64(f)
            } else {
                rspark_core::value::Value::Null
            }
        }
        JsonValue::Bool(b) => rspark_core::value::Value::Boolean(*b),
        JsonValue::Null => rspark_core::value::Value::Null,
        other => rspark_core::value::Value::String(other.to_string()),
    }
}

/// Convert a list of JSON-object rows to a `RecordBatch` with a union
/// schema (all keys observed across rows, in insertion order).
fn ndjson_rows_to_batch(rows: &[HashMap<String, JsonValue>]) -> Result<RecordBatch> {
    let mut key_order: Vec<String> = Vec::new();
    for row in rows {
        for k in row.keys() {
            if !key_order.contains(k) {
                key_order.push(k.clone());
            }
        }
    }
    let schema = Schema::new(
        key_order
            .iter()
            .map(|k| Field::new(k, DataType::String))
            .collect::<Vec<_>>(),
    );
    let mut records = Vec::with_capacity(rows.len());
    for row in rows {
        let values: Vec<rspark_core::value::Value> = key_order
            .iter()
            .map(|k| {
                row.get(k)
                    .map(json_value_to_rspark)
                    .unwrap_or(rspark_core::value::Value::Null)
            })
            .collect();
        records.push(Record::new(values));
    }
    RecordBatch::from_records(schema, records)
}

/// Like [`read_ndjson`] but takes an already-open `BufRead`.
pub fn read_ndjson_reader<R: BufRead>(reader: R) -> Result<RecordBatch> {
    let mut rows: Vec<HashMap<String, JsonValue>> = Vec::new();
    for (i, line) in reader.lines().enumerate() {
        let line = line.map_err(Error::Io)?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let v: JsonValue = serde_json::from_str(trimmed)
            .map_err(|e| Error::Parse(format!("ndjson line {i}: {e}")))?;
        if let JsonValue::Object(map) = v {
            rows.push(map.into_iter().collect());
        }
    }
    ndjson_rows_to_batch(&rows)
}

/// Seek a file to a position before reading. Used by tests.
#[allow(dead_code)]
pub fn open_at(path: &Path, pos: u64) -> Result<std::fs::File> {
    let mut f = OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(Error::Io)?;
    f.seek(SeekFrom::Start(pos)).map_err(Error::Io)?;
    Ok(f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[tokio::test]
    async fn null_source_returns_empty() {
        let mut s = NullSource;
        let b = s.poll_batch().await.unwrap();
        assert_eq!(b.len(), 0);
    }

    #[tokio::test]
    async fn file_tail_reads_new_files() {
        let dir = tempdir();
        write_ndjson(&dir.join("a.ndjson"), "{\"k\":\"v1\"}\n{\"k\":\"v2\"}\n");
        let mut src = FileTailSource::new(&dir);
        let b = src.poll_batch().await.unwrap();
        assert_eq!(b.len(), 2);
        // Second poll returns empty (already committed).
        let b2 = src.poll_batch().await.unwrap();
        assert_eq!(b2.len(), 0);
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rspark-pipelines-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_ndjson(path: &Path, body: &str) {
        let mut f = File::create(path).unwrap();
        f.write_all(body.as_bytes()).unwrap();
    }
}
