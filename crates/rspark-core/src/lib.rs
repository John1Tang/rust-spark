//! Core types: errors, value, schema, and records shared across crates.

pub mod error;
pub mod schema;
pub mod value;
pub mod record;
pub mod expr;

pub use error::{Error, Result};
pub use schema::{DataType, Field, Schema};
pub use value::Value;
pub use record::{Record, RecordBatch};
pub use expr::{BinaryOp, Expr, Literal};
