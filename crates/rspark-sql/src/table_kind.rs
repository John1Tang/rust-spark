//! Catalog table kind. Drives how the dashboard groups autocomplete
//! suggestions and how the planner decides whether a table can be the
//! source of a streaming-table flow.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableKind {
    /// A regular CSV/JSON/Parquet file (or S3 object) scanned each query.
    #[default]
    Batch,
    /// The output of a streaming-table flow — backed by a Kafka topic
    /// (or another streaming source) and refreshed continuously.
    StreamingTable,
    /// The output of a materialised-view flow — full-refresh on each
    /// pipeline run, derived from upstream flows.
    MaterializedView,
}

impl TableKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TableKind::Batch => "batch",
            TableKind::StreamingTable => "streaming_table",
            TableKind::MaterializedView => "materialized_view",
        }
    }
}