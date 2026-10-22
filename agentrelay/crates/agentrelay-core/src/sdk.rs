//! Stable extension boundary for providers and developer-tool integrations.
//!
//! Extensions describe detection and capabilities in TOML. Compiled adapters may implement the
//! traits below, but they never receive a [`ContextStore`](crate::storage::ContextStore) or a raw
//! database connection. They return normalized facts or action proposals; [`AgentRelayCore`](crate::AgentRelayCore)
//! remains the only authority that persists events and approves executable actions.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

use crate::{
    context::ContextSnapshot,
    detection::DetectedAgent,
    domain::{
        ActionKind, ActionRequestInput, AgentCapabilityProfile, Capability, EventKind, EventSource,
        HandoffSnapshot, ProviderId,
    },
};

pub const SDK_API_VERSION: u16 = 1;

/// Compiled provider hook. Declarative process detection uses the same manifest registry.
pub trait AgentProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn detect(&self) -> Result<Vec<DetectedAgent>, PluginError>;
    fn capabilities(&self) -> CapabilityProfile;
    fn prepare_context(&self, context: &ContextSnapshot) -> Result<ProviderContext, PluginError>;
}

/// Converts an integration-specific payload into facts understood by the core.
pub trait EventAdapter: Send + Sync {
    fn integration_id(&self) -> &str;
    fn normalize(&self, raw: RawProviderEvent) -> Result<Vec<NormalizedProjectEvent>, PluginError>;
}

/// Action adapters produce proposals only. Despite the method name, no process is executed here;
/// the core still checks project permissions and delegates approved work to its sandbox.
pub trait ActionAdapter: Send + Sync {
    fn integration_id(&self) -> &str;
    fn supported_actions(&self) -> Vec<ActionKind>;
    fn execute(&self, request: PluginActionRequest) -> Result<ActionProposal, PluginError>;
}

