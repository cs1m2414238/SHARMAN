use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::ProviderId;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DetectedAgent {
    pub provider: ProviderId,
    pub pid: u32,
    pub executable: String,
    pub working_directory: Option<PathBuf>,
    #[serde(default)]
    pub command_line: Vec<String>,
}

/// Platform adapters inspect processes and return provider-neutral detections. Session lifecycle
/// code consumes these records without knowing executable names or operating-system APIs.
pub trait AgentDetector: Send + Sync {
    fn detect_agents(&self) -> Result<Vec<DetectedAgent>, DetectionError>;
}

#[derive(Debug, Error)]
pub enum DetectionError {
    #[error("agent process inspection failed: {0}")]
    Inspection(String),
}
