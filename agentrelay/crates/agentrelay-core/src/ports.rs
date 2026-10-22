use std::{path::PathBuf, time::Duration};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Implemented by the Nebius/Nemotron adapter. The model only receives facts already selected by
/// the deterministic context compiler and must cite their stable source keys in its response.
pub trait ContextCompressor: Send + Sync {
    fn model(&self) -> &str;
    fn compress(
        &self,
        request: &CompressionRequest,
    ) -> Result<CompressionResponse, CompressionError>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompressionRequest {
    pub project_name: String,
    pub profile: String,
    pub max_output_tokens: usize,
    pub facts: Vec<GroundedFact>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GroundedFact {
    pub source_key: String,
    pub tier: String,
    pub title: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompressionResponse {
    pub model: String,
    pub bullets: Vec<CompressedBullet>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompressedBullet {
    pub text: String,
    pub source_keys: Vec<String>,
}

#[derive(Debug, Error)]
pub enum CompressionError {
    #[error("Nemotron request failed: {0}")]
    Request(String),
    #[error("Nemotron returned an unreadable response: {0}")]
    InvalidResponse(String),
}

/// A remote execution boundary. Callers provide an executable and separate arguments; shell
/// command strings are intentionally not part of this interface.
pub trait ExecutionSandbox: Send + Sync {
    fn name(&self) -> &str;
    fn execute(
        &self,
        request: &SandboxExecutionRequest,
    ) -> Result<SandboxExecutionOutput, SandboxError>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
}

impl CommandSpec {
    pub fn argv(&self) -> Vec<String> {
        std::iter::once(self.program.clone())
            .chain(self.args.clone())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxExecutionRequest {
    pub project_id: String,
    pub repository_path: PathBuf,
    pub action_id: String,
    pub command: CommandSpec,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxExecutionOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
}

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("sandbox is unavailable: {0}")]
    Unavailable(String),
    #[error("sandbox execution failed: {0}")]
    Execution(String),
    #[error("sandbox execution exceeded {0} seconds")]
    Timeout(u64),
}
