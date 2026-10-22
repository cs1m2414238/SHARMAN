use serde::{Deserialize, Serialize};

use crate::git::ProjectStatus;

use super::{
    ActionRun, AgentSession, ConflictRecord, Decision, FreshnessRecord, Goal, HandoffSnapshot,
    ProjectEvent, ProjectPermissions,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRecord {
    pub id: String,
    pub path: String,
    pub name: String,
    pub branch: String,
    pub last_commit: String,
    pub change_count: usize,
    pub last_scanned_at: String,
    pub tracking_enabled: bool,
    pub latest_status: Option<ProjectStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMemory {
    pub project: ProjectRecord,
    pub goals: Vec<Goal>,
    pub decisions: Vec<Decision>,
    pub events: Vec<ProjectEvent>,
    pub agent_sessions: Vec<AgentSession>,
    pub last_agent_session: Option<AgentSession>,
    pub latest_handoff: Option<HandoffSnapshot>,
    pub handoffs: Vec<HandoffSnapshot>,
    pub conflicts: Vec<ConflictRecord>,
    pub freshness: Vec<FreshnessRecord>,
    pub permissions: ProjectPermissions,
    pub action_runs: Vec<ActionRun>,
}
