mod actions;
mod conflict;
pub mod context;
pub mod detection;
pub mod domain;
mod environment;
pub mod git;
mod graph;
pub mod integrations;
mod orchestration;
mod planner;
pub mod ports;
mod replay;
pub mod sdk;
pub mod session;
pub mod source_control;
#[path = "memory.rs"]
pub mod storage;
mod verification;
pub mod worktree;

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::{SecondsFormat, Utc};
use sha2::{Digest, Sha256};

use actions::resolve_action;
use context::{
    compile_snapshot, compression_request, estimate_tokens, render_markdown, validate_compression,
    CompressionMode, CompressionState, ContextCompileInput, ContextDepth, ContextPackage,
    ContextProfile, ContextRequest,
};
use detection::DetectedAgent;
use domain::{
    ActionRequestInput, ActionRun, ActionStatus, AgentCapabilityProfile, AgentSession,
    ApplicationSettings, ApprovePlanInput, ApproveTaskAssignmentInput, BackupRecord,
    CollaborativeExecutionSnapshot, CompareReplayInput, CompleteProjectOnboardingInput,
    ConflictRecord, ContextVersion, CreateDecisionInput, CreateGoalInput, CreateTaskInput,
    Decision, DeliverySnapshot, DeliverySyncReport, EndSessionInput, EnvironmentDashboard,
    EnvironmentManifest, EnvironmentSnapshot, EnvironmentStatus, EvidenceReference,
    ExplainHistoryInput, ExplanationCategory, ExplanationReason, ExplanationRecord,
    GeneratePlanInput, Goal, HandoffSnapshot, HistoryExplanation, ImpactAnalysis, ImpactRequest,
    OrchestrationSnapshot, PlanApprovalResult, PlannerSnapshot, PrepareTaskWorkspaceInput,
    ProjectGraph, ProjectMemory, ProjectOnboardingResult, ProjectPermissions, ProjectRecord,
    RecoveryReport, ReplanInput, ReplayComparison, ReplayRequest, ReplayState,
    RepositorySetupInspection, RunVerificationInput, SandboxStatus, SaveEnvironmentManifestInput,
    StartRoutedTaskInput, StartSessionInput, StartTaskWorkspaceInput, SyncDeliveryInput,
    TaskAssignmentStatus, TaskStatus, TaskWorkspace, TaskWorkspaceStatus, TimelineSnapshot,
    UpdateCapabilityProfileInput, UpdateGoalInput, UpdateProjectPermissionsInput, UpdateTaskInput,
    UpdateTaskOrchestrationInput, ValidateTaskWorkspaceInput, ValidationCheck, ValidationCheckKind,
    ValidationCheckStatus, VerificationGateKind, VerificationImpact, VerificationSnapshot,
    VerificationStatus, VerificationSuite, WaiveVerificationGateInput, WorkTask, WorkspaceIdInput,
    WorkspaceValidation, WorkspaceValidationResult, WorkspaceValidationStatus,
};
use environment::{EnvironmentError, EnvironmentInspection, EnvironmentService};
use git::{GitError, GitScanner, ProjectStatus};
use integrations::{
    github::GitHubSourceControl, nebius::NebiusNemotron, sandbox::ContreeCliSandbox,
};
use ports::{ContextCompressor, ExecutionSandbox, SandboxExecutionRequest};
use sdk::{
    IngestExternalEventInput, IntegrationRegistry, PluginActionRequest, PluginError,
    ProviderDetectionRule, RegistrySnapshot,
};
use session::{SessionError, SessionService};
use source_control::{SourceControlError, SourceControlProvider};
use storage::{ContextStore, StorageError};
use thiserror::Error;
use uuid::Uuid;
use worktree::{GitWorktreeManager, WorktreeError};

/// Application service shared by the Tauri shell, a future daemon, CLI and MCP server.
pub struct AgentRelayCore {
    git: GitScanner,
    store: ContextStore,
    compressor: Option<Box<dyn ContextCompressor>>,
    compression_configuration_error: Option<String>,
    sandbox: Option<Box<dyn ExecutionSandbox>>,
    sandbox_configuration_error: Option<String>,
    registry: IntegrationRegistry,
    source_control: Box<dyn SourceControlProvider>,
    worktrees: GitWorktreeManager,
    environments: EnvironmentService,
    environment_cache_root: PathBuf,
}

impl AgentRelayCore {
    pub fn open(
        database_path: impl AsRef<Path>,
        command_timeout: Duration,
    ) -> Result<Self, CoreError> {
        let database_path = database_path.as_ref().to_path_buf();
        let (compressor, compression_configuration_error) = match NebiusNemotron::from_env() {
            Ok(compressor) => (
                compressor.map(|value| Box::new(value) as Box<dyn ContextCompressor>),
                None,
            ),
            Err(error) => (None, Some(error)),
        };
        let (sandbox, sandbox_configuration_error) = match ContreeCliSandbox::from_env() {
            Ok(Some(value)) => (Some(Box::new(value) as Box<dyn ExecutionSandbox>), None),
            Ok(None) => (
                None,
                Some(
                    "Set AGENTRELAY_SANDBOX_IMAGE after authenticating the ConTree CLI to enable remote execution."
                        .into(),
                ),
            ),
            Err(error) => (None, Some(error)),
        };
        let environment_cache_root = database_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("environment-cache");
        Ok(Self {
            git: GitScanner::new(command_timeout),
            registry: IntegrationRegistry::load(
                database_path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join("plugins"),
            ),
            store: ContextStore::open(&database_path)?,
            source_control: Box::new(GitHubSourceControl::from_env()),
            worktrees: GitWorktreeManager::new(command_timeout),
            environments: EnvironmentService::new(Duration::from_secs(30 * 60)),
            environment_cache_root,
            compressor,
            compression_configuration_error,
            sandbox,
            sandbox_configuration_error,
        })
    }

    #[cfg(test)]
    fn open_with_sandbox(
        database_path: impl AsRef<Path>,
        command_timeout: Duration,
        sandbox: Box<dyn ExecutionSandbox>,
    ) -> Result<Self, CoreError> {
        let database_path = database_path.as_ref().to_path_buf();
        let environment_cache_root = database_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("environment-cache");
        Ok(Self {
            git: GitScanner::new(command_timeout),
            store: ContextStore::open(database_path)?,
            compressor: None,
            compression_configuration_error: None,
            sandbox: Some(sandbox),
            sandbox_configuration_error: None,
            registry: IntegrationRegistry::built_in(),
            source_control: Box::new(GitHubSourceControl::from_env()),
            worktrees: GitWorktreeManager::new(command_timeout),
            environments: EnvironmentService::new(Duration::from_secs(30 * 60)),
            environment_cache_root,
        })
    }

    pub fn scan_project(
        &self,
        repository_path: impl AsRef<Path>,
    ) -> Result<ProjectStatus, CoreError> {
        let status = self.git.scan(repository_path)?;
        self.store.record_scan(&status)?;
        self.store.evaluate_conflicts(&status.repository)?;
        Ok(status)
    }

    pub fn integration_registry(&self) -> RegistrySnapshot {
        self.registry.snapshot()
    }

    pub fn provider_detection_rules(&self) -> Vec<ProviderDetectionRule> {
        self.registry.provider_detection_rules()
    }

    pub fn ingest_external_event(
        &self,
        input: IngestExternalEventInput,
    ) -> Result<Vec<domain::ProjectEvent>, CoreError> {
        let repository_path = input.repository_path.clone();
        let normalized = self.registry.normalize(input.into_raw())?;
        let mut recorded = Vec::with_capacity(normalized.len());
        for event in normalized {
            recorded.push(self.store.record_external_event(&repository_path, event)?);
        }
        self.store.evaluate_conflicts(&repository_path)?;
        Ok(recorded)
    }

    pub fn request_plugin_action(
        &self,
        input: PluginActionRequest,
    ) -> Result<ActionRun, CoreError> {
        let proposal = self.registry.propose_action(input)?;
        self.store.record_plugin_action_proposed(
            &proposal.request.repository_path,
            &proposal.integration_id,
            proposal.request.action,
        )?;
        self.request_action(proposal.request)
    }

    pub fn last_repository(&self) -> Result<Option<String>, CoreError> {
        Ok(self.store.last_repository()?)
    }

    pub fn application_settings(&self) -> Result<ApplicationSettings, CoreError> {
        Ok(self.store.application_settings()?)
    }

    pub fn last_recovery_report(&self) -> Result<Option<RecoveryReport>, CoreError> {
        Ok(self.store.last_recovery_report()?)
    }

    pub fn save_application_settings(
        &self,
        settings: ApplicationSettings,
    ) -> Result<ApplicationSettings, CoreError> {
        Ok(self.store.save_application_settings(settings)?)
    }

    pub fn backups(&self) -> Result<Vec<BackupRecord>, CoreError> {
        Ok(self.store.backups()?)
    }

    pub fn create_backup(&self) -> Result<BackupRecord, CoreError> {
        Ok(self.store.create_backup()?)
    }

    pub fn inspect_repository_setup(
        &self,
        repository_path: &str,
    ) -> Result<RepositorySetupInspection, CoreError> {
        let path = Path::new(repository_path);
        if !path.is_dir() {
            return Err(CoreError::Routing(
                "select an existing repository directory".into(),
            ));
        }
        let mut stack = Vec::new();
        let mut gates = Vec::new();
        if path.join("Cargo.toml").is_file() {
            stack.push("Rust".into());
            gates.extend(
                ["cargo fmt", "cargo clippy", "cargo test", "cargo build"].map(str::to_owned),
            );
        }
        if path.join("package.json").is_file() {
            stack.push("Node.js / TypeScript".into());
            gates.extend(["npm test", "npm run build"].map(str::to_owned));
        }
        if path.join("src-tauri/tauri.conf.json").is_file()
            || path.join("frontend/src-tauri/tauri.conf.json").is_file()
        {
            stack.push("Tauri".into());
            gates.push("cargo tauri build".into());
        }
        if path.join("pyproject.toml").is_file() || path.join("requirements.txt").is_file() {
            stack.push("Python".into());
            gates.push("python tests".into());
        }
        if stack.is_empty() {
            stack.push("Unrecognized / custom".into());
        }
        gates.sort();
        gates.dedup();
        let git_repository = path.join(".git").exists();
        let existing_memory = self.store.last_repository()?.is_some_and(|last| {
            Path::new(&last)
                .canonicalize()
                .ok()
                .zip(path.canonicalize().ok())
                .is_some_and(|(left, right)| left == right)
        }) || self
            .store
            .project_is_onboarded(repository_path)
            .unwrap_or(false);
        let mut warnings = Vec::new();
        if !git_repository {
            warnings.push("This folder is not a Git repository.".into());
        }
        if path.join("AGENT_CONTEXT.md").is_file() {
            warnings.push(
                "An existing AGENT_CONTEXT.md will be archived before AgentRelay generates its projection."
                    .into(),
            );
        }
        Ok(RepositorySetupInspection {
            repository_path: path.to_string_lossy().replace('\\', "/"),
            name: path
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_else(|| repository_path.into()),
            git_repository,
            detected_stack: stack,
            existing_context: path.join("AGENT_CONTEXT.md").is_file(),
            existing_memory,
            suggested_quality_gates: gates,
            warnings,
        })
    }

    pub fn complete_project_onboarding(
        &self,
        input: CompleteProjectOnboardingInput,
    ) -> Result<ProjectOnboardingResult, CoreError> {
        let inspection = self.inspect_repository_setup(&input.repository_path)?;
        if !inspection.git_repository {
            return Err(CoreError::Routing(
                "AgentRelay requires a Git repository for safe change tracking".into(),
            ));
        }
        let status = self.scan_project(&input.repository_path)?;
        let context_path = Path::new(&status.repository).join("AGENT_CONTEXT.md");
        let imported_context_path = if input.preserve_existing_context && context_path.is_file() {
            let imports = Path::new(&status.repository).join(".agentrelay/imports");
            fs::create_dir_all(&imports)?;
            let destination = imports.join(format!(
                "AGENT_CONTEXT.before-agentrelay-{}.md",
                Utc::now().format("%Y%m%dT%H%M%SZ")
            ));
            fs::copy(&context_path, &destination)?;
            Some(destination.to_string_lossy().replace('\\', "/"))
        } else {
            None
        };
        self.set_project_tracking(&status.repository, true)?;
        self.update_project_permissions(UpdateProjectPermissionsInput {
            repository_path: status.repository.clone(),
            run_tests: input.run_tests,
            run_build: input.run_build,
        })?;
        self.generate_context(&status.repository)?;
        self.store.mark_project_onboarded(&status.repository)?;
        let mut settings = self.application_settings()?;
        settings.onboarding_completed = true;
        self.save_application_settings(settings)?;
        let backup = self.create_backup()?;
        Ok(ProjectOnboardingResult {
            repository_path: status.repository,
            imported_context_path,
            generated_context_path: context_path.to_string_lossy().replace('\\', "/"),
            backup,
        })
    }

    pub fn tracked_projects(&self) -> Result<Vec<ProjectRecord>, CoreError> {
        Ok(self.store.tracked_projects()?)
    }

    pub fn set_project_tracking(
        &self,
        repository_path: &str,
        enabled: bool,
    ) -> Result<ProjectRecord, CoreError> {
        Ok(self.store.set_project_tracking(repository_path, enabled)?)
    }

    pub fn idle_timeout_minutes(&self) -> Result<u64, CoreError> {
        Ok(self.store.idle_timeout_minutes()?)
    }

