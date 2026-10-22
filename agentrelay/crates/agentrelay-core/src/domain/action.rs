use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ActionKind {
    RunTests,
    RunBuild,
    ModifySource,
    GitCommit,
    GitPush,
    DeleteFiles,
}

impl ActionKind {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::RunTests => "run_tests",
            Self::RunBuild => "run_build",
            Self::ModifySource => "modify_source",
            Self::GitCommit => "git_commit",
            Self::GitPush => "git_push",
            Self::DeleteFiles => "delete_files",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "run_tests" => Some(Self::RunTests),
            "run_build" => Some(Self::RunBuild),
            "modify_source" => Some(Self::ModifySource),
            "git_commit" => Some(Self::GitCommit),
            "git_push" => Some(Self::GitPush),
            "delete_files" => Some(Self::DeleteFiles),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ActionStatus {
    Requested,
    ApprovalRequired,
    Denied,
    Running,
    Succeeded,
    Failed,
    Unavailable,
}

impl ActionStatus {
    pub(crate) fn as_db(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::ApprovalRequired => "approval_required",
            Self::Denied => "denied",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Unavailable => "unavailable",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "requested" => Some(Self::Requested),
            "approval_required" => Some(Self::ApprovalRequired),
            "denied" => Some(Self::Denied),
            "running" => Some(Self::Running),
            "succeeded" => Some(Self::Succeeded),
            "failed" => Some(Self::Failed),
            "unavailable" => Some(Self::Unavailable),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    Allow,
    Ask,
    Deny,
}

impl PermissionMode {
    pub(crate) const fn as_db(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Ask => "ask",
            Self::Deny => "deny",
        }
    }

    pub(crate) fn from_db(value: &str) -> Self {
        match value {
            "allow" => Self::Allow,
            "ask" => Self::Ask,
            _ => Self::Deny,
        }
    }

    pub const fn permits(self, approval_granted: bool) -> bool {
        matches!(self, Self::Allow) || matches!(self, Self::Ask) && approval_granted
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPermissions {
    pub project_id: String,
    pub read_repository: bool,
    pub read_git: bool,
    pub write_memory: bool,
    pub write_context_files: bool,
    pub run_tests: PermissionMode,
    pub run_build: PermissionMode,
    pub modify_source: bool,
    pub git_commit: bool,
    pub git_push: bool,
    pub delete_files: bool,
    pub updated_at: String,
}

impl ProjectPermissions {
    pub fn mode_for(&self, action: ActionKind) -> PermissionMode {
        match action {
            ActionKind::RunTests => self.run_tests,
            ActionKind::RunBuild => self.run_build,
            ActionKind::ModifySource if self.modify_source => PermissionMode::Allow,
            ActionKind::GitCommit if self.git_commit => PermissionMode::Allow,
            ActionKind::GitPush if self.git_push => PermissionMode::Allow,
            ActionKind::DeleteFiles if self.delete_files => PermissionMode::Allow,
            _ => PermissionMode::Deny,
        }
    }

    pub fn permits(&self, action: ActionKind, approval_granted: bool) -> bool {
        self.mode_for(action).permits(approval_granted)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProjectPermissionsInput {
    pub repository_path: String,
    pub run_tests: PermissionMode,
    pub run_build: PermissionMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActionRequestInput {
    pub repository_path: String,
    pub session_id: Option<String>,
    pub requested_by: String,
    pub action: ActionKind,
    #[serde(default)]
    pub approval_granted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActionRun {
    pub id: String,
    pub project_id: String,
    pub session_id: Option<String>,
    pub requested_by: String,
    pub action: ActionKind,
    pub status: ActionStatus,
    pub command: Vec<String>,
    pub sandbox: Option<String>,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub error: Option<String>,
    pub requested_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SandboxStatus {
    pub configured: bool,
    pub name: Option<String>,
    pub detail: String,
}
