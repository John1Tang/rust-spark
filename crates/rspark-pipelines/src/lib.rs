//! Declarative Pipeline runner for rspark.
//!
//! A pipeline is a YAML spec describing a DAG of flows. Each flow is
//! either a `StreamingTable` (incremental) or a `MaterializedView` (full
//! refresh). The runner resolves the DAG into layers (a topological
//! order), executes each layer in order, and writes each flow's output
//! to its declared destination.
//!
//! Mirrors Spark Declarative Pipelines / Lakeflow SDP at a learning-
//! project scale. Two source kinds ship today:
//!
//! * [`crate::source::FileTailSource`] — tails NDJSON files in a
//!   directory, remembering the last byte offset per file.
//! * [`crate::source::KafkaSource`] — stubs out a Kafka source (no
//!   in-cluster broker yet); reads via `kafkacat` if installed.
//!
//! Future S3 sink writes go through [`crate::sink::write_destination`].

pub mod dag;
pub mod runner;
pub mod sink;
pub mod source;
pub mod spec;

pub use dag::{FlowId, PipelineDag};
pub use runner::{PipelineRunReport, PipelineRunner, RunStats};
pub use sink::{describe, write_destination};
pub use source::{FileTailSource, KafkaSource, NullSource, StreamingSource};
pub use spec::{Destination, Flow, FlowKind, Pipeline, Refresh, SourceSpec};