    pub fn set_idle_timeout_minutes(&self, minutes: u64) -> Result<u64, CoreError> {
        Ok(self.store.set_idle_timeout_minutes(minutes)?)
    }

    pub fn project_memory(&self, repository_path: &str) -> Result<ProjectMemory, CoreError> {
        Ok(self.store.project_memory(repository_path)?)
    }

    pub fn audit_trail(&self, repository_path: &str) -> Result<Vec<ExplanationRecord>, CoreError> {
        Ok(self.store.audit_trail(repository_path)?)
    }

    // Explanation records intentionally mirror the complete audit schema at the single write
    // boundary so callers cannot omit the decision, outcome, reasons, or evidence references.
    #[allow(clippy::too_many_arguments)]
    fn record_explanation(
        &self,
        project_id: &str,
        category: ExplanationCategory,
        subject_type: &str,
        subject_id: &str,
        decision: String,
        outcome: &str,
        reasons: Vec<ExplanationReason>,
        evidence_refs: Vec<EvidenceReference>,
    ) -> Result<ExplanationRecord, CoreError> {
        let record = ExplanationRecord {
            id: Uuid::new_v4().to_string(),
            project_id: project_id.to_owned(),
            category,
            subject_type: subject_type.to_owned(),
            subject_id: subject_id.to_owned(),
            decision,
            outcome: outcome.to_owned(),
            reasons,
            evidence_refs,
            created_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        };
        self.store.record_explanation(&record)?;
        Ok(record)
    }

    fn record_plan_explanation(
        &self,
        plan: &domain::PlanVersion,
        category: ExplanationCategory,
        decision: String,
        outcome: &str,
    ) -> Result<(), CoreError> {
        let mut reasons = vec![audit_reason(
            "plan-rationale",
            &plan.reason,
            "The deterministic planner recorded this rationale with the immutable plan version.",
            None,
        )];
        reasons.extend(plan.validation_issues.iter().map(|issue| {
            audit_reason(
                &issue.code,
                &issue.detail,
                &format!("Plan validation classified this as {:?}.", issue.severity),
                None,
            )
        }));
        if !plan.changed_task_ids.is_empty() {
            reasons.push(audit_reason(
                "changed-tasks",
                &format!("{} task(s) changed", plan.changed_task_ids.len()),
                &plan.changed_task_ids.join(", "),
                None,
            ));
        }
        if !plan.changed_dependencies.is_empty() {
            reasons.push(audit_reason(
                "changed-dependencies",
                &format!(
                    "{} dependency relationship(s) changed",
                    plan.changed_dependencies.len()
                ),
                &plan.changed_dependencies.join(", "),
                None,
            ));
        }
        let mut evidence_refs = vec![
            audit_evidence("plan", &plan.id, &format!("Plan v{}", plan.version)),
            audit_evidence("goal", &plan.goal_id, "Plan goal"),
        ];
        evidence_refs.extend(
            plan.tasks
                .iter()
                .map(|task| audit_evidence("plannedTask", &task.id, &task.title)),
        );
        if let Some(parent) = &plan.parent_plan_id {
            evidence_refs.push(audit_evidence("plan", parent, "Parent plan"));
        }
        self.record_explanation(
            &plan.project_id,
            category,
            "plan",
            &plan.id,
            decision,
            outcome,
            reasons,
            evidence_refs,
        )?;
        Ok(())
    }

