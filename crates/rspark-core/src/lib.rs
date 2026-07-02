//! Core types: errors, value, schema, and records shared across crates.

pub mod error;
pub mod expr;
pub mod record;
pub mod schema;
pub mod value;

pub use error::{Error, Result};
pub use expr::{BinaryOp, Expr, Literal};
pub use record::{Record, RecordBatch};
pub use schema::{DataType, Field, Schema};
pub use value::Value;
