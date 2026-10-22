use serde::{Deserialize, Serialize};

use super::{ActionKind, ProviderId, WorkTask};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "camelCase")]
pub enum Capability {
    Architecture,
    Backend,
    Frontend,
    Refactoring,
    Research,
    Testing,
    Documentation,
    Rust,
    TypeScript,
    Security,
    DevOps,
}

impl Capability {
    pub fn as_key(self) -> &'static str {
        match self {
            Self::Architecture => "architecture",
            Self::Backend => "backend",
            Self::Frontend => "frontend",
            Self::Refactoring => "refactoring",
            Self::Research => "research",
            Self::Testing => "testing",
            Self::Documentation => "documentation",
            Self::Rust => "rust",
            Self::TypeScript => "typescript",
            Self::Security => "security",
            Self::DevOps => "devops",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilityProfile {
    pub id: String,
    pub project_id: String,
    pub provider: ProviderId,
    pub display_name: String,
    pub strengths: Vec<Capability>,
    pub allowed_actions: Vec<ActionKind>,
    pub max_context_tokens: usize,
    pub enabled: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCapabilityProfileInput {
    pub repository_path: String,
    pub provider: ProviderId,
    pub display_name: String,
    pub strengths: Vec<Capability>,
    pub allowed_actions: Vec<ActionKind>,
    pub max_context_tokens: usize,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TaskAssignmentStatus {
    Unassigned,
    Suggested,
    Assigned,
    Running,
    Waiting,
    Blocked,
    Completed,
}

impl TaskAssignmentStatus {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::Unassigned => "unassigned",
            Self::Suggested => "suggested",
            Self::Assigned => "assigned",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "unassigned" => Some(Self::Unassigned),
            "suggested" => Some(Self::Suggested),
            "assigned" => Some(Self::Assigned),
            "running" => Some(Self::Running),
            "waiting" => Some(Self::Waiting),
            "blocked" => Some(Self::Blocked),
            "completed" => Some(Self::Completed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTaskOrchestrationInput {
    pub task_id: String,
    #[serde(default)]
    pub required_capabilities: Vec<Capability>,
    #[serde(default)]
    pub blocked_by: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub produces: Vec<String>,
    #[serde(default)]
    pub consumes: Vec<String>,
    #[serde(default)]
    pub claimed_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveTaskAssignmentInput {
    pub task_id: String,
    pub provider: ProviderId,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRoutedTaskInput {
    pub repository_path: String,
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskRouteRecommendation {
    pub task_id: String,
    pub provider: ProviderId,
    pub display_name: String,
    pub score: u16,
    pub reasons: Vec<String>,
    pub required_capabilities: Vec<Capability>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceLockStatus {
    Active,
    Released,
}

impl WorkspaceLockStatus {
    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "released" => Some(Self::Released),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceLock {
    pub id: String,
    pub project_id: String,
    pub task_id: String,
    pub session_id: Option<String>,
    pub provider: ProviderId,
    pub path_pattern: String,
    pub status: WorkspaceLockStatus,
    pub acquired_at: String,
    pub released_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ParallelAssessment {
    pub left_task_id: String,
    pub right_task_id: String,
    pub safe: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OrchestrationSnapshot {
    pub tasks: Vec<WorkTask>,
    pub profiles: Vec<AgentCapabilityProfile>,
    pub locks: Vec<WorkspaceLock>,
    pub recommendations: Vec<TaskRouteRecommendation>,
    pub parallel_assessments: Vec<ParallelAssessment>,
    pub next_recommended_agent: Option<TaskRouteRecommendation>,
}
