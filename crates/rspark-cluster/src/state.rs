use crate::job::Job;
use crate::stage::Stage;
use crate::task::{Task, TaskId, TaskStatus};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerInfo {
    pub id: String,
    pub address: String,
    pub last_heartbeat: DateTime<Utc>,
    pub cores: usize,
    pub memory_mb: usize,
    pub status: WorkerStatus,
    pub running_tasks: Vec<TaskId>,
}

impl WorkerInfo {
    pub fn new(address: impl Into<String>, cores: usize, memory_mb: usize) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            address: address.into(),
            last_heartbeat: Utc::now(),
            cores,
            memory_mb,
            status: WorkerStatus::Alive,
            running_tasks: vec![],
        }
    }

    pub fn is_alive(&self) -> bool {
        matches!(self.status, WorkerStatus::Alive)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkerStatus {
    Alive,
    Dead,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterSnapshot {
    pub master_id: String,
    pub captured_at: DateTime<Utc>,
    pub workers: Vec<WorkerInfo>,
    pub jobs: Vec<Job>,
    pub stages: Vec<Stage>,
    pub tasks: Vec<Task>,
    pub pending_queue: Vec<TaskId>,
    pub running_round: u64,
    pub total_completed_rounds: u64,
    pub total_runs: u64,
}

#[derive(Clone)]
pub struct ClusterState {
    inner: Arc<ClusterInner>,
}

struct ClusterInner {
    master_id: String,
    workers: RwLock<HashMap<String, WorkerInfo>>,
    jobs: RwLock<HashMap<String, Job>>,
    stages: RwLock<HashMap<String, Stage>>,
    tasks: RwLock<HashMap<String, Task>>,
    pending: RwLock<VecDeque<String>>,
    running_round: RwLock<u64>,
    total_runs: RwLock<u64>,
    total_completed_rounds: RwLock<u64>,
}

impl ClusterState {
    pub fn new(master_id: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(ClusterInner {
                master_id: master_id.into(),
                workers: RwLock::new(HashMap::new()),
                jobs: RwLock::new(HashMap::new()),
                stages: RwLock::new(HashMap::new()),
                tasks: RwLock::new(HashMap::new()),
                pending: RwLock::new(VecDeque::new()),
                running_round: RwLock::new(0),
                total_runs: RwLock::new(0),
                total_completed_rounds: RwLock::new(0),
            }),
        }
    }

    pub fn master_id(&self) -> String {
        self.inner.master_id.clone()
    }

    pub fn register_worker(&self, worker: WorkerInfo) {
        self.inner.workers.write().insert(worker.id.clone(), worker);
    }

    pub fn remove_worker(&self, worker_id: &str) {
        self.inner.workers.write().remove(worker_id);
    }

    pub fn update_worker_heartbeat(&self, worker_id: &str) {
        if let Some(w) = self.inner.workers.write().get_mut(worker_id) {
            w.last_heartbeat = Utc::now();
        }
    }

    pub fn list_workers(&self) -> Vec<WorkerInfo> {
        self.inner.workers.read().values().cloned().collect()
    }

    pub fn worker(&self, worker_id: &str) -> Option<WorkerInfo> {
        self.inner.workers.read().get(worker_id).cloned()
    }

    pub fn insert_job(&self, job: Job) {
        self.inner.jobs.write().insert(job.id.clone(), job);
    }

    pub fn update_job(&self, job: Job) {
        self.inner.jobs.write().insert(job.id.clone(), job);
    }

    pub fn job(&self, job_id: &str) -> Option<Job> {
        self.inner.jobs.read().get(job_id).cloned()
    }

    pub fn list_jobs(&self) -> Vec<Job> {
        self.inner.jobs.read().values().cloned().collect()
    }

    pub fn insert_stage(&self, stage: Stage) {
        self.inner.stages.write().insert(stage.id.clone(), stage);
    }

    pub fn update_stage(&self, stage: Stage) {
        self.inner.stages.write().insert(stage.id.clone(), stage);
    }

    pub fn stage(&self, stage_id: &str) -> Option<Stage> {
        self.inner.stages.read().get(stage_id).cloned()
    }

    pub fn list_stages(&self) -> Vec<Stage> {
        self.inner.stages.read().values().cloned().collect()
    }

    pub fn stages_for_job(&self, job_id: &str) -> Vec<Stage> {
        self.inner
            .stages
            .read()
            .values()
            .filter(|s| s.job_id == job_id)
            .cloned()
            .collect()
    }

    pub fn insert_task(&self, task: Task) {
        let id = task.id.clone();
        self.inner.tasks.write().insert(id.clone(), task);
        self.inner.pending.write().push_back(id);
    }

    pub fn update_task(&self, task: Task) {
        let id = task.id.clone();
        let was_terminal = matches!(
            self.inner.tasks.read().get(&id).map(|t| t.status.clone()),
            Some(TaskStatus::Succeeded) | Some(TaskStatus::Failed(_))
        );
        self.inner.tasks.write().insert(id.clone(), task.clone());
        if !was_terminal && matches!(task.status, TaskStatus::Succeeded | TaskStatus::Failed(_)) {
            self.inner.pending.write().retain(|t| t != &id);
            if matches!(task.status, TaskStatus::Succeeded) {
                *self.inner.total_runs.write() += 1;
            }
        }
    }

    pub fn task(&self, task_id: &str) -> Option<Task> {
        self.inner.tasks.read().get(task_id).cloned()
    }

    pub fn list_tasks(&self) -> Vec<Task> {
        self.inner.tasks.read().values().cloned().collect()
    }

    pub fn tasks_for_job(&self, job_id: &str) -> Vec<Task> {
        self.inner
            .tasks
            .read()
            .values()
            .filter(|t| t.job_id == job_id)
            .cloned()
            .collect()
    }

    pub fn pop_pending_task(&self) -> Option<Task> {
        let id = self.inner.pending.write().pop_front()?;
        self.inner.tasks.read().get(&id).cloned()
    }

    pub fn pending_count(&self) -> usize {
        self.inner.pending.read().len()
    }

    pub fn running_round(&self) -> u64 {
        *self.inner.running_round.read()
    }

    pub fn inc_running_round(&self) -> u64 {
        let mut guard = self.inner.running_round.write();
        *guard += 1;
        *guard
    }

    pub fn total_runs(&self) -> u64 {
        *self.inner.total_runs.read()
    }

    pub fn total_completed_rounds(&self) -> u64 {
        *self.inner.total_completed_rounds.read()
    }

    pub fn record_completed_round(&self) {
        *self.inner.total_completed_rounds.write() += 1;
    }

    pub fn snapshot(&self) -> ClusterSnapshot {
        ClusterSnapshot {
            master_id: self.inner.master_id.clone(),
            captured_at: Utc::now(),
            workers: self.list_workers(),
            jobs: self.list_jobs(),
            stages: self.list_stages(),
            tasks: self.list_tasks(),
            pending_queue: self.inner.pending.read().iter().cloned().collect(),
            running_round: *self.inner.running_round.read(),
            total_completed_rounds: *self.inner.total_completed_rounds.read(),
            total_runs: *self.inner.total_runs.read(),
        }
    }
}

pub fn default_master_id() -> String {
    format!("master-{}", short_id())
}

fn short_id() -> String {
    Uuid::new_v4()
        .to_string()
        .split('-')
        .next()
        .unwrap_or("0")
        .to_string()
}
