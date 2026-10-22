use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::VerificationSuite;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ExecutionProfile {
    NativeWindows,
    SandboxedLocal,
    Container,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentVariableRequirement {
    pub name: String,
    pub required: bool,
    pub secret: bool,
    /// Public configuration only. Secret requirements must always leave this unset.
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentManifest {
    pub schema_version: u32,
    pub profile: ExecutionProfile,
    pub working_directory: String,
    pub container_image: Option<String>,
    pub disposable_verification: bool,
    pub runtimes: BTreeMap<String, String>,
    pub package_manager: Option<String>,
    pub system_tools: Vec<String>,
    /// Named, fixed argv. No shell command strings are accepted.
    pub commands: BTreeMap<String, Vec<String>>,
    pub environment: Vec<EnvironmentVariableRequirement>,
    pub cache_kinds: Vec<String>,
    pub architecture_rules: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ToolStatus {
    Matched,
    Drift,
    Missing,
    Detected,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolObservation {
    pub name: String,
    pub expected: Option<String>,
    pub detected: Option<String>,
    pub status: ToolStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentVariableState {
    pub name: String,
    pub required: bool,
    pub secret: bool,
    pub configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FingerprintInput {
    pub kind: String,
    pub path: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentDrift {
    pub subject: String,
    pub expected: String,
    pub detected: String,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EnvironmentStatus {
    Reproducible,
    EnvironmentDrift,
    Incomplete,
    Unmanaged,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSnapshot {
    pub id: String,
    pub project_id: String,
    pub profile: ExecutionProfile,
    pub manifest_source: String,
    pub fingerprint: String,
    pub status: EnvironmentStatus,
    pub tools: Vec<ToolObservation>,
    pub environment: Vec<EnvironmentVariableState>,
    pub fingerprint_inputs: Vec<FingerprintInput>,
    pub drift: Vec<EnvironmentDrift>,
    pub os: String,
    pub architecture: String,
    pub commit: String,
    pub branch: String,
    pub worktree: String,
    pub captured_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CacheStatus {
    Valid,
    Invalidated,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentCacheRecord {
    pub id: String,
    pub project_id: String,
    pub kind: String,
    pub fingerprint: String,
    pub path: String,
    pub status: CacheStatus,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentDashboard {
    pub manifest: EnvironmentManifest,
    pub snapshot: EnvironmentSnapshot,
    pub caches: Vec<EnvironmentCacheRecord>,
    pub latest_verification: Option<VerificationSuite>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveEnvironmentManifestInput {
    pub repository_path: String,
    pub manifest: EnvironmentManifest,
}
