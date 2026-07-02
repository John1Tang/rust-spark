//! Physical execution: operators that materialize [`RecordBatch`] streams
//! from a [`LogicalPlan`]. Used by both the local and cluster executors.

pub mod executor;
pub mod operators;
pub mod partition;

pub use executor::{ExecutionContext, LocalExecutor};
pub use operators::{
    AggregateOp, FilterOp, JoinOp, LimitOp, PhysicalOp, ProjectOp, ScanOp, SortOp,
};
pub use partition::Partition;
