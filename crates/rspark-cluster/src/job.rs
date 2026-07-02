use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type JobId = String;
pub type StageId = String;
pub type TaskId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRequest {
    pub name: String,
    pub sql: String,
    pub submission_time: DateTime<Utc>,
    pub parallelism: usize,
    pub input_paths: Vec<String>,
}

impl JobRequest {
    pub fn new(name: impl Into<String>, sql: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            sql: sql.into(),
            submission_time: Utc::now(),
            parallelism: 1,
            input_paths: vec![],
        }
    }

    pub fn with_parallelism(mut self, p: usize) -> Self {
        self.parallelism = p.max(1);
        self
    }

    pub fn with_input_paths(mut self, paths: Vec<String>) -> Self {
        self.input_paths = paths;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum JobStatus {
    Pending,
    Running,
    Succeeded,
    Failed(String),
    Cancelled,
}

impl JobStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, JobStatus::Succeeded | JobStatus::Failed(_) | JobStatus::Cancelled)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: JobId,
    pub name: String,
    pub sql: String,
    pub status: JobStatus,
    pub submitted_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub stages: Vec<StageId>,
    pub result_rows: Option<usize>,
    pub error: Option<String>,
    pub submission_time: DateTime<Utc>,
    pub parallelism: usize,
}

impl Job {
    pub fn new(name: impl Into<String>, sql: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            sql: sql.into(),
            status: JobStatus::Pending,
            submitted_at: now,
            started_at: None,
            completed_at: None,
            stages: vec![],
            result_rows: None,
            error: None,
            submission_time: now,
            parallelism: 1,
        }
    }

    pub fn duration_ms(&self) -> Option<i64> {
        match (self.started_at, self.completed_at) {
            (Some(start), Some(end)) => Some((end - start).num_milliseconds()),
            _ => None,
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self.status, JobStatus::Running)
    }
}
