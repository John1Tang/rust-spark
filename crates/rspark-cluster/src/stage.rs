use crate::task::{Task, TaskId, TaskStatus};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type StageId = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stage {
    pub id: StageId,
    pub job_id: String,
    pub index: usize,
    pub label: String,
    pub status: StageStatus,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub tasks: Vec<Task>,
}

impl Stage {
    pub fn new(job_id: impl Into<String>, index: usize, label: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            job_id: job_id.into(),
            index,
            label: label.into(),
            status: StageStatus::Pending,
            started_at: None,
            completed_at: None,
            tasks: vec![],
        }
    }

    pub fn progress(&self) -> (usize, usize) {
        let done = self
            .tasks
            .iter()
            .filter(|t| matches!(t.status, TaskStatus::Succeeded | TaskStatus::Failed(_)))
            .count();
        (done, self.tasks.len())
    }

    pub fn is_complete(&self) -> bool {
        !self.tasks.is_empty()
            && self
                .tasks
                .iter()
                .all(|t| matches!(t.status, TaskStatus::Succeeded))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StageStatus {
    Pending,
    Running,
    Succeeded,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageTaskRef {
    pub task_id: TaskId,
    pub stage_id: StageId,
}
