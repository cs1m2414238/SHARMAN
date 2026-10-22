use serde::{Deserialize, Serialize};

use super::{ChangeRisk, ExecutionProfile};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum VerificationStatus {
    NotRun,
    Running,
    Passed,
    Failed,
    Waived,
}

impl VerificationStatus {
    pub(crate) const fn as_db(self) -> &'static str {
        match self {
            Self::NotRun => "not_run",
            Self::Running => "running",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Waived => "waived",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "not_run" => Some(Self::NotRun),
            "running" => Some(Self::Running),
            "passed" => Some(Self::Passed),
            "failed" => Some(Self::Failed),
            "waived" => Some(Self::Waived),
            _ => None,
        }
    }

    pub const fn satisfies_gate(self) -> bool {
        matches!(self, Self::Passed | Self::Waived)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum VerificationGateKind {
    Environment,
    Format,
    Lint,
    Tests,
    Build,
    FrontendBuild,
    DesktopBuild,
    Architecture,
    AcceptanceCriteria,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VerificationGate {
    pub id: String,
    pub kind: VerificationGateKind,
    pub name: String,
    pub required: bool,
    pub status: VerificationStatus,
    pub command: Vec<String>,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u64>,
    pub stdout_summary: String,
    pub stderr_summary: String,
    pub detail: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub waiver_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VerificationImpact {
    pub changed_files: Vec<String>,
    pub highest_risk: Option<ChangeRisk>,
    pub affected_modules: Vec<String>,
    pub tests_to_run: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VerificationSuite {
    pub id: String,
    pub project_id: String,
    pub task_id: String,
    pub workspace_id: String,
    pub status: VerificationStatus,
    pub environment_id: String,
    pub environment_fingerprint: String,
    pub environment_profile: ExecutionProfile,
    pub commit: String,
    pub branch: String,
    pub worktree: String,
    pub acceptance_criteria: Vec<String>,
    pub impact: VerificationImpact,
    pub gates: Vec<VerificationGate>,
    pub repair_task_id: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VerificationSnapshot {
    pub suites: Vec<VerificationSuite>,
    pub latest: Option<VerificationSuite>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunVerificationInput {
    pub workspace_id: String,
    #[serde(default)]
    pub approval_granted: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaiveVerificationGateInput {
    pub suite_id: String,
    pub gate_id: String,
    pub reason: String,
}
