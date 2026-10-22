use serde::{Deserialize, Serialize};

use super::{ActionRun, ParallelAssessment, ProviderId};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TaskWorkspaceStatus {
    Ready,
    Active,
    ValidationPending,
    ValidationPassed,
    ValidationFailed,
    MergeReady,
    MergeConflict,
    Merged,
    Archived,
}

impl TaskWorkspaceStatus {
    pub(crate) const fn as_db(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Active => "active",
            Self::ValidationPending => "validation_pending",
            Self::ValidationPassed => "validation_passed",
            Self::ValidationFailed => "validation_failed",
            Self::MergeReady => "merge_ready",
            Self::MergeConflict => "merge_conflict",
            Self::Merged => "merged",
            Self::Archived => "archived",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "ready" => Some(Self::Ready),
            "active" => Some(Self::Active),
            "validation_pending" => Some(Self::ValidationPending),
            "validation_passed" => Some(Self::ValidationPassed),
            "validation_failed" => Some(Self::ValidationFailed),
            "merge_ready" => Some(Self::MergeReady),
            "merge_conflict" => Some(Self::MergeConflict),
            "merged" => Some(Self::Merged),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }

    pub const fn is_live(self) -> bool {
        !matches!(self, Self::Merged | Self::Archived)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskWorkspace {
    pub id: String,
    pub project_id: String,
    pub task_id: String,
    pub assigned_provider: ProviderId,
    pub branch: String,
    pub worktree_path: String,
    pub base_commit: String,
    pub head_commit: String,
    pub status: TaskWorkspaceStatus,
    pub session_id: Option<String>,
    pub latest_validation_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub merged_at: Option<String>,
    pub archived_at: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ValidationCheckKind {
    CleanWorktree,
    HasChanges,
    BaseAncestry,
    ParallelSafety,
    Mergeability,
    Tests,
    Build,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ValidationCheckStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ValidationCheck {
    pub kind: ValidationCheckKind,
    pub status: ValidationCheckStatus,
    pub detail: String,
    pub action_run_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceValidationStatus {
    Passed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceValidation {
    pub id: String,
    pub workspace_id: String,
    pub status: WorkspaceValidationStatus,
    pub validated_head: String,
    pub changed_files: Vec<String>,
    pub checks: Vec<ValidationCheck>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CollaborativeExecutionSnapshot {
    pub workspaces: Vec<TaskWorkspace>,
    pub validations: Vec<WorkspaceValidation>,
    pub parallel_assessments: Vec<ParallelAssessment>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareTaskWorkspaceInput {
    pub repository_path: String,
    pub task_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartTaskWorkspaceInput {
    pub workspace_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateTaskWorkspaceInput {
    pub workspace_id: String,
    #[serde(default)]
    pub run_tests: bool,
    #[serde(default)]
    pub run_build: bool,
    #[serde(default)]
    pub approval_granted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceValidationResult {
    pub workspace: TaskWorkspace,
    pub validation: WorkspaceValidation,
    pub action_runs: Vec<ActionRun>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceIdInput {
    pub workspace_id: String,
}
