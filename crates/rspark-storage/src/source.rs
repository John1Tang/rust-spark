use async_trait::async_trait;
use rspark_core::error::Result;
use rspark_core::{RecordBatch, Schema};
use std::sync::Arc;

/// Trait implemented by every data source (CSV, JSON, Parquet, …).
///
/// [`DataSource::scan`] is synchronous; the cluster layer wraps it in a
/// [`tokio::task::spawn_blocking`] so it does not block the worker reactor.
pub trait DataSource: Send + Sync {
    fn name(&self) -> &'static str;
    fn infer_schema(&self, path: &str) -> Result<Schema>;
    fn scan(&self, path: &str, schema: Option<&Schema>) -> Result<RecordBatch>;
}

pub type BoxedDataSource = Arc<dyn DataSource>;

/// Async variant of [`DataSource`] used by the cluster executor.
#[async_trait]
pub trait AsyncDataSource: Send + Sync {
    fn name(&self) -> &'static str;
    async fn infer_schema(&self, path: &str) -> Result<Schema>;
    async fn scan(&self, path: &str, schema: Option<&Schema>) -> Result<RecordBatch>;
}

pub type BoxedAsyncDataSource = Arc<dyn AsyncDataSource>;
