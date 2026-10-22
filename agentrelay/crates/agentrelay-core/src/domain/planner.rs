use serde::{Deserialize, Serialize};

use super::{Capability, ProviderId, RiskLevel, TaskPriority, WorkTask};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum PlanStatus {
    Valid,
    Invalid,
    Approved,
    Superseded,
}

impl PlanStatus {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Invalid => "invalid",
            Self::Approved => "approved",
            Self::Superseded => "superseded",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "valid" => Some(Self::Valid),
            "invalid" => Some(Self::Invalid),
            "approved" => Some(Self::Approved),
            "superseded" => Some(Self::Superseded),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum PlannedTaskKind {
    Architecture,
    Implementation,
    Persistence,
    Testing,
    Documentation,
    MigrationConfig,
    Recovery,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PlannedTaskStatus {
    Ready,
    Blocked,
    Materialized,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlannedTask {
    pub id: String,
    pub title: String,
    pub description: String,
    pub kind: PlannedTaskKind,
    pub status: PlannedTaskStatus,
    pub priority: TaskPriority,
    pub required_capabilities: Vec<Capability>,
    pub depends_on: Vec<String>,
    pub produces: Vec<String>,
    pub consumes: Vec<String>,
    pub affected_modules: Vec<String>,
    pub expected_files: Vec<String>,
    pub conflict_risk: RiskLevel,
    pub risk_score: u8,
    pub risk_reasons: Vec<String>,
    pub can_run_in_parallel: bool,
    pub parallel_group: usize,
    pub suggested_provider: Option<ProviderId>,
    pub suggestion_reasons: Vec<String>,
    pub materialized_task_id: Option<String>,
    pub recovery_for: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PlanIssueSeverity {
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanValidationIssue {
    pub code: String,
    pub severity: PlanIssueSeverity,
    pub task_ids: Vec<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanVersion {
    pub id: String,
    pub project_id: String,
    pub goal_id: String,
    pub version: u32,
    pub status: PlanStatus,
    pub reason: String,
    pub parent_plan_id: Option<String>,
    pub tasks: Vec<PlannedTask>,
    pub validation_issues: Vec<PlanValidationIssue>,
    pub changed_task_ids: Vec<String>,
    pub changed_dependencies: Vec<String>,
    pub created_at: String,
    pub approved_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlannerSnapshot {
    pub plans: Vec<PlanVersion>,
    pub active_plan: Option<PlanVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlanApprovalResult {
    pub plan: PlanVersion,
    pub tasks: Vec<WorkTask>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratePlanInput {
    pub repository_path: String,
    pub goal_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovePlanInput {
    pub plan_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplanInput {
    pub plan_id: String,
    pub failed_plan_task_id: String,
    pub failure_reason: String,
}