    fn record_verification_explanations(&self, suite: &VerificationSuite) -> Result<(), CoreError> {
        let failed = suite
            .gates
            .iter()
            .filter(|gate| gate.required && gate.status == VerificationStatus::Failed)
            .count();
        let waived = suite
            .gates
            .iter()
            .filter(|gate| gate.status == VerificationStatus::Waived)
            .count();
        self.record_explanation(
            &suite.project_id,
            ExplanationCategory::Verification,
            "verificationSuite",
            &suite.id,
            format!("Verification suite {:?}", suite.status),
            &format!("{:?}", suite.status),
            vec![
                audit_reason(
                    "required-gate-failures",
                    &format!("{failed} required gate(s) failed"),
                    "A suite cannot pass while any required gate remains failed.",
                    Some(failed as i32),
                ),
                audit_reason(
                    "explicit-waivers",
                    &format!("{waived} gate(s) explicitly waived"),
                    "Waivers require a durable user-provided reason and remain visible in the audit trail.",
                    Some(waived as i32),
                ),
                audit_reason(
                    "environment-binding",
                    "Verification bound to a reproducible environment",
                    &format!(
                        "Environment {} with fingerprint {} verified commit {}.",
                        suite.environment_id, suite.environment_fingerprint, suite.commit
                    ),
                    None,
                ),
            ],
            vec![
                audit_evidence("task", &suite.task_id, "Verified task"),
                audit_evidence("workspace", &suite.workspace_id, &suite.worktree),
                audit_evidence("commit", &suite.commit, &suite.branch),
                audit_evidence(
                    "environment",
                    &suite.environment_id,
                    &suite.environment_fingerprint,
                ),
            ],
        )?;
        for gate in &suite.gates {
            let mut reasons = vec![audit_reason(
                "gate-result",
                &gate.detail,
                &format!(
                    "Required: {}. Exit code: {}. Duration: {} ms.",
                    gate.required,
                    gate.exit_code
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "not available".into()),
                    gate.duration_ms
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "not available".into())
                ),
                gate.exit_code,
            )];
            if let Some(reason) = &gate.waiver_reason {
                reasons.push(audit_reason(
                    "waiver-reason",
                    reason,
                    "This gate result was explicitly waived by the user.",
                    None,
                ));
            }
            self.record_explanation(
                &suite.project_id,
                ExplanationCategory::Verification,
                "verificationGate",
                &gate.id,
                format!("{} gate {:?}", gate.name, gate.status),
                &format!("{:?}", gate.status),
                reasons,
                vec![
                    audit_evidence("verificationSuite", &suite.id, "Parent suite"),
                    audit_evidence("task", &suite.task_id, "Verified task"),
                    audit_evidence("commit", &suite.commit, &suite.branch),
                ],
            )?;
        }
        Ok(())
    }

    fn record_context_explanations(
        &self,
        version: &ContextVersion,
        package: &ContextPackage,
    ) -> Result<(), CoreError> {
        self.record_explanation(
            &version.project_id,
            ExplanationCategory::Context,
            "contextSnapshot",
            &version.id,
            format!(
                "Compiled {} grounded context item(s)",
                package.snapshot.items.len()
            ),
            "generated",
            vec![
                audit_reason(
                    "token-budget",
                    &format!(
                        "{} of {} estimated tokens used",
                        package.snapshot.estimated_tokens,
                        package.snapshot.request.token_budget
                    ),
                    "Deterministic selection stops before the configured context budget is exceeded.",
                    Some(package.snapshot.estimated_tokens.min(i32::MAX as usize) as i32),
                ),
                audit_reason(
                    "context-profile",
                    &format!("{:?} profile", package.snapshot.request.profile),
                    "The profile changes relevance weights without changing the underlying SQLite facts.",
                    None,
                ),
                audit_reason(
                    "omitted-items",
                    &format!(
                        "{} lower-ranked item(s) omitted",
                        package.snapshot.omitted_item_count
                    ),
                    "Items may be omitted because of token budget, tier limits, supersession, invalidation, or lower relevance to the active task.",
                    Some(package.snapshot.omitted_item_count.min(i32::MAX as usize) as i32),
                ),
                audit_reason(
                    "compression-mode",
                    &format!("{:?}", package.snapshot.compression.mode),
                    &package.snapshot.compression.detail,
                    None,
                ),
            ],
            vec![
                audit_evidence("contextHash", &version.context_snapshot_hash, "SHA-256 context hash"),
                audit_evidence("commit", &version.commit, &version.branch),
            ],
        )?;
        for item in &package.snapshot.items {
            self.record_explanation(
                &version.project_id,
                ExplanationCategory::Context,
                "contextItem",
                &item.id,
                format!("Included {} in agent context", item.title),
                &format!("{:?}", item.tier),
                item.inclusion_reasons
                    .iter()
                    .enumerate()
                    .map(|(index, reason)| {
                        audit_reason(
                            &format!("context-reason-{}", index + 1),
                            reason,
                            &format!(
                                "Deterministic relevance score {} placed this item in the {:?} tier.",
                                item.relevance_score, item.tier
                            ),
                            (index == 0).then_some(i32::from(item.relevance_score)),
                        )
                    })
                    .collect(),
                item.sources
                    .iter()
                    .map(|source| {
                        audit_evidence(
                            &format!("{:?}", source.kind),
                            &source.id,
                            &source.label,
                        )
                    })
                    .collect(),
            )?;
        }
        Ok(())
    }

    /// Returns the append-only project timeline without scanning Git or changing live state.
    pub fn timeline(&self, repository_path: &str) -> Result<TimelineSnapshot, CoreError> {
        let source = self.replay_source(repository_path)?;
        Ok(replay::timeline(&source))
    }

    /// Reconstructs a historical projection. This path is intentionally SQLite-read-only.
    pub fn replay_project(&self, input: ReplayRequest) -> Result<ReplayState, CoreError> {
        let source = self.replay_source(&input.repository_path)?;
        replay::replay(&source, &input).map_err(CoreError::Replay)
    }

    pub fn compare_replay_points(
        &self,
        input: CompareReplayInput,
    ) -> Result<ReplayComparison, CoreError> {
        let source = self.replay_source(&input.repository_path)?;
        replay::compare(&source, &input.left_event_id, &input.right_event_id)
            .map_err(CoreError::Replay)
    }

    pub fn explain_history(
        &self,
        input: ExplainHistoryInput,
    ) -> Result<HistoryExplanation, CoreError> {
        let source = self.replay_source(&input.repository_path)?;
        replay::explain(&source, &input).map_err(CoreError::Replay)
    }

    fn replay_source(&self, repository_path: &str) -> Result<replay::ReplaySource, CoreError> {
        Ok(replay::ReplaySource {
            memory: self.store.project_memory(repository_path)?,
            events: self.store.replay_events(repository_path)?,
            sessions: self.store.replay_sessions(repository_path)?,
            contexts: self.store.context_versions(repository_path)?,
            plans: self.store.plans_for_project(repository_path)?,
            verifications: self.store.verification_suites(repository_path)?,
            environments: self.store.environment_snapshots(repository_path)?,
        })
    }

    pub fn project_graph(&self, repository_path: &str) -> Result<ProjectGraph, CoreError> {
        let memory = self.store.project_memory(repository_path)?;
        let files = self.git.project_files(repository_path)?;
        Ok(graph::build(&memory, &files))
    }

    pub fn impact_analysis(&self, input: ImpactRequest) -> Result<ImpactAnalysis, CoreError> {
        let memory = self.store.project_memory(&input.repository_path)?;
        let files = self.git.project_files(&input.repository_path)?;
        let project_graph = graph::build(&memory, &files);
        let analysis = graph::analyze(&project_graph, &memory, &input.target)?;
        let mut evidence_refs = vec![audit_evidence(
            "file",
            &analysis.target.id,
            &analysis.target.label,
        )];
        evidence_refs.extend(
            analysis
                .affected_tasks
                .iter()
                .map(|node| audit_evidence("task", &node.id, &node.label)),
        );
        evidence_refs.extend(
            analysis
                .relevant_decisions
                .iter()
                .map(|node| audit_evidence("decision", &node.id, &node.label)),
        );
        evidence_refs.extend(
            analysis
                .validating_tests
                .iter()
                .map(|node| audit_evidence("test", &node.id, &node.label)),
        );
        let reasons = if analysis.risk.factors.is_empty() {
            vec![audit_reason(
                "risk-baseline",
                "No elevated deterministic risk factors",
                "The project graph found no dependents, conflicts, uncovered tests, or active work that increased this file's score.",
                Some(0),
            )]
        } else {
            analysis
                .risk
                .factors
                .iter()
                .map(|factor| {
                    audit_reason(
                        &factor.key,
                        &factor.label,
                        &factor.detail,
                        Some(i32::from(factor.points)),
                    )
                })
                .collect()
        };
        self.record_explanation(
            &memory.project.id,
            ExplanationCategory::Risk,
            "file",
            &analysis.target.id,
            format!(
                "Risk {:?}: {}/100 for {}",
                analysis.risk.level, analysis.risk.score, analysis.target.label
            ),
            &format!("{:?}", analysis.risk.level),
            reasons,
            evidence_refs,
        )?;
        Ok(analysis)
    }

    pub fn environment_dashboard(
        &self,
        repository_path: &str,
    ) -> Result<EnvironmentDashboard, CoreError> {
        let memory = self.store.project_memory(repository_path)?;
        let status = self.git.scan(repository_path)?;
        let (manifest, snapshot) = self.capture_environment(
            repository_path,
            repository_path,
            &status.head_commit,
            &status.branch,
            true,
        )?;
        let caches = self.store.environment_caches(repository_path)?;
        let latest_verification = self
            .store
            .verification_suites(repository_path)?
            .into_iter()
            .next();
        debug_assert_eq!(snapshot.project_id, memory.project.id);
        Ok(EnvironmentDashboard {
            manifest,
            snapshot,
            caches,
            latest_verification,
        })
    }

    pub fn save_environment_manifest(
        &self,
        input: SaveEnvironmentManifestInput,
    ) -> Result<EnvironmentDashboard, CoreError> {
        let path = self
            .environments
            .save(Path::new(&input.repository_path), &input.manifest)?;
        self.store.record_environment_manifest_saved(
            &input.repository_path,
            &path.to_string_lossy().replace('\\', "/"),
        )?;
        self.environment_dashboard(&input.repository_path)
    }

    pub fn verification_snapshot(
        &self,
        repository_path: &str,
    ) -> Result<VerificationSnapshot, CoreError> {
        let suites = self.store.verification_suites(repository_path)?;
        Ok(VerificationSnapshot {
            latest: suites.first().cloned(),
            suites,
        })
    }

    pub fn run_workspace_verification(
        &self,
        input: RunVerificationInput,
    ) -> Result<VerificationSuite, CoreError> {
        let workspace = self.store.workspace(&input.workspace_id)?;
        if workspace.status != TaskWorkspaceStatus::ValidationPassed {
            return Err(CoreError::Routing(
                "deterministic workspace validation must pass before independent verification"
                    .into(),
            ));
        }
        let repository = self
            .store
            .repository_for_project_id(&workspace.project_id)?;
        let inspection = self
            .worktrees
            .inspect(&workspace.worktree_path, &workspace.base_commit)?;
        if !inspection.clean || inspection.head_commit != workspace.head_commit {
            return Err(CoreError::Routing(
                "the workspace changed after validation; validate the current head again".into(),
            ));
        }
        let memory = self.store.project_memory(&repository)?;
        let task = memory
            .goals
            .iter()
            .flat_map(|goal| &goal.tasks)
            .find(|task| task.id == workspace.task_id)
            .cloned()
            .ok_or_else(|| StorageError::TaskNotFound(workspace.task_id.clone()))?;
        let goal_description = memory
            .goals
            .iter()
            .find(|goal| goal.id == task.goal_id)
            .map(|goal| goal.description.as_str())
            .unwrap_or("");
        let (manifest, environment) = self.capture_environment(
            &repository,
            &workspace.worktree_path,
            &inspection.head_commit,
            &workspace.branch,
            false,
        )?;
        let impact =
            self.verification_impact(&memory, &workspace.worktree_path, &inspection.changed_files)?;
        let mut suite = verification::create_suite(
            &workspace.project_id,
            &task,
            &workspace,
            &environment,
            &manifest,
            impact,
            goal_description,
        );
        self.store.save_verification_suite(&suite)?;
        let permissions = self.store.project_permissions(&repository)?;
        let environment_ready = environment.status == EnvironmentStatus::Reproducible;
        for gate in suite
            .gates
            .iter_mut()
            .filter(|gate| !gate.command.is_empty())
        {
            gate.started_at = Some(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true));
            let permitted = match gate.kind {
                VerificationGateKind::Format
                | VerificationGateKind::Lint
                | VerificationGateKind::Tests => {
                    permissions.run_tests.permits(input.approval_granted)
                }
                VerificationGateKind::Build
                | VerificationGateKind::FrontendBuild
                | VerificationGateKind::DesktopBuild => {
                    permissions.run_build.permits(input.approval_granted)
                }
                _ => true,
            };
            if !environment_ready {
                verification::finish_gate(
                    gate,
                    VerificationStatus::Failed,
                    None,
                    Some(0),
                    "",
                    "",
                    "Gate was not run because the environment is not reproducible.".into(),
                );
                continue;
            }
            if !permitted {
                verification::finish_gate(
                    gate,
                    VerificationStatus::Failed,
                    None,
                    Some(0),
                    "",
                    "",
                    "Project policy denied this verification command. Enable the matching test or build permission, then rerun verification.".into(),
                );
                continue;
            }
            match self.environments.execute(
                &manifest,
                &environment,
                Path::new(&workspace.worktree_path),
                &gate.command,
                &self.environment_cache_root,
                &suite.id,
            ) {
                Ok(output) => {
                    let passed = output.exit_code == 0;
                    verification::finish_gate(
                        gate,
                        if passed {
                            VerificationStatus::Passed
                        } else {
                            VerificationStatus::Failed
                        },
                        Some(output.exit_code),
                        Some(output.duration.as_millis().min(u64::MAX as u128) as u64),
                        &output.stdout,
                        &output.stderr,
                        if passed {
                            "Command completed successfully in the captured environment.".into()
                        } else {
                            "Command failed in the captured environment.".into()
                        },
                    );
                }
                Err(error) => verification::finish_gate(
                    gate,
                    VerificationStatus::Failed,
                    None,
                    None,
                    "",
                    "",
                    error.to_string(),
                ),
            }
        }
        if let Some(gate) = suite
            .gates
            .iter_mut()
            .find(|gate| gate.kind == VerificationGateKind::Architecture)
        {
            gate.started_at = Some(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true));
            let (status, detail) = verification::verify_architecture(
                Path::new(&workspace.worktree_path),
                &manifest.architecture_rules,
            );
            verification::finish_gate(gate, status, None, Some(0), "", "", detail);
        }
        if let Some(gate) = suite
            .gates
            .iter_mut()
            .find(|gate| gate.kind == VerificationGateKind::AcceptanceCriteria)
        {
            gate.started_at = Some(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true));
            let (status, detail) = verification::verify_acceptance(
                &task,
                &inspection.changed_files,
                &suite.acceptance_criteria,
            );
            verification::finish_gate(gate, status, None, Some(0), "", "", detail);
        }
        verification::complete_suite(&mut suite);
        if suite.status == VerificationStatus::Failed {
            suite.repair_task_id = self.handle_verification_failure(
                &repository,
                &task,
                &suite,
                &inspection.changed_files,
            )?;
        }
        if let Err(error) =
            self.environments
                .cleanup_disposable(&manifest, &self.environment_cache_root, &suite.id)
        {
            if let Some(gate) = suite
                .gates
                .iter_mut()
                .find(|gate| gate.kind == VerificationGateKind::Environment)
            {
                gate.stderr_summary = format!("Verifier cleanup warning: {error}");
            }
        }
        let suite = self.store.save_verification_suite(&suite)?;
        self.record_verification_explanations(&suite)?;
        Ok(suite)
    }

    pub fn waive_verification_gate(
        &self,
        input: WaiveVerificationGateInput,
    ) -> Result<VerificationSuite, CoreError> {
        let mut suite = self.store.verification_suite(&input.suite_id)?;
        verification::waive_gate(&mut suite, &input.gate_id, &input.reason)
            .map_err(CoreError::Routing)?;
        let suite = self.store.save_verification_suite(&suite)?;
        self.record_verification_explanations(&suite)?;
        Ok(suite)
    }

    fn capture_environment(
        &self,
        project_repository: &str,
        execution_repository: &str,
        commit: &str,
        branch: &str,
        compare_last_verified: bool,
    ) -> Result<(EnvironmentManifest, EnvironmentSnapshot), CoreError> {
        let memory = self.store.project_memory(project_repository)?;
        let (manifest, source) = self
            .environments
            .load_or_infer(Path::new(project_repository))?;
        let last_verified = if compare_last_verified {
            self.store.latest_verified_fingerprint(&memory.project.id)?
        } else {
            None
        };
        let mut snapshot = self.environments.inspect(EnvironmentInspection {
            manifest: &manifest,
            manifest_source: &source,
            repository: Path::new(execution_repository),
            project_id: &memory.project.id,
            commit,
            branch,
            last_verified_fingerprint: last_verified.as_deref(),
        })?;
        if project_repository != execution_repository
            && worktree_environment_manifest_changed(
                Path::new(project_repository),
                Path::new(execution_repository),
                &manifest,
            )
        {
            snapshot.drift.push(domain::EnvironmentDrift {
                subject: "Environment manifest trust".into(),
                expected: "project-approved manifest".into(),
                detected: "task-worktree manifest differs".into(),
                detail: "Verification commands remain anchored to the project checkout. Approve and commit environment-manifest changes before using them in an isolated verifier.".into(),
            });
            snapshot.status = EnvironmentStatus::EnvironmentDrift;
        }
        self.store.record_environment_snapshot(
            &snapshot,
            &self.environment_cache_root,
            &manifest.cache_kinds,
        )?;
        Ok((manifest, snapshot))
    }

    fn verification_impact(
        &self,
        memory: &ProjectMemory,
        workspace_path: &str,
        changed_files: &[String],
    ) -> Result<VerificationImpact, CoreError> {
        let files = self.git.project_files(workspace_path)?;
        let project_graph = graph::build(memory, &files);
        let mut risks = Vec::new();
        let mut modules = BTreeSet::new();
        let mut tests = BTreeSet::new();
        for path in changed_files {
            let Ok(impact) = graph::analyze(&project_graph, memory, path) else {
                continue;
            };
            risks.push(impact.risk);
            modules.extend(impact.affected_modules.into_iter().map(|node| node.label));
            tests.extend(impact.tests_to_run);
        }
        risks.sort_by(|left, right| {
            right
                .level
                .cmp(&left.level)
                .then_with(|| right.score.cmp(&left.score))
        });
        Ok(VerificationImpact {
            changed_files: changed_files.to_vec(),
            highest_risk: risks.into_iter().next(),
            affected_modules: modules.into_iter().collect(),
            tests_to_run: tests.into_iter().collect(),
        })
    }

    fn handle_verification_failure(
        &self,
        repository: &str,
        task: &WorkTask,
        suite: &VerificationSuite,
        changed_files: &[String],
    ) -> Result<Option<String>, CoreError> {
        let failure = suite
            .gates
            .iter()
            .filter(|gate| gate.required && gate.status == VerificationStatus::Failed)
            .map(|gate| format!("{}: {}", gate.name, gate.detail))
            .collect::<Vec<_>>()
            .join(" ");
        if let Some((plan_id, plan_task_id)) = self
            .store
            .plans_for_project(repository)?
            .into_iter()
            .find(|plan| plan.status == domain::PlanStatus::Approved)
            .and_then(|plan| {
                plan.tasks
                    .iter()
                    .find(|planned| {
                        planned.materialized_task_id.as_deref() == Some(task.id.as_str())
                    })
                    .map(|planned| (plan.id.clone(), planned.id.clone()))
            })
        {
            let replanned = self.replan(ReplanInput {
                plan_id,
                failed_plan_task_id: plan_task_id,
                failure_reason: failure,
            })?;
            return Ok(replanned
                .tasks
                .iter()
                .find(|planned| planned.kind == domain::PlannedTaskKind::Recovery)
                .map(|planned| planned.id.clone()));
        }

        let repair = self.create_task(CreateTaskInput {
            goal_id: task.goal_id.clone(),
            title: format!("Repair verification failure: {}", task.title),
            description: format!(
                "Independent verification suite {} failed. {}",
                suite.id, failure
            ),
            priority: domain::TaskPriority::High,
            assigned_agent: None,
            source_agent: Some("verification-agent".into()),
        })?;
        let repair = self.update_task_orchestration(UpdateTaskOrchestrationInput {
            task_id: repair.id.clone(),
            required_capabilities: vec![domain::Capability::Testing, domain::Capability::Backend],
            blocked_by: vec![],
            depends_on: vec![],
            produces: vec![format!("verification-repair:{}", suite.id)],
            consumes: vec![],
            claimed_paths: changed_files.to_vec(),
        })?;
        let _ = self.suggest_task_route(&repair.id);
        self.store
            .record_repair_task_created(&suite.project_id, &suite.id, &repair.id)?;
        Ok(Some(repair.id))
    }

    pub fn planner_snapshot(&self, repository_path: &str) -> Result<PlannerSnapshot, CoreError> {
        let plans = self.store.plans_for_project(repository_path)?;
        let active_plan = plans
            .iter()
            .find(|plan| plan.status != domain::PlanStatus::Superseded)
            .cloned();
        Ok(PlannerSnapshot { plans, active_plan })
    }

    pub fn generate_plan(
        &self,
        input: GeneratePlanInput,
    ) -> Result<domain::PlanVersion, CoreError> {
        let memory = self.store.project_memory(&input.repository_path)?;
        let goal = memory
            .goals
            .iter()
            .find(|goal| goal.id == input.goal_id)
            .ok_or_else(|| StorageError::GoalNotFound(input.goal_id.clone()))?;
        let files = self.git.project_files(&input.repository_path)?;
        let project_graph = graph::build(&memory, &files);
        let profiles = self.synchronized_capability_profiles(&input.repository_path)?;
        let version = self
            .store
            .plans_for_project(&input.repository_path)?
            .iter()
            .filter(|plan| plan.goal_id == goal.id)
            .map(|plan| plan.version)
            .max()
            .unwrap_or(0)
            + 1;
        let plan = planner::generate(goal, &project_graph, &memory, &profiles, version);
        let plan = self.store.record_plan(&plan)?;
        self.record_plan_explanation(
            &plan,
            ExplanationCategory::Planner,
            format!(
                "Generated plan v{} with {} task(s)",
                plan.version,
                plan.tasks.len()
            ),
            &format!("{:?}", plan.status),
        )?;
        Ok(plan)
    }

    pub fn approve_plan(&self, input: ApprovePlanInput) -> Result<PlanApprovalResult, CoreError> {
        let approved = self.store.approve_plan(&input.plan_id)?;
        self.record_explanation(
            &approved.plan.project_id,
            ExplanationCategory::Planner,
            "plan",
            &approved.plan.id,
            format!("Approved plan v{} for execution", approved.plan.version),
            "approved",
            vec![
                audit_reason(
                    "user-approval",
                    "User explicitly approved the proposed plan",
                    "Plan generation remains advisory until this approval creates ordinary AgentRelay tasks.",
                    None,
                ),
                audit_reason(
                    "validation-status",
                    "Planner validation passed",
                    &format!(
                        "The approved DAG contains {} task(s) and {} validation finding(s).",
                        approved.plan.tasks.len(),
                        approved.plan.validation_issues.len()
                    ),
                    Some(approved.plan.validation_issues.len().min(i32::MAX as usize) as i32),
                ),
            ],
            vec![
                audit_evidence("plan", &approved.plan.id, "Approved plan version"),
                audit_evidence("goal", &approved.plan.goal_id, "Planned goal"),
            ],
        )?;
        self.store.evaluate_project(&approved.plan.project_id)?;
        Ok(approved)
    }

    pub fn replan(&self, input: ReplanInput) -> Result<domain::PlanVersion, CoreError> {
        let previous = self.store.plan(&input.plan_id)?;
        if previous.status == domain::PlanStatus::Superseded {
            return Err(CoreError::Routing(
                "a superseded plan cannot be replanned".into(),
            ));
        }
        let repository = self.store.repository_for_project_id(&previous.project_id)?;
        let memory = self.store.project_memory(&repository)?;
        let profiles = self.synchronized_capability_profiles(&repository)?;
        let next = planner::replan(
            &previous,
            &input.failed_plan_task_id,
            &input.failure_reason,
            &memory,
            &profiles,
        )
        .map_err(CoreError::Routing)?;
        let next = self.store.record_plan(&next)?;
        self.record_plan_explanation(
            &next,
            ExplanationCategory::Replan,
            format!("Replanned after task {} failed", input.failed_plan_task_id),
            &format!("{:?}", next.status),
        )?;
        Ok(next)
    }

    pub fn delivery_snapshot(
        &self,
        repository_path: &str,
    ) -> Result<Option<DeliverySnapshot>, CoreError> {
        Ok(self.store.delivery_snapshot(repository_path)?)
    }

    pub fn sync_delivery(&self, input: SyncDeliveryInput) -> Result<DeliverySyncReport, CoreError> {
        let remote_or_slug = match input.repository {
            Some(repository) if !repository.trim().is_empty() => repository,
            _ => self.git.origin_url(&input.repository_path)?,
        };
        let repository = self
            .source_control
            .resolve_repository(remote_or_slug.trim())?;
        let (issues, pull_requests) = self.source_control.synchronize(&repository)?;
        let report = self.store.apply_delivery_snapshot(
            &input.repository_path,
            repository,
            issues,
            pull_requests,
        )?;
        self.store.evaluate_conflicts(&input.repository_path)?;
        Ok(report)
    }

    pub fn project_permissions(
        &self,
        repository_path: &str,
    ) -> Result<ProjectPermissions, CoreError> {
        Ok(self.store.project_permissions(repository_path)?)
    }

    pub fn update_project_permissions(
        &self,
        input: UpdateProjectPermissionsInput,
    ) -> Result<ProjectPermissions, CoreError> {
        let repository_path = input.repository_path.clone();
        let permissions = self.store.update_project_permissions(input)?;
        self.record_explanation(
            &permissions.project_id,
            ExplanationCategory::Permission,
            "projectPermissions",
            &permissions.project_id,
            "Project execution permissions updated".into(),
            "recorded",
            vec![
                audit_reason(
                    "tests-policy",
                    &format!("Test execution policy is {:?}", permissions.run_tests),
                    "The user-controlled per-project test permission determines whether test, lint, and format gates may execute.",
                    None,
                ),
                audit_reason(
                    "build-policy",
                    &format!("Build execution policy is {:?}", permissions.run_build),
                    "The user-controlled per-project build permission determines whether build gates may execute.",
                    None,
                ),
                audit_reason(
                    "locked-capabilities",
                    "Destructive capabilities remain disabled",
                    "Source writes, Git commit, Git push, and file deletion cannot be enabled by this permission update.",
                    None,
                ),
            ],
            vec![audit_evidence("repository", &permissions.project_id, &repository_path)],
        )?;
        Ok(permissions)
    }

    pub fn sandbox_status(&self) -> SandboxStatus {
        match &self.sandbox {
            Some(sandbox) => SandboxStatus {
                configured: true,
                name: Some(sandbox.name().into()),
                detail: "Actions run in a disposable remote sandbox; no project command runs on this Windows host."
                    .into(),
            },
            None => SandboxStatus {
                configured: false,
                name: None,
                detail: self.sandbox_configuration_error.clone().unwrap_or_else(|| {
                    "Token Factory Sandbox is not configured.".into()
                }),
            },
        }
    }

    pub fn request_action(&self, input: ActionRequestInput) -> Result<ActionRun, CoreError> {
        let result = self.request_action_in_directory(
            &input.repository_path,
            &input.repository_path,
            input.session_id.as_deref(),
            &input.requested_by,
            input.action,
            input.approval_granted,
        )?;
        let mut evidence = vec![audit_evidence(
            "actionRun",
            &result.id,
            "Recorded action request",
        )];
        if let Some(session_id) = &result.session_id {
            evidence.push(audit_evidence(
                "session",
                session_id,
                "Requesting agent session",
            ));
        }
        self.record_explanation(
            &result.project_id,
            ExplanationCategory::Permission,
            "actionRun",
            &result.id,
            format!("{:?} request finished {:?}", result.action, result.status),
            &format!("{:?}", result.status),
            vec![
                audit_reason(
                    "permission-gate",
                    match result.status {
                        ActionStatus::Denied => "Project policy denied the requested capability",
                        ActionStatus::ApprovalRequired => {
                            "Project policy requires explicit user approval"
                        }
                        _ => "Project policy allowed evaluation of the requested capability",
                    },
                    "Every controlled action is checked against the per-project permission record before execution.",
                    None,
                ),
                audit_reason(
                    "fixed-command",
                    if result.command.is_empty() {
                        "No trusted command was available"
                    } else {
                        "Command resolved from trusted project configuration"
                    },
                    &if result.command.is_empty() {
                        "AgentRelay did not accept arbitrary shell text.".into()
                    } else {
                        format!("Resolved argv: {}", result.command.join(" "))
                    },
                    result.exit_code,
                ),
                audit_reason(
                    "execution-boundary",
                    result.sandbox.as_deref().unwrap_or("No sandbox execution"),
                    result
                        .error
                        .as_deref()
                        .unwrap_or("The result and exit evidence were captured by the controlled action layer."),
                    None,
                ),
            ],
            evidence,
        )?;
        Ok(result)
    }

    fn request_action_in_directory(
        &self,
        project_repository_path: &str,
        execution_directory: &str,
        session_id: Option<&str>,
        requested_by: &str,
        action: domain::ActionKind,
        approval_granted: bool,
    ) -> Result<ActionRun, CoreError> {
        let resolved = resolve_action(Path::new(execution_directory), action);
        let command = resolved
            .as_ref()
            .map(|command| command.argv())
            .unwrap_or_default();
        let requested = self.store.record_action_request(
            project_repository_path,
            session_id,
            requested_by,
            action,
            &command,
        )?;
        let permissions = self.store.project_permissions(project_repository_path)?;
        let permission_mode = permissions.mode_for(action);
        if !permissions.permits(action, approval_granted) {
            let (status, error) = if permission_mode == domain::PermissionMode::Ask {
                (
                    ActionStatus::ApprovalRequired,
                    "the project permission policy requires explicit user approval",
                )
            } else {
                (
                    ActionStatus::Denied,
                    "the project permission policy denied this action",
                )
            };
            return Ok(self.store.transition_action(
                &requested.id,
                status,
                None,
                None,
                "",
                "",
                Some(error),
            )?);
        }
        let command = match resolved {
            Ok(command) => command,
            Err(error) => {
                return Ok(self.store.transition_action(
                    &requested.id,
                    ActionStatus::Unavailable,
                    None,
                    None,
                    "",
                    "",
                    Some(&error.to_string()),
                )?);
            }
        };
        let Some(sandbox) = &self.sandbox else {
            return Ok(self.store.transition_action(
                &requested.id,
                ActionStatus::Unavailable,
                None,
                None,
                "",
                "",
                self.sandbox_configuration_error.as_deref(),
            )?);
        };
        let sandbox_name = sandbox.name().to_owned();
        self.store.transition_action(
            &requested.id,
            ActionStatus::Running,
            Some(&sandbox_name),
            None,
            "",
            "",
            None,
        )?;
        let execution = sandbox.execute(&SandboxExecutionRequest {
            project_id: requested.project_id.clone(),
            repository_path: Path::new(execution_directory).to_path_buf(),
            action_id: requested.id.clone(),
            command,
            timeout: Duration::from_secs(15 * 60),
        });
        let result = match execution {
            Ok(output) => {
                let status = if output.exit_code == 0 {
                    ActionStatus::Succeeded
                } else {
                    ActionStatus::Failed
                };
                self.store.transition_action(
                    &requested.id,
                    status,
                    Some(&sandbox_name),
                    Some(output.exit_code),
                    &output.stdout,
                    &output.stderr,
                    None,
                )?
            }
            Err(error) => self.store.transition_action(
                &requested.id,
                ActionStatus::Failed,
                Some(&sandbox_name),
                None,
                "",
                "",
                Some(&error.to_string()),
            )?,
        };
        self.store.evaluate_project(&requested.project_id)?;
        Ok(result)
    }

    pub fn create_goal(&self, input: CreateGoalInput) -> Result<Goal, CoreError> {
        let goal = self.store.create_goal(input)?;
        self.store.evaluate_project(&goal.project_id)?;
        Ok(goal)
    }

    pub fn update_goal(&self, input: UpdateGoalInput) -> Result<Goal, CoreError> {
        let goal = self.store.update_goal(input)?;
        self.store.evaluate_project(&goal.project_id)?;
        Ok(goal)
    }

    pub fn create_task(&self, input: CreateTaskInput) -> Result<WorkTask, CoreError> {
        let task = self.store.create_task(input)?;
        let project_id = self.store.project_id_for_task(&task.id)?;
        self.store.evaluate_project(&project_id)?;
        Ok(task)
    }

    pub fn update_task(&self, input: UpdateTaskInput) -> Result<WorkTask, CoreError> {
        let task = self.store.update_task(input)?;
        let project_id = self.store.project_id_for_task(&task.id)?;
        self.store.evaluate_project(&project_id)?;
        Ok(task)
    }

    pub fn update_task_orchestration(
        &self,
        input: UpdateTaskOrchestrationInput,
    ) -> Result<WorkTask, CoreError> {
        let task = self.store.update_task_orchestration(input)?;
        let project_id = self.store.project_id_for_task(&task.id)?;
        self.store.evaluate_project(&project_id)?;
        Ok(task)
    }

    pub fn orchestration_snapshot(
        &self,
        repository_path: &str,
    ) -> Result<OrchestrationSnapshot, CoreError> {
        let memory = self.store.project_memory(repository_path)?;
        let tasks = memory
            .goals
            .iter()
            .flat_map(|goal| goal.tasks.iter().cloned())
            .collect::<Vec<_>>();
        let profiles = self.synchronized_capability_profiles(repository_path)?;
        let locks = self.store.workspace_locks(repository_path)?;
        let recommendations = tasks
            .iter()
            .filter(|task| !matches!(task.status, TaskStatus::Completed | TaskStatus::Archived))
            .filter_map(|task| orchestration::recommend(task, &profiles))
            .collect::<Vec<_>>();
        let completed = tasks
            .iter()
            .filter(|task| task.status == TaskStatus::Completed)
            .map(|task| task.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let next_recommended_agent = recommendations
            .iter()
            .find(|route| {
                tasks
                    .iter()
                    .find(|task| task.id == route.task_id)
                    .is_some_and(|task| {
                        task.depends_on
                            .iter()
                            .chain(&task.blocked_by)
                            .all(|dependency| completed.contains(dependency.as_str()))
                            && !matches!(
                                task.assignment_status,
                                TaskAssignmentStatus::Running | TaskAssignmentStatus::Completed
                            )
                    })
            })
            .cloned();
        let parallel_assessments =
            orchestration::assess_parallel_with_decisions(&tasks, &memory.decisions);
        Ok(OrchestrationSnapshot {
            tasks,
            profiles,
            locks,
            recommendations,
            parallel_assessments,
            next_recommended_agent,
        })
    }

    pub fn update_capability_profile(
        &self,
        input: UpdateCapabilityProfileInput,
    ) -> Result<AgentCapabilityProfile, CoreError> {
        Ok(self.store.update_capability_profile(input)?)
    }

    fn synchronized_capability_profiles(
        &self,
        repository_path: &str,
    ) -> Result<Vec<AgentCapabilityProfile>, CoreError> {
        let mut profiles = self.store.capability_profiles(repository_path)?;
        for template in self.registry.capability_profile_templates() {
            if profiles
                .iter()
                .any(|profile| profile.provider == template.provider)
            {
                continue;
            }
            profiles.push(
                self.store
                    .update_capability_profile(UpdateCapabilityProfileInput {
                        repository_path: repository_path.into(),
                        provider: template.provider,
                        display_name: template.display_name,
                        strengths: template.capabilities.strengths,
                        allowed_actions: template.capabilities.allowed_actions,
                        max_context_tokens: template.capabilities.max_context_tokens,
                        enabled: true,
                    })?,
            );
        }
        profiles.sort_by(|left, right| left.display_name.cmp(&right.display_name));
        Ok(profiles)
    }

    pub fn suggest_task_route(&self, task_id: &str) -> Result<WorkTask, CoreError> {
        let repository = self.store.repository_for_task(task_id)?;
        let snapshot = self.orchestration_snapshot(&repository)?;
        let recommendation = snapshot
            .recommendations
            .iter()
            .find(|route| route.task_id == task_id)
            .ok_or_else(|| {
                CoreError::Routing("no enabled capability profile can handle this task".into())
            })?;
        let task = self.store.record_route_recommendation(recommendation)?;
        let memory = self.store.project_memory(&repository)?;
        self.record_explanation(
            &memory.project.id,
            ExplanationCategory::Assignment,
            "task",
            &task.id,
            format!(
                "Suggested {} for {}",
                recommendation.display_name, task.title
            ),
            "suggested",
            recommendation
                .reasons
                .iter()
                .enumerate()
                .map(|(index, reason)| {
                    audit_reason(
                        &format!("route-reason-{}", index + 1),
                        reason,
                        "Derived from the task's required capabilities and the project's configurable provider profiles.",
                        (index == 0).then_some(i32::from(recommendation.score)),
                    )
                })
                .collect(),
            vec![
                audit_evidence("task", &task.id, &task.title),
                audit_evidence(
                    "providerProfile",
                    recommendation.provider.as_str(),
                    &recommendation.display_name,
                ),
            ],
        )?;
        Ok(task)
    }

    pub fn approve_task_assignment(
        &self,
        input: ApproveTaskAssignmentInput,
    ) -> Result<WorkTask, CoreError> {
        let repository = self.store.repository_for_task(&input.task_id)?;
        let memory = self.store.project_memory(&repository)?;
        if memory.freshness.iter().any(|item| {
            item.subject_type.eq_ignore_ascii_case("context")
                && matches!(
                    item.status,
                    domain::FreshnessStatus::Stale | domain::FreshnessStatus::Invalidated
                )
        }) {
            return Err(CoreError::Routing(
                "project context is stale; regenerate current context before assigning this task"
                    .into(),
            ));
        }
        let provider = input.provider.clone();
        let task = self.store.approve_task_assignment(input)?;
        self.record_explanation(
            &memory.project.id,
            ExplanationCategory::Assignment,
            "task",
            &task.id,
            format!("Assigned {} to {}", task.title, provider.display_name()),
            "assigned",
            task.route_reasons
                .iter()
                .enumerate()
                .map(|(index, reason)| {
                    audit_reason(
                        &format!("assignment-reason-{}", index + 1),
                        reason,
                        "This evidence was preserved from the deterministic routing recommendation approved by the user.",
                        None,
                    )
                })
                .chain(std::iter::once(audit_reason(
                    "user-approval",
                    "User approved the suggested assignment",
                    "AgentRelay recommends providers but does not silently assign or launch them.",
                    None,
                )))
                .collect(),
            vec![
                audit_evidence("task", &task.id, &task.title),
                audit_evidence("providerProfile", provider.as_str(), &provider.display_name()),
                audit_evidence("context", "AGENT_CONTEXT.md", "Current project context"),
            ],
        )?;
        Ok(task)
    }

    pub fn clear_task_assignment(&self, task_id: &str) -> Result<WorkTask, CoreError> {
        Ok(self.store.clear_task_assignment(task_id)?)
    }

    pub fn start_routed_task(
        &self,
        input: StartRoutedTaskInput,
    ) -> Result<AgentSession, CoreError> {
        let workspace = self.prepare_task_workspace(PrepareTaskWorkspaceInput {
            repository_path: input.repository_path,
            task_id: input.task_id,
        })?;
        self.start_task_workspace(StartTaskWorkspaceInput {
            workspace_id: workspace.id,
        })
    }

    pub fn collaborative_execution_snapshot(
        &self,
        repository_path: &str,
    ) -> Result<CollaborativeExecutionSnapshot, CoreError> {
        let memory = self.store.project_memory(repository_path)?;
        let tasks = memory
            .goals
            .iter()
            .flat_map(|goal| goal.tasks.iter().cloned())
            .collect::<Vec<_>>();
        Ok(CollaborativeExecutionSnapshot {
            workspaces: self.store.task_workspaces(repository_path)?,
            validations: self.store.workspace_validations(repository_path)?,
            parallel_assessments: orchestration::assess_parallel_with_decisions(
                &tasks,
                &memory.decisions,
            ),
        })
    }

    pub fn prepare_task_workspace(
        &self,
        input: PrepareTaskWorkspaceInput,
    ) -> Result<TaskWorkspace, CoreError> {
        let memory = self.store.project_memory(&input.repository_path)?;
        let tasks = memory
            .goals
            .iter()
            .flat_map(|goal| goal.tasks.iter().cloned())
            .collect::<Vec<_>>();
        let task = tasks
            .iter()
            .find(|task| task.id == input.task_id)
            .ok_or_else(|| StorageError::TaskNotFound(input.task_id.clone()))?;
        if task.assignment_status != TaskAssignmentStatus::Assigned {
            return Err(CoreError::Routing(
                "approve an unblocked agent assignment before creating a worktree".into(),
            ));
        }
        let provider = task.assigned_provider.clone().ok_or_else(|| {
            CoreError::Routing("the task has no approved provider assignment".into())
        })?;
        for workspace in self
            .store
            .task_workspaces(&input.repository_path)?
            .into_iter()
            .filter(|workspace| workspace.status.is_live())
        {
            if workspace.task_id == task.id {
                return Err(CoreError::Routing(
                    "this task already has a live isolated workspace".into(),
                ));
            }
            let Some(other) = tasks
                .iter()
                .find(|candidate| candidate.id == workspace.task_id)
            else {
                continue;
            };
            let assessment =
                orchestration::assess_pair_with_decisions(task, other, &memory.decisions);
            if !assessment.safe {
                let mut pair = [task.id.as_str(), other.id.as_str()];
                pair.sort_unstable();
                self.record_explanation(
                    &memory.project.id,
                    ExplanationCategory::ParallelSafety,
                    "taskPair",
                    &pair.join(":"),
                    format!(
                        "Blocked {} from running in parallel with {}",
                        task.title, other.title
                    ),
                    "serialized",
                    assessment
                        .reasons
                        .iter()
                        .enumerate()
                        .map(|(index, reason)| {
                            audit_reason(
                                &format!("parallel-conflict-{}", index + 1),
                                reason,
                                "The deterministic parallel-safety engine found overlapping scope, dependencies, artifacts, or architectural constraints.",
                                None,
                            )
                        })
                        .collect(),
                    vec![
                        audit_evidence("task", &task.id, &task.title),
                        audit_evidence("task", &other.id, &other.title),
                        audit_evidence("workspace", &workspace.id, &workspace.branch),
                    ],
                )?;
                return Err(CoreError::Routing(format!(
                    "parallel safety check requires serialization with `{}`: {}",
                    other.title,
                    assessment.reasons.join(" ")
                )));
            }
        }
        let id = Uuid::new_v4().to_string();
        let created = self
            .worktrees
            .create(&input.repository_path, &id, &task.title, &provider)?;
        let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let workspace = TaskWorkspace {
            id: id.clone(),
            project_id: memory.project.id,
            task_id: task.id.clone(),
            assigned_provider: provider,
            branch: created.branch,
            worktree_path: created.path.to_string_lossy().replace('\\', "/"),
            base_commit: created.base_commit.clone(),
            head_commit: created.base_commit,
            status: TaskWorkspaceStatus::Ready,
            session_id: None,
            latest_validation_id: None,
            created_at: timestamp.clone(),
            updated_at: timestamp,
            merged_at: None,
            archived_at: None,
        };
        match self.store.create_task_workspace(&workspace) {
            Ok(saved) => Ok(saved),
            Err(error) => {
                let _ = self
                    .worktrees
                    .remove(&input.repository_path, &workspace.worktree_path);
                Err(error.into())
            }
        }
    }

    pub fn start_task_workspace(
        &self,
        input: StartTaskWorkspaceInput,
    ) -> Result<AgentSession, CoreError> {
        let workspace = self.store.workspace(&input.workspace_id)?;
        if workspace.status != TaskWorkspaceStatus::Ready {
            return Err(CoreError::Routing(
                "only a ready isolated workspace can start a session".into(),
            ));
        }
        let repository = self
            .store
            .repository_for_project_id(&workspace.project_id)?;
        let session = SessionService::new(&self.git, &self.store).start_isolated_session(
            repository,
            workspace.worktree_path.clone(),
            workspace.assigned_provider.clone(),
            workspace.assigned_provider.display_name(),
            workspace.task_id.clone(),
        )?;
        self.store
            .bind_workspace_session(&workspace.id, &session.id)?;
        let _ = self.bind_latest_context_to_session(&workspace.worktree_path, &session);
        Ok(session)
    }

    pub fn validate_task_workspace(
        &self,
        input: ValidateTaskWorkspaceInput,
    ) -> Result<WorkspaceValidationResult, CoreError> {
        let workspace = self.store.workspace(&input.workspace_id)?;
        if !matches!(
            workspace.status,
            TaskWorkspaceStatus::ValidationPending
                | TaskWorkspaceStatus::ValidationFailed
                | TaskWorkspaceStatus::MergeConflict
                | TaskWorkspaceStatus::ValidationPassed
        ) {
            return Err(CoreError::Routing(
                "finish the isolated agent session before validation".into(),
            ));
        }
        let repository = self
            .store
            .repository_for_project_id(&workspace.project_id)?;
        let inspection = self
            .worktrees
            .inspect(&workspace.worktree_path, &workspace.base_commit)?;
        let memory = self.store.project_memory(&repository)?;
        let tasks = memory
            .goals
            .iter()
            .flat_map(|goal| goal.tasks.iter().cloned())
            .collect::<Vec<_>>();
        let task = tasks
            .iter()
            .find(|task| task.id == workspace.task_id)
            .ok_or_else(|| StorageError::TaskNotFound(workspace.task_id.clone()))?;
        let parallel_conflicts = self
            .store
            .task_workspaces(&repository)?
            .into_iter()
            .filter(|other| {
                other.id != workspace.id
                    && other.status.is_live()
                    && matches!(
                        other.status,
                        TaskWorkspaceStatus::Active | TaskWorkspaceStatus::MergeReady
                    )
            })
            .filter_map(|other| {
                let other_task = tasks.iter().find(|task| task.id == other.task_id)?;
                let assessment =
                    orchestration::assess_pair_with_decisions(task, other_task, &memory.decisions);
                (!assessment.safe).then_some(assessment.reasons.join(" "))
            })
            .collect::<Vec<_>>();
        let ancestor = self.worktrees.base_is_ancestor(
            &workspace.worktree_path,
            &workspace.base_commit,
            &inspection.head_commit,
        )?;
        let merge = self
            .worktrees
            .assess_merge(&repository, &workspace.branch)?;
        let mut checks = vec![
            validation_check(
                ValidationCheckKind::CleanWorktree,
                inspection.clean,
                if inspection.clean {
                    "The task worktree has no uncommitted changes."
                } else {
                    "Commit or discard task-worktree changes before merge preparation."
                },
            ),
            validation_check(
                ValidationCheckKind::HasChanges,
                !inspection.changed_files.is_empty(),
                if inspection.changed_files.is_empty() {
                    "No task changes exist relative to the recorded base commit."
                } else {
                    "The candidate branch contains task changes."
                },
            ),
            validation_check(
                ValidationCheckKind::BaseAncestry,
                ancestor,
                if ancestor {
                    "The recorded base commit remains an ancestor of the candidate head."
                } else {
                    "The candidate branch no longer descends from its recorded base."
                },
            ),
            validation_check(
                ValidationCheckKind::ParallelSafety,
                parallel_conflicts.is_empty(),
                if parallel_conflicts.is_empty() {
                    "No active workspace has a dependency, file, module, or decision overlap."
                        .into()
                } else {
                    parallel_conflicts.join(" ")
                },
            ),
            validation_check(
                ValidationCheckKind::Mergeability,
                merge.mergeable,
                merge.detail,
            ),
        ];
        let mut action_runs = Vec::new();
        for (requested, action, kind) in [
            (
                input.run_tests,
                domain::ActionKind::RunTests,
                ValidationCheckKind::Tests,
            ),
            (
                input.run_build,
                domain::ActionKind::RunBuild,
                ValidationCheckKind::Build,
            ),
        ] {
            if !requested {
                checks.push(ValidationCheck {
                    kind,
                    status: ValidationCheckStatus::Skipped,
                    detail: "Not requested for this validation run.".into(),
                    action_run_id: None,
                });
                continue;
            }
            let run = self.request_action_in_directory(
                &repository,
                &workspace.worktree_path,
                workspace.session_id.as_deref(),
                workspace.assigned_provider.as_str(),
                action,
                input.approval_granted,
            )?;
            let passed = run.status == ActionStatus::Succeeded;
            checks.push(ValidationCheck {
                kind,
                status: if passed {
                    ValidationCheckStatus::Passed
                } else {
                    ValidationCheckStatus::Failed
                },
                detail: run.error.clone().unwrap_or_else(|| {
                    format!("Controlled action finished with status {:?}.", run.status)
                }),
                action_run_id: Some(run.id.clone()),
            });
            action_runs.push(run);
        }
        let status = if checks
            .iter()
            .any(|check| check.status == ValidationCheckStatus::Failed)
        {
            WorkspaceValidationStatus::Failed
        } else {
            WorkspaceValidationStatus::Passed
        };
        let validation = WorkspaceValidation {
            id: Uuid::new_v4().to_string(),
            workspace_id: workspace.id.clone(),
            status,
            validated_head: inspection.head_commit,
            changed_files: inspection.changed_files,
            checks,
            created_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        };
        let workspace = self.store.save_workspace_validation(&validation)?;
        if validation.status == WorkspaceValidationStatus::Failed {
            let failure_reason = validation
                .checks
                .iter()
                .filter(|check| check.status == ValidationCheckStatus::Failed)
                .map(|check| check.detail.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            if let Some((plan_id, plan_task_id)) = self
                .store
                .plans_for_project(&repository)?
                .into_iter()
                .find(|plan| plan.status == domain::PlanStatus::Approved)
                .and_then(|plan| {
                    plan.tasks
                        .iter()
                        .find(|task| {
                            task.materialized_task_id.as_deref() == Some(workspace.task_id.as_str())
                        })
                        .map(|task| (plan.id.clone(), task.id.clone()))
                })
            {
                let _ = self.replan(ReplanInput {
                    plan_id,
                    failed_plan_task_id: plan_task_id,
                    failure_reason,
                });
            }
        }
        Ok(WorkspaceValidationResult {
            workspace,
            validation,
            action_runs,
        })
    }

    pub fn prepare_workspace_merge(
        &self,
        input: WorkspaceIdInput,
    ) -> Result<TaskWorkspace, CoreError> {
        let workspace = self.store.workspace(&input.workspace_id)?;
        if workspace.status != TaskWorkspaceStatus::ValidationPassed {
            return Err(CoreError::Routing(
                "a passing validation for the current head is required before merge preparation"
                    .into(),
            ));
        }
        let inspection = self
            .worktrees
            .inspect(&workspace.worktree_path, &workspace.base_commit)?;
        if !inspection.clean || inspection.head_commit != workspace.head_commit {
            return Err(CoreError::Routing(
                "the task branch changed after validation; validate the current head again".into(),
            ));
        }
        let verification = self
            .store
            .passing_verification_for_workspace(&workspace.id, &inspection.head_commit)?
            .ok_or_else(|| CoreError::Routing(
                "MERGE_READY requires a passing or explicitly waived verification suite for the current head"
                    .into()
            ))?;
        let repository = self
            .store
            .repository_for_project_id(&workspace.project_id)?;
        let merge = self
            .worktrees
            .assess_merge(&repository, &workspace.branch)?;
        let assessed = self.store.mark_workspace_merge_assessment(
            &workspace.id,
            &inspection.head_commit,
            merge.mergeable,
            &merge.detail,
        )?;
        self.record_explanation(
            &workspace.project_id,
            ExplanationCategory::MergeReadiness,
            "workspace",
            &workspace.id,
            if merge.mergeable {
                "Workspace is merge ready".into()
            } else {
                "Workspace has a merge conflict".into()
            },
            if merge.mergeable { "ready" } else { "blocked" },
            vec![
                audit_reason(
                    "workspace-validation",
                    "Deterministic workspace validation passed",
                    "The candidate is clean and still points to the exact validated head.",
                    None,
                ),
                audit_reason(
                    "verification-suite",
                    "Independent verification passed or was explicitly waived",
                    &format!(
                        "Suite {} verified commit {} in environment {}.",
                        verification.id, verification.commit, verification.environment_fingerprint
                    ),
                    None,
                ),
                audit_reason(
                    "merge-tree",
                    &merge.detail,
                    "Git merge-tree assessed the candidate without mutating the main checkout.",
                    None,
                ),
            ],
            vec![
                audit_evidence("workspace", &workspace.id, &workspace.branch),
                audit_evidence(
                    "verificationSuite",
                    &verification.id,
                    "Required quality gates",
                ),
                audit_evidence("commit", &inspection.head_commit, &workspace.branch),
            ],
        )?;
        Ok(assessed)
    }

    pub fn approve_workspace_merge(
        &self,
        input: WorkspaceIdInput,
    ) -> Result<TaskWorkspace, CoreError> {
        let workspace = self.store.workspace(&input.workspace_id)?;
        if workspace.status != TaskWorkspaceStatus::MergeReady {
            return Err(CoreError::Routing(
                "prepare a conflict-free merge before approving it".into(),
            ));
        }
        if let Some(session_id) = &workspace.session_id {
            if self.store.session(session_id)?.status == domain::AgentSessionStatus::Active {
                return Err(CoreError::Routing(
                    "end the isolated agent session before approving its merge".into(),
                ));
            }
        }
        let inspection = self
            .worktrees
            .inspect(&workspace.worktree_path, &workspace.base_commit)?;
        if !inspection.clean || inspection.head_commit != workspace.head_commit {
            return Err(CoreError::Routing(
                "the candidate branch changed after merge preparation".into(),
            ));
        }
        let repository = self
            .store
            .repository_for_project_id(&workspace.project_id)?;
        let merged_head = self.worktrees.merge(&repository, &workspace.branch)?;
        let workspace = self
            .store
            .mark_workspace_merged(&workspace.id, &merged_head)?;
        self.scan_project(&repository)?;
        Ok(workspace)
    }

    pub fn cleanup_task_workspace(
        &self,
        input: WorkspaceIdInput,
    ) -> Result<TaskWorkspace, CoreError> {
        let workspace = self.store.workspace(&input.workspace_id)?;
        if workspace.status != TaskWorkspaceStatus::Merged {
            return Err(CoreError::Routing(
                "only an approved, merged workspace can be cleaned up".into(),
            ));
        }
        let repository = self
            .store
            .repository_for_project_id(&workspace.project_id)?;
        self.worktrees
            .remove(&repository, &workspace.worktree_path)?;
        Ok(self.store.archive_workspace(&workspace.id)?)
    }

    pub fn record_decision(&self, input: CreateDecisionInput) -> Result<Decision, CoreError> {
        let repository_path = input.repository_path.clone();
        let supersedes = input.supersedes_decision_id.clone();
        let decision = self.store.record_decision(input)?;
        self.store.evaluate_conflicts(&repository_path)?;
        let mut evidence_refs = decision
            .affected
            .iter()
            .map(|path| audit_evidence("file", path, path))
            .collect::<Vec<_>>();
        if let Some(previous_id) = &supersedes {
            evidence_refs.push(audit_evidence(
                "decision",
                previous_id,
                "Superseded decision",
            ));
        }
        self.record_explanation(
            &decision.project_id,
            ExplanationCategory::Decision,
            "decision",
            &decision.id,
            format!("Recorded decision: {}", decision.summary),
            "active",
            vec![
                audit_reason(
                    "decision-rationale",
                    &decision.reason,
                    "This rationale was supplied with the durable architectural decision.",
                    None,
                ),
                audit_reason(
                    "alternatives-considered",
                    &format!("{} alternative(s) recorded", decision.alternatives.len()),
                    &decision.alternatives.join(", "),
                    None,
                ),
            ],
            evidence_refs,
        )?;
        if let Some(previous_id) = supersedes {
            self.record_explanation(
                &decision.project_id,
                ExplanationCategory::Freshness,
                "decision",
                &previous_id,
                format!("Decision superseded by {}", decision.summary),
                "superseded",
                vec![audit_reason(
                    "explicit-supersession",
                    "A newer explicit decision replaced this guidance",
                    "Decision supersession is a user-recorded relationship; it is not inferred by an LLM.",
                    None,
                )],
                vec![
                    audit_evidence("decision", &previous_id, "Previous decision"),
                    audit_evidence("decision", &decision.id, &decision.summary),
                ],
            )?;
        }
        Ok(decision)
    }

    pub fn context_preview(&self, repository_path: &str) -> Result<String, CoreError> {
        Ok(self
            .compile_context(ContextCompileInput {
                repository_path: repository_path.into(),
                session_id: None,
                task_id: None,
                depth: ContextDepth::Normal,
                profile: None,
            })?
            .markdown)
    }

    pub fn generate_context(&self, repository_path: &str) -> Result<String, CoreError> {
        Ok(self
            .generate_smart_context(ContextCompileInput {
                repository_path: repository_path.into(),
                session_id: None,
                task_id: None,
                depth: ContextDepth::Normal,
                profile: None,
            })?
            .markdown)
    }

    pub fn compile_context(&self, input: ContextCompileInput) -> Result<ContextPackage, CoreError> {
        self.build_context(input, false)
    }

    pub fn generate_smart_context(
        &self,
        input: ContextCompileInput,
    ) -> Result<ContextPackage, CoreError> {
        let package = self.build_context(input, true)?;
        let output_path = Path::new(&package.snapshot.repository).join("AGENT_CONTEXT.md");
        fs::write(&output_path, &package.markdown)?;
        let version = self.capture_context_version(&package)?;
        self.record_context_explanations(&version, &package)?;
        self.store
            .evaluate_project(&package.snapshot.request.project_id)?;
        Ok(package)
    }

    fn capture_context_version(
        &self,
        package: &ContextPackage,
    ) -> Result<ContextVersion, CoreError> {
        let memory = self.store.project_memory(&package.snapshot.repository)?;
        let plan = self
            .store
            .plans_for_project(&package.snapshot.repository)?
            .into_iter()
            .next();
        let environment = self
            .store
            .environment_snapshots(&package.snapshot.repository)?
            .into_iter()
            .next();
        let hash = format!("{:x}", Sha256::digest(package.markdown.as_bytes()));
        let commit = memory
            .project
            .latest_status
            .as_ref()
            .map(|status| status.head_commit.clone())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| memory.project.last_commit.clone());
        let branch = memory
            .project
            .latest_status
            .as_ref()
            .map(|status| status.branch.clone())
            .unwrap_or_else(|| memory.project.branch.clone());
        let version = ContextVersion {
            id: Uuid::new_v4().to_string(),
            project_id: package.snapshot.request.project_id.clone(),
            session_id: package.snapshot.request.session_id.clone(),
            task_id: package.snapshot.request.task_id.clone(),
            context_snapshot_hash: hash,
            markdown: package.markdown.clone(),
            snapshot: Some(package.snapshot.clone()),
            commit,
            branch,
            plan_id: plan.as_ref().map(|item| item.id.clone()),
            plan_version: plan.as_ref().map(|item| item.version),
            environment_fingerprint: environment.map(|item| item.fingerprint),
            created_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        };
        self.store.save_context_version(&version)?;
        Ok(version)
    }

    pub fn start_agent_session(&self, input: StartSessionInput) -> Result<AgentSession, CoreError> {
        if input.task_id.is_some() {
            return Err(CoreError::Routing(
                "routed task sessions must start through an isolated AgentRelay worktree".into(),
            ));
        }
        let repository_path = input.repository_path.clone();
        let session = SessionService::new(&self.git, &self.store).start_session(input)?;
        let _ = self.bind_latest_context_to_session(&repository_path, &session);
        Ok(session)
    }

    pub fn start_detected_agent_session(
        &self,
        repository_path: String,
        detected: &DetectedAgent,
    ) -> Result<AgentSession, CoreError> {
        let agent_name = self
            .registry
            .manifest(detected.provider.as_str())
            .map(|manifest| manifest.name.clone())
            .unwrap_or_else(|| detected.provider.display_name());
        let session = SessionService::new(&self.git, &self.store).start_detected_session(
            repository_path.clone(),
            detected.provider.clone(),
            agent_name,
            detected.pid,
        )?;
        let _ = self.bind_latest_context_to_session(&repository_path, &session);
        Ok(session)
    }

    fn bind_latest_context_to_session(
        &self,
        _repository_path: &str,
        session: &AgentSession,
    ) -> Result<(), CoreError> {
        let lookup_repository = self.store.repository_for_project_id(&session.project_id)?;
        let Some(mut version) = self
            .store
            .context_versions(&lookup_repository)?
            .into_iter()
            .next()
        else {
            return Ok(());
        };
        version.id = Uuid::new_v4().to_string();
        version.session_id = Some(session.id.clone());
        version.task_id = session.task_id.clone();
        version.created_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        self.store.save_context_version(&version)?;
        Ok(())
    }

    pub fn end_agent_session(&self, input: EndSessionInput) -> Result<HandoffSnapshot, CoreError> {
        let session = self.store.session(&input.session_id)?;
        let repository_path = self.store.repository_for_project_id(&session.project_id)?;
        let handoff = SessionService::new(&self.git, &self.store).end_session(input)?;
        let context_input = ContextCompileInput {
            repository_path,
            session_id: Some(handoff.session_id.clone()),
            task_id: None,
            depth: ContextDepth::Normal,
            profile: None,
        };
        if session.working_directory == context_input.repository_path {
            self.generate_smart_context(context_input)?;
        } else {
            self.compile_context(context_input)?;
        }
        Ok(handoff)
    }

    pub fn active_agent_session(
        &self,
        repository_path: &str,
    ) -> Result<Option<AgentSession>, CoreError> {
        Ok(SessionService::new(&self.git, &self.store).active_session(repository_path)?)
    }

    pub fn mark_agent_session_idle(&self, session_id: &str) -> Result<AgentSession, CoreError> {
        Ok(SessionService::new(&self.git, &self.store).mark_idle(session_id)?)
    }

    pub fn latest_handoff(
        &self,
        repository_path: &str,
    ) -> Result<Option<HandoffSnapshot>, CoreError> {
        Ok(self.store.project_memory(repository_path)?.latest_handoff)
    }

    pub fn evaluate_conflicts(
        &self,
        repository_path: &str,
    ) -> Result<Vec<ConflictRecord>, CoreError> {
        let conflicts = self.store.evaluate_conflicts(repository_path)?;
        let project_id = self.store.project_memory(repository_path)?.project.id;
        for conflict in &conflicts {
            let mut evidence_refs = conflict
                .affected_paths
                .iter()
                .map(|path| audit_evidence("file", path, path))
                .collect::<Vec<_>>();
            evidence_refs.extend(
                conflict
                    .related_decision_ids
                    .iter()
                    .map(|id| audit_evidence("decision", id, id)),
            );
            if let Some(session_id) = &conflict.primary_session_id {
                evidence_refs.push(audit_evidence("session", session_id, "Primary session"));
            }
            if let Some(session_id) = &conflict.related_session_id {
                evidence_refs.push(audit_evidence("session", session_id, "Related session"));
            }
            self.record_explanation(
                &project_id,
                ExplanationCategory::Conflict,
                "conflict",
                &conflict.id,
                conflict.title.clone(),
                &format!("{:?}", conflict.status),
                vec![audit_reason(
                    &format!("conflict-{:?}", conflict.kind).to_ascii_lowercase(),
                    &conflict.description,
                    &format!(
                        "Deterministic conflict rule {:?} produced severity {:?}.",
                        conflict.kind, conflict.severity
                    ),
                    None,
                )],
                evidence_refs,
            )?;
        }
        Ok(conflicts)
    }

    pub fn acknowledge_conflict(&self, id: &str) -> Result<ConflictRecord, CoreError> {
        Ok(self.store.acknowledge_conflict(id)?)
    }

    pub fn resolve_conflict(&self, id: &str) -> Result<ConflictRecord, CoreError> {
        Ok(self.store.resolve_conflict(id)?)
    }

    fn build_context(
        &self,
        input: ContextCompileInput,
        run_compression: bool,
    ) -> Result<ContextPackage, CoreError> {
        self.store.evaluate_conflicts(&input.repository_path)?;
        let memory = self.store.project_memory(&input.repository_path)?;
        let capability_profiles = self.synchronized_capability_profiles(&input.repository_path)?;
        let profile = input.profile.unwrap_or_else(|| {
            input
                .session_id
                .as_ref()
                .and_then(|id| {
                    memory
                        .agent_sessions
                        .iter()
                        .find(|session| &session.id == id)
                })
                .or_else(|| {
                    memory
                        .agent_sessions
                        .iter()
                        .find(|session| session.status == domain::AgentSessionStatus::Active)
                })
                .or(memory.last_agent_session.as_ref())
                .and_then(|session| {
                    capability_profiles
                        .iter()
                        .find(|profile| profile.provider == session.provider)
                })
                .map(|profile| ContextProfile::for_capabilities(&profile.strengths))
                .unwrap_or(ContextProfile::Balanced)
        });
        let request = ContextRequest {
            project_id: memory.project.id.clone(),
            session_id: input.session_id,
            task_id: input.task_id,
            token_budget: input.depth.token_budget(),
            profile,
        };
        let mut snapshot = compile_snapshot(&memory, request);
        if let Some(error) = &self.compression_configuration_error {
            snapshot.compression = CompressionState {
                mode: CompressionMode::NemotronFallback,
                model: None,
                detail: error.clone(),
            };
        } else if let Some(compressor) = &self.compressor {
            snapshot.compression = CompressionState {
                mode: CompressionMode::NemotronAvailable,
                model: Some(compressor.model().into()),
                detail: "Nemotron is configured; generation will validate every cited source."
                    .into(),
            };
            if run_compression {
                match compressor.compress(&compression_request(&snapshot)) {
                    Ok(response) => match validate_compression(&snapshot, response) {
                        Ok(summary) => {
                            snapshot.compression = CompressionState {
                                mode: CompressionMode::NemotronValidated,
                                model: Some(summary.model.clone()),
                                detail:
                                    "Model output passed source-key and token-budget validation."
                                        .into(),
                            };
                            snapshot.summary = Some(summary);
                        }
                        Err(error) => {
                            snapshot.compression = CompressionState {
                                mode: CompressionMode::NemotronFallback,
                                model: Some(compressor.model().into()),
                                detail: format!(
                                    "Validation rejected the model output; deterministic context retained: {error}"
                                ),
                            };
                        }
                    },
                    Err(error) => {
                        snapshot.compression = CompressionState {
                            mode: CompressionMode::NemotronFallback,
                            model: Some(compressor.model().into()),
                            detail: format!(
                                "Compression was unavailable; deterministic context retained: {error}"
                            ),
                        };
                    }
                }
            }
        }
        let markdown = loop {
            let candidate = render_markdown(&snapshot);
            snapshot.estimated_tokens = estimate_tokens(&candidate);
            let candidate = render_markdown(&snapshot);
            snapshot.estimated_tokens = estimate_tokens(&candidate);
            if snapshot.estimated_tokens <= snapshot.request.token_budget {
                break render_markdown(&snapshot);
            }
            if snapshot.summary.take().is_some() {
                snapshot.compression = CompressionState {
                    mode: CompressionMode::NemotronFallback,
                    model: snapshot.compression.model.clone(),
                    detail: "Validated summary would exceed the total context budget; deterministic context retained."
                        .into(),
                };
                continue;
            }
            if snapshot.items.pop().is_some() {
                snapshot.omitted_item_count += 1;
            } else {
                break candidate;
            }
        };
        Ok(ContextPackage { snapshot, markdown })
    }
}

