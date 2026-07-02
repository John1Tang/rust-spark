use crate::job::{Job, JobRequest, JobStatus};
use crate::partitioner::plan_partitions;
use crate::stage::{Stage, StageStatus};
use crate::state::{ClusterState, WorkerInfo};
use crate::task::Task;
use chrono::Utc;
use parking_lot::Mutex;
use rspark_core::error::{Error, Result};
use rspark_sql::Planner;
use std::sync::Arc;

pub struct Master {
    state: ClusterState,
    planner: Planner,
    callback: Arc<Mutex<Option<StageCallback>>>,
}

type StageCallback = Box<dyn Fn(&Stage) + Send + Sync>;

impl Master {
    pub fn new(state: ClusterState) -> Self {
        Self {
            state,
            planner: Planner::new(),
            callback: Arc::new(Mutex::new(None)),
        }
    }

    pub fn state(&self) -> ClusterState {
        self.state.clone()
    }

    pub fn set_callback<F>(&mut self, callback: F)
    where
        F: Fn(&Stage) + Send + Sync + 'static,
    {
        *self.callback.lock() = Some(Box::new(callback));
    }

    pub fn register_worker(&self, info: WorkerInfo) {
        self.state.register_worker(info);
    }

    /// Submit a new SQL job, expanding it into a single stage containing
    /// partition tasks. In a future revision this will split into scan and
    /// result stages.
    pub fn submit_job(
        &self,
        request: JobRequest,
        catalog: &dyn rspark_sql::planner::Catalog,
    ) -> Result<Job> {
        let mut job = Job::new(request.name.clone(), request.sql.clone());
        job.parallelism = request.parallelism.max(1);
        let plan = self.planner.plan_sql(&request.sql, catalog)?;
        let partitions = plan_partitions(&plan, request.parallelism)?;
        let mut stage = Stage::new(job.id.clone(), 0, "main");
        stage.tasks = partitions
            .iter()
            .map(|p| {
                Task::new(
                    job.id.clone(),
                    stage.id.clone(),
                    p.label.clone(),
                    request.sql.clone(),
                )
            })
            .collect();
        let stage_id = stage.id.clone();
        self.state.insert_stage(stage);
        for task in self.state.stage(&stage_id).unwrap().tasks.clone() {
            self.state.insert_task(task);
        }
        job.stages = vec![stage_id.clone()];
        let job_id = job.id.clone();
        self.state.insert_job(job.clone());

        let mut updated = job.clone();
        updated.status = JobStatus::Running;
        updated.started_at = Some(Utc::now());
        self.state.update_job(updated);

        if let Some(cb) = self.callback.lock().as_ref() {
            if let Some(s) = self.state.stage(&stage_id) {
                cb(&s);
            }
        }
        let _ = job_id;
        Ok(self.state.job(&job.id).unwrap_or(job))
    }

    pub fn try_assign_task(&self, worker_id: &str) -> Result<Option<Task>> {
        let task = self.state.pop_pending_task();
        if let Some(mut task) = task {
            task.assigned_worker = Some(worker_id.to_string());
            task.status = crate::task::TaskStatus::Assigned;
            self.state.update_task(task.clone());
            if let Some(stage_id) = Some(task.stage_id.clone()) {
                if let Some(mut stage) = self.state.stage(&stage_id) {
                    if let Some(slot) = stage.tasks.iter_mut().find(|t| t.id == task.id) {
                        slot.assigned_worker = task.assigned_worker.clone();
                        slot.status = task.status.clone();
                    }
                    self.state.update_stage(stage);
                }
            }
            return Ok(Some(task));
        }
        Ok(None)
    }

    /// Record a job without planning or scheduling it. Used for statements
    /// (like `SHOW CREATE TABLE`) that the master handles inline rather than
    /// dispatching to workers.
    pub fn submit_job_skip_plan(&self, request: JobRequest) -> Result<Job> {
        let mut job = Job::new(request.name.clone(), request.sql.clone());
        job.parallelism = request.parallelism.max(1);
        job.status = JobStatus::Running;
        job.started_at = Some(Utc::now());
        self.state.insert_job(job.clone());
        Ok(job)
    }

