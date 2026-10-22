use serde::{Deserialize, Serialize};

use crate::context::ContextSnapshot;

use super::{
    AgentSession, Decision, EnvironmentSnapshot, Goal, PlanVersion, ProjectEvent, VerificationSuite,
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ReplayAccuracy {
    Exact,
    LegacyBestEffort,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TimelineCategory {
    Git,
    Goal,
    Task,
    Decision,
    Session,
    Context,
    Plan,
    Verification,
    Environment,
    Delivery,
    Conflict,
    Workspace,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEntry {
    pub id: String,
    pub event_id: String,
    pub session_id: Option<String>,
    pub timestamp: String,
    pub category: TimelineCategory,
    pub title: String,
    pub summary: String,
    pub source: String,
    pub commit: Option<String>,
    pub branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TimelineSnapshot {
    pub project_id: String,
    pub repository: String,
    pub entries: Vec<TimelineEntry>,
    pub first_timestamp: Option<String>,
    pub last_timestamp: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReplayRequest {
    pub repository_path: String,
    #[serde(default)]
    pub event_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReplayPoint {
    pub event_id: Option<String>,
    pub session_id: Option<String>,
    pub timestamp: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReplayGitState {
    pub commit: String,
    pub branch: String,
    pub worktree: String,
    pub dirty: bool,
    pub modified_files: Vec<String>,
    pub added_files: Vec<String>,
    pub deleted_files: Vec<String>,
    pub observed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalFile {
    pub path: String,
    pub change_type: String,
    pub introduced_at: String,
    pub last_changed_at: String,
    pub session_ids: Vec<String>,
    pub task_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalBlocker {
    pub id: String,
    pub kind: String,
    pub summary: String,
    pub introduced_at: String,
    pub resolved_at: Option<String>,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContextVersion {
    pub id: String,
    pub project_id: String,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub context_snapshot_hash: String,
    pub markdown: String,
    pub snapshot: Option<ContextSnapshot>,
    pub commit: String,
    pub branch: String,
    pub plan_id: Option<String>,
    pub plan_version: Option<u32>,
    pub environment_fingerprint: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReplayState {
    pub project_id: String,
    pub repository: String,
    pub point: ReplayPoint,
    pub accuracy: ReplayAccuracy,
    pub git: Option<ReplayGitState>,
    pub goals: Vec<Goal>,
    pub decisions: Vec<Decision>,
    pub sessions: Vec<AgentSession>,
    pub files: Vec<HistoricalFile>,
    pub blockers: Vec<HistoricalBlocker>,
    pub context: Option<ContextVersion>,
    pub plan: Option<PlanVersion>,
    pub verification: Option<VerificationSuite>,
    pub environment: Option<EnvironmentSnapshot>,
    pub events: Vec<ProjectEvent>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompareReplayInput {
    pub repository_path: String,
    pub left_event_id: String,
    pub right_event_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReplayComparison {
    pub left: ReplayPoint,
    pub right: ReplayPoint,
    pub new_decisions: Vec<String>,
    pub superseded_decisions: Vec<String>,
    pub completed_tasks: Vec<String>,
    pub changed_files: Vec<String>,
    pub new_blockers: Vec<String>,
    pub resolved_blockers: Vec<String>,
    pub context_hash_changed: bool,
    pub plan_version_changed: bool,
    pub environment_changed: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExplainHistoryInput {
    pub repository_path: String,
    pub entity_type: String,
    pub entity_id: String,
    #[serde(default)]
    pub at_event_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReplayEvidence {
    pub event_id: String,
    pub timestamp: String,
    pub kind: String,
    pub explanation: String,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub verification_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryExplanation {
    pub entity_type: String,
    pub entity_id: String,
    pub title: String,
    pub summary: String,
    pub introduced_at: Option<String>,
    pub introduced_by: Option<String>,
    pub superseded_at: Option<String>,
    pub evidence: Vec<ReplayEvidence>,
}
