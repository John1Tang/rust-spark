//! Cluster coordination: master node, worker nodes, jobs, stages, tasks.
//!
//! The master keeps a [`ClusterState`] in memory and exposes it via HTTP
//! (see `rspark-api`). Workers register themselves, poll for tasks, run them
//! against their local [`ExecutionContext`], and report results back.

pub mod job;
pub mod master;
pub mod partitioner;
pub mod stage;
pub mod state;
pub mod task;
pub mod worker;

pub use job::{Job, JobId, JobRequest, JobStatus};
pub use master::Master;
pub use partitioner::{plan_partitions, PartitionSpec};
pub use stage::{Stage, StageId, StageStatus};
pub use state::{ClusterState, WorkerInfo};
pub use task::{Task, TaskAttempt, TaskId, TaskStatus};
pub use worker::Worker;
