use serde::{Deserialize, Serialize};

use super::ProviderId;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationSettings {
    pub onboarding_completed: bool,
    pub selected_agents: Vec<ProviderId>,
    pub launch_at_startup: bool,
    pub close_to_tray: bool,
    pub tracking_paused: bool,
    pub notifications_enabled: bool,
    pub notify_agent_sessions: bool,
    pub notify_conflicts: bool,
    pub notify_handoffs: bool,
    pub notify_blockers: bool,
    pub default_context_depth: String,
    pub default_context_profile: String,
    pub backup_interval_hours: u64,
    pub diagnostics_include_paths: bool,
    pub advanced_logging: bool,
}

impl Default for ApplicationSettings {
    fn default() -> Self {
        Self {
            onboarding_completed: false,
            selected_agents: ["claude-code", "codex", "gemini", "cursor", "github-copilot"]
                .into_iter()
                .filter_map(|value| ProviderId::new(value).ok())
                .collect(),
            launch_at_startup: false,
            close_to_tray: true,
            tracking_paused: false,
            notifications_enabled: true,
            notify_agent_sessions: true,
            notify_conflicts: true,
            notify_handoffs: true,
            notify_blockers: true,
            default_context_depth: "normal".into(),
            default_context_profile: "auto".into(),
            backup_interval_hours: 24,
            diagnostics_include_paths: false,
            advanced_logging: false,
        }
    }
}

impl ApplicationSettings {
    pub fn normalized(mut self) -> Self {
        self.selected_agents.sort();
        self.selected_agents.dedup();
        if !matches!(
            self.default_context_depth.as_str(),
            "compact" | "normal" | "deep"
        ) {
            self.default_context_depth = "normal".into();
        }
        if !matches!(
            self.default_context_profile.as_str(),
            "auto" | "balanced" | "architecture" | "implementation" | "product"
        ) {
            self.default_context_profile = "auto".into();
        }
        self.backup_interval_hours = self.backup_interval_hours.clamp(1, 24 * 30);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RepositorySetupInspection {
    pub repository_path: String,
    pub name: String,
    pub git_repository: bool,
    pub detected_stack: Vec<String>,
    pub existing_context: bool,
    pub existing_memory: bool,
    pub suggested_quality_gates: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompleteProjectOnboardingInput {
    pub repository_path: String,
    pub preserve_existing_context: bool,
    pub run_tests: super::PermissionMode,
    pub run_build: super::PermissionMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOnboardingResult {
    pub repository_path: String,
    pub imported_context_path: Option<String>,
    pub generated_context_path: String,
    pub backup: BackupRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupRecord {
    pub path: String,
    pub created_at: String,
    pub size_bytes: u64,
}
