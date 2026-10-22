use serde::{Deserialize, Serialize};

use super::{Capability, ProviderId, TaskAssignmentStatus};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkTask {
    pub id: String,
    pub goal_id: String,
    pub title: String,
    pub description: String,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    pub assigned_agent: Option<String>,
    pub source_agent: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub assignment_status: TaskAssignmentStatus,
    pub suggested_provider: Option<ProviderId>,
    pub assigned_provider: Option<ProviderId>,
    pub route_reasons: Vec<String>,
    pub required_capabilities: Vec<Capability>,
    pub blocked_by: Vec<String>,
    pub depends_on: Vec<String>,
    pub produces: Vec<String>,
    pub consumes: Vec<String>,
    pub claimed_paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus {
    Todo,
    InProgress,
    Completed,
    Archived,
}

impl TaskStatus {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Archived => "archived",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "todo" => Some(Self::Todo),
            "in_progress" => Some(Self::InProgress),
            "completed" => Some(Self::Completed),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum TaskPriority {
    Low,
    Medium,
    High,
    Critical,
}

impl TaskPriority {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "critical" => Some(Self::Critical),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskInput {
    pub goal_id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub priority: TaskPriority,
    pub assigned_agent: Option<String>,
    pub source_agent: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTaskInput {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    pub assigned_agent: Option<String>,
}
