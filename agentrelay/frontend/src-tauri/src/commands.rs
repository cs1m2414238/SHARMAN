use agentrelay_core::{
    context::{ContextCompileInput, ContextPackage},
    domain::{
        ActionRequestInput, ActionRun, AgentCapabilityProfile, AgentSession, ApplicationSettings,
        ApprovePlanInput, ApproveTaskAssignmentInput, BackupRecord, CollaborativeExecutionSnapshot,
        CompareReplayInput, CompleteProjectOnboardingInput, ConflictRecord, CreateDecisionInput,
        CreateGoalInput, CreateTaskInput, Decision, DeliverySnapshot, DeliverySyncReport,
        EndSessionInput, EnvironmentDashboard, ExplainHistoryInput, ExplanationRecord,
        GeneratePlanInput, Goal, HandoffSnapshot, HistoryExplanation, ImpactAnalysis,
        ImpactRequest, OrchestrationSnapshot, PlanApprovalResult, PlanVersion, PlannerSnapshot,
        PrepareTaskWorkspaceInput, ProjectEvent, ProjectGraph, ProjectMemory,
        ProjectOnboardingResult, ProjectPermissions, ProjectRecord, RecoveryReport, ReplanInput,
        ReplayComparison, ReplayRequest, ReplayState, RunVerificationInput, SandboxStatus,
        SaveEnvironmentManifestInput, StartRoutedTaskInput, StartSessionInput,
        StartTaskWorkspaceInput, SyncDeliveryInput, TaskWorkspace, TimelineSnapshot,
        UpdateCapabilityProfileInput, UpdateGoalInput, UpdateProjectPermissionsInput,
        UpdateTaskInput, UpdateTaskOrchestrationInput, ValidateTaskWorkspaceInput,
        VerificationSnapshot, VerificationSuite, WaiveVerificationGateInput, WorkTask,
        WorkspaceIdInput, WorkspaceValidationResult,
    },
    git::ProjectStatus,
    sdk::{IngestExternalEventInput, PluginActionRequest, RegistrySnapshot},
};
use tauri::{AppHandle, State};

use crate::{
    assistant::{
        AssistantAction, AssistantHomeSnapshot, AssistantMemorySnapshot, AssistantRequest,
        AssistantResponse, DeviceProfile,
    },
    audit::AssistantAuditRecord,
    background::{send_notification, BackgroundStatus},
    gmail::{GmailConnectionStatus, GmailSearchMode, InboxDashboard},
    personal::PersonalDashboard,
    product::{DiagnosticExportResult, OnboardingInspection, ProjectHealthSnapshot},
    AppState,
};

#[tauri::command]
pub fn get_assistant_home(state: State<'_, AppState>) -> Result<AssistantHomeSnapshot, String> {
    let mut snapshot = crate::assistant::home_snapshot(&state.core)?;
    let gmail = state.gmail.status()?;
    if let Some(capability) = snapshot
        .capabilities
        .iter_mut()
        .find(|capability| capability.id == "email")
    {
        capability.status = if gmail.connected {
            "ready"
        } else {
            "notConnected"
        }
        .to_owned();
        capability.detail = if gmail.connected {
            format!(
                "Read-only Gmail connected{}",
                gmail
                    .account_email
                    .as_deref()
                    .map(|email| format!(" as {email}"))
                    .unwrap_or_default()
            )
        } else {
            "Connect Gmail from the Action Inbox for read-only triage".to_owned()
        };
    }
    Ok(snapshot)
}

