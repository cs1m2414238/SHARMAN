use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExternalRepository {
    pub provider: String,
    pub owner: String,
    pub name: String,
    pub web_url: String,
}

impl ExternalRepository {
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ExternalIssueState {
    Open,
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExternalIssue {
    pub id: String,
    pub number: u64,
    pub title: String,
    pub description: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub state: ExternalIssueState,
    pub linked_branch: Option<String>,
    pub url: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PullRequestStatus {
    Draft,
    Open,
    Closed,
    Merged,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MergeState {
    Unknown,
    Ready,
    Blocked,
    Behind,
    Unstable,
    Dirty,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CiStatus {
    Queued,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CiConclusion {
    Pending,
    Success,
    Failure,
    Neutral,
    Cancelled,
    Skipped,
    TimedOut,
    ActionRequired,
}

impl CiConclusion {
    pub fn is_failure(self) -> bool {
        matches!(self, Self::Failure | Self::TimedOut | Self::ActionRequired)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CiCategory {
    Build,
    Test,
    Lint,
    Security,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CiCheck {
    pub id: String,
    pub name: String,
    pub category: CiCategory,
    pub status: CiStatus,
    pub conclusion: CiConclusion,
    pub revision: String,
    pub details_url: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReviewComment {
    pub id: String,
    pub author: String,
    pub body: String,
    pub state: String,
    pub path: Option<String>,
    pub line: Option<u64>,
    pub url: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestState {
    pub id: String,
    pub number: u64,
    pub title: String,
    pub description: String,
    pub status: PullRequestStatus,
    pub merge_state: MergeState,
    pub branch: String,
    pub base_branch: String,
    pub head_revision: String,
    pub author: String,
    pub reviewers: Vec<String>,
    pub files: Vec<String>,
    pub commits: Vec<String>,
    pub linked_issue_numbers: Vec<u64>,
    pub ci_checks: Vec<CiCheck>,
    pub review_comments: Vec<ReviewComment>,
    pub url: String,
    pub updated_at: String,
}

impl PullRequestState {
    pub fn failing_checks(&self) -> Vec<&CiCheck> {
        self.ci_checks
            .iter()
            .filter(|check| check.conclusion.is_failure())
            .collect()
    }

    pub fn is_ready(&self) -> bool {
        self.status == PullRequestStatus::Open
            && !self.ci_checks.is_empty()
            && self
                .ci_checks
                .iter()
                .all(|check| check.conclusion == CiConclusion::Success)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LocalEntityType {
    Task,
    Session,
    Decision,
}

impl LocalEntityType {
    pub(crate) const fn as_db(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Session => "session",
            Self::Decision => "decision",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "task" => Some(Self::Task),
            "session" => Some(Self::Session),
            "decision" => Some(Self::Decision),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ExternalEntityType {
    Issue,
    PullRequest,
    ReviewComment,
    CiCheck,
}

impl ExternalEntityType {
    pub(crate) const fn as_db(self) -> &'static str {
        match self {
            Self::Issue => "issue",
            Self::PullRequest => "pull_request",
            Self::ReviewComment => "review_comment",
            Self::CiCheck => "ci_check",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "issue" => Some(Self::Issue),
            "pull_request" => Some(Self::PullRequest),
            "review_comment" => Some(Self::ReviewComment),
            "ci_check" => Some(Self::CiCheck),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExternalLink {
    pub id: String,
    pub project_id: String,
    pub local_entity_type: LocalEntityType,
    pub local_entity_id: String,
    pub provider: String,
    pub external_entity_type: ExternalEntityType,
    pub external_id: String,
    pub external_url: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryBlocker {
    pub id: String,
    pub task_id: String,
    pub pull_request_number: u64,
    pub check_id: String,
    pub summary: String,
    pub url: Option<String>,
    pub active: bool,
    pub detected_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeliverySnapshot {
    pub project_id: String,
    pub provider: String,
    pub repository: ExternalRepository,
    pub issues: Vec<ExternalIssue>,
    pub pull_requests: Vec<PullRequestState>,
    pub links: Vec<ExternalLink>,
    pub blockers: Vec<DeliveryBlocker>,
    pub synced_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeliverySyncReport {
    pub snapshot: DeliverySnapshot,
    pub created_tasks: usize,
    pub updated_tasks: usize,
    pub created_links: usize,
    pub blocked_tasks: usize,
    pub recorded_events: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SyncDeliveryInput {
    pub repository_path: String,
    #[serde(default)]
    pub repository: Option<String>,
}