    pub fn complete_task(
        &self,
        task_id: &str,
        rows: usize,
        success: bool,
        error: Option<String>,
    ) -> Result<()> {
        let mut task = self
            .state
            .task(task_id)
            .ok_or_else(|| Error::NotFound(format!("task {task_id} not found")))?;
        task.completed_at = Some(Utc::now());
        task.result_rows = Some(rows);
        task.status = if success {
            crate::task::TaskStatus::Succeeded
        } else {
            crate::task::TaskStatus::Failed(error.clone().unwrap_or_default())
        };
        if !success {
            task.error = error.clone();
        }
        self.state.update_task(task.clone());
        if let Some(mut stage) = self.state.stage(&task.stage_id) {
            if let Some(slot) = stage.tasks.iter_mut().find(|t| t.id == task.id) {
                slot.status = task.status.clone();
                slot.completed_at = task.completed_at;
                slot.result_rows = task.result_rows;
                slot.error = task.error.clone();
            }
            stage.status = if stage.is_complete() {
                StageStatus::Succeeded
            } else if stage
                .tasks
                .iter()
                .any(|t| matches!(t.status, crate::task::TaskStatus::Failed(_)))
            {
                StageStatus::Failed("one or more tasks failed".into())
            } else {
                StageStatus::Running
            };
            self.state.update_stage(stage);
        }
        let job_id = task.job_id.clone();
        let job_tasks = self.state.tasks_for_job(&job_id);
        let mut job = self.state.job(&job_id).unwrap();
        let all_succeeded = !job_tasks.is_empty()
            && job_tasks
                .iter()
                .all(|t| matches!(t.status, crate::task::TaskStatus::Succeeded));
        let any_failed = job_tasks
            .iter()
            .any(|t| matches!(t.status, crate::task::TaskStatus::Failed(_)));
        if all_succeeded {
            job.status = JobStatus::Succeeded;
            job.completed_at = Some(Utc::now());
            job.result_rows = Some(job_tasks.iter().filter_map(|t| t.result_rows).sum());
        } else if any_failed {
            job.status = JobStatus::Failed(
                job_tasks
                    .iter()
                    .find_map(|t| match &t.status {
                        crate::task::TaskStatus::Failed(msg) => Some(msg.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| "task failed".into()),
            );
            job.completed_at = Some(Utc::now());
        }
        self.state.update_job(job);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::default_master_id;
    use rspark_core::schema::{DataType, Field, Schema};
    use std::collections::HashMap;
    use std::sync::RwLock;

    struct InMemoryCatalog {
        tables: RwLock<HashMap<String, (String, String, Schema)>>,
    }

    impl InMemoryCatalog {
        fn new() -> Self {
            Self {
                tables: RwLock::new(HashMap::new()),
            }
        }
        fn register(&self, name: &str, path: &str) {
            let schema = Schema::new(vec![
                Field::new("id", DataType::Int64),
                Field::new("name", DataType::String),
            ]);
            self.tables.write().unwrap().insert(
                name.to_string(),
                (path.to_string(), "csv".to_string(), schema),
            );
        }
    }

    impl rspark_sql::planner::Catalog for InMemoryCatalog {
        fn table_schema(&self, name: &str) -> Result<Schema> {
            self.tables
                .read()
                .unwrap()
                .get(name)
                .map(|t| t.2.clone())
                .ok_or_else(|| Error::NotFound(format!("table '{name}' not found")))
        }
        fn table_location(&self, name: &str) -> Result<(String, String)> {
            self.tables
                .read()
                .unwrap()
                .get(name)
                .map(|t| (t.0.clone(), t.1.clone()))
                .ok_or_else(|| Error::NotFound(format!("table '{name}' not found")))
        }
        fn list_tables(&self) -> Result<Vec<String>> {
            Ok(self.tables.read().unwrap().keys().cloned().collect())
        }
        fn register_table(
            &mut self,
            name: &str,
            path: &str,
            source: &str,
            schema: Schema,
        ) -> Result<()> {
            self.tables.write().unwrap().insert(
                name.to_string(),
                (path.to_string(), source.to_string(), schema),
            );
            Ok(())
        }
    }

    #[test]
    fn submit_creates_stage_and_tasks() {
        let state = ClusterState::new(default_master_id());
        let master = Master::new(state.clone());
        let mut catalog = InMemoryCatalog::new();
        catalog.register("users", "/tmp/users.csv");
        let job = master
            .submit_job(
                JobRequest::new("test", "SELECT id, name FROM users").with_parallelism(2),
                &catalog,
            )
            .unwrap();
        assert!(job.is_running());
        let stages = state.stages_for_job(&job.id);
        assert_eq!(stages.len(), 1);
        assert_eq!(stages[0].tasks.len(), 2);
        assert_eq!(state.pending_count(), 2);
    }

    #[test]
    fn complete_task_marks_job_succeeded() {
        let state = ClusterState::new(default_master_id());
        let master = Master::new(state.clone());
        let mut catalog = InMemoryCatalog::new();
        catalog.register("users", "/tmp/users.csv");
        let job = master
            .submit_job(
                JobRequest::new("test", "SELECT id FROM users").with_parallelism(1),
                &catalog,
            )
            .unwrap();
        let task = master.try_assign_task("worker-1").unwrap().unwrap();
        master.complete_task(&task.id, 100, true, None).unwrap();
        let updated = state.job(&job.id).unwrap();
        assert_eq!(updated.status, JobStatus::Succeeded);
        assert_eq!(updated.result_rows, Some(100));
    }
}