fn worktree_environment_manifest_changed(
    trusted_repository: &Path,
    execution_repository: &Path,
    trusted_manifest: &EnvironmentManifest,
) -> bool {
    let relative = Path::new(".agentrelay").join("environment.toml");
    let trusted_exists = trusted_repository.join(&relative).is_file();
    let candidate_path = execution_repository.join(&relative);
    let candidate = fs::read_to_string(&candidate_path)
        .ok()
        .and_then(|source| toml::from_str::<EnvironmentManifest>(&source).ok());
    if trusted_exists {
        candidate.as_ref() != Some(trusted_manifest)
    } else {
        candidate_path.is_file()
    }
}

fn validation_check(
    kind: ValidationCheckKind,
    passed: bool,
    detail: impl Into<String>,
) -> ValidationCheck {
    ValidationCheck {
        kind,
        status: if passed {
            ValidationCheckStatus::Passed
        } else {
            ValidationCheckStatus::Failed
        },
        detail: detail.into(),
        action_run_id: None,
    }
}

fn audit_reason(code: &str, summary: &str, detail: &str, score: Option<i32>) -> ExplanationReason {
    ExplanationReason {
        code: code.to_owned(),
        summary: summary.to_owned(),
        detail: detail.to_owned(),
        score,
    }
}

