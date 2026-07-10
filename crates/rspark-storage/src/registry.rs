use crate::csv_source::CsvSource;
use crate::json_source::JsonSource;
use crate::source::{BoxedAsyncDataSource, BoxedDataSource};
use rspark_core::error::{Error, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

/// Source registry. Holds two parallel maps: one of sync sources
/// (CSV, JSON) and one of async sources (S3, Kafka, Arrow Flight).
/// Most queries hit the sync map; async-aware callers can reach for
/// the async map.
pub struct SourceRegistry {
    sources: RwLock<HashMap<String, BoxedDataSource>>,
    async_sources: RwLock<HashMap<String, BoxedAsyncDataSource>>,
}

impl SourceRegistry {
    pub fn with_defaults() -> Self {
        let mut map: HashMap<String, BoxedDataSource> = HashMap::new();
        map.insert(
            "csv".to_string(),
            Arc::new(CsvSource::new()) as BoxedDataSource,
        );
        map.insert(
            "json".to_string(),
            Arc::new(JsonSource::new()) as BoxedDataSource,
        );
        Self {
            sources: RwLock::new(map),
            async_sources: RwLock::new(HashMap::new()),
        }
    }

    pub fn register(&self, name: &str, source: BoxedDataSource) -> Result<()> {
        let mut sources = self
            .sources
            .write()
            .map_err(|e| Error::InvalidState(format!("registry lock poisoned: {e}")))?;
        sources.insert(name.to_string(), source);
        Ok(())
    }

    /// Register an async source (S3, Kafka, …). Stored in the
    /// parallel async map; sync callers can't see it.
    pub fn register_async(&self, name: &str, source: BoxedAsyncDataSource) -> Result<()> {
        let mut sources = self
            .async_sources
            .write()
            .map_err(|e| Error::InvalidState(format!("registry lock poisoned: {e}")))?;
        sources.insert(name.to_string(), source);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Result<BoxedDataSource> {
        self.sources
            .read()
            .map_err(|e| Error::InvalidState(format!("registry lock poisoned: {e}")))?
            .get(name)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("data source '{name}' not registered")))
    }

    pub fn get_async(&self, name: &str) -> Result<BoxedAsyncDataSource> {
        self.async_sources
            .read()
            .map_err(|e| Error::InvalidState(format!("registry lock poisoned: {e}")))?
            .get(name)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("async data source '{name}' not registered")))
    }

    pub fn infer_from_path(&self, path: &str) -> Result<BoxedDataSource> {
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let name = match ext.as_str() {
            "csv" => "csv",
            "json" | "jsonl" | "ndjson" => "json",
            other => {
                return Err(Error::Storage(format!(
                    "unsupported file extension: .{other}"
                )))
            }
        };
        self.get(name)
    }
}

impl Default for SourceRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}