#[tauri::command]
pub fn get_assistant_memory(
    conversation_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<AssistantMemorySnapshot, String> {
    state.assistant_memory.snapshot(conversation_id.as_deref())
}

#[tauri::command]
pub fn create_assistant_conversation(
    state: State<'_, AppState>,
) -> Result<AssistantMemorySnapshot, String> {
    let id = state.assistant_memory.create_conversation()?;
    state.assistant_memory.snapshot(Some(&id))
}

#[tauri::command]
pub fn forget_assistant_memory(
    memory_id: Option<i64>,
    state: State<'_, AppState>,
) -> Result<AssistantMemorySnapshot, String> {
    state.assistant_memory.forget(memory_id)?;
    state.assistant_memory.snapshot(None)
}

#[tauri::command]
pub fn get_personal_dashboard(state: State<'_, AppState>) -> Result<PersonalDashboard, String> {
    state.personal.dashboard(chrono::Local::now())
}

#[tauri::command]
pub fn set_personal_task_completed(
    task_id: String,
    completed: bool,
    state: State<'_, AppState>,
) -> Result<PersonalDashboard, String> {
    state.personal.set_task_completed(&task_id, completed)?;
    state.personal.dashboard(chrono::Local::now())
}

#[tauri::command]
pub fn dismiss_personal_reminder(
    reminder_id: String,
    state: State<'_, AppState>,
) -> Result<PersonalDashboard, String> {
    state.personal.dismiss_reminder(&reminder_id)?;
    state.personal.dashboard(chrono::Local::now())
}

#[tauri::command]
pub fn get_inbox_dashboard(state: State<'_, AppState>) -> Result<InboxDashboard, String> {
    state.gmail.dashboard()
}

#[tauri::command]
pub async fn connect_gmail(state: State<'_, AppState>) -> Result<GmailConnectionStatus, String> {
    let gmail = std::sync::Arc::clone(&state.gmail);
    let audit = std::sync::Arc::clone(&state.assistant_audit);
    tauri::async_runtime::spawn_blocking(move || {
        let result = gmail.connect_interactive();
        let (outcome, summary) = match &result {
            Ok(status) => (
                "passed",
                format!(
                    "Connected read-only Gmail account {}",
                    status.account_email.as_deref().unwrap_or("unknown")
                ),
            ),
            Err(error) => ("failed", error.clone()),
        };
        let _ = audit.record(
            "user",
            "gmail-connect",
            "gmail.oauth.connect",
            "CONFIRM",
            outcome,
            &summary,
        );
        result
    })
    .await
    .map_err(|error| format!("Gmail connection worker stopped unexpectedly: {error}"))?
}

#[tauri::command]
pub fn disconnect_gmail(state: State<'_, AppState>) -> Result<GmailConnectionStatus, String> {
    let status = state.gmail.disconnect()?;
    state.assistant_audit.record(
        "user",
        "gmail-disconnect",
        "gmail.oauth.disconnect",
        "CONFIRM",
        "passed",
        "Removed the Gmail OAuth credential from Windows Credential Manager",
    )?;
    Ok(status)
}

#[tauri::command]
pub async fn refresh_gmail_inbox(
    mode: GmailSearchMode,
    state: State<'_, AppState>,
) -> Result<InboxDashboard, String> {
    let gmail = std::sync::Arc::clone(&state.gmail);
    let audit = std::sync::Arc::clone(&state.assistant_audit);
    tauri::async_runtime::spawn_blocking(move || {
        let result = gmail.refresh(mode);
        let (outcome, summary) = match &result {
            Ok(dashboard) => (
                "passed",
                dashboard.scan.as_ref().map_or_else(
                    || format!("Loaded {} actionable item(s)", dashboard.needs_action.len()),
                    |scan| {
                        format!(
                        "Scanned {} message(s), matched {} deterministic action signal(s) from {}",
                        scan.scanned,
                        scan.matched,
                        mode.label()
                    )
                    },
                ),
            ),
            Err(error) => ("failed", error.clone()),
        };
        let _ = audit.record(
            "supervisor",
            "inbox-intelligence",
            "gmail.search",
            "READ",
            outcome,
            &summary,
        );
        result
    })
    .await
    .map_err(|error| format!("Gmail search worker stopped unexpectedly: {error}"))?
}

#[tauri::command]
pub fn dismiss_email_action(
    action_id: String,
    state: State<'_, AppState>,
) -> Result<InboxDashboard, String> {
    let dashboard = state.gmail.dismiss_action(&action_id)?;
    state.assistant_audit.record(
        "user",
        "inbox-action",
        "gmail.action.dismiss",
        "LOW_RISK",
        "passed",
        &format!("Dismissed local email action {action_id}"),
    )?;
    Ok(dashboard)
}

#[tauri::command]
pub fn create_email_reminder(
    action_id: String,
    state: State<'_, AppState>,
) -> Result<PersonalDashboard, String> {
    let action = state.gmail.action(&action_id)?;
    if action.reminder_created {
        return Err("A reminder already exists for this email action.".to_owned());
    }
    if action.confidence < 0.85 {
        return Err(
            "The extracted deadline is not confident enough. Open the email and verify the date first."
                .to_owned(),
        );
    }
    let deadline = action
        .deadline
        .as_deref()
        .ok_or_else(|| "This email does not contain a verified deadline.".to_owned())?;
    let deadline = chrono::DateTime::parse_from_rfc3339(deadline)
        .map_err(|_| "The extracted email deadline is invalid.".to_owned())?
        .with_timezone(&chrono::Local);
    let remind_at = deadline - chrono::Duration::hours(2);
    if remind_at <= chrono::Local::now() {
        return Err("That deadline is too close or has already passed.".to_owned());
    }
    state
        .personal
        .create_reminder_task(&action.title, remind_at)?;
    state.gmail.mark_reminder_created(&action_id)?;
    state.assistant_audit.record(
        "user",
        "email-deadline",
        "reminders.create",
        "LOW_RISK",
        "passed",
        &format!(
            "Created a local reminder two hours before “{}”",
            action.title
        ),
    )?;
    state.personal.dashboard(chrono::Local::now())
}

#[tauri::command]
pub fn open_gmail_message(thread_id: String, state: State<'_, AppState>) -> Result<(), String> {
    crate::gmail::open_gmail_message(&thread_id)?;
    state.assistant_audit.record(
        "user",
        "email-open",
        "gmail.open_in_browser",
        "READ",
        "passed",
        "Opened a validated Gmail thread in the default browser",
    )
}

#[tauri::command]
pub fn get_assistant_audit(
    state: State<'_, AppState>,
) -> Result<Vec<AssistantAuditRecord>, String> {
    state.assistant_audit.recent(50)
}

#[tauri::command]
pub async fn ask_sharman(
    input: AssistantRequest,
    state: State<'_, AppState>,
) -> Result<AssistantResponse, String> {
    let core = std::sync::Arc::clone(&state.core);
    let memory = std::sync::Arc::clone(&state.assistant_memory);
    let personal = std::sync::Arc::clone(&state.personal);
    let gmail = std::sync::Arc::clone(&state.gmail);
    let audit = std::sync::Arc::clone(&state.assistant_audit);
    tauri::async_runtime::spawn_blocking(move || {
        crate::assistant::answer(&core, &memory, &personal, &gmail, &audit, input)
    })
    .await
    .map_err(|error| format!("assistant worker stopped unexpectedly: {error}"))?
}

#[tauri::command]
pub fn execute_assistant_action(
    action: AssistantAction,
    state: State<'_, AppState>,
) -> Result<(), String> {
    crate::assistant::execute_action(&state.core, action)
}

#[tauri::command]
pub async fn install_local_ai_runtime() -> Result<DeviceProfile, String> {
    tauri::async_runtime::spawn_blocking(crate::assistant::install_local_runtime)
        .await
        .map_err(|error| format!("local AI installer stopped unexpectedly: {error}"))?
}

#[tauri::command]
pub async fn pull_recommended_local_model() -> Result<DeviceProfile, String> {
    tauri::async_runtime::spawn_blocking(crate::assistant::pull_recommended_model)
        .await
        .map_err(|error| format!("local model download stopped unexpectedly: {error}"))?
}

#[tauri::command]
pub fn scan_project(
    repository_path: String,
    state: State<'_, AppState>,
) -> Result<ProjectStatus, String> {
    state
        .core
        .scan_project(repository_path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_last_repository(state: State<'_, AppState>) -> Result<Option<String>, String> {
    state
        .core
        .last_repository()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_tracked_projects(state: State<'_, AppState>) -> Result<Vec<ProjectRecord>, String> {
    state
        .core
        .tracked_projects()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_application_settings(state: State<'_, AppState>) -> Result<ApplicationSettings, String> {
    let mut settings = state
        .core
        .application_settings()
        .map_err(|error| error.to_string())?;
    settings.launch_at_startup = crate::product::startup_enabled();
    Ok(settings)
}

#[tauri::command]
pub fn get_startup_recovery_report(
    state: State<'_, AppState>,
) -> Result<Option<RecoveryReport>, String> {
    state
        .core
        .last_recovery_report()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn update_application_settings(
    settings: ApplicationSettings,
    state: State<'_, AppState>,
) -> Result<ApplicationSettings, String> {
    crate::product::apply_startup_setting(settings.launch_at_startup)?;
    state
        .core
        .save_application_settings(settings)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn choose_repository() -> Option<String> {
    crate::product::choose_repository()
}

#[tauri::command]
pub fn inspect_onboarding(
    repository_path: Option<String>,
    state: State<'_, AppState>,
) -> Result<OnboardingInspection, String> {
    crate::product::onboarding_inspection(&state.core, repository_path.as_deref())
}

#[tauri::command]
pub fn complete_project_onboarding(
    input: CompleteProjectOnboardingInput,
    state: State<'_, AppState>,
) -> Result<ProjectOnboardingResult, String> {
    state
        .core
        .complete_project_onboarding(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_project_health(
    repository_path: String,
    state: State<'_, AppState>,
) -> ProjectHealthSnapshot {
    crate::product::project_health(&state.core, &state.background, &repository_path)
}

#[tauri::command]
pub fn list_backups(state: State<'_, AppState>) -> Result<Vec<BackupRecord>, String> {
    state.core.backups().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn create_backup(state: State<'_, AppState>) -> Result<BackupRecord, String> {
    state
        .core
        .create_backup()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn export_diagnostics(
    repository_path: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<DiagnosticExportResult, String> {
    crate::product::export_diagnostics(
        &app,
        &state.core,
        &state.background,
        repository_path.as_deref(),
    )
}

#[tauri::command]
pub fn get_project_memory(
    repository_path: String,
    state: State<'_, AppState>,
) -> Result<ProjectMemory, String> {
    state
        .core
        .project_memory(&repository_path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_audit_trail(
    repository_path: String,
    state: State<'_, AppState>,
) -> Result<Vec<ExplanationRecord>, String> {
    state
        .core
        .audit_trail(&repository_path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_timeline(
    repository_path: String,
    state: State<'_, AppState>,
) -> Result<TimelineSnapshot, String> {
    state
        .core
        .timeline(&repository_path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn replay_project(
    input: ReplayRequest,
    state: State<'_, AppState>,
) -> Result<ReplayState, String> {
    state
        .core
        .replay_project(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn compare_replay_points(
    input: CompareReplayInput,
    state: State<'_, AppState>,
) -> Result<ReplayComparison, String> {
    state
        .core
        .compare_replay_points(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn explain_history(
    input: ExplainHistoryInput,
    state: State<'_, AppState>,
) -> Result<HistoryExplanation, String> {
    state
        .core
        .explain_history(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_project_permissions(
    repository_path: String,
    state: State<'_, AppState>,
) -> Result<ProjectPermissions, String> {
    state
        .core
        .project_permissions(&repository_path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn update_project_permissions(
    input: UpdateProjectPermissionsInput,
    state: State<'_, AppState>,
) -> Result<ProjectPermissions, String> {
    state
        .core
        .update_project_permissions(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_sandbox_status(state: State<'_, AppState>) -> SandboxStatus {
    state.core.sandbox_status()
}

#[tauri::command]
pub fn get_integration_registry(state: State<'_, AppState>) -> RegistrySnapshot {
    state.core.integration_registry()
}

#[tauri::command]
pub async fn get_project_graph(
    repository_path: String,
    state: State<'_, AppState>,
) -> Result<ProjectGraph, String> {
    let core = std::sync::Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.project_graph(&repository_path))
        .await
        .map_err(|error| format!("project graph worker stopped unexpectedly: {error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn analyze_impact(
    input: ImpactRequest,
    state: State<'_, AppState>,
) -> Result<ImpactAnalysis, String> {
    let core = std::sync::Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.impact_analysis(input))
        .await
        .map_err(|error| format!("impact analysis worker stopped unexpectedly: {error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_environment_dashboard(
    repository_path: String,
    state: State<'_, AppState>,
) -> Result<EnvironmentDashboard, String> {
    state
        .core
        .environment_dashboard(&repository_path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_environment_manifest(
    input: SaveEnvironmentManifestInput,
    state: State<'_, AppState>,
) -> Result<EnvironmentDashboard, String> {
    state
        .core
        .save_environment_manifest(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_verification_snapshot(
    repository_path: String,
    state: State<'_, AppState>,
) -> Result<VerificationSnapshot, String> {
    state
        .core
        .verification_snapshot(&repository_path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn run_workspace_verification(
    input: RunVerificationInput,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<VerificationSuite, String> {
    let core = std::sync::Arc::clone(&state.core);
    let suite =
        tauri::async_runtime::spawn_blocking(move || core.run_workspace_verification(input))
            .await
            .map_err(|error| format!("verification worker stopped unexpectedly: {error}"))?
            .map_err(|error| error.to_string())?;
    send_notification(
        &app,
        if suite.status == agentrelay_core::domain::VerificationStatus::Passed {
            "Independent verification passed"
        } else {
            "Independent verification needs attention"
        },
        "Build, test, lint, architecture, acceptance, and environment evidence was recorded.",
    );
    Ok(suite)
}

#[tauri::command]
pub fn waive_verification_gate(
    input: WaiveVerificationGateInput,
    state: State<'_, AppState>,
) -> Result<VerificationSuite, String> {
    state
        .core
        .waive_verification_gate(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_delivery_snapshot(
    repository_path: String,
    state: State<'_, AppState>,
) -> Result<Option<DeliverySnapshot>, String> {
    state
        .core
        .delivery_snapshot(&repository_path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn sync_github_delivery(
    input: SyncDeliveryInput,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<DeliverySyncReport, String> {
    let core = std::sync::Arc::clone(&state.core);
    let report = tauri::async_runtime::spawn_blocking(move || core.sync_delivery(input))
        .await
        .map_err(|error| format!("GitHub synchronization worker stopped unexpectedly: {error}"))?
        .map_err(|error| error.to_string())?;
    if report.blocked_tasks > 0 {
        send_notification(
            &app,
            "GitHub CI blocker detected",
            &format!(
                "{} AgentRelay task(s) are blocked by failing checks.",
                report.blocked_tasks
            ),
        );
    } else {
        send_notification(
            &app,
            "GitHub delivery synchronized",
            "Issues, pull requests, reviews, and CI are current.",
        );
    }
    Ok(report)
}

#[tauri::command]
pub fn ingest_external_event(
    input: IngestExternalEventInput,
    state: State<'_, AppState>,
) -> Result<Vec<ProjectEvent>, String> {
    state
        .core
        .ingest_external_event(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn request_plugin_action(
    input: PluginActionRequest,
    state: State<'_, AppState>,
) -> Result<ActionRun, String> {
    let core = std::sync::Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.request_plugin_action(input))
        .await
        .map_err(|error| format!("plugin action worker stopped unexpectedly: {error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn request_action(
    input: ActionRequestInput,
    state: State<'_, AppState>,
) -> Result<ActionRun, String> {
    let core = std::sync::Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.request_action(input))
        .await
        .map_err(|error| format!("action worker stopped unexpectedly: {error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn create_goal(input: CreateGoalInput, state: State<'_, AppState>) -> Result<Goal, String> {
    state
        .core
        .create_goal(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn update_goal(input: UpdateGoalInput, state: State<'_, AppState>) -> Result<Goal, String> {
    state
        .core
        .update_goal(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn create_task(input: CreateTaskInput, state: State<'_, AppState>) -> Result<WorkTask, String> {
    state
        .core
        .create_task(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn update_task(input: UpdateTaskInput, state: State<'_, AppState>) -> Result<WorkTask, String> {
    state
        .core
        .update_task(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn update_task_orchestration(
    input: UpdateTaskOrchestrationInput,
    state: State<'_, AppState>,
) -> Result<WorkTask, String> {
    state
        .core
        .update_task_orchestration(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_orchestration_snapshot(
    repository_path: String,
    state: State<'_, AppState>,
) -> Result<OrchestrationSnapshot, String> {
    state
        .core
        .orchestration_snapshot(&repository_path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_planner_snapshot(
    repository_path: String,
    state: State<'_, AppState>,
) -> Result<PlannerSnapshot, String> {
    state
        .core
        .planner_snapshot(&repository_path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn generate_plan(
    input: GeneratePlanInput,
    state: State<'_, AppState>,
) -> Result<PlanVersion, String> {
    state
        .core
        .generate_plan(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn approve_plan(
    input: ApprovePlanInput,
    state: State<'_, AppState>,
) -> Result<PlanApprovalResult, String> {
    state
        .core
        .approve_plan(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn replan(input: ReplanInput, state: State<'_, AppState>) -> Result<PlanVersion, String> {
    state.core.replan(input).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn update_capability_profile(
    input: UpdateCapabilityProfileInput,
    state: State<'_, AppState>,
) -> Result<AgentCapabilityProfile, String> {
    state
        .core
        .update_capability_profile(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn suggest_task_route(task_id: String, state: State<'_, AppState>) -> Result<WorkTask, String> {
    state
        .core
        .suggest_task_route(&task_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn approve_task_assignment(
    input: ApproveTaskAssignmentInput,
    state: State<'_, AppState>,
) -> Result<WorkTask, String> {
    state
        .core
        .approve_task_assignment(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn clear_task_assignment(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<WorkTask, String> {
    state
        .core
        .clear_task_assignment(&task_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn start_routed_task(
    input: StartRoutedTaskInput,
    state: State<'_, AppState>,
) -> Result<AgentSession, String> {
    let core = std::sync::Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.start_routed_task(input))
        .await
        .map_err(|error| format!("session worker stopped unexpectedly: {error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_collaborative_execution_snapshot(
    repository_path: String,
    state: State<'_, AppState>,
) -> Result<CollaborativeExecutionSnapshot, String> {
    state
        .core
        .collaborative_execution_snapshot(&repository_path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn prepare_task_workspace(
    input: PrepareTaskWorkspaceInput,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<TaskWorkspace, String> {
    let core = std::sync::Arc::clone(&state.core);
    let workspace =
        tauri::async_runtime::spawn_blocking(move || core.prepare_task_workspace(input))
            .await
            .map_err(|error| format!("worktree worker stopped unexpectedly: {error}"))?
            .map_err(|error| error.to_string())?;
    send_notification(
        &app,
        "Isolated workspace ready",
        &format!(
            "{} is ready for {}.",
            workspace.branch, workspace.assigned_provider
        ),
    );
    Ok(workspace)
}

#[tauri::command]
pub async fn start_task_workspace(
    input: StartTaskWorkspaceInput,
    state: State<'_, AppState>,
) -> Result<AgentSession, String> {
    let core = std::sync::Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.start_task_workspace(input))
        .await
        .map_err(|error| format!("isolated session worker stopped unexpectedly: {error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn validate_task_workspace(
    input: ValidateTaskWorkspaceInput,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<WorkspaceValidationResult, String> {
    let core = std::sync::Arc::clone(&state.core);
    let result = tauri::async_runtime::spawn_blocking(move || core.validate_task_workspace(input))
        .await
        .map_err(|error| format!("workspace validation worker stopped unexpectedly: {error}"))?
        .map_err(|error| error.to_string())?;
    send_notification(
        &app,
        if result.validation.status == agentrelay_core::domain::WorkspaceValidationStatus::Passed {
            "Workspace validation passed"
        } else {
            "Workspace validation failed"
        },
        "Review the deterministic checks before preparing a merge.",
    );
    Ok(result)
}

#[tauri::command]
pub async fn prepare_workspace_merge(
    input: WorkspaceIdInput,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<TaskWorkspace, String> {
    let core = std::sync::Arc::clone(&state.core);
    let workspace =
        tauri::async_runtime::spawn_blocking(move || core.prepare_workspace_merge(input))
            .await
            .map_err(|error| format!("merge preparation worker stopped unexpectedly: {error}"))?
            .map_err(|error| error.to_string())?;
    send_notification(
        &app,
        if workspace.status == agentrelay_core::domain::TaskWorkspaceStatus::MergeReady {
            "Merge ready for approval"
        } else {
            "Merge conflict detected"
        },
        &workspace.branch,
    );
    Ok(workspace)
}

#[tauri::command]
pub async fn approve_workspace_merge(
    input: WorkspaceIdInput,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<TaskWorkspace, String> {
    let core = std::sync::Arc::clone(&state.core);
    let workspace =
        tauri::async_runtime::spawn_blocking(move || core.approve_workspace_merge(input))
            .await
            .map_err(|error| format!("approved merge worker stopped unexpectedly: {error}"))?
            .map_err(|error| error.to_string())?;
    send_notification(&app, "Task branch merged", &workspace.branch);
    Ok(workspace)
}

#[tauri::command]
pub async fn cleanup_task_workspace(
    input: WorkspaceIdInput,
    state: State<'_, AppState>,
) -> Result<TaskWorkspace, String> {
    let core = std::sync::Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.cleanup_task_workspace(input))
        .await
        .map_err(|error| format!("worktree cleanup worker stopped unexpectedly: {error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn record_decision(
    input: CreateDecisionInput,
    state: State<'_, AppState>,
) -> Result<Decision, String> {
    state
        .core
        .record_decision(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_context_preview(
    repository_path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    state
        .core
        .context_preview(&repository_path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn generate_context(
    repository_path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    state
        .core
        .generate_context(&repository_path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn compile_context(
    input: ContextCompileInput,
    state: State<'_, AppState>,
) -> Result<ContextPackage, String> {
    state
        .core
        .compile_context(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn generate_smart_context(
    input: ContextCompileInput,
    state: State<'_, AppState>,
) -> Result<ContextPackage, String> {
    state
        .core
        .generate_smart_context(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn start_agent_session(
    input: StartSessionInput,
    state: State<'_, AppState>,
) -> Result<AgentSession, String> {
    state
        .core
        .start_agent_session(input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn end_agent_session(
    input: EndSessionInput,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<HandoffSnapshot, String> {
    let blocker_count = input.blockers.len();
    let handoff = state
        .core
        .end_agent_session(input)
        .map_err(|error| error.to_string())?;
    send_notification(
        &app,
        "Handoff ready",
        "AGENT_CONTEXT.md was regenerated for the next agent.",
    );
    if blocker_count > 0 {
        send_notification(
            &app,
            "Task blocked",
            &format!("The handoff contains {blocker_count} unresolved blocker(s)."),
        );
    }
    Ok(handoff)
}

#[tauri::command]
pub fn get_active_session(
    repository_path: String,
    state: State<'_, AppState>,
) -> Result<Option<AgentSession>, String> {
    state
        .core
        .active_agent_session(&repository_path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_latest_handoff(
    repository_path: String,
    state: State<'_, AppState>,
) -> Result<Option<HandoffSnapshot>, String> {
    state
        .core
        .latest_handoff(&repository_path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn mark_session_idle(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<AgentSession, String> {
    state
        .core
        .mark_agent_session_idle(&session_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn evaluate_conflicts(
    repository_path: String,
    state: State<'_, AppState>,
) -> Result<Vec<ConflictRecord>, String> {
    state
        .core
        .evaluate_conflicts(&repository_path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn acknowledge_conflict(
    conflict_id: String,
    state: State<'_, AppState>,
) -> Result<ConflictRecord, String> {
    state
        .core
        .acknowledge_conflict(&conflict_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn resolve_conflict(
    conflict_id: String,
    state: State<'_, AppState>,
) -> Result<ConflictRecord, String> {
    state
        .core
        .resolve_conflict(&conflict_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_project_tracking(
    repository_path: String,
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<ProjectRecord, String> {
    state
        .core
        .set_project_tracking(&repository_path, enabled)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_idle_timeout_minutes(state: State<'_, AppState>) -> Result<u64, String> {
    state
        .core
        .idle_timeout_minutes()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_idle_timeout_minutes(minutes: u64, state: State<'_, AppState>) -> Result<u64, String> {
    state
        .core
        .set_idle_timeout_minutes(minutes)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_background_status(state: State<'_, AppState>) -> BackgroundStatus {
    state.background.snapshot()
}

#[tauri::command]
pub fn open_context_file(repository_path: String) -> Result<(), String> {
    let path = std::path::Path::new(&repository_path).join("AGENT_CONTEXT.md");
    if !path.is_file() {
        return Err("AGENT_CONTEXT.md has not been generated yet".to_owned());
    }
    #[cfg(windows)]
    std::process::Command::new("explorer.exe")
        .arg(&path)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("unable to open AGENT_CONTEXT.md: {error}"))?;
    #[cfg(not(windows))]
    return Err("opening context files is currently supported on Windows only".to_owned());
    #[cfg(windows)]
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HouseholdOverviewSnapshot {
    pub members: Vec<crate::household::HouseholdMember>,
    pub zones: Vec<crate::household::HouseholdZone>,
    pub access_points: Vec<crate::household::AccessPoint>,
    pub schedules: Vec<crate::household::HouseholdSchedule>,
    pub occupancy: crate::household::OccupancyOverview,
}

#[tauri::command]
pub fn get_household_overview(
    state: State<'_, AppState>,
) -> Result<HouseholdOverviewSnapshot, String> {
    let members = state.household.list_members()?;
    let zones = state.household.list_zones()?;
    let access_points = state.household.list_access_points()?;
    let schedules = state.household.list_schedules()?;
    let occupancy = state.household.occupancy_overview()?;
    Ok(HouseholdOverviewSnapshot {
        members,
        zones,
        access_points,
        schedules,
        occupancy,
    })
}

#[tauri::command]
pub fn ingest_household_event(
    event: crate::household::NewHomeEvent,
    state: State<'_, AppState>,
) -> Result<crate::household::IngestResult, String> {
    state.household.ingest_event(event)
}

#[tauri::command]
pub fn list_household_events(
    limit: usize,
    state: State<'_, AppState>,
) -> Result<Vec<crate::household::HomeEvent>, String> {
    state.household.list_events(limit)
}

#[tauri::command]
pub fn reconstruct_household_state(
    at_rfc3339: String,
    state: State<'_, AppState>,
) -> Result<crate::household::ReconstructedHouseholdState, String> {
    let at = chrono::DateTime::parse_from_rfc3339(&at_rfc3339)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .map_err(|e| format!("Invalid timestamp: {e}"))?;
    state.household.reconstruct_state_at(at)
}
