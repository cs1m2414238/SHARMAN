use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryReport {
    pub recovered_at: String,
    pub database_integrity: String,
    pub migration_backup: Option<String>,
    pub interrupted_sessions: usize,
    pub interrupted_actions: usize,
    pub interrupted_verifications: usize,
    pub recoverable_workspaces: usize,
    pub missing_workspaces: usize,
    pub invalidated_caches: usize,
    pub warnings: Vec<String>,
}

impl RecoveryReport {
    pub fn is_clean(&self) -> bool {
        self.interrupted_sessions == 0
            && self.interrupted_actions == 0
            && self.interrupted_verifications == 0
            && self.missing_workspaces == 0
            && self.invalidated_caches == 0
            && self.warnings.is_empty()
    }
}