fn audit_evidence(kind: &str, id: &str, label: &str) -> EvidenceReference {
    EvidenceReference {
        kind: kind.to_owned(),
        id: id.to_owned(),
        label: label.to_owned(),
    }
}

#[derive(Debug, Error)]
pub enum CoreError {
    #[error(transparent)]
    Git(#[from] GitError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("unable to write AGENT_CONTEXT.md: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error("task routing failed: {0}")]
    Routing(String),
    #[error("historical replay failed: {0}")]
    Replay(String),
    #[error(transparent)]
    Plugin(#[from] PluginError),
    #[error(transparent)]
    Graph(#[from] graph::GraphError),
    #[error(transparent)]
    SourceControl(#[from] SourceControlError),
    #[error(transparent)]
    Worktree(#[from] WorktreeError),
    #[error(transparent)]
    Environment(#[from] EnvironmentError),
}

#[cfg(test)]
mod action_tests {
    use std::{
        collections::BTreeMap,
        process::Command,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };

    use tempfile::TempDir;

    use super::*;
    use crate::{
        domain::{
            ActionKind, ActionRequestInput, ActionStatus, ApproveTaskAssignmentInput,
            CreateGoalInput, CreateTaskInput, EndSessionInput, EnvironmentManifest,
            ExecutionProfile, PermissionMode, PrepareTaskWorkspaceInput, ProviderId,
            RunVerificationInput, SaveEnvironmentManifestInput, StartTaskWorkspaceInput,
            TaskPriority, TaskStatus, TaskWorkspaceStatus, UpdateProjectPermissionsInput,
            UpdateTaskInput, UpdateTaskOrchestrationInput, ValidateTaskWorkspaceInput,
            VerificationStatus, WorkspaceIdInput, WorkspaceValidationStatus,
        },
        ports::{ExecutionSandbox, SandboxError, SandboxExecutionOutput, SandboxExecutionRequest},
        sdk::{IngestExternalEventInput, PluginActionRequest},
    };
    use serde_json::json;

    struct MockSandbox {
        calls: Arc<AtomicUsize>,
    }

    impl ExecutionSandbox for MockSandbox {
        fn name(&self) -> &str {
            "test sandbox"
        }

        fn execute(
            &self,
            request: &SandboxExecutionRequest,
        ) -> Result<SandboxExecutionOutput, SandboxError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(request.command.argv(), ["cargo", "test", "--workspace"]);
            Ok(SandboxExecutionOutput {
                exit_code: 0,
                stdout: "all tests passed".into(),
                stderr: String::new(),
                duration: Duration::from_millis(10),
            })
        }
    }

    fn core_with_project(directory: &TempDir, calls: Arc<AtomicUsize>) -> (AgentRelayCore, String) {
        fs::write(directory.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        let repository = directory.path().to_string_lossy().into_owned();
        let core = AgentRelayCore::open_with_sandbox(
            directory.path().join("agentrelay.db"),
            Duration::from_secs(1),
            Box::new(MockSandbox { calls }),
        )
        .unwrap();
        core.store
            .record_scan(&ProjectStatus {
                repository: repository.clone(),
                branch: "main".into(),
                modified_files: Vec::new(),
                added_files: Vec::new(),
                deleted_files: Vec::new(),
                last_commit: "Initial".into(),
                head_commit: "abc123".into(),
                recent_commits: Vec::new(),
                diff_stat: Vec::new(),
                scanned_at: "2026-09-20T10:00:00.000Z".into(),
            })
            .unwrap();
        (core, repository)
    }

    fn git(directory: &Path, arguments: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(directory)
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn denied_action_never_reaches_the_sandbox() {
        let directory = TempDir::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let (core, repository) = core_with_project(&directory, Arc::clone(&calls));
        core.update_project_permissions(UpdateProjectPermissionsInput {
            repository_path: repository.clone(),
            run_tests: PermissionMode::Deny,
            run_build: PermissionMode::Deny,
        })
        .unwrap();

        let result = core
            .request_action(ActionRequestInput {
                repository_path: repository.clone(),
                session_id: None,
                requested_by: "Codex".into(),
                action: ActionKind::RunTests,
                approval_granted: false,
            })
            .unwrap();

        assert_eq!(result.status, ActionStatus::Denied);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(core
            .project_memory(&repository)
            .unwrap()
            .events
            .iter()
            .any(|event| event.kind == domain::EventKind::ActionDenied));
    }

    #[test]
    fn ask_permission_requires_fresh_approval_before_execution() {
        let directory = TempDir::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let (core, repository) = core_with_project(&directory, Arc::clone(&calls));

        let waiting = core
            .request_action(ActionRequestInput {
                repository_path: repository.clone(),
                session_id: None,
                requested_by: "Codex".into(),
                action: ActionKind::RunTests,
                approval_granted: false,
            })
            .unwrap();
        assert_eq!(waiting.status, ActionStatus::ApprovalRequired);
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let approved = core
            .request_action(ActionRequestInput {
                repository_path: repository,
                session_id: None,
                requested_by: "user".into(),
                action: ActionKind::RunTests,
                approval_granted: true,
            })
            .unwrap();
        assert_eq!(approved.status, ActionStatus::Succeeded);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn approved_test_uses_trusted_command_and_records_the_result() {
        let directory = TempDir::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let (core, repository) = core_with_project(&directory, Arc::clone(&calls));
        core.update_project_permissions(UpdateProjectPermissionsInput {
            repository_path: repository.clone(),
            run_tests: PermissionMode::Allow,
            run_build: PermissionMode::Deny,
        })
        .unwrap();

        let result = core
            .request_action(ActionRequestInput {
                repository_path: repository.clone(),
                session_id: None,
                requested_by: "Codex".into(),
                action: ActionKind::RunTests,
                approval_granted: false,
            })
            .unwrap();

        assert_eq!(result.status, ActionStatus::Succeeded);
        assert_eq!(result.command, ["cargo", "test", "--workspace"]);
        assert_eq!(result.stdout, "all tests passed");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let memory = core.project_memory(&repository).unwrap();
        assert!(memory
            .events
            .iter()
            .any(|event| event.kind == domain::EventKind::ActionCompleted));
        assert_eq!(memory.action_runs[0].status, ActionStatus::Succeeded);
    }

    #[test]
    fn privileged_repository_writes_remain_denied_even_with_approval() {
        let directory = TempDir::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let (core, repository) = core_with_project(&directory, Arc::clone(&calls));
        for action in [
            ActionKind::ModifySource,
            ActionKind::GitCommit,
            ActionKind::GitPush,
            ActionKind::DeleteFiles,
        ] {
            let result = core
                .request_action(ActionRequestInput {
                    repository_path: repository.clone(),
                    session_id: None,
                    requested_by: "Claude".into(),
                    action,
                    approval_granted: true,
                })
                .unwrap();
            assert_eq!(result.status, ActionStatus::Denied, "{action:?}");
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn verifier_rejects_an_agent_modified_environment_manifest() {
        let directory = TempDir::new().unwrap();
        let trusted = directory.path().join("trusted");
        let execution = directory.path().join("execution");
        fs::create_dir(&trusted).unwrap();
        fs::create_dir(&execution).unwrap();
        let service = EnvironmentService::new(Duration::from_secs(1));
        let mut manifest = EnvironmentManifest {
            schema_version: 1,
            profile: ExecutionProfile::NativeWindows,
            working_directory: ".".into(),
            container_image: None,
            disposable_verification: true,
            runtimes: BTreeMap::new(),
            package_manager: None,
            system_tools: Vec::new(),
            commands: BTreeMap::new(),
            environment: Vec::new(),
            cache_kinds: Vec::new(),
            architecture_rules: Vec::new(),
        };
        service.save(&trusted, &manifest).unwrap();
        service.save(&execution, &manifest).unwrap();
        assert!(!worktree_environment_manifest_changed(
            &trusted, &execution, &manifest
        ));

        manifest.commands.insert(
            "test".into(),
            vec!["powershell".into(), "-Command".into(), "untrusted".into()],
        );
        service.save(&execution, &manifest).unwrap();
        let trusted_manifest = service.load_or_infer(&trusted).unwrap().0;
        assert!(worktree_environment_manifest_changed(
            &trusted,
            &execution,
            &trusted_manifest
        ));
    }

    #[test]
    fn missing_remote_sandbox_has_no_local_fallback() {
        let directory = TempDir::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let (mut core, repository) = core_with_project(&directory, Arc::clone(&calls));
        core.sandbox = None;
        core.sandbox_configuration_error = Some("sandbox deliberately absent".into());
        core.update_project_permissions(UpdateProjectPermissionsInput {
            repository_path: repository.clone(),
            run_tests: PermissionMode::Allow,
            run_build: PermissionMode::Deny,
        })
        .unwrap();

        let result = core
            .request_action(ActionRequestInput {
                repository_path: repository,
                session_id: None,
                requested_by: "Codex".into(),
                action: ActionKind::RunTests,
                approval_granted: false,
            })
            .unwrap();

        assert_eq!(result.status, ActionStatus::Unavailable);
        assert_eq!(result.error.as_deref(), Some("sandbox deliberately absent"));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn external_events_are_normalized_before_the_core_persists_them() {
        let directory = TempDir::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let (core, repository) = core_with_project(&directory, calls);

        let recorded = core
            .ingest_external_event(IngestExternalEventInput {
                repository_path: repository.clone(),
                integration_id: "github".into(),
                event_type: "pull_request.updated".into(),
                occurred_at: None,
                payload: json!({"number": 7, "state": "open"}),
            })
            .unwrap();

        assert_eq!(recorded.len(), 1);
        assert_eq!(
            recorded[0].kind,
            domain::EventKind::ExternalPullRequestUpdated
        );
        assert_eq!(recorded[0].payload["integrationId"], "github");
        assert!(core
            .project_memory(&repository)
            .unwrap()
            .events
            .iter()
            .any(|event| event.id == recorded[0].id));
    }

    #[test]
    fn plugin_action_proposals_still_pass_through_core_policy() {
        let directory = TempDir::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let (core, repository) = core_with_project(&directory, Arc::clone(&calls));
        core.update_project_permissions(UpdateProjectPermissionsInput {
            repository_path: repository.clone(),
            run_tests: PermissionMode::Deny,
            run_build: PermissionMode::Deny,
        })
        .unwrap();

        let result = core
            .request_plugin_action(PluginActionRequest {
                integration_id: "test-runner".into(),
                repository_path: repository.clone(),
                session_id: None,
                requested_by: "Codex".into(),
                action: ActionKind::RunTests,
                approval_granted: false,
            })
            .unwrap();

        assert_eq!(result.status, ActionStatus::Denied);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let memory = core.project_memory(&repository).unwrap();
        assert!(memory
            .events
            .iter()
            .any(|event| event.kind == domain::EventKind::PluginActionProposed));
        assert!(memory
            .events
            .iter()
            .any(|event| event.kind == domain::EventKind::ActionDenied));
    }

    #[test]
    fn replay_is_event_bounded_exact_and_read_only() {
        let directory = TempDir::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let (core, repository) = core_with_project(&directory, calls);
        let goal = core
            .create_goal(CreateGoalInput {
                repository_path: repository.clone(),
                title: "Historical goal".into(),
                description: "Original description".into(),
            })
            .unwrap();
        let task = core
            .create_task(CreateTaskInput {
                goal_id: goal.id,
                title: "Replay this task".into(),
                description: "Must still be todo at creation".into(),
                priority: TaskPriority::High,
                assigned_agent: None,
                source_agent: Some("user".into()),
            })
            .unwrap();
        let created_event = core
            .store
            .replay_events(&repository)
            .unwrap()
            .into_iter()
            .find(|event| event.kind == domain::EventKind::TaskCreated)
            .unwrap();
        core.update_task(UpdateTaskInput {
            id: task.id.clone(),
            title: task.title.clone(),
            description: task.description.clone(),
            status: TaskStatus::Completed,
            priority: task.priority,
            assigned_agent: None,
        })
        .unwrap();
        let completed_event = core
            .store
            .replay_events(&repository)
            .unwrap()
            .into_iter()
            .find(|event| event.kind == domain::EventKind::TaskCompleted)
            .unwrap();
        let event_count = core.store.replay_events(&repository).unwrap().len();

        let at_creation = core
            .replay_project(ReplayRequest {
                repository_path: repository.clone(),
                event_id: Some(created_event.id.clone()),
                session_id: None,
                timestamp: None,
            })
            .unwrap();
        assert_eq!(at_creation.accuracy, domain::ReplayAccuracy::Exact);
        assert_eq!(at_creation.goals[0].tasks[0].status, TaskStatus::Todo);

        let at_completion = core
            .replay_project(ReplayRequest {
                repository_path: repository.clone(),
                event_id: Some(completed_event.id.clone()),
                session_id: None,
                timestamp: None,
            })
            .unwrap();
        assert_eq!(
            at_completion.goals[0].tasks[0].status,
            TaskStatus::Completed
        );
        let comparison = core
            .compare_replay_points(CompareReplayInput {
                repository_path: repository.clone(),
                left_event_id: created_event.id,
                right_event_id: completed_event.id,
            })
            .unwrap();
        assert_eq!(comparison.completed_tasks, vec![task.id]);
        assert_eq!(
            core.store.replay_events(&repository).unwrap().len(),
            event_count
        );
    }

    #[test]
    fn generated_context_is_versioned_with_a_verifiable_hash() {
        let directory = TempDir::new().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let (core, repository) = core_with_project(&directory, calls);
        let markdown = core.generate_context(&repository).unwrap();
        let versions = core.store.context_versions(&repository).unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].markdown, markdown);
        assert_eq!(versions[0].commit, "abc123");
        assert_eq!(
            versions[0].context_snapshot_hash,
            format!("{:x}", Sha256::digest(markdown.as_bytes()))
        );
        assert!(versions[0].snapshot.is_some());
        assert!(core
            .timeline(&repository)
            .unwrap()
            .entries
            .iter()
            .any(|entry| entry.category == domain::TimelineCategory::Context));
        let audit = core.audit_trail(&repository).unwrap();
        assert!(audit.iter().any(|record| {
            record.category == domain::ExplanationCategory::Context
                && record.subject_type == "contextSnapshot"
                && record.subject_id == versions[0].id
        }));
        assert!(audit.iter().all(|record| !record.reasons.is_empty()));
    }

    #[test]
    fn isolated_task_workspace_runs_through_validation_merge_and_cleanup() {
        let directory = TempDir::new().unwrap();
        let repository = directory.path().join("project");
        fs::create_dir(&repository).unwrap();
        git(&repository, &["init"]);
        git(
            &repository,
            &["config", "user.email", "agentrelay@example.test"],
        );
        git(&repository, &["config", "user.name", "AgentRelay Test"]);
        fs::write(repository.join("README.md"), "base\n").unwrap();
        git(&repository, &["add", "README.md"]);
        git(&repository, &["commit", "-m", "base"]);

        let calls = Arc::new(AtomicUsize::new(0));
        let core = AgentRelayCore::open_with_sandbox(
            directory.path().join("agentrelay.db"),
            Duration::from_secs(10),
            Box::new(MockSandbox { calls }),
        )
        .unwrap();
        let mut status = core.scan_project(&repository).unwrap();
        core.save_environment_manifest(SaveEnvironmentManifestInput {
            repository_path: status.repository.clone(),
            manifest: EnvironmentManifest {
                schema_version: 1,
                profile: ExecutionProfile::NativeWindows,
                working_directory: ".".into(),
                container_image: None,
                disposable_verification: true,
                runtimes: BTreeMap::new(),
                package_manager: None,
                system_tools: vec!["git".into()],
                commands: BTreeMap::new(),
                environment: Vec::new(),
                cache_kinds: Vec::new(),
                architecture_rules: Vec::new(),
            },
        })
        .unwrap();
        git(&repository, &["add", ".agentrelay/environment.toml"]);
        git(
            &repository,
            &["commit", "-m", "record execution environment"],
        );
        status = core.scan_project(&repository).unwrap();
        let goal = core
            .create_goal(CreateGoalInput {
                repository_path: status.repository.clone(),
                title: "Provider plugin system".into(),
                description: String::new(),
            })
            .unwrap();
        let task = core
            .create_task(CreateTaskInput {
                goal_id: goal.id,
                title: "Implement provider registry".into(),
                description: String::new(),
                priority: TaskPriority::High,
                assigned_agent: None,
                source_agent: Some("user".into()),
            })
            .unwrap();
        core.update_task_orchestration(UpdateTaskOrchestrationInput {
            task_id: task.id.clone(),
            required_capabilities: Vec::new(),
            blocked_by: Vec::new(),
            depends_on: Vec::new(),
            produces: vec!["provider registry".into()],
            consumes: Vec::new(),
            claimed_paths: vec!["src/provider.rs".into()],
        })
        .unwrap();
        core.generate_context(&status.repository).unwrap();
        git(&repository, &["add", "AGENT_CONTEXT.md"]);
        git(&repository, &["commit", "-m", "refresh agent context"]);
        core.approve_task_assignment(ApproveTaskAssignmentInput {
            task_id: task.id.clone(),
            provider: ProviderId::codex(),
        })
        .unwrap();
        let workspace = core
            .prepare_task_workspace(PrepareTaskWorkspaceInput {
                repository_path: status.repository.clone(),
                task_id: task.id.clone(),
            })
            .unwrap();
        assert_ne!(workspace.worktree_path, status.repository);
        let session = core
            .start_task_workspace(StartTaskWorkspaceInput {
                workspace_id: workspace.id.clone(),
            })
            .unwrap();
        let workspace_path = Path::new(&workspace.worktree_path);
        fs::create_dir(workspace_path.join("src")).unwrap();
        fs::write(
            workspace_path.join("src/provider.rs"),
            "pub struct ProviderRegistry;\n",
        )
        .unwrap();
        git(workspace_path, &["add", "src/provider.rs"]);
        git(workspace_path, &["commit", "-m", "add provider registry"]);
        core.end_agent_session(EndSessionInput {
            session_id: session.id,
            summary_status: "Provider registry implemented".into(),
            blockers: Vec::new(),
        })
        .unwrap();

        let validated = core
            .validate_task_workspace(ValidateTaskWorkspaceInput {
                workspace_id: workspace.id.clone(),
                run_tests: false,
                run_build: false,
                approval_granted: false,
            })
            .unwrap();
        assert_eq!(
            validated.validation.status,
            WorkspaceValidationStatus::Passed
        );
        assert!(core
            .prepare_workspace_merge(WorkspaceIdInput {
                workspace_id: workspace.id.clone(),
            })
            .is_err());
        let verification = core
            .run_workspace_verification(RunVerificationInput {
                workspace_id: workspace.id.clone(),
                approval_granted: false,
            })
            .unwrap();
        assert_eq!(verification.status, VerificationStatus::Passed);
        assert_eq!(verification.commit, validated.validation.validated_head);
        let ready = core
            .prepare_workspace_merge(WorkspaceIdInput {
                workspace_id: workspace.id.clone(),
            })
            .unwrap();
        assert_eq!(ready.status, TaskWorkspaceStatus::MergeReady);
        let merged = core
            .approve_workspace_merge(WorkspaceIdInput {
                workspace_id: workspace.id.clone(),
            })
            .unwrap();
        assert_eq!(merged.status, TaskWorkspaceStatus::Merged);
        assert!(repository.join("src/provider.rs").is_file());
        let archived = core
            .cleanup_task_workspace(WorkspaceIdInput {
                workspace_id: workspace.id,
            })
            .unwrap();
        assert_eq!(archived.status, TaskWorkspaceStatus::Archived);
        assert!(!workspace_path.exists());
        assert_eq!(
            core.project_memory(&status.repository).unwrap().goals[0].tasks[0].status,
            TaskStatus::Completed
        );
        let branches = Command::new("git")
            .arg("-C")
            .arg(&repository)
            .args(["branch", "--list", &workspace.branch])
            .output()
            .unwrap();
        assert!(!String::from_utf8_lossy(&branches.stdout).trim().is_empty());
    }
}
