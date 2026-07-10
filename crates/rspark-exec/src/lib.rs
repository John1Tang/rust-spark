//! Physical execution: operators that materialize [`RecordBatch`] streams
//! from a [`LogicalPlan`]. Used by both the local and cluster executors.

pub mod arrow_batch;
pub mod arrow_ops;
pub mod executor;
pub mod operators;
pub mod partition;

pub use arrow_batch::{arrow_from_core, arrow_to_core, ArrowBatch};
pub use arrow_ops::{filter, filter_via, limit, select_columns, sort_by_column};
pub use executor::{ExecutionContext, LocalExecutor};
pub use operators::{
    AggregateOp, FilterOp, JoinOp, LimitOp, PhysicalOp, ProjectOp, ScanOp, SortOp,
};
pub use partition::Partition;
