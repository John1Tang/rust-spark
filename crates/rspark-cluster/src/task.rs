use crate::stage::StageId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type TaskId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub stage_id: StageId,
    pub job_id: String,
    pub partition_label: String,
    pub sql: String,
    pub status: TaskStatus,
    pub assigned_worker: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub result_rows: Option<usize>,
    pub error: Option<String>,
}

impl Task {
    pub fn new(
        job_id: impl Into<String>,
        stage_id: impl Into<String>,
        partition_label: impl Into<String>,
        sql: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            stage_id: stage_id.into(),
            job_id: job_id.into(),
            partition_label: partition_label.into(),
            sql: sql.into(),
            status: TaskStatus::Pending,
            assigned_worker: None,
            started_at: None,
            completed_at: None,
            result_rows: None,
            error: None,
        }
    }

    pub fn duration_ms(&self) -> Option<i64> {
        match (self.started_at, self.completed_at) {
            (Some(start), Some(end)) => Some((end - start).num_milliseconds()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Assigned,
    Running,
    Succeeded,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAttempt {
    pub task_id: TaskId,
    pub worker_id: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub success: Option<bool>,
    pub error: Option<String>,
    pub rows: Option<usize>,
}
