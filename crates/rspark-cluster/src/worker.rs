use crate::state::{ClusterState, WorkerInfo};
use rspark_core::error::Result;
use rspark_exec::{ExecutionContext, LocalExecutor};
use rspark_sql::Planner;
use std::sync::Arc;

pub struct Worker {
    state: ClusterState,
    pub info: WorkerInfo,
    pub context: ExecutionContext,
    planner: Planner,
}

impl Worker {
    pub fn new(
        state: ClusterState,
        address: impl Into<String>,
        cores: usize,
        memory_mb: usize,
        context: ExecutionContext,
    ) -> Self {
        let info = WorkerInfo::new(address, cores, memory_mb);
        Self {
            state,
            info,
            context,
            planner: Planner::new(),
        }
    }

    pub fn id(&self) -> &str {
        &self.info.id
    }

    pub fn register(&mut self) -> Result<()> {
        self.info.last_heartbeat = chrono::Utc::now();
        self.state.register_worker(self.info.clone());
        Ok(())
    }

    pub fn heartbeat(&self) {
        self.state.update_worker_heartbeat(&self.info.id);
    }

    /// Run a single task against the local execution context.
    pub fn execute_task(
        &self,
        task: &crate::task::Task,
        catalog: &dyn rspark_sql::planner::Catalog,
    ) -> Result<rspark_core::RecordBatch> {
        let plan = self.planner.plan_sql(&task.sql, catalog)?;
        let executor = LocalExecutor::new(&self.context);
        executor.execute(&plan)
    }
}

/// Stand-alone helper for cluster clients that only need the execution context.
pub fn default_context() -> Arc<rspark_storage::SourceRegistry> {
    Arc::new(rspark_storage::SourceRegistry::with_defaults())
}