/// Pure formatting hook for provider-specific delivery formats.
pub trait HandoffAdapter: Send + Sync {
    fn integration_id(&self) -> &str;
    fn format_handoff(&self, snapshot: &HandoffSnapshot) -> Result<String, PluginError>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderContext {
    pub content_type: String,
    pub content: String,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityProfile {
    #[serde(default)]
    pub strengths: Vec<Capability>,
    #[serde(default, alias = "allowed_actions")]
    pub allowed_actions: Vec<ActionKind>,
    #[serde(default = "default_context_tokens", alias = "max_context_tokens")]
    pub max_context_tokens: usize,
    #[serde(default, alias = "code_edit")]
    pub code_edit: bool,
    #[serde(default)]
    pub terminal: bool,
    #[serde(default)]
    pub planning: bool,
    #[serde(default)]
    pub documentation: bool,
}

impl Default for CapabilityProfile {
    fn default() -> Self {
        Self {
            strengths: Vec::new(),
            allowed_actions: Vec::new(),
            max_context_tokens: default_context_tokens(),
            code_edit: false,
            terminal: false,
            planning: false,
            documentation: false,
        }
    }
}

fn default_context_tokens() -> usize {
    6_000
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum PluginKind {
    Provider,
    #[serde(alias = "developer_tool")]
    DeveloperTool,
    Automation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProcessDetection {
    #[serde(default)]
    pub processes: Vec<String>,
    #[serde(default, alias = "command_contains")]
    pub command_contains: Vec<String>,
    #[serde(default)]
    pub rules: Vec<ProcessRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRule {
    pub process: String,
    #[serde(default, alias = "command_contains")]
    pub command_contains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginPermissions {
    #[serde(default = "enabled_by_default", alias = "read_repo")]
    pub read_repo: bool,
    #[serde(default, alias = "write_repo")]
    pub write_repo: bool,
    #[serde(default, alias = "run_commands")]
    pub run_commands: bool,
}

impl Default for PluginPermissions {
    fn default() -> Self {
        Self {
            read_repo: true,
            write_repo: false,
            run_commands: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct AdapterManifest {
    #[serde(default, alias = "event_types")]
    pub event_types: Vec<String>,
    #[serde(default)]
    pub actions: Vec<ActionKind>,
    #[serde(default, alias = "handoff_format")]
    pub handoff_format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: PluginKind,
    #[serde(default = "default_sdk_api_version", alias = "sdk_api_version")]
    pub sdk_api_version: u16,
    #[serde(default)]
    pub description: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    #[serde(default)]
    pub capabilities: CapabilityProfile,
    #[serde(default)]
    pub detection: ProcessDetection,
    #[serde(default)]
    pub permissions: PluginPermissions,
    #[serde(default)]
    pub adapters: AdapterManifest,
}

fn default_sdk_api_version() -> u16 {
    SDK_API_VERSION
}

fn enabled_by_default() -> bool {
    true
}

impl PluginManifest {
    pub fn parse_toml(source: &str) -> Result<Self, PluginError> {
        let manifest: Self = toml::from_str(source)?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), PluginError> {
        validate_plugin_id(&self.id)?;
        if self.name.trim().is_empty() {
            return Err(PluginError::InvalidManifest("name cannot be empty".into()));
        }
        if self.version.trim().is_empty()
            || !self
                .version
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return Err(PluginError::InvalidManifest(
                "version must use numeric dotted components, for example 0.1.0".into(),
            ));
        }
        if self.sdk_api_version != SDK_API_VERSION {
            return Err(PluginError::UnsupportedApiVersion {
                plugin: self.id.clone(),
                expected: SDK_API_VERSION,
                actual: self.sdk_api_version,
            });
        }
        if self.capabilities.max_context_tokens < 512
            || self.capabilities.max_context_tokens > 32_000
        {
            return Err(PluginError::InvalidManifest(
                "maxContextTokens must be between 512 and 32000".into(),
            ));
        }
        for rule in &self.detection.rules {
            if rule.process.trim().is_empty() {
                return Err(PluginError::InvalidManifest(
                    "detection rule process cannot be empty".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn provider_id(&self) -> Option<ProviderId> {
        (self.kind == PluginKind::Provider)
            .then(|| ProviderId::new(self.id.clone()).ok())
            .flatten()
    }
}

fn validate_plugin_id(value: &str) -> Result<(), PluginError> {
    ProviderId::new(value.to_owned())
        .map(|_| ())
        .map_err(|error| PluginError::InvalidManifest(error.to_string()))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PluginOrigin {
    BuiltIn,
    Directory,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RegistryEntry {
    pub manifest: PluginManifest,
    pub origin: PluginOrigin,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RegistrySnapshot {
    pub sdk_api_version: u16,
    pub plugin_directory: String,
    pub entries: Vec<RegistryEntry>,
    pub load_errors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct IntegrationRegistry {
    plugin_directory: PathBuf,
    entries: BTreeMap<String, RegistryEntry>,
    load_errors: Vec<String>,
}

impl IntegrationRegistry {
    pub fn load(plugin_directory: impl AsRef<Path>) -> Self {
        let plugin_directory = plugin_directory.as_ref().to_path_buf();
        let mut registry = Self {
            plugin_directory,
            entries: BTreeMap::new(),
            load_errors: Vec::new(),
        };
        for (source, contents) in builtin_manifests() {
            registry.insert(source, contents, PluginOrigin::BuiltIn);
        }
        registry.load_directory();
        registry
    }

    pub fn built_in() -> Self {
        Self::load(PathBuf::new())
    }

    pub fn snapshot(&self) -> RegistrySnapshot {
        RegistrySnapshot {
            sdk_api_version: SDK_API_VERSION,
            plugin_directory: self.plugin_directory.to_string_lossy().into_owned(),
            entries: self.entries.values().cloned().collect(),
            load_errors: self.load_errors.clone(),
        }
    }

    pub fn manifest(&self, id: &str) -> Option<&PluginManifest> {
        self.entries.get(id).map(|entry| &entry.manifest)
    }

    pub fn provider_detection_rules(&self) -> Vec<ProviderDetectionRule> {
        self.entries
            .values()
            .filter(|entry| entry.manifest.enabled && entry.manifest.kind == PluginKind::Provider)
            .filter_map(|entry| {
                entry
                    .manifest
                    .provider_id()
                    .map(|provider| ProviderDetectionRule {
                        provider,
                        display_name: entry.manifest.name.clone(),
                        detection: entry.manifest.detection.clone(),
                    })
            })
            .collect()
    }

    pub fn normalize(
        &self,
        raw: RawProviderEvent,
    ) -> Result<Vec<NormalizedProjectEvent>, PluginError> {
        let manifest = self
            .manifest(&raw.integration_id)
            .ok_or_else(|| PluginError::NotRegistered(raw.integration_id.clone()))?;
        if !manifest.enabled {
            return Err(PluginError::Disabled(raw.integration_id));
        }
        if !manifest.adapters.event_types.is_empty()
            && !manifest
                .adapters
                .event_types
                .iter()
                .any(|event_type| event_type == "*" || event_type == &raw.event_type)
        {
            return Err(PluginError::UnsupportedEvent {
                plugin: raw.integration_id,
                event_type: raw.event_type,
            });
        }
        ManifestEventAdapter {
            integration_id: manifest.id.clone(),
        }
        .normalize(raw)
    }

    pub fn propose_action(
        &self,
        request: PluginActionRequest,
    ) -> Result<ActionProposal, PluginError> {
        let manifest = self
            .manifest(&request.integration_id)
            .ok_or_else(|| PluginError::NotRegistered(request.integration_id.clone()))?;
        if !manifest.enabled {
            return Err(PluginError::Disabled(request.integration_id));
        }
        let adapter = ManifestActionAdapter {
            integration_id: manifest.id.clone(),
            actions: manifest.adapters.actions.clone(),
        };
        adapter.execute(request)
    }

    pub fn capability_profile_templates(&self) -> Vec<ProviderProfileTemplate> {
        self.entries
            .values()
            .filter(|entry| entry.manifest.enabled && entry.manifest.kind == PluginKind::Provider)
            .filter_map(|entry| {
                entry
                    .manifest
                    .provider_id()
                    .map(|provider| ProviderProfileTemplate {
                        provider,
                        display_name: entry.manifest.name.clone(),
                        capabilities: entry.manifest.capabilities.clone(),
                    })
            })
            .collect()
    }

    fn insert(&mut self, source: &str, contents: &str, origin: PluginOrigin) {
        match PluginManifest::parse_toml(contents) {
            Ok(manifest) if self.entries.contains_key(&manifest.id) => {
                self.load_errors.push(format!(
                    "{source}: duplicate plugin id '{}' was ignored",
                    manifest.id
                ))
            }
            Ok(manifest) => {
                self.entries.insert(
                    manifest.id.clone(),
                    RegistryEntry {
                        manifest,
                        origin,
                        source: source.into(),
                    },
                );
            }
            Err(error) => self.load_errors.push(format!("{source}: {error}")),
        }
    }

    fn load_directory(&mut self) {
        if self.plugin_directory.as_os_str().is_empty() {
            return;
        }
        if let Err(error) = fs::create_dir_all(&self.plugin_directory) {
            self.load_errors.push(format!(
                "{}: unable to create plugin directory: {error}",
                self.plugin_directory.display()
            ));
            return;
        }
        let entries = match fs::read_dir(&self.plugin_directory) {
            Ok(entries) => entries,
            Err(error) => {
                self.load_errors.push(format!(
                    "{}: unable to read plugin directory: {error}",
                    self.plugin_directory.display()
                ));
                return;
            }
        };
        let mut paths = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "toml")
            })
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            match fs::read_to_string(&path) {
                Ok(contents) => {
                    self.insert(&path.to_string_lossy(), &contents, PluginOrigin::Directory)
                }
                Err(error) => self
                    .load_errors
                    .push(format!("{}: {error}", path.display())),
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderProfileTemplate {
    pub provider: ProviderId,
    pub display_name: String,
    pub capabilities: CapabilityProfile,
}

impl ProviderProfileTemplate {
    pub fn materialize(&self, project_id: &str, updated_at: &str) -> AgentCapabilityProfile {
        AgentCapabilityProfile {
            id: format!("{project_id}:{}", self.provider),
            project_id: project_id.into(),
            provider: self.provider.clone(),
            display_name: self.display_name.clone(),
            strengths: self.capabilities.strengths.clone(),
            allowed_actions: self.capabilities.allowed_actions.clone(),
            max_context_tokens: self.capabilities.max_context_tokens,
            enabled: true,
            updated_at: updated_at.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDetectionRule {
    pub provider: ProviderId,
    pub display_name: String,
    pub detection: ProcessDetection,
}

impl ProviderDetectionRule {
    pub fn matches(&self, executable: &str, command_line: &[String]) -> bool {
        let executable = Path::new(executable)
            .file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_else(|| executable.to_ascii_lowercase());
        let command = command_line.join(" ").to_ascii_lowercase();
        if !self.detection.rules.is_empty() {
            return self.detection.rules.iter().any(|rule| {
                executable == rule.process.to_ascii_lowercase()
                    && (rule.command_contains.is_empty()
                        || rule
                            .command_contains
                            .iter()
                            .any(|needle| command.contains(&needle.to_ascii_lowercase())))
            });
        }
        self.detection
            .processes
            .iter()
            .any(|process| executable == process.to_ascii_lowercase())
            && (self.detection.command_contains.is_empty()
                || self
                    .detection
                    .command_contains
                    .iter()
                    .any(|needle| command.contains(&needle.to_ascii_lowercase())))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RawProviderEvent {
    pub integration_id: String,
    pub event_type: String,
    #[serde(default)]
    pub occurred_at: Option<String>,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IngestExternalEventInput {
    pub repository_path: String,
    pub integration_id: String,
    pub event_type: String,
    #[serde(default)]
    pub occurred_at: Option<String>,
    #[serde(default)]
    pub payload: Value,
}

impl IngestExternalEventInput {
    pub fn into_raw(self) -> RawProviderEvent {
        RawProviderEvent {
            integration_id: self.integration_id,
            event_type: self.event_type,
            occurred_at: self.occurred_at,
            payload: self.payload,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedProjectEvent {
    pub kind: EventKind,
    pub source: EventSource,
    pub payload: Value,
    pub occurred_at: Option<String>,
}

struct ManifestEventAdapter {
    integration_id: String,
}

impl EventAdapter for ManifestEventAdapter {
    fn integration_id(&self) -> &str {
        &self.integration_id
    }

    fn normalize(&self, raw: RawProviderEvent) -> Result<Vec<NormalizedProjectEvent>, PluginError> {
        validate_event_type(&raw.event_type)?;
        let kind = semantic_event_kind(&raw.event_type);
        let payload = json!({
            "integrationId": self.integration_id,
            "eventType": raw.event_type,
            "data": raw.payload,
        });
        Ok(vec![NormalizedProjectEvent {
            kind,
            source: EventSource::Other,
            payload,
            occurred_at: raw.occurred_at,
        }])
    }
}

fn semantic_event_kind(event_type: &str) -> EventKind {
    let event_type = event_type.to_ascii_lowercase();
    if event_type.contains("pull_request") || event_type.contains("pull-request") {
        EventKind::ExternalPullRequestUpdated
    } else if event_type.contains("issue") {
        EventKind::ExternalIssueUpdated
    } else if event_type.contains("branch") {
        EventKind::ExternalBranchUpdated
    } else if ["ci", "build", "test", "workflow", "check"]
        .iter()
        .any(|needle| event_type.contains(needle))
    {
        EventKind::ExternalCiUpdated
    } else if ["ide", "editor", "workspace", "active_file"]
        .iter()
        .any(|needle| event_type.contains(needle))
    {
        EventKind::ExternalIdeStateUpdated
    } else {
        EventKind::ExternalEventReceived
    }
}

fn validate_event_type(value: &str) -> Result<(), PluginError> {
    let valid = !value.is_empty()
        && value.len() <= 96
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        });
    valid
        .then_some(())
        .ok_or_else(|| PluginError::InvalidEventType(value.into()))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginActionRequest {
    pub integration_id: String,
    pub repository_path: String,
    pub session_id: Option<String>,
    pub requested_by: String,
    pub action: ActionKind,
    #[serde(default)]
    pub approval_granted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActionProposal {
    pub integration_id: String,
    pub request: ActionRequestInput,
}

struct ManifestActionAdapter {
    integration_id: String,
    actions: Vec<ActionKind>,
}

impl ActionAdapter for ManifestActionAdapter {
    fn integration_id(&self) -> &str {
        &self.integration_id
    }

    fn supported_actions(&self) -> Vec<ActionKind> {
        self.actions.clone()
    }

    fn execute(&self, request: PluginActionRequest) -> Result<ActionProposal, PluginError> {
        if !self.actions.contains(&request.action) {
            return Err(PluginError::UnsupportedAction {
                plugin: self.integration_id.clone(),
                action: request.action,
            });
        }
        Ok(ActionProposal {
            integration_id: self.integration_id.clone(),
            request: ActionRequestInput {
                repository_path: request.repository_path,
                session_id: request.session_id,
                requested_by: format!("{} via {}", request.requested_by, self.integration_id),
                action: request.action,
                approval_granted: request.approval_granted,
            },
        })
    }
}

pub fn builtin_provider_profile_templates() -> Vec<ProviderProfileTemplate> {
    IntegrationRegistry::built_in().capability_profile_templates()
}

fn builtin_manifests() -> [(&'static str, &'static str); 11] {
    [
        (
            "builtin/claude.toml",
            include_str!("../manifests/claude.toml"),
        ),
        (
            "builtin/codex.toml",
            include_str!("../manifests/codex.toml"),
        ),
        (
            "builtin/gemini.toml",
            include_str!("../manifests/gemini.toml"),
        ),
        (
            "builtin/cursor.toml",
            include_str!("../manifests/cursor.toml"),
        ),
        (
            "builtin/copilot.toml",
            include_str!("../manifests/copilot.toml"),
        ),
        (
            "builtin/github.toml",
            include_str!("../manifests/github.toml"),
        ),
        (
            "builtin/local-git.toml",
            include_str!("../manifests/local-git.toml"),
        ),
        (
            "builtin/vscode.toml",
            include_str!("../manifests/vscode.toml"),
        ),
        ("builtin/ci.toml", include_str!("../manifests/ci.toml")),
        (
            "builtin/test-runner.toml",
            include_str!("../manifests/test-runner.toml"),
        ),
        (
            "builtin/build-runner.toml",
            include_str!("../manifests/build-runner.toml"),
        ),
    ]
}

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("invalid plugin manifest: {0}")]
    InvalidManifest(String),
    #[error("invalid event type '{0}'")]
    InvalidEventType(String),
    #[error("plugin '{0}' is not registered")]
    NotRegistered(String),
    #[error("plugin '{0}' is disabled")]
    Disabled(String),
    #[error("plugin '{plugin}' uses SDK API {actual}; this build supports {expected}")]
    UnsupportedApiVersion {
        plugin: String,
        expected: u16,
        actual: u16,
    },
    #[error("plugin '{plugin}' does not accept event type '{event_type}'")]
    UnsupportedEvent { plugin: String, event_type: String },
    #[error("plugin '{plugin}' does not support action {action:?}")]
    UnsupportedAction { plugin: String, action: ActionKind },
    #[error("plugin adapter failed: {0}")]
    Adapter(String),
    #[error(transparent)]
    Toml(#[from] toml::de::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn builtins_are_valid_and_grouped() {
        let snapshot = IntegrationRegistry::built_in().snapshot();
        assert!(
            snapshot.load_errors.is_empty(),
            "{:?}",
            snapshot.load_errors
        );
        assert_eq!(snapshot.entries.len(), 11);
        assert!(snapshot
            .entries
            .iter()
            .any(|entry| entry.manifest.kind == PluginKind::Provider));
        assert!(snapshot
            .entries
            .iter()
            .any(|entry| entry.manifest.kind == PluginKind::DeveloperTool));
        assert!(snapshot
            .entries
            .iter()
            .any(|entry| entry.manifest.kind == PluginKind::Automation));
    }

    #[test]
    fn process_matching_is_manifest_driven() {
        let registry = IntegrationRegistry::built_in();
        let rules = registry.provider_detection_rules();
        assert_eq!(
            rules
                .iter()
                .find(|rule| rule.matches("codex.exe", &[]))
                .map(|rule| rule.provider.clone()),
            Some(ProviderId::codex())
        );
        assert_eq!(
            rules
                .iter()
                .find(|rule| rule.matches("node.exe", &["@anthropic-ai/claude-code".into()]))
                .map(|rule| rule.provider.clone()),
            Some(ProviderId::claude())
        );
        assert!(rules.iter().all(|rule| !rule.matches("Code.exe", &[])));
    }

    #[test]
    fn adapters_only_return_normalized_events_and_action_proposals() {
        let registry = IntegrationRegistry::built_in();
        let event = registry
            .normalize(RawProviderEvent {
                integration_id: "github".into(),
                event_type: "issue.updated".into(),
                occurred_at: None,
                payload: json!({"number": 42, "title": "Fix auth"}),
            })
            .unwrap();
        assert_eq!(event[0].kind, EventKind::ExternalIssueUpdated);

        let proposal = registry
            .propose_action(PluginActionRequest {
                integration_id: "test-runner".into(),
                repository_path: "C:/project".into(),
                session_id: None,
                requested_by: "Codex".into(),
                action: ActionKind::RunTests,
                approval_granted: false,
            })
            .unwrap();
        assert_eq!(proposal.request.action, ActionKind::RunTests);
    }

    #[test]
    fn unknown_manifest_events_remain_forward_compatible() {
        let registry = IntegrationRegistry::built_in();
        let event = registry
            .normalize(RawProviderEvent {
                integration_id: "local-git".into(),
                event_type: "tag.created".into(),
                occurred_at: None,
                payload: json!({"tag": "v1.0.0"}),
            })
            .unwrap();
        assert_eq!(event[0].kind, EventKind::ExternalEventReceived);
    }

    #[test]
    fn directory_manifest_adds_a_provider_without_core_changes() {
        let directory = TempDir::new().unwrap();
        fs::write(
            directory.path().join("future-agent.toml"),
            r#"
id = "future-agent"
name = "Future Agent"
version = "1.0.0"
kind = "provider"
sdk_api_version = 1

[capabilities]
strengths = ["security"]
max_context_tokens = 4096

[detection]
processes = ["future-agent.exe"]

[permissions]
read_repo = true
write_repo = false
run_commands = false

[adapters]
event_types = ["*"]
"#,
        )
        .unwrap();

        let registry = IntegrationRegistry::load(directory.path());
        assert!(registry.snapshot().load_errors.is_empty());
        let manifest = registry.manifest("future-agent").unwrap();
        assert_eq!(manifest.kind, PluginKind::Provider);
        assert_eq!(manifest.capabilities.max_context_tokens, 4_096);
        assert!(registry
            .provider_detection_rules()
            .iter()
            .any(|rule| rule.matches("future-agent.exe", &[])));
    }
}
