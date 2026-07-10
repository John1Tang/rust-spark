//! Storage layer: CSV / JSON / S3-compatible readers and writers.
//!
//! Sources are simple synchronous readers today; the async twin
//! ([`AsyncDataSource`]) covers S3, Kafka, and any future
//! network-backed reader. The cluster layer wraps the sync readers
//! in [`tokio::task::spawn_blocking`] so they don't block the reactor.

pub mod csv_source;
pub mod json_source;
pub mod registry;
pub mod s3_source;
pub mod s3_writer;
pub mod source;
pub mod writer;

pub use csv_source::CsvSource;
pub use json_source::JsonSource;
pub use registry::SourceRegistry;
pub use s3_source::{S3Config, S3Source};
pub use s3_writer::S3Writer;
pub use source::{AsyncDataSource, BoxedAsyncDataSource, BoxedDataSource, DataSource};
pub use writer::OutputWriter;
