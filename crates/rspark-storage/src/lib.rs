//! Storage layer: CSV / JSON / Parquet readers and writers.
//!
//! Sources are simple synchronous readers today; cluster execution moves them
//! behind an async trait so workers can stream partitions over HTTP.

pub mod csv_source;
pub mod json_source;
pub mod registry;
pub mod source;
pub mod writer;

pub use csv_source::CsvSource;
pub use json_source::JsonSource;
pub use registry::SourceRegistry;
pub use source::{BoxedDataSource, DataSource};
pub use writer::OutputWriter;
