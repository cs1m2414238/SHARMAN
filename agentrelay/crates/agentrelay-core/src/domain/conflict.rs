use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConflictKind {
    FileOverlap,
    DecisionContradiction,
    StaleContext,
    CompletedTaskDrift,
}

impl ConflictKind {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::FileOverlap => "file_overlap",
            Self::DecisionContradiction => "decision_contradiction",
            Self::StaleContext => "stale_context",
            Self::CompletedTaskDrift => "completed_task_drift",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "file_overlap" => Some(Self::FileOverlap),
            "decision_contradiction" => Some(Self::DecisionContradiction),
            "stale_context" => Some(Self::StaleContext),
            "completed_task_drift" => Some(Self::CompletedTaskDrift),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConflictSeverity {
    Info,
    Warning,
    Critical,
}

impl ConflictSeverity {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "info" => Some(Self::Info),
            "warning" => Some(Self::Warning),
            "critical" => Some(Self::Critical),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConflictStatus {
    Open,
    Acknowledged,
    Resolved,
}

impl ConflictStatus {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Acknowledged => "acknowledged",
            Self::Resolved => "resolved",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "open" => Some(Self::Open),
            "acknowledged" => Some(Self::Acknowledged),
            "resolved" => Some(Self::Resolved),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConflictRecord {
    pub id: String,
    pub project_id: String,
    pub fingerprint: String,
    pub primary_session_id: Option<String>,
    pub related_session_id: Option<String>,
    pub kind: ConflictKind,
    pub severity: ConflictSeverity,
    pub title: String,
    pub description: String,
    pub affected_paths: Vec<String>,
    pub related_decision_ids: Vec<String>,
    pub status: ConflictStatus,
    pub detected_at: String,
    pub updated_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FreshnessStatus {
    Current,
    Stale,
    Superseded,
    Invalidated,
}

impl FreshnessStatus {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Stale => "stale",
            Self::Superseded => "superseded",
            Self::Invalidated => "invalidated",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "current" => Some(Self::Current),
            "stale" => Some(Self::Stale),
            "superseded" => Some(Self::Superseded),
            "invalidated" => Some(Self::Invalidated),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FreshnessRecord {
    pub project_id: String,
    pub subject_type: String,
    pub subject_id: String,
    pub status: FreshnessStatus,
    pub reason: String,
    pub superseded_by: Option<String>,
    pub evaluated_at: String,
}
