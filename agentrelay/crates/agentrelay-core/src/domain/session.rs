use std::fmt;

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

use crate::git::ProjectStatus;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSession {
    pub id: String,
    pub project_id: String,
    pub provider: ProviderId,
    pub agent_name: String,
    pub status: AgentSessionStatus,
    pub working_directory: String,
    pub initial_git_head: String,
    pub final_git_head: Option<String>,
    pub initial_git_state: Option<ProjectStatus>,
    pub final_git_state: Option<ProjectStatus>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub summary_status: String,
    pub detected_pid: Option<u32>,
    pub automatic: bool,
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum AgentSessionStatus {
    Active,
    Completed,
    Interrupted,
}

impl AgentSessionStatus {
    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "completed" => Some(Self::Completed),
            "interrupted" => Some(Self::Interrupted),
            _ => None,
        }
    }
}

/// Stable, open provider identity used by sessions, manifests and orchestration.
///
/// It deliberately is not an enum: installing a provider must not require adding a core variant.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderId(String);

impl ProviderId {
    pub fn new(value: impl Into<String>) -> Result<Self, ProviderIdError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 64
            && value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_.".contains(&byte)
            });
        valid.then_some(Self(value)).ok_or(ProviderIdError)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn display_name(&self) -> String {
        self.0
            .split(['-', '_', '.'])
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn claude() -> Self {
        Self("claude".into())
    }

    pub fn codex() -> Self {
        Self("codex".into())
    }

    pub fn gemini() -> Self {
        Self("gemini".into())
    }

    pub fn cursor() -> Self {
        Self("cursor".into())
    }

    pub fn copilot() -> Self {
        Self("copilot".into())
    }

    pub fn unknown() -> Self {
        Self("unknown".into())
    }

    pub(crate) fn as_db(&self) -> &str {
        self.as_str()
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        Self::new(value).ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderIdError;

impl fmt::Display for ProviderIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .write_str("provider id must be 1-64 lowercase ASCII letters, numbers, '-', '_' or '.'")
    }
}

impl std::error::Error for ProviderIdError {}

impl fmt::Display for ProviderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for ProviderId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ProviderId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartSessionInput {
    pub repository_path: String,
    pub provider: ProviderId,
    pub agent_name: String,
    #[serde(default)]
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndSessionInput {
    pub session_id: String,
    #[serde(default)]
    pub summary_status: String,
    #[serde(default)]
    pub blockers: Vec<String>,
}
