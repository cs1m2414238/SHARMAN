use serde::{Deserialize, Serialize};

use super::{DecisionStatus, ProviderId, TaskPriority, TaskStatus};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HandoffSnapshot {
    pub id: String,
    pub project_id: String,
    pub session_id: String,
    pub previous_agent: Option<String>,
    pub branch: String,
    pub base_commit: String,
    pub final_commit: String,
    pub changed_files: Vec<FileChange>,
    pub commits: Vec<String>,
    pub completed_tasks: Vec<TaskSummary>,
    pub active_tasks: Vec<TaskSummary>,
    pub recent_decisions: Vec<DecisionSummary>,
    pub blockers: Vec<Blocker>,
    pub recommended_next_actions: Vec<String>,
    #[serde(default)]
    pub next_recommended_provider: Option<ProviderId>,
    #[serde(default)]
    pub routing_reasons: Vec<String>,
    pub started_at: String,
    pub ended_at: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    pub change_type: FileChangeType,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum FileChangeType {
    Modified,
    Created,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskSummary {
    pub id: String,
    pub title: String,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    pub assigned_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DecisionSummary {
    pub id: String,
    pub summary: String,
    pub reason: String,
    pub status: DecisionStatus,
    pub created_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Blocker {
    pub description: String,
}
