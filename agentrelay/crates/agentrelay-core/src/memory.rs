use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::{json, Value};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    conflict::{analyze, Analysis},
    domain::{
        ActionKind, ActionRun, ActionStatus, AgentCapabilityProfile, AgentSession,
        AgentSessionStatus, ApplicationSettings, ApproveTaskAssignmentInput, BackupRecord,
        CacheStatus, CiCategory, CiCheck, CiConclusion, CiStatus, ConflictKind, ConflictRecord,
        ConflictSeverity, ConflictStatus, ContextVersion, CreateDecisionInput, CreateGoalInput,
        CreateTaskInput, Decision, DecisionStatus, DeliveryBlocker, DeliverySnapshot,
        DeliverySyncReport, EnvironmentCacheRecord, EnvironmentSnapshot, EnvironmentStatus,
        EventKind, EventSource, ExplanationRecord, ExternalEntityType, ExternalIssue,
        ExternalIssueState, ExternalLink, ExternalRepository, FreshnessRecord, FreshnessStatus,
        Goal, GoalStatus, HandoffSnapshot, LocalEntityType, PermissionMode, PlanApprovalResult,
        PlanIssueSeverity, PlanStatus, PlanVersion, PlannedTaskStatus, ProjectEvent, ProjectMemory,
        ProjectPermissions, ProjectRecord, ProviderId, PullRequestState, PullRequestStatus,
        RecoveryReport, TaskAssignmentStatus, TaskPriority, TaskRouteRecommendation, TaskStatus,
        TaskWorkspace, TaskWorkspaceStatus, UpdateCapabilityProfileInput, UpdateGoalInput,
        UpdateProjectPermissionsInput, UpdateTaskInput, UpdateTaskOrchestrationInput,
        VerificationStatus, VerificationSuite, WorkTask, WorkspaceLock, WorkspaceLockStatus,
        WorkspaceValidation, WorkspaceValidationStatus,
    },
    git::ProjectStatus,
    orchestration::path_patterns_overlap,
    sdk::{builtin_provider_profile_templates, NormalizedProjectEvent},
};

/// SQLite-backed project memory. A short-lived connection per operation keeps the core reusable
/// by the desktop shell, a future daemon, CLI, and MCP server without introducing HTTP.
pub struct ContextStore {
    database_path: PathBuf,
}

const CURRENT_SCHEMA_VERSION: i64 = 20;

impl ContextStore {
    pub fn open(database_path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let database_path = database_path.as_ref().to_path_buf();
        if let Some(parent) = database_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let migration_backup = prepare_migration_backup(&database_path)?;
        let store = Self { database_path };
        if let Err(error) = store.initialize() {
            if let Some(backup) = &migration_backup {
                restore_migration_backup(&store.database_path, backup)?;
                return Err(StorageError::MigrationFailed {
                    detail: error.to_string(),
                    backup: backup.to_string_lossy().replace('\\', "/"),
                });
            }
            return Err(error);
        }
        store.recover_interrupted_state(
            migration_backup
                .as_deref()
                .map(|path| path.to_string_lossy().replace('\\', "/")),
        )?;
        Ok(store)
    }

    pub fn record_scan(&self, status: &ProjectStatus) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let previous = transaction
            .query_row(
                "SELECT id, status_signature, status_json FROM projects WHERE path = ?1",
                params![status.repository],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        let project_id = previous
            .as_ref()
            .map(|record| record.0.clone())
            .unwrap_or_else(new_id);
        let status_json = serde_json::to_string(status)?;
        let signature = scan_signature(status)?;
        let project_name = Path::new(&status.repository)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| status.repository.clone());

        transaction.execute(
            "INSERT INTO projects(
               id, path, name, branch, last_commit, change_count, last_scanned_at,
               status_signature, status_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(path) DO UPDATE SET
               name=excluded.name, branch=excluded.branch, last_commit=excluded.last_commit,
               change_count=excluded.change_count, last_scanned_at=excluded.last_scanned_at,
               status_signature=excluded.status_signature, status_json=excluded.status_json",
            params![
                project_id,
                status.repository,
                project_name,
                status.branch,
                status.last_commit,
                status.change_count() as i64,
                status.scanned_at,
                signature,
                status_json
            ],
        )?;
        transaction.execute(
            "INSERT INTO settings(key, value) VALUES ('last_repository', ?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![status.repository],
        )?;

        append_event(
            &transaction,
            &project_id,
            EventKind::RepositoryScanned,
            EventSource::Git,
            serde_json::to_value(status)?,
            &status.scanned_at,
        )?;

        if previous.as_ref().is_none_or(|record| record.1 != signature) {
            for (change_type, files) in [
                ("modified", &status.modified_files),
                ("added", &status.added_files),
                ("deleted", &status.deleted_files),
            ] {
                for path in files {
                    append_event(
                        &transaction,
                        &project_id,
                        EventKind::FileChanged,
                        EventSource::Git,
                        json!({"path": path, "changeType": change_type}),
                        &status.scanned_at,
                    )?;
                }
            }
        }

        if let Some((_, _, previous_json)) = &previous {
            if previous_json != "{}" {
                let old: ProjectStatus = serde_json::from_str(previous_json)?;
                if !status.recent_commits.is_empty()
                    && old.recent_commits.first() != status.recent_commits.first()
                {
                    append_event(
                        &transaction,
                        &project_id,
                        EventKind::CommitDetected,
                        EventSource::Git,
                        json!({"commit": status.recent_commits.first()}),
                        &status.scanned_at,
                    )?;
                }
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn last_repository(&self) -> Result<Option<String>, StorageError> {
        Ok(self
            .connection()?
            .query_row(
                "SELECT value FROM settings WHERE key='last_repository'",
                [],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn application_settings(&self) -> Result<ApplicationSettings, StorageError> {
        let value = self
            .connection()?
            .query_row(
                "SELECT value FROM settings WHERE key='application_settings_v19'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if let Some(value) = value {
            return Ok(serde_json::from_str::<ApplicationSettings>(&value)?.normalized());
        }
        Ok(ApplicationSettings {
            onboarding_completed: self.last_repository()?.is_some(),
            ..ApplicationSettings::default()
        })
    }

    pub fn save_application_settings(
        &self,
        settings: ApplicationSettings,
    ) -> Result<ApplicationSettings, StorageError> {
        let settings = settings.normalized();
        self.connection()?.execute(
            "INSERT INTO settings(key,value) VALUES ('application_settings_v19',?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![serde_json::to_string(&settings)?],
        )?;
        Ok(settings)
    }

    pub fn last_recovery_report(&self) -> Result<Option<RecoveryReport>, StorageError> {
        self.connection()?
            .query_row(
                "SELECT value FROM settings WHERE key='last_recovery_report_v20'",
                [],
                |row| {
                    let value: String = row.get(0)?;
                    json_from_db(&value, 0)
                },
            )
            .optional()
            .map_err(StorageError::from)
    }

    fn recover_interrupted_state(
        &self,
        migration_backup: Option<String>,
    ) -> Result<RecoveryReport, StorageError> {
        let mut connection = self.connection()?;
        let integrity: String =
            connection.query_row("PRAGMA quick_check(1)", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Err(StorageError::IntegrityCheckFailed(integrity));
        }
        let transaction = connection.transaction()?;
        let timestamp = now();
        let active_sessions = {
            let mut statement = transaction.prepare(
                "SELECT id,project_id,task_id FROM agent_sessions WHERE status='active'",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        let mut report = RecoveryReport {
            recovered_at: timestamp.clone(),
            database_integrity: "ok".into(),
            migration_backup,
            interrupted_sessions: active_sessions.len(),
            interrupted_actions: 0,
            interrupted_verifications: 0,
            recoverable_workspaces: 0,
            missing_workspaces: 0,
            invalidated_caches: 0,
            warnings: Vec::new(),
        };

        for (session_id, project_id, task_id) in &active_sessions {
            let workspace = transaction
                .query_row(
                    "SELECT id,worktree_path FROM task_workspaces
                     WHERE session_id=?1 AND status='active' LIMIT 1",
                    params![session_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?;
            let recoverable_workspace = workspace
                .as_ref()
                .is_some_and(|(_, path)| Path::new(path).is_dir());
            if let Some((workspace_id, path)) = &workspace {
                if recoverable_workspace {
                    transaction.execute(
                        "UPDATE task_workspaces SET status='validation_pending',updated_at=?2
                         WHERE id=?1",
                        params![workspace_id, timestamp],
                    )?;
                    report.recoverable_workspaces += 1;
                    if let Some(task_id) = task_id {
                        transaction.execute(
                            "UPDATE tasks SET assignment_status='waiting' WHERE id=?1
                             AND status NOT IN ('completed','archived')",
                            params![task_id],
                        )?;
                    }
                } else {
                    mark_missing_workspace(
                        &transaction,
                        workspace_id,
                        project_id,
                        task_id.as_deref(),
                        path,
                        &timestamp,
                    )?;
                    report.missing_workspaces += 1;
                }
            } else if let Some(task_id) = task_id {
                transaction.execute(
                    "UPDATE tasks SET assignment_status='waiting' WHERE id=?1
                     AND status NOT IN ('completed','archived')",
                    params![task_id],
                )?;
                release_task_locks(&transaction, project_id, task_id, &timestamp)?;
            }
            transaction.execute(
                "UPDATE agent_sessions SET status='interrupted',ended_at=?2,
                 summary_status=CASE WHEN summary_status='' THEN ?3
                 ELSE summary_status || ' · ' || ?3 END WHERE id=?1",
                params![
                    session_id,
                    timestamp,
                    "Interrupted by an AgentRelay restart; recovered without assuming completion."
                ],
            )?;
            insert_event(
                &transaction,
                project_id,
                Some(session_id),
                EventKind::SessionEnded,
                EventSource::System,
                json!({"recovered": true, "status": "interrupted",
                    "reason": "AgentRelay restarted before the session ended"}),
                &timestamp,
            )?;
        }

        let interrupted_actions = {
            let mut statement = transaction.prepare(
                "SELECT id,project_id FROM action_runs WHERE status IN ('requested','running')",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        for (action_id, project_id) in &interrupted_actions {
            transaction.execute(
                "UPDATE action_runs SET status='failed',error=?2,finished_at=?3 WHERE id=?1",
                params![
                    action_id,
                    "AgentRelay restarted before the action returned a result.",
                    timestamp
                ],
            )?;
            insert_event(
                &transaction,
                project_id,
                None,
                EventKind::ActionFailed,
                EventSource::System,
                json!({"actionRunId": action_id, "recovered": true,
                    "reason": "interrupted by restart"}),
                &timestamp,
            )?;
        }
        report.interrupted_actions = interrupted_actions.len();

        let interrupted_verifications = {
            let mut statement = transaction.prepare(
                "SELECT id,project_id,snapshot_json FROM verification_suites WHERE status='running'",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        for (suite_id, project_id, snapshot) in &interrupted_verifications {
            let mut suite: VerificationSuite = serde_json::from_str(snapshot)?;
            suite.status = VerificationStatus::Failed;
            suite.completed_at = Some(timestamp.clone());
            for gate in &mut suite.gates {
                if gate.status == VerificationStatus::Running {
                    gate.status = VerificationStatus::Failed;
                    gate.finished_at = Some(timestamp.clone());
                    gate.detail = "AgentRelay restarted while this gate was running.".into();
                }
            }
            transaction.execute(
                "UPDATE verification_suites SET status='failed',snapshot_json=?2,
                 completed_at=?3 WHERE id=?1",
                params![suite_id, serde_json::to_string(&suite)?, timestamp],
            )?;
            insert_event(
                &transaction,
                project_id,
                None,
                EventKind::VerificationFailed,
                EventSource::System,
                json!({"suiteId": suite_id, "recovered": true,
                    "reason": "interrupted by restart"}),
                &timestamp,
            )?;
        }
        report.interrupted_verifications = interrupted_verifications.len();

        let valid_caches = {
            let mut statement = transaction.prepare(
                "SELECT id,project_id,path FROM environment_caches WHERE status='valid'",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        for (cache_id, project_id, path) in valid_caches {
            if Path::new(&path).exists() {
                continue;
            }
            transaction.execute(
                "UPDATE environment_caches SET status='invalidated',updated_at=?2 WHERE id=?1",
                params![cache_id, timestamp],
            )?;
            insert_event(
                &transaction,
                &project_id,
                None,
                EventKind::EnvironmentCacheInvalidated,
                EventSource::System,
                json!({"cacheId": cache_id, "recovered": true,
                    "reason": "cache directory was missing at startup"}),
                &timestamp,
            )?;
            report.invalidated_caches += 1;
        }

        let live_workspaces = {
            let mut statement = transaction.prepare(
                "SELECT id,project_id,task_id,worktree_path FROM task_workspaces
                 WHERE status NOT IN ('merged','archived','validation_failed')",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        for (workspace_id, project_id, task_id, path) in live_workspaces {
            if Path::new(&path).is_dir() {
                continue;
            }
            mark_missing_workspace(
                &transaction,
                &workspace_id,
                &project_id,
                Some(&task_id),
                &path,
                &timestamp,
            )?;
            report.missing_workspaces += 1;
        }
        if report.missing_workspaces > 0 {
            report.warnings.push(format!(
                "{} isolated workspace(s) were missing and their tasks were blocked.",
                report.missing_workspaces
            ));
        }
        transaction.execute(
            "INSERT INTO settings(key,value) VALUES ('last_recovery_report_v20',?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![serde_json::to_string(&report)?],
        )?;
        transaction.commit()?;
        Ok(report)
    }

    pub fn project_is_onboarded(&self, repository_path: &str) -> Result<bool, StorageError> {
        let connection = self.connection()?;
        let project_id = project_id_for_path(&connection, repository_path)?;
        Ok(connection
            .query_row(
                "SELECT value FROM settings WHERE key=?1",
                params![format!("project_onboarded:{project_id}")],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .is_some())
    }

    pub fn mark_project_onboarded(&self, repository_path: &str) -> Result<(), StorageError> {
        let connection = self.connection()?;
        let project_id = project_id_for_path(&connection, repository_path)?;
        connection.execute(
            "INSERT INTO settings(key,value) VALUES (?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![format!("project_onboarded:{project_id}"), now()],
        )?;
        Ok(())
    }

    pub fn create_backup(&self) -> Result<BackupRecord, StorageError> {
        let backup_directory = self
            .database_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("backups");
        fs::create_dir_all(&backup_directory)?;
        let created_at = now();
        let filename = format!("agentrelay-{}.db", Utc::now().format("%Y%m%dT%H%M%S%.3fZ"));
        let destination = backup_directory.join(filename);
        let connection = self.connection()?;
        connection.execute_batch("PRAGMA wal_checkpoint(PASSIVE);")?;
        connection.execute(
            "VACUUM INTO ?1",
            params![destination.to_string_lossy().to_string()],
        )?;
        Ok(BackupRecord {
            size_bytes: fs::metadata(&destination)?.len(),
            path: destination.to_string_lossy().replace('\\', "/"),
            created_at,
        })
    }

    pub fn backups(&self) -> Result<Vec<BackupRecord>, StorageError> {
        let directory = self
            .database_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("backups");
        if !directory.exists() {
            return Ok(Vec::new());
        }
        let mut records = fs::read_dir(directory)?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let metadata = entry.metadata().ok()?;
                if !metadata.is_file() || entry.path().extension()?.to_string_lossy() != "db" {
                    return None;
                }
                let modified = metadata.modified().ok()?;
                let created_at = chrono::DateTime::<Utc>::from(modified)
                    .to_rfc3339_opts(SecondsFormat::Millis, true);
                Some(BackupRecord {
                    path: entry.path().to_string_lossy().replace('\\', "/"),
                    created_at,
                    size_bytes: metadata.len(),
                })
            })
            .collect::<Vec<_>>();
        records.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(records)
    }

    pub fn tracked_projects(&self) -> Result<Vec<ProjectRecord>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id,path,name,branch,last_commit,change_count,last_scanned_at,
             tracking_enabled,status_json FROM projects WHERE tracking_enabled=1
             ORDER BY name,path",
        )?;
        let records = statement
            .query_map([], project_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn set_project_tracking(
        &self,
        repository_path: &str,
        enabled: bool,
    ) -> Result<ProjectRecord, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let project_id = project_id_for_path(&transaction, repository_path)?;
        transaction.execute(
            "UPDATE projects SET tracking_enabled=?2 WHERE path=?1",
            params![repository_path, enabled],
        )?;
        let timestamp = now();
        append_event(
            &transaction,
            &project_id,
            EventKind::ProjectTrackingChanged,
            EventSource::User,
            json!({"enabled": enabled}),
            &timestamp,
        )?;
        let project = transaction.query_row(
            "SELECT id,path,name,branch,last_commit,change_count,last_scanned_at,
                 tracking_enabled,status_json FROM projects WHERE path=?1",
            params![repository_path],
            project_from_row,
        )?;
        transaction.commit()?;
        Ok(project)
    }

    pub fn idle_timeout_minutes(&self) -> Result<u64, StorageError> {
        let value = self
            .connection()?
            .query_row(
                "SELECT value FROM settings WHERE key='idle_timeout_minutes'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(value
            .as_deref()
            .and_then(|raw| raw.parse::<u64>().ok())
            .unwrap_or(15)
            .clamp(1, 240))
    }

    pub fn set_idle_timeout_minutes(&self, minutes: u64) -> Result<u64, StorageError> {
        let minutes = minutes.clamp(1, 240);
        self.connection()?.execute(
            "INSERT INTO settings(key,value) VALUES ('idle_timeout_minutes',?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![minutes.to_string()],
        )?;
        Ok(minutes)
    }

    pub(crate) fn record_external_event(
        &self,
        repository_path: &str,
        event: NormalizedProjectEvent,
    ) -> Result<ProjectEvent, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let project_id = project_id_for_path(&transaction, repository_path)?;
        let session_id = active_session_id(&transaction, &project_id)?;
        let id = new_id();
        let created_at = event.occurred_at.unwrap_or_else(now);
        transaction.execute(
            "INSERT INTO events(id,project_id,session_id,kind,source,payload_json,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                id,
                project_id,
                session_id,
                event.kind.as_db(),
                event.source.as_db(),
                serde_json::to_string(&event.payload)?,
                created_at,
            ],
        )?;
        let recorded = ProjectEvent {
            id,
            project_id,
            session_id,
            kind: event.kind,
            source: event.source,
            payload: event.payload,
            created_at,
        };
        transaction.commit()?;
        Ok(recorded)
    }

    pub fn delivery_snapshot(
        &self,
        repository_path: &str,
    ) -> Result<Option<DeliverySnapshot>, StorageError> {
        let connection = self.connection()?;
        let project_id = project_id_for_path(&connection, repository_path)?;
        delivery_snapshot_for_project(&connection, &project_id, "github")
    }

    pub(crate) fn apply_delivery_snapshot(
        &self,
        repository_path: &str,
        repository: ExternalRepository,
        issues: Vec<ExternalIssue>,
        pull_requests: Vec<PullRequestState>,
    ) -> Result<DeliverySyncReport, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let project_id = project_id_for_path(&transaction, repository_path)?;
        let report =
            apply_delivery_sync(&transaction, &project_id, repository, issues, pull_requests)?;
        transaction.commit()?;
        Ok(report)
    }

    pub(crate) fn active_delivery_blockers(
        &self,
        repository_path: &str,
    ) -> Result<Vec<DeliveryBlocker>, StorageError> {
        let connection = self.connection()?;
        let project_id = project_id_for_path(&connection, repository_path)?;
        delivery_blockers_for_project(&connection, &project_id, true)
    }

    pub(crate) fn record_plugin_action_proposed(
        &self,
        repository_path: &str,
        integration_id: &str,
        action: ActionKind,
    ) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let project_id = project_id_for_path(&transaction, repository_path)?;
        let timestamp = now();
        append_event(
            &transaction,
            &project_id,
            EventKind::PluginActionProposed,
            EventSource::Other,
            json!({"integrationId": integration_id, "action": action}),
            &timestamp,
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn project_memory(&self, repository_path: &str) -> Result<ProjectMemory, StorageError> {
        let connection = self.connection()?;
        let project = connection
            .query_row(
                "SELECT id,path,name,branch,last_commit,change_count,last_scanned_at,
                 tracking_enabled,status_json
             FROM projects WHERE path=?1",
                params![repository_path],
                project_from_row,
            )
            .optional()?
            .ok_or_else(|| StorageError::ProjectNotFound(repository_path.to_owned()))?;

        let mut goals = {
            let mut statement = connection.prepare(
                "SELECT id,project_id,title,description,status,created_at,updated_at FROM goals
                 WHERE project_id=?1 ORDER BY CASE status WHEN 'active' THEN 0 WHEN 'completed'
                 THEN 1 ELSE 2 END, created_at, id",
            )?;
            let records = statement
                .query_map(params![project.id], goal_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            records
        };
        for goal in &mut goals {
            goal.tasks = tasks_for_goal(&connection, &goal.id)?;
        }
        let decisions = {
            let mut statement = connection.prepare(
                "SELECT id,project_id,summary,reason,alternatives_json,affected_json,status,
                 created_by,superseded_by,created_at FROM decisions WHERE project_id=?1
                 ORDER BY created_at,id",
            )?;
            let records = statement
                .query_map(params![project.id], decision_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            records
        };
        let events = {
            let mut statement = connection.prepare(
                "SELECT id,project_id,session_id,kind,source,payload_json,created_at FROM events
                 WHERE project_id=?1 ORDER BY created_at DESC,rowid DESC LIMIT 200",
            )?;
            let records = statement
                .query_map(params![project.id], event_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            records
        };
        let agent_sessions = {
            let mut statement = connection.prepare(
                "SELECT id,project_id,provider,agent_name,status,working_directory,
                 initial_git_head,final_git_head,initial_git_state_json,final_git_state_json,
                 started_at,ended_at,summary_status,detected_pid,automatic,task_id FROM agent_sessions
                 WHERE project_id=?1 ORDER BY started_at DESC,id DESC LIMIT 50",
            )?;
            let records = statement
                .query_map(params![project.id], session_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            records
        };
        let last_agent_session = agent_sessions.first().cloned();
        let handoffs = {
            let mut statement = connection.prepare(
                "SELECT snapshot_json FROM handoffs WHERE project_id=?1
                 ORDER BY created_at DESC,id DESC LIMIT 50",
            )?;
            let records = statement
                .query_map(params![project.id], |row| {
                    let value: String = row.get(0)?;
                    json_from_db(&value, 0)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            records
        };
        let latest_handoff = handoffs.first().cloned();
        let conflicts = {
            let mut statement = connection.prepare(
                "SELECT id,project_id,fingerprint,primary_session_id,related_session_id,kind,
                 severity,title,description,affected_paths_json,related_decision_ids_json,status,
                 detected_at,updated_at,resolved_at FROM conflicts WHERE project_id=?1
                 ORDER BY CASE status WHEN 'open' THEN 0 WHEN 'acknowledged' THEN 1 ELSE 2 END,
                 updated_at DESC,id DESC LIMIT 100",
            )?;
            let records = statement
                .query_map(params![project.id], conflict_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            records
        };
        let freshness = {
            let mut statement = connection.prepare(
                "SELECT project_id,subject_type,subject_id,status,reason,superseded_by,evaluated_at
                 FROM freshness WHERE project_id=?1 ORDER BY subject_type,subject_id",
            )?;
            let records = statement
                .query_map(params![project.id], freshness_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            records
        };
        let permissions = permissions_for_project(&connection, &project.id)?;
        let action_runs = {
            let mut statement = connection.prepare(
                "SELECT id,project_id,session_id,requested_by,action_kind,status,command_json,
                 sandbox,exit_code,stdout,stderr,error,requested_at,started_at,finished_at
                 FROM action_runs WHERE project_id=?1 ORDER BY requested_at DESC,id DESC LIMIT 50",
            )?;
            let records = statement
                .query_map(params![project.id], action_run_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            records
        };
        Ok(ProjectMemory {
            project,
            goals,
            decisions,
            events,
            agent_sessions,
            last_agent_session,
            latest_handoff,
            handoffs,
            conflicts,
            freshness,
            permissions,
            action_runs,
        })
    }

    pub fn project_permissions(
        &self,
        repository_path: &str,
    ) -> Result<ProjectPermissions, StorageError> {
        let connection = self.connection()?;
        let project_id = project_id_for_path(&connection, repository_path)?;
        permissions_for_project(&connection, &project_id)
    }

    pub fn update_project_permissions(
        &self,
        input: UpdateProjectPermissionsInput,
    ) -> Result<ProjectPermissions, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let project_id = project_id_for_path(&transaction, &input.repository_path)?;
        let timestamp = now();
        transaction.execute(
            "INSERT INTO project_permissions(project_id,read_repository,read_git,write_memory,
             write_context_files,run_tests,run_build,modify_source,git_commit,git_push,
             delete_files,updated_at,run_tests_mode,run_build_mode)
             VALUES (?1,1,1,1,1,?2,?3,0,0,0,0,?4,?5,?6)
             ON CONFLICT(project_id) DO UPDATE SET run_tests=excluded.run_tests,
             run_build=excluded.run_build,updated_at=excluded.updated_at,
             run_tests_mode=excluded.run_tests_mode,run_build_mode=excluded.run_build_mode",
            params![
                project_id,
                matches!(input.run_tests, PermissionMode::Allow) as i64,
                matches!(input.run_build, PermissionMode::Allow) as i64,
                timestamp,
                input.run_tests.as_db(),
                input.run_build.as_db(),
            ],
        )?;
        append_event(
            &transaction,
            &project_id,
            EventKind::PermissionsUpdated,
            EventSource::User,
            json!({"runTests": input.run_tests, "runBuild": input.run_build,
                "lockedPermissionsRemainDisabled": true}),
            &timestamp,
        )?;
        let permissions = permissions_for_project(&transaction, &project_id)?;
        transaction.commit()?;
        Ok(permissions)
    }

    pub fn record_action_request(
        &self,
        repository_path: &str,
        session_id: Option<&str>,
        requested_by: &str,
        action: ActionKind,
        command: &[String],
    ) -> Result<ActionRun, StorageError> {
        validate_required("requested by", requested_by)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let project_id = project_id_for_path(&transaction, repository_path)?;
        if let Some(session_id) = session_id {
            let session_project: Option<String> = transaction
                .query_row(
                    "SELECT project_id FROM agent_sessions WHERE id=?1",
                    params![session_id],
                    |row| row.get(0),
                )
                .optional()?;
            if session_project.as_deref() != Some(project_id.as_str()) {
                return Err(StorageError::Validation(
                    "action session does not belong to the selected project".into(),
                ));
            }
        }
        let id = new_id();
        let timestamp = now();
        transaction.execute(
            "INSERT INTO action_runs(id,project_id,session_id,requested_by,action_kind,status,
             command_json,stdout,stderr,requested_at) VALUES (?1,?2,?3,?4,?5,'requested',?6,'','',?7)",
            params![
                id,
                project_id,
                session_id,
                requested_by.trim(),
                action.as_db(),
                serde_json::to_string(command)?,
                timestamp
            ],
        )?;
        insert_event(
            &transaction,
            &project_id,
            session_id,
            EventKind::ActionRequested,
            EventSource::System,
            json!({"actionId": id, "action": action, "requestedBy": requested_by.trim(),
                "command": command}),
            &timestamp,
        )?;
        let action_run = action_run_by_id(&transaction, &id)?;
        transaction.commit()?;
        Ok(action_run)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn transition_action(
        &self,
        id: &str,
        status: ActionStatus,
        sandbox: Option<&str>,
        exit_code: Option<i32>,
        stdout: &str,
        stderr: &str,
        error: Option<&str>,
    ) -> Result<ActionRun, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let previous = action_run_by_id(&transaction, id)?;
        let timestamp = now();
        let started_at = if status == ActionStatus::Running {
            Some(timestamp.as_str())
        } else {
            previous.started_at.as_deref()
        };
        let finished_at = if matches!(
            status,
            ActionStatus::ApprovalRequired
                | ActionStatus::Denied
                | ActionStatus::Succeeded
                | ActionStatus::Failed
                | ActionStatus::Unavailable
        ) {
            Some(timestamp.as_str())
        } else {
            previous.finished_at.as_deref()
        };
        transaction.execute(
            "UPDATE action_runs SET status=?2,sandbox=COALESCE(?3,sandbox),exit_code=?4,
             stdout=?5,stderr=?6,error=?7,started_at=?8,finished_at=?9 WHERE id=?1",
            params![
                id,
                status.as_db(),
                sandbox,
                exit_code,
                stdout,
                stderr,
                error,
                started_at,
                finished_at
            ],
        )?;
        let event_kind = match status {
            ActionStatus::ApprovalRequired | ActionStatus::Denied => EventKind::ActionDenied,
            ActionStatus::Running => EventKind::ActionStarted,
            ActionStatus::Succeeded => EventKind::ActionCompleted,
            ActionStatus::Failed | ActionStatus::Unavailable => EventKind::ActionFailed,
            ActionStatus::Requested => EventKind::ActionRequested,
        };
        insert_event(
            &transaction,
            &previous.project_id,
            previous.session_id.as_deref(),
            event_kind,
            EventSource::System,
            json!({"actionId": id, "action": previous.action, "status": status,
                "exitCode": exit_code, "sandbox": sandbox, "error": error}),
            &timestamp,
        )?;
        let action_run = action_run_by_id(&transaction, id)?;
        transaction.commit()?;
        Ok(action_run)
    }

    pub fn create_goal(&self, input: CreateGoalInput) -> Result<Goal, StorageError> {
        validate_required("goal title", &input.title)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let project_id = project_id_for_path(&transaction, &input.repository_path)?;
        let id = new_id();
        let timestamp = now();
        transaction.execute(
            "INSERT INTO goals(id,project_id,title,description,status,created_at,updated_at)
             VALUES (?1,?2,?3,?4,'active',?5,?5)",
            params![
                id,
                project_id,
                input.title.trim(),
                input.description.trim(),
                timestamp
            ],
        )?;
        let goal = goal_by_id(&transaction, &id)?;
        append_event(
            &transaction,
            &project_id,
            EventKind::GoalCreated,
            EventSource::User,
            json!({"goalId": id, "title": input.title.trim(), "goal": &goal}),
            &timestamp,
        )?;
        transaction.commit()?;
        Ok(goal)
    }

    pub fn update_goal(&self, input: UpdateGoalInput) -> Result<Goal, StorageError> {
        validate_required("goal title", &input.title)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let project_id = transaction
            .query_row(
                "SELECT project_id FROM goals WHERE id=?1",
                params![input.id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::GoalNotFound(input.id.clone()))?;
        let timestamp = now();
        transaction.execute(
            "UPDATE goals SET title=?2,description=?3,status=?4,updated_at=?5 WHERE id=?1",
            params![
                input.id,
                input.title.trim(),
                input.description.trim(),
                input.status.as_db(),
                timestamp
            ],
        )?;
        let goal = goal_by_id(&transaction, &input.id)?;
        append_event(
            &transaction,
            &project_id,
            EventKind::GoalUpdated,
            EventSource::User,
            json!({"goalId": input.id, "title": input.title.trim(), "status": input.status,
                "goal": &goal}),
            &timestamp,
        )?;
        transaction.commit()?;
        Ok(goal)
    }

    pub fn create_task(&self, input: CreateTaskInput) -> Result<WorkTask, StorageError> {
        validate_required("task title", &input.title)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let project_id = transaction
            .query_row(
                "SELECT project_id FROM goals WHERE id=?1",
                params![input.goal_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::GoalNotFound(input.goal_id.clone()))?;
        let id = new_id();
        let timestamp = now();
        transaction.execute(
            "INSERT INTO tasks(id,goal_id,title,description,status,priority,assigned_agent,
             source_agent,created_at,completed_at) VALUES (?1,?2,?3,?4,'todo',?5,?6,?7,?8,NULL)",
            params![
                id,
                input.goal_id,
                input.title.trim(),
                input.description.trim(),
                input.priority.as_db(),
                input.assigned_agent,
                input.source_agent,
                timestamp
            ],
        )?;
        let task = task_by_id(&transaction, &id)?;
        append_event(
            &transaction,
            &project_id,
            EventKind::TaskCreated,
            EventSource::User,
            json!({"taskId": id, "goalId": input.goal_id, "title": input.title.trim(),
                "task": &task}),
            &timestamp,
        )?;
        transaction.commit()?;
        Ok(task)
    }

    pub fn update_task(&self, input: UpdateTaskInput) -> Result<WorkTask, StorageError> {
        validate_required("task title", &input.title)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let existing = transaction.query_row(
            "SELECT goals.project_id,tasks.status FROM tasks JOIN goals ON goals.id=tasks.goal_id
             WHERE tasks.id=?1", params![input.id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ).optional()?.ok_or_else(|| StorageError::TaskNotFound(input.id.clone()))?;
        let timestamp = now();
        let completed_at = (input.status == TaskStatus::Completed).then_some(timestamp.as_str());
        transaction.execute(
            "UPDATE tasks SET title=?2,description=?3,status=?4,priority=?5,assigned_agent=?6,
             completed_at=?7,assignment_status=CASE WHEN ?4='completed' THEN 'completed'
             WHEN assignment_status='completed' THEN 'unassigned' ELSE assignment_status END WHERE id=?1",
            params![
                input.id,
                input.title.trim(),
                input.description.trim(),
                input.status.as_db(),
                input.priority.as_db(),
                input.assigned_agent,
                completed_at
            ],
        )?;
        let task = task_by_id(&transaction, &input.id)?;
        append_event(
            &transaction,
            &existing.0,
            EventKind::TaskUpdated,
            EventSource::User,
            json!({"taskId": input.id, "title": input.title.trim(), "status": input.status,
                "task": &task}),
            &timestamp,
        )?;
        if existing.1 != TaskStatus::Completed.as_db() && input.status == TaskStatus::Completed {
            append_event(
                &transaction,
                &existing.0,
                EventKind::TaskCompleted,
                EventSource::User,
                json!({"taskId": input.id, "title": input.title.trim(), "task": &task}),
                &timestamp,
            )?;
            release_task_locks(&transaction, &existing.0, &input.id, &timestamp)?;
            refresh_waiting_assignments(&transaction, &existing.0, &timestamp)?;
        }
        transaction.commit()?;
        Ok(task)
    }

    pub fn update_task_orchestration(
        &self,
        input: UpdateTaskOrchestrationInput,
    ) -> Result<WorkTask, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let task = task_by_id(&transaction, &input.task_id)?;
        let project_id = project_id_for_task(&transaction, &input.task_id)?;
        let blocked_by = normalized_values(input.blocked_by);
        let depends_on = normalized_values(input.depends_on);
        let produces = normalized_values(input.produces);
        let consumes = normalized_values(input.consumes);
        let claimed_paths = normalized_paths(input.claimed_paths)?;
        validate_dependencies(
            &transaction,
            &project_id,
            &task.id,
            &blocked_by,
            &depends_on,
        )?;
        let timestamp = now();
        transaction.execute(
            "UPDATE tasks SET required_capabilities_json=?2,blocked_by_json=?3,
             depends_on_json=?4,produces_json=?5,consumes_json=?6,claimed_paths_json=?7,
             assignment_status=CASE WHEN assignment_status='suggested' THEN 'unassigned'
             ELSE assignment_status END,suggested_provider=NULL,route_reasons_json='[]' WHERE id=?1",
            params![
                input.task_id,
                serde_json::to_string(&input.required_capabilities)?,
                serde_json::to_string(&blocked_by)?,
                serde_json::to_string(&depends_on)?,
                serde_json::to_string(&produces)?,
                serde_json::to_string(&consumes)?,
                serde_json::to_string(&claimed_paths)?,
            ],
        )?;
        insert_event(
            &transaction,
            &project_id,
            None,
            EventKind::TaskAssignmentUpdated,
            EventSource::User,
            json!({"taskId": input.task_id, "orchestrationMetadataUpdated": true}),
            &timestamp,
        )?;
        let task = task_by_id(&transaction, &input.task_id)?;
        transaction.commit()?;
        Ok(task)
    }

    pub fn capability_profiles(
        &self,
        repository_path: &str,
    ) -> Result<Vec<AgentCapabilityProfile>, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let project_id = project_id_for_path(&transaction, repository_path)?;
        ensure_default_profiles(&transaction, &project_id)?;
        let profiles = profiles_for_project(&transaction, &project_id)?;
        transaction.commit()?;
        Ok(profiles)
    }

    pub fn update_capability_profile(
        &self,
        input: UpdateCapabilityProfileInput,
    ) -> Result<AgentCapabilityProfile, StorageError> {
        validate_required("profile name", &input.display_name)?;
        if !(512..=128_000).contains(&input.max_context_tokens) {
            return Err(StorageError::Validation(
                "max context tokens must be between 512 and 128000".into(),
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let project_id = project_id_for_path(&transaction, &input.repository_path)?;
        ensure_default_profiles(&transaction, &project_id)?;
        let timestamp = now();
        transaction.execute(
            "INSERT INTO agent_capability_profiles(id,project_id,provider,display_name,
             strengths_json,allowed_actions_json,max_context_tokens,enabled,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(project_id,provider) DO UPDATE SET display_name=excluded.display_name,
             strengths_json=excluded.strengths_json,allowed_actions_json=excluded.allowed_actions_json,
             max_context_tokens=excluded.max_context_tokens,enabled=excluded.enabled,
             updated_at=excluded.updated_at",
            params![
                new_id(),
                project_id,
                input.provider.as_db(),
                input.display_name.trim(),
                serde_json::to_string(&input.strengths)?,
                serde_json::to_string(&input.allowed_actions)?,
                input.max_context_tokens as i64,
                input.enabled as i64,
                timestamp,
            ],
        )?;
        insert_event(
            &transaction,
            &project_id,
            None,
            EventKind::CapabilityProfileUpdated,
            EventSource::User,
            json!({"provider": input.provider, "displayName": input.display_name.trim(),
                "enabled": input.enabled}),
            &timestamp,
        )?;
        let profile = profile_by_provider(&transaction, &project_id, &input.provider)?;
        transaction.commit()?;
        Ok(profile)
    }

    pub fn record_route_recommendation(
        &self,
        recommendation: &TaskRouteRecommendation,
    ) -> Result<WorkTask, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let project_id = project_id_for_task(&transaction, &recommendation.task_id)?;
        let timestamp = now();
        transaction.execute(
            "UPDATE tasks SET assignment_status='suggested',suggested_provider=?2,
             route_reasons_json=?3 WHERE id=?1 AND assignment_status IN ('unassigned','suggested','waiting')",
            params![
                recommendation.task_id,
                recommendation.provider.as_db(),
                serde_json::to_string(&recommendation.reasons)?,
            ],
        )?;
        insert_event(
            &transaction,
            &project_id,
            None,
            EventKind::TaskRouteSuggested,
            EventSource::System,
            json!({"taskId": recommendation.task_id, "provider": recommendation.provider,
                "score": recommendation.score, "reasons": recommendation.reasons}),
            &timestamp,
        )?;
        let task = task_by_id(&transaction, &recommendation.task_id)?;
        transaction.commit()?;
        Ok(task)
    }

    pub fn approve_task_assignment(
        &self,
        input: ApproveTaskAssignmentInput,
    ) -> Result<WorkTask, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let task = task_by_id(&transaction, &input.task_id)?;
        let project_id = project_id_for_task(&transaction, &input.task_id)?;
        ensure_default_profiles(&transaction, &project_id)?;
        let profile = profile_by_provider(&transaction, &project_id, &input.provider)?;
        if !profile.enabled {
            return Err(StorageError::Validation(
                "the selected agent capability profile is disabled".into(),
            ));
        }
        if matches!(task.status, TaskStatus::Completed | TaskStatus::Archived) {
            return Err(StorageError::Validation(
                "completed or archived tasks cannot be assigned".into(),
            ));
        }
        let timestamp = now();
        release_task_locks(&transaction, &project_id, &task.id, &timestamp)?;
        let active_locks = workspace_locks_for_project(&transaction, &project_id, true)?;
        for path in &task.claimed_paths {
            if let Some(existing) = active_locks.iter().find(|lock| {
                lock.task_id != task.id && path_patterns_overlap(path, &lock.path_pattern)
            }) {
                return Err(StorageError::WorkspaceLocked {
                    path: path.clone(),
                    provider: existing.provider.display_name(),
                });
            }
        }
        let status = dependency_assignment_status(&transaction, &task)?;
        transaction.execute(
            "UPDATE tasks SET assignment_status=?2,assigned_provider=?3,assigned_agent=?4 WHERE id=?1",
            params![
                task.id,
                status.as_db(),
                input.provider.as_db(),
                profile.display_name,
            ],
        )?;
        for path in &task.claimed_paths {
            transaction.execute(
                "INSERT INTO workspace_locks(id,project_id,task_id,session_id,provider,path_pattern,
                 status,acquired_at,released_at) VALUES (?1,?2,?3,NULL,?4,?5,'active',?6,NULL)",
                params![new_id(), project_id, task.id, input.provider.as_db(), path, timestamp],
            )?;
            insert_event(
                &transaction,
                &project_id,
                None,
                EventKind::WorkspaceLockAcquired,
                EventSource::User,
                json!({"taskId": task.id, "provider": input.provider, "pathPattern": path}),
                &timestamp,
            )?;
        }
        insert_event(
            &transaction,
            &project_id,
            None,
            EventKind::TaskAssigned,
            EventSource::User,
            json!({"taskId": task.id, "provider": input.provider, "status": status}),
            &timestamp,
        )?;
        let task = task_by_id(&transaction, &task.id)?;
        transaction.commit()?;
        Ok(task)
    }

    pub fn clear_task_assignment(&self, task_id: &str) -> Result<WorkTask, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let task = task_by_id(&transaction, task_id)?;
        if task.assignment_status == TaskAssignmentStatus::Running {
            return Err(StorageError::Validation(
                "end the running session before clearing its assignment".into(),
            ));
        }
        let project_id = project_id_for_task(&transaction, task_id)?;
        let timestamp = now();
        release_task_locks(&transaction, &project_id, task_id, &timestamp)?;
        transaction.execute(
            "UPDATE tasks SET assignment_status='unassigned',suggested_provider=NULL,
             assigned_provider=NULL,assigned_agent=NULL,route_reasons_json='[]' WHERE id=?1",
            params![task_id],
        )?;
        insert_event(
            &transaction,
            &project_id,
            None,
            EventKind::TaskAssignmentUpdated,
            EventSource::User,
            json!({"taskId": task_id, "status": "unassigned"}),
            &timestamp,
        )?;
        let task = task_by_id(&transaction, task_id)?;
        transaction.commit()?;
        Ok(task)
    }

    pub fn workspace_locks(
        &self,
        repository_path: &str,
    ) -> Result<Vec<WorkspaceLock>, StorageError> {
        let connection = self.connection()?;
        let project_id = project_id_for_path(&connection, repository_path)?;
        workspace_locks_for_project(&connection, &project_id, false)
    }

    pub(crate) fn task_workspaces(
        &self,
        repository_path: &str,
    ) -> Result<Vec<TaskWorkspace>, StorageError> {
        let connection = self.connection()?;
        let project_id = project_id_for_path(&connection, repository_path)?;
        task_workspaces_for_project(&connection, &project_id)
    }

    pub(crate) fn workspace(&self, id: &str) -> Result<TaskWorkspace, StorageError> {
        task_workspace_by_id(&self.connection()?, id)
    }

    pub(crate) fn workspace_validations(
        &self,
        repository_path: &str,
    ) -> Result<Vec<WorkspaceValidation>, StorageError> {
        let connection = self.connection()?;
        let project_id = project_id_for_path(&connection, repository_path)?;
        workspace_validations_for_project(&connection, &project_id)
    }

    pub(crate) fn create_task_workspace(
        &self,
        workspace: &TaskWorkspace,
    ) -> Result<TaskWorkspace, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if project_id_for_task(&transaction, &workspace.task_id)? != workspace.project_id {
            return Err(StorageError::Validation(
                "task workspace does not belong to the selected project".into(),
            ));
        }
        let live: Option<String> = transaction
            .query_row(
                "SELECT id FROM task_workspaces WHERE task_id=?1 AND status NOT IN ('merged','archived') LIMIT 1",
                params![workspace.task_id],
                |row| row.get(0),
            )
            .optional()?;
        if live.is_some() {
            return Err(StorageError::Validation(
                "this task already has a live isolated workspace".into(),
            ));
        }
        transaction.execute(
            "INSERT INTO task_workspaces(id,project_id,task_id,assigned_provider,branch,
             worktree_path,base_commit,head_commit,status,session_id,latest_validation_id,
             created_at,updated_at,merged_at,archived_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL,NULL,?10,?10,NULL,NULL)",
            params![
                workspace.id,
                workspace.project_id,
                workspace.task_id,
                workspace.assigned_provider.as_db(),
                workspace.branch,
                workspace.worktree_path,
                workspace.base_commit,
                workspace.head_commit,
                workspace.status.as_db(),
                workspace.created_at,
            ],
        )?;
        insert_event(
            &transaction,
            &workspace.project_id,
            None,
            EventKind::WorktreeReady,
            EventSource::System,
            json!({"workspaceId": workspace.id, "taskId": workspace.task_id,
                "provider": workspace.assigned_provider, "branch": workspace.branch,
                "worktreePath": workspace.worktree_path, "baseCommit": workspace.base_commit}),
            &workspace.created_at,
        )?;
        let saved = task_workspace_by_id(&transaction, &workspace.id)?;
        transaction.commit()?;
        Ok(saved)
    }

    pub(crate) fn bind_workspace_session(
        &self,
        workspace_id: &str,
        session_id: &str,
    ) -> Result<TaskWorkspace, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let workspace = task_workspace_by_id(&transaction, workspace_id)?;
        if workspace.status != TaskWorkspaceStatus::Ready {
            return Err(StorageError::Validation(
                "only a ready worktree can start an isolated session".into(),
            ));
        }
        let session_project: String = transaction.query_row(
            "SELECT project_id FROM agent_sessions WHERE id=?1",
            params![session_id],
            |row| row.get(0),
        )?;
        if session_project != workspace.project_id {
            return Err(StorageError::Validation(
                "isolated session does not belong to this workspace project".into(),
            ));
        }
        let timestamp = now();
        transaction.execute(
            "UPDATE task_workspaces SET status='active',session_id=?2,updated_at=?3 WHERE id=?1",
            params![workspace_id, session_id, timestamp],
        )?;
        let saved = task_workspace_by_id(&transaction, workspace_id)?;
        transaction.commit()?;
        Ok(saved)
    }

    pub(crate) fn save_workspace_validation(
        &self,
        validation: &WorkspaceValidation,
    ) -> Result<TaskWorkspace, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let workspace = task_workspace_by_id(&transaction, &validation.workspace_id)?;
        let workspace_status = match validation.status {
            WorkspaceValidationStatus::Passed => TaskWorkspaceStatus::ValidationPassed,
            WorkspaceValidationStatus::Failed => TaskWorkspaceStatus::ValidationFailed,
        };
        transaction.execute(
            "INSERT INTO workspace_validations(id,workspace_id,project_id,status,validated_head,
             snapshot_json,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                validation.id,
                validation.workspace_id,
                workspace.project_id,
                match validation.status {
                    WorkspaceValidationStatus::Passed => "passed",
                    WorkspaceValidationStatus::Failed => "failed",
                },
                validation.validated_head,
                serde_json::to_string(validation)?,
                validation.created_at,
            ],
        )?;
        transaction.execute(
            "UPDATE task_workspaces SET status=?2,head_commit=?3,latest_validation_id=?4,
             updated_at=?5 WHERE id=?1",
            params![
                validation.workspace_id,
                workspace_status.as_db(),
                validation.validated_head,
                validation.id,
                validation.created_at,
            ],
        )?;
        let event_kind = match validation.status {
            WorkspaceValidationStatus::Passed => EventKind::ValidationPassed,
            WorkspaceValidationStatus::Failed => EventKind::ValidationFailed,
        };
        insert_event(
            &transaction,
            &workspace.project_id,
            workspace.session_id.as_deref(),
            event_kind,
            EventSource::System,
            json!({"workspaceId": workspace.id, "taskId": workspace.task_id,
                "validationId": validation.id, "head": validation.validated_head,
                "changedFiles": validation.changed_files, "checks": validation.checks}),
            &validation.created_at,
        )?;
        let saved = task_workspace_by_id(&transaction, &validation.workspace_id)?;
        transaction.commit()?;
        Ok(saved)
    }

    pub(crate) fn mark_workspace_merge_assessment(
        &self,
        workspace_id: &str,
        head_commit: &str,
        mergeable: bool,
        detail: &str,
    ) -> Result<TaskWorkspace, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let workspace = task_workspace_by_id(&transaction, workspace_id)?;
        let status = if mergeable {
            TaskWorkspaceStatus::MergeReady
        } else {
            TaskWorkspaceStatus::MergeConflict
        };
        let timestamp = now();
        transaction.execute(
            "UPDATE task_workspaces SET status=?2,head_commit=?3,updated_at=?4 WHERE id=?1",
            params![workspace_id, status.as_db(), head_commit, timestamp],
        )?;
        insert_event(
            &transaction,
            &workspace.project_id,
            workspace.session_id.as_deref(),
            if mergeable {
                EventKind::MergeReady
            } else {
                EventKind::MergeConflictDetected
            },
            EventSource::Git,
            json!({"workspaceId": workspace.id, "taskId": workspace.task_id,
                "branch": workspace.branch, "head": head_commit, "detail": detail}),
            &timestamp,
        )?;
        let saved = task_workspace_by_id(&transaction, workspace_id)?;
        transaction.commit()?;
        Ok(saved)
    }

    pub(crate) fn mark_workspace_merged(
        &self,
        workspace_id: &str,
        merged_head: &str,
    ) -> Result<TaskWorkspace, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let workspace = task_workspace_by_id(&transaction, workspace_id)?;
        let timestamp = now();
        transaction.execute(
            "UPDATE task_workspaces SET status='merged',head_commit=?2,updated_at=?3,
             merged_at=?3 WHERE id=?1",
            params![workspace_id, merged_head, timestamp],
        )?;
        transaction.execute(
            "UPDATE tasks SET status='completed',assignment_status='completed',completed_at=?2
             WHERE id=?1",
            params![workspace.task_id, timestamp],
        )?;
        release_task_locks(
            &transaction,
            &workspace.project_id,
            &workspace.task_id,
            &timestamp,
        )?;
        insert_event(
            &transaction,
            &workspace.project_id,
            workspace.session_id.as_deref(),
            EventKind::MergeCompleted,
            EventSource::Git,
            json!({"workspaceId": workspace.id, "taskId": workspace.task_id,
                "branch": workspace.branch, "mergedHead": merged_head}),
            &timestamp,
        )?;
        insert_event(
            &transaction,
            &workspace.project_id,
            workspace.session_id.as_deref(),
            EventKind::TaskCompleted,
            EventSource::System,
            json!({"taskId": workspace.task_id, "workspaceId": workspace.id,
                "completion": "approvedWorktreeMerge"}),
            &timestamp,
        )?;
        refresh_waiting_assignments(&transaction, &workspace.project_id, &timestamp)?;
        let saved = task_workspace_by_id(&transaction, workspace_id)?;
        transaction.commit()?;
        Ok(saved)
    }

    pub(crate) fn archive_workspace(
        &self,
        workspace_id: &str,
    ) -> Result<TaskWorkspace, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let workspace = task_workspace_by_id(&transaction, workspace_id)?;
        if workspace.status != TaskWorkspaceStatus::Merged {
            return Err(StorageError::Validation(
                "only a merged workspace can be cleaned up".into(),
            ));
        }
        let timestamp = now();
        transaction.execute(
            "UPDATE task_workspaces SET status='archived',updated_at=?2,archived_at=?2 WHERE id=?1",
            params![workspace_id, timestamp],
        )?;
        insert_event(
            &transaction,
            &workspace.project_id,
            workspace.session_id.as_deref(),
            EventKind::WorktreeArchived,
            EventSource::System,
            json!({"workspaceId": workspace.id, "taskId": workspace.task_id,
                "branchPreserved": workspace.branch, "worktreePath": workspace.worktree_path}),
            &timestamp,
        )?;
        let saved = task_workspace_by_id(&transaction, workspace_id)?;
        transaction.commit()?;
        Ok(saved)
    }

    pub fn tasks_for_project(&self, repository_path: &str) -> Result<Vec<WorkTask>, StorageError> {
        let connection = self.connection()?;
        let project_id = project_id_for_path(&connection, repository_path)?;
        let mut statement = connection.prepare(&format!(
            "{} WHERE goal_id IN (SELECT id FROM goals WHERE project_id=?1) ORDER BY created_at,id",
            task_select()
        ))?;
        let records = statement
            .query_map(params![project_id], task_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn record_plan(&self, plan: &PlanVersion) -> Result<PlanVersion, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let goal_project = transaction
            .query_row(
                "SELECT project_id FROM goals WHERE id=?1",
                params![plan.goal_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::GoalNotFound(plan.goal_id.clone()))?;
        if goal_project != plan.project_id {
            return Err(StorageError::Validation(
                "a plan can only target a goal in the same project".into(),
            ));
        }
        let latest = transaction.query_row(
            "SELECT COALESCE(MAX(version),0) FROM planner_versions WHERE goal_id=?1",
            params![plan.goal_id],
            |row| row.get::<_, u32>(0),
        )?;
        if plan.version != latest + 1 {
            return Err(StorageError::Validation(format!(
                "plan version must be {}, received {}",
                latest + 1,
                plan.version
            )));
        }
        transaction.execute(
            "INSERT INTO planner_versions(id,project_id,goal_id,version,status,reason,
             parent_plan_id,snapshot_json,created_at,approved_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL)",
            params![
                plan.id,
                plan.project_id,
                plan.goal_id,
                plan.version,
                plan.status.as_db(),
                plan.reason,
                plan.parent_plan_id,
                serde_json::to_string(plan)?,
                plan.created_at,
            ],
        )?;
        let event = if plan.parent_plan_id.is_some() {
            EventKind::PlanReplanned
        } else if plan.status == PlanStatus::Invalid {
            EventKind::PlanValidationFailed
        } else {
            EventKind::PlanGenerated
        };
        insert_event(
            &transaction,
            &plan.project_id,
            None,
            event,
            EventSource::System,
            json!({"planId": plan.id, "goalId": plan.goal_id, "version": plan.version,
                "status": plan.status, "reason": plan.reason,
                "validationIssues": plan.validation_issues}),
            &plan.created_at,
        )?;
        if plan.parent_plan_id.is_some() {
            transaction.execute(
                "UPDATE planner_versions SET status='superseded' WHERE id=?1",
                params![plan.parent_plan_id],
            )?;
            if plan.tasks.iter().any(|task| task.recovery_for.is_some()) {
                insert_event(
                    &transaction,
                    &plan.project_id,
                    None,
                    EventKind::RecoveryTaskProposed,
                    EventSource::System,
                    json!({"planId": plan.id, "version": plan.version,
                        "recoveryTasks": plan.tasks.iter().filter(|task| task.recovery_for.is_some())
                            .map(|task| &task.id).collect::<Vec<_>>() }),
                    &plan.created_at,
                )?;
            }
            for task in plan.tasks.iter().filter(|task| {
                matches!(
                    task.status,
                    PlannedTaskStatus::Failed | PlannedTaskStatus::Blocked
                )
            }) {
                if let Some(task_id) = &task.materialized_task_id {
                    if task.status == PlannedTaskStatus::Failed {
                        transaction.execute(
                            "UPDATE tasks SET status='archived',assignment_status='blocked'
                             WHERE id=?1 AND status!='completed'",
                            params![task_id],
                        )?;
                    } else {
                        transaction.execute(
                            "UPDATE tasks SET assignment_status='blocked' WHERE id=?1
                             AND status NOT IN ('completed','archived')",
                            params![task_id],
                        )?;
                    }
                }
            }
        }
        transaction.commit()?;
        Ok(plan.clone())
    }

    pub fn plans_for_project(
        &self,
        repository_path: &str,
    ) -> Result<Vec<PlanVersion>, StorageError> {
        let connection = self.connection()?;
        let project_id = project_id_for_path(&connection, repository_path)?;
        let mut statement = connection.prepare(
            "SELECT snapshot_json,status,approved_at FROM planner_versions
             WHERE project_id=?1 ORDER BY created_at DESC,version DESC",
        )?;
        let plans = statement
            .query_map(params![project_id], plan_from_row)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)?;
        Ok(plans)
    }

    pub fn plan(&self, plan_id: &str) -> Result<PlanVersion, StorageError> {
        self.connection()?
            .query_row(
                "SELECT snapshot_json,status,approved_at FROM planner_versions WHERE id=?1",
                params![plan_id],
                plan_from_row,
            )
            .optional()?
            .ok_or_else(|| StorageError::PlanNotFound(plan_id.to_owned()))
    }

    pub fn approve_plan(&self, plan_id: &str) -> Result<PlanApprovalResult, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let mut plan = transaction
            .query_row(
                "SELECT snapshot_json,status,approved_at FROM planner_versions WHERE id=?1",
                params![plan_id],
                plan_from_row,
            )
            .optional()?
            .ok_or_else(|| StorageError::PlanNotFound(plan_id.to_owned()))?;
        if plan.status != PlanStatus::Valid {
            return Err(StorageError::Validation(
                "only a valid, unapproved plan can be approved".into(),
            ));
        }
        if plan
            .validation_issues
            .iter()
            .any(|issue| issue.severity == PlanIssueSeverity::Error)
        {
            return Err(StorageError::Validation(
                "the plan contains blocking validation errors".into(),
            ));
        }
        let timestamp = now();
        let mut task_ids = plan
            .tasks
            .iter()
            .filter_map(|task| {
                task.materialized_task_id
                    .as_ref()
                    .map(|id| (task.id.clone(), id.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        for task in &plan.tasks {
            task_ids.entry(task.id.clone()).or_insert_with(new_id);
        }
        for planned in &mut plan.tasks {
            let task_id = task_ids[&planned.id].clone();
            let depends_on = planned
                .depends_on
                .iter()
                .filter_map(|dependency| task_ids.get(dependency).cloned())
                .collect::<Vec<_>>();
            let claimed_paths = normalized_paths(planned.expected_files.clone())?;
            let suggested = planned.suggested_provider.as_ref().map(ProviderId::as_db);
            if planned.materialized_task_id.is_none() {
                transaction.execute(
                    "INSERT INTO tasks(id,goal_id,title,description,status,priority,assigned_agent,
                     source_agent,created_at,completed_at,assignment_status,suggested_provider,
                     assigned_provider,route_reasons_json,required_capabilities_json,blocked_by_json,
                     depends_on_json,produces_json,consumes_json,claimed_paths_json)
                     VALUES (?1,?2,?3,?4,'todo',?5,NULL,'planner',?6,NULL,?7,?8,NULL,?9,?10,
                     ?11,?11,?12,?13,?14)",
                    params![
                        task_id,
                        plan.goal_id,
                        planned.title,
                        planned.description,
                        planned.priority.as_db(),
                        timestamp,
                        if suggested.is_some() { "suggested" } else { "unassigned" },
                        suggested,
                        serde_json::to_string(&planned.suggestion_reasons)?,
                        serde_json::to_string(&planned.required_capabilities)?,
                        serde_json::to_string(&depends_on)?,
                        serde_json::to_string(&planned.produces)?,
                        serde_json::to_string(&planned.consumes)?,
                        serde_json::to_string(&claimed_paths)?,
                    ],
                )?;
                insert_event(
                    &transaction,
                    &plan.project_id,
                    None,
                    EventKind::TaskCreated,
                    EventSource::System,
                    json!({"taskId": task_id, "goalId": plan.goal_id, "title": planned.title,
                        "source": "approvedPlan", "planId": plan.id, "planTaskId": planned.id}),
                    &timestamp,
                )?;
            } else {
                transaction.execute(
                    "UPDATE tasks SET blocked_by_json=?2,depends_on_json=?2 WHERE id=?1",
                    params![task_id, serde_json::to_string(&depends_on)?],
                )?;
            }
            planned.materialized_task_id = Some(task_id);
            planned.status = PlannedTaskStatus::Materialized;
        }
        plan.status = PlanStatus::Approved;
        plan.approved_at = Some(timestamp.clone());
        transaction.execute(
            "UPDATE planner_versions SET status='approved',snapshot_json=?2,approved_at=?3 WHERE id=?1",
            params![plan.id, serde_json::to_string(&plan)?, timestamp],
        )?;
        insert_event(
            &transaction,
            &plan.project_id,
            None,
            EventKind::PlanApproved,
            EventSource::User,
            json!({"planId": plan.id, "goalId": plan.goal_id, "version": plan.version,
                "taskCount": plan.tasks.len()}),
            &timestamp,
        )?;
        let tasks = plan
            .tasks
            .iter()
            .filter_map(|task| task.materialized_task_id.as_deref())
            .map(|id| task_by_id(&transaction, id))
            .collect::<Result<Vec<_>, _>>()?;
        transaction.commit()?;
        Ok(PlanApprovalResult { plan, tasks })
    }

    pub fn record_environment_manifest_saved(
        &self,
        repository_path: &str,
        manifest_path: &str,
    ) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let project_id = project_id_for_path(&transaction, repository_path)?;
        let timestamp = now();
        insert_event(
            &transaction,
            &project_id,
            None,
            EventKind::EnvironmentManifestSaved,
            EventSource::User,
            json!({"path": manifest_path, "secretsStored": false}),
            &timestamp,
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn latest_verified_fingerprint(
        &self,
        project_id: &str,
    ) -> Result<Option<String>, StorageError> {
        Ok(self
            .connection()?
            .query_row(
                "SELECT environment_fingerprint FROM verification_suites
                 WHERE project_id=?1 AND status IN ('passed','waived')
                 ORDER BY created_at DESC LIMIT 1",
                params![project_id],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn record_environment_snapshot(
        &self,
        snapshot: &EnvironmentSnapshot,
        cache_root: &Path,
        cache_kinds: &[String],
    ) -> Result<Vec<EnvironmentCacheRecord>, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let known_project = transaction.query_row(
            "SELECT COUNT(*) FROM projects WHERE id=?1",
            params![snapshot.project_id],
            |row| row.get::<_, i64>(0),
        )?;
        if known_project == 0 {
            return Err(StorageError::ProjectIdNotFound(snapshot.project_id.clone()));
        }
        transaction.execute(
            "INSERT INTO environment_snapshots(id,project_id,fingerprint,status,snapshot_json,created_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                snapshot.id,
                snapshot.project_id,
                snapshot.fingerprint,
                environment_status_as_db(snapshot.status),
                serde_json::to_string(snapshot)?,
                snapshot.captured_at,
            ],
        )?;
        let invalidated = transaction.execute(
            "UPDATE environment_caches SET status='invalidated',updated_at=?3
             WHERE project_id=?1 AND fingerprint<>?2 AND status='valid'",
            params![
                snapshot.project_id,
                snapshot.fingerprint,
                snapshot.captured_at
            ],
        )?;
        if invalidated > 0 {
            insert_event(
                &transaction,
                &snapshot.project_id,
                None,
                EventKind::EnvironmentCacheInvalidated,
                EventSource::System,
                json!({"fingerprint": snapshot.fingerprint, "invalidated": invalidated}),
                &snapshot.captured_at,
            )?;
        }
        for kind in cache_kinds {
            let id = new_id();
            let path = cache_root
                .join(&snapshot.fingerprint)
                .join(cache_directory_name(kind))
                .to_string_lossy()
                .replace('\\', "/");
            transaction.execute(
                "INSERT INTO environment_caches(id,project_id,kind,fingerprint,path,status,updated_at)
                 VALUES (?1,?2,?3,?4,?5,'valid',?6)
                 ON CONFLICT(project_id,kind,fingerprint) DO UPDATE SET
                 path=excluded.path,status='valid',updated_at=excluded.updated_at",
                params![
                    id,
                    snapshot.project_id,
                    kind,
                    snapshot.fingerprint,
                    path,
                    snapshot.captured_at,
                ],
            )?;
        }
        insert_event(
            &transaction,
            &snapshot.project_id,
            None,
            if snapshot.status == EnvironmentStatus::EnvironmentDrift {
                EventKind::EnvironmentDriftDetected
            } else {
                EventKind::EnvironmentCaptured
            },
            EventSource::System,
            json!({"environmentId": snapshot.id, "fingerprint": snapshot.fingerprint,
                "profile": snapshot.profile, "status": snapshot.status,
                "worktree": snapshot.worktree, "secretsStored": false}),
            &snapshot.captured_at,
        )?;
        let caches = environment_caches_for_project(&transaction, &snapshot.project_id)?;
        transaction.commit()?;
        Ok(caches)
    }

    pub fn environment_caches(
        &self,
        repository_path: &str,
    ) -> Result<Vec<EnvironmentCacheRecord>, StorageError> {
        let connection = self.connection()?;
        let project_id = project_id_for_path(&connection, repository_path)?;
        environment_caches_for_project(&connection, &project_id)
    }

    pub fn save_verification_suite(
        &self,
        suite: &VerificationSuite,
    ) -> Result<VerificationSuite, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let prior_status = transaction
            .query_row(
                "SELECT status FROM verification_suites WHERE id=?1",
                params![suite.id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        transaction.execute(
            "INSERT INTO verification_suites(id,project_id,task_id,workspace_id,status,
             environment_id,environment_fingerprint,verified_head,snapshot_json,created_at,completed_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT(id) DO UPDATE SET status=excluded.status,
             environment_id=excluded.environment_id,
             environment_fingerprint=excluded.environment_fingerprint,
             verified_head=excluded.verified_head,snapshot_json=excluded.snapshot_json,
             completed_at=excluded.completed_at",
            params![
                suite.id,
                suite.project_id,
                suite.task_id,
                suite.workspace_id,
                suite.status.as_db(),
                suite.environment_id,
                suite.environment_fingerprint,
                suite.commit,
                serde_json::to_string(suite)?,
                suite.created_at,
                suite.completed_at,
            ],
        )?;
        let event_kind = match suite.status {
            VerificationStatus::Running if prior_status.is_none() => {
                Some(EventKind::VerificationStarted)
            }
            VerificationStatus::Passed => Some(EventKind::VerificationPassed),
            VerificationStatus::Failed => Some(EventKind::VerificationFailed),
            VerificationStatus::Waived => Some(EventKind::VerificationWaived),
            _ => None,
        };
        if let Some(kind) = event_kind {
            insert_event(
                &transaction,
                &suite.project_id,
                None,
                kind,
                EventSource::System,
                json!({"suiteId": suite.id, "taskId": suite.task_id,
                    "workspaceId": suite.workspace_id, "status": suite.status,
                    "environmentId": suite.environment_id,
                    "fingerprint": suite.environment_fingerprint,
                    "commit": suite.commit, "repairTaskId": suite.repair_task_id}),
                suite.completed_at.as_deref().unwrap_or(&suite.created_at),
            )?;
        }
        if suite.status != VerificationStatus::Running {
            for gate in &suite.gates {
                insert_event(
                    &transaction,
                    &suite.project_id,
                    None,
                    EventKind::VerificationGateCompleted,
                    EventSource::System,
                    json!({"suiteId": suite.id, "gateId": gate.id, "kind": gate.kind,
                        "required": gate.required, "status": gate.status,
                        "command": gate.command, "exitCode": gate.exit_code,
                        "durationMs": gate.duration_ms}),
                    gate.finished_at.as_deref().unwrap_or(&suite.created_at),
                )?;
            }
        }
        transaction.commit()?;
        Ok(suite.clone())
    }

    pub fn verification_suite(&self, suite_id: &str) -> Result<VerificationSuite, StorageError> {
        self.connection()?
            .query_row(
                "SELECT snapshot_json,status,completed_at FROM verification_suites WHERE id=?1",
                params![suite_id],
                verification_suite_from_row,
            )
            .optional()?
            .ok_or_else(|| StorageError::VerificationSuiteNotFound(suite_id.into()))
    }

    pub fn verification_suites(
        &self,
        repository_path: &str,
    ) -> Result<Vec<VerificationSuite>, StorageError> {
        let connection = self.connection()?;
        let project_id = project_id_for_path(&connection, repository_path)?;
        let mut statement = connection.prepare(
            "SELECT snapshot_json,status,completed_at FROM verification_suites
             WHERE project_id=?1 ORDER BY created_at DESC,id DESC",
        )?;
        let suites = statement
            .query_map(params![project_id], verification_suite_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(suites)
    }

    pub fn passing_verification_for_workspace(
        &self,
        workspace_id: &str,
        head: &str,
    ) -> Result<Option<VerificationSuite>, StorageError> {
        Ok(self
            .connection()?
            .query_row(
                "SELECT snapshot_json,status,completed_at FROM verification_suites
                 WHERE workspace_id=?1 AND verified_head=?2 AND status IN ('passed','waived')
                 ORDER BY created_at DESC LIMIT 1",
                params![workspace_id, head],
                verification_suite_from_row,
            )
            .optional()?)
    }

    pub fn record_repair_task_created(
        &self,
        project_id: &str,
        suite_id: &str,
        repair_task_id: &str,
    ) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let timestamp = now();
        insert_event(
            &transaction,
            project_id,
            None,
            EventKind::RepairTaskCreated,
            EventSource::System,
            json!({"suiteId": suite_id, "repairTaskId": repair_task_id}),
            &timestamp,
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn record_decision(&self, input: CreateDecisionInput) -> Result<Decision, StorageError> {
        validate_required("decision summary", &input.summary)?;
        validate_required("decision reason", &input.reason)?;
        validate_required("decision author", &input.created_by)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let project_id = project_id_for_path(&transaction, &input.repository_path)?;
        let supersedes = input.supersedes_decision_id.as_deref();
        if let Some(previous_id) = supersedes {
            let previous = transaction
                .query_row(
                    "SELECT project_id,status FROM decisions WHERE id=?1",
                    params![previous_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?
                .ok_or_else(|| StorageError::DecisionNotFound(previous_id.to_owned()))?;
            if previous.0 != project_id {
                return Err(StorageError::Validation(
                    "a decision can only supersede another decision in the same project".into(),
                ));
            }
            if previous.1 != DecisionStatus::Active.as_db() {
                return Err(StorageError::Validation(
                    "only an active decision can be superseded".into(),
                ));
            }
        }
        let id = new_id();
        let timestamp = now();
        transaction.execute(
            "INSERT INTO decisions(id,project_id,summary,reason,alternatives_json,affected_json,
             status,created_by,superseded_by,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,'active',?7,NULL,?8)",
            params![
                id,
                project_id,
                input.summary.trim(),
                input.reason.trim(),
                serde_json::to_string(&input.alternatives)?,
                serde_json::to_string(&input.affected)?,
                input.created_by.trim(),
                timestamp
            ],
        )?;
        if let Some(previous_id) = supersedes {
            transaction.execute(
                "UPDATE decisions SET status='superseded',superseded_by=?2 WHERE id=?1",
                params![previous_id, id],
            )?;
            let previous = decision_by_id(&transaction, previous_id)?;
            append_event(
                &transaction,
                &project_id,
                EventKind::DecisionSuperseded,
                EventSource::User,
                json!({"decisionId": previous_id, "supersededBy": id, "decision": previous}),
                &timestamp,
            )?;
        }
        let decision = decision_by_id(&transaction, &id)?;
        append_event(
            &transaction,
            &project_id,
            EventKind::DecisionRecorded,
            EventSource::User,
            json!({"decisionId": id, "summary": input.summary.trim(), "decision": &decision}),
            &timestamp,
        )?;
        transaction.commit()?;
        Ok(decision)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start_session(
        &self,
        project_repository_path: &str,
        working_directory: &str,
        provider: ProviderId,
        agent_name: &str,
        initial_state: &ProjectStatus,
        detected_pid: Option<u32>,
        automatic: bool,
        task_id: Option<&str>,
    ) -> Result<AgentSession, StorageError> {
        validate_required("agent name", agent_name)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let project_id = project_id_for_path(&transaction, project_repository_path)?;
        if task_id.is_none() && active_session_id(&transaction, &project_id)?.is_some() {
            return Err(StorageError::ActiveSessionExists(
                project_repository_path.to_owned(),
            ));
        }
        if let Some(task_id) = task_id {
            validate_routed_task_start(&transaction, &project_id, task_id, &provider)?;
        }
        let id = new_id();
        let timestamp = now();
        transaction.execute(
            "INSERT INTO agent_sessions(
               id,project_id,agent,provider,agent_name,status,working_directory,initial_git_head,
               final_git_head,initial_git_state_json,final_git_state_json,started_at,ended_at,
               summary,summary_status,detected_pid,automatic,task_id
             ) VALUES (?1,?2,?3,?4,?3,'active',?5,?6,NULL,?7,NULL,?8,NULL,'','',?9,?10,?11)",
            params![
                id,
                project_id,
                agent_name.trim(),
                provider.as_db(),
                working_directory,
                initial_state.head_commit,
                serde_json::to_string(initial_state)?,
                timestamp,
                detected_pid,
                automatic,
                task_id,
            ],
        )?;
        if let Some(task_id) = task_id {
            transaction.execute(
                "UPDATE tasks SET assignment_status='running',status='in_progress' WHERE id=?1",
                params![task_id],
            )?;
            transaction.execute(
                "UPDATE workspace_locks SET session_id=?2 WHERE task_id=?1 AND status='active'",
                params![task_id, id],
            )?;
        }
        insert_event(
            &transaction,
            &project_id,
            Some(&id),
            EventKind::SessionStarted,
            EventSource::System,
            json!({"provider": provider, "agentName": agent_name.trim(), "branch": initial_state.branch,
                "head": initial_state.head_commit, "detectedPid": detected_pid,
                "automatic": automatic, "taskId": task_id}),
            &timestamp,
        )?;
        let session = session_by_id(&transaction, &id)?;
        transaction.commit()?;
        Ok(session)
    }

    pub(crate) fn active_session(
        &self,
        repository_path: &str,
    ) -> Result<Option<AgentSession>, StorageError> {
        let connection = self.connection()?;
        let project_id = project_id_for_path(&connection, repository_path)?;
        connection
            .query_row(
                &format!(
                    "{} WHERE project_id=?1 AND status='active' ORDER BY started_at DESC LIMIT 1",
                    session_select()
                ),
                params![project_id],
                session_from_row,
            )
            .optional()
            .map_err(StorageError::from)
    }

    pub(crate) fn session(&self, id: &str) -> Result<AgentSession, StorageError> {
        session_by_id(&self.connection()?, id)
    }

    pub(crate) fn mark_session_idle(&self, id: &str) -> Result<AgentSession, StorageError> {
        let connection = self.connection()?;
        let session = session_by_id(&connection, id)?;
        if session.status != AgentSessionStatus::Active {
            return Err(StorageError::SessionNotActive(id.to_owned()));
        }
        insert_event(
            &connection,
            &session.project_id,
            Some(id),
            EventKind::SessionIdle,
            EventSource::System,
            json!({"agentName": session.agent_name}),
            &now(),
        )?;
        Ok(session)
    }

    pub(crate) fn record_session_delta(
        &self,
        session: &AgentSession,
        changes: &[crate::domain::FileChange],
        commits: &[String],
        final_state: &ProjectStatus,
    ) -> Result<(), StorageError> {
        let connection = self.connection()?;
        let timestamp = now();
        if session
            .initial_git_state
            .as_ref()
            .map(|state| state.branch.as_str())
            != Some(final_state.branch.as_str())
        {
            insert_event(
                &connection,
                &session.project_id,
                Some(&session.id),
                EventKind::BranchChanged,
                EventSource::Git,
                json!({"from": session.initial_git_state.as_ref().map(|state| &state.branch),
                    "to": final_state.branch}),
                &timestamp,
            )?;
        }
        for change in changes {
            let kind = match change.change_type {
                crate::domain::FileChangeType::Created => EventKind::FileCreated,
                crate::domain::FileChangeType::Deleted => EventKind::FileDeleted,
                crate::domain::FileChangeType::Modified
                | crate::domain::FileChangeType::Renamed => EventKind::FileModified,
            };
            insert_event(
                &connection,
                &session.project_id,
                Some(&session.id),
                kind,
                EventSource::Git,
                serde_json::to_value(change)?,
                &timestamp,
            )?;
        }
        for commit in commits {
            insert_event(
                &connection,
                &session.project_id,
                Some(&session.id),
                EventKind::CommitCreated,
                EventSource::Git,
                json!({"commit": commit}),
                &timestamp,
            )?;
        }
        Ok(())
    }

    pub(crate) fn finish_session(
        &self,
        id: &str,
        final_state: &ProjectStatus,
        summary_status: &str,
    ) -> Result<AgentSession, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let current = session_by_id(&transaction, id)?;
        if current.status != AgentSessionStatus::Active {
            return Err(StorageError::SessionNotActive(id.to_owned()));
        }
        let timestamp = now();
        transaction.execute(
            "UPDATE agent_sessions SET status='completed',final_git_head=?2,
             final_git_state_json=?3,ended_at=?4,summary=?5,summary_status=?5 WHERE id=?1",
            params![
                id,
                final_state.head_commit,
                serde_json::to_string(final_state)?,
                timestamp,
                summary_status.trim()
            ],
        )?;
        insert_event(
            &transaction,
            &current.project_id,
            Some(id),
            EventKind::SessionEnded,
            EventSource::System,
            json!({"agentName": current.agent_name, "branch": final_state.branch,
                "head": final_state.head_commit, "summaryStatus": summary_status.trim()}),
            &timestamp,
        )?;
        if let Some(task_id) = &current.task_id {
            let isolated_workspace: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM task_workspaces WHERE session_id=?1
                 AND status NOT IN ('merged','archived'))",
                params![id],
                |row| row.get::<_, i64>(0),
            )? != 0;
            let task_status: String = transaction.query_row(
                "SELECT status FROM tasks WHERE id=?1",
                params![task_id],
                |row| row.get(0),
            )?;
            let assignment = if task_status == TaskStatus::Completed.as_db() {
                TaskAssignmentStatus::Completed
            } else {
                TaskAssignmentStatus::Waiting
            };
            transaction.execute(
                "UPDATE tasks SET assignment_status=?2 WHERE id=?1",
                params![task_id, assignment.as_db()],
            )?;
            if !isolated_workspace {
                release_task_locks(&transaction, &current.project_id, task_id, &timestamp)?;
            }
        }
        transaction.execute(
            "UPDATE task_workspaces SET status='validation_pending',head_commit=?2,
             updated_at=?3 WHERE session_id=?1 AND status='active'",
            params![id, final_state.head_commit, timestamp],
        )?;
        let session = session_by_id(&transaction, id)?;
        transaction.commit()?;
        Ok(session)
    }

    pub(crate) fn save_handoff(&self, handoff: &HandoffSnapshot) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO handoffs(id,project_id,session_id,snapshot_json,created_at)
             VALUES (?1,?2,?3,?4,?5)",
            params![
                handoff.id,
                handoff.project_id,
                handoff.session_id,
                serde_json::to_string(handoff)?,
                handoff.created_at
            ],
        )?;
        insert_event(
            &transaction,
            &handoff.project_id,
            Some(&handoff.session_id),
            EventKind::HandoffGenerated,
            EventSource::System,
            json!({"handoffId": handoff.id, "changedFiles": handoff.changed_files.len(),
                "completedTasks": handoff.completed_tasks.len()}),
            &handoff.created_at,
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn record_context_generated(
        &self,
        project_id: &str,
        path: &str,
    ) -> Result<(), StorageError> {
        let connection = self.connection()?;
        let status_json: String = connection.query_row(
            "SELECT status_json FROM projects WHERE id=?1",
            params![project_id],
            |row| row.get(0),
        )?;
        let status: ProjectStatus = serde_json::from_str(&status_json)?;
        append_event(
            &connection,
            project_id,
            EventKind::ContextGenerated,
            EventSource::System,
            json!({"path": path, "head": status.head_commit, "branch": status.branch}),
            &now(),
        )
    }

    pub fn save_context_version(&self, version: &ContextVersion) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let known_project = transaction.query_row(
            "SELECT COUNT(*) FROM projects WHERE id=?1",
            params![version.project_id],
            |row| row.get::<_, i64>(0),
        )?;
        if known_project == 0 {
            return Err(StorageError::ProjectIdNotFound(version.project_id.clone()));
        }
        transaction.execute(
            "INSERT INTO context_versions(id,project_id,session_id,task_id,context_hash,
             commit_sha,branch,plan_id,plan_version,environment_fingerprint,snapshot_json,
             markdown,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                version.id,
                version.project_id,
                version.session_id,
                version.task_id,
                version.context_snapshot_hash,
                version.commit,
                version.branch,
                version.plan_id,
                version.plan_version,
                version.environment_fingerprint,
                version
                    .snapshot
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()?,
                version.markdown,
                version.created_at,
            ],
        )?;
        insert_event(
            &transaction,
            &version.project_id,
            version.session_id.as_deref(),
            EventKind::ContextGenerated,
            EventSource::System,
            json!({
                "contextVersionId": version.id,
                "contextSnapshotHash": version.context_snapshot_hash,
                "head": version.commit,
                "branch": version.branch,
                "taskId": version.task_id,
                "planId": version.plan_id,
                "planVersion": version.plan_version,
                "environmentFingerprint": version.environment_fingerprint,
            }),
            &version.created_at,
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn replay_events(&self, repository_path: &str) -> Result<Vec<ProjectEvent>, StorageError> {
        let connection = self.connection()?;
        let project_id = project_id_for_path(&connection, repository_path)?;
        let mut statement = connection.prepare(
            "SELECT id,project_id,session_id,kind,source,payload_json,created_at FROM events
             WHERE project_id=?1 ORDER BY rowid DESC",
        )?;
        let records = statement
            .query_map(params![project_id], event_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn replay_sessions(
        &self,
        repository_path: &str,
    ) -> Result<Vec<AgentSession>, StorageError> {
        let connection = self.connection()?;
        let project_id = project_id_for_path(&connection, repository_path)?;
        sessions_for_project(&connection, &project_id)
    }

    pub fn context_versions(
        &self,
        repository_path: &str,
    ) -> Result<Vec<ContextVersion>, StorageError> {
        let connection = self.connection()?;
        let project_id = project_id_for_path(&connection, repository_path)?;
        let mut statement = connection.prepare(
            "SELECT id,project_id,session_id,task_id,context_hash,commit_sha,branch,plan_id,
             plan_version,environment_fingerprint,snapshot_json,markdown,created_at
             FROM context_versions WHERE project_id=?1 ORDER BY created_at DESC,id DESC",
        )?;
        let records = statement
            .query_map(params![project_id], |row| {
                let snapshot_json: Option<String> = row.get(10)?;
                Ok(ContextVersion {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    session_id: row.get(2)?,
                    task_id: row.get(3)?,
                    context_snapshot_hash: row.get(4)?,
                    commit: row.get(5)?,
                    branch: row.get(6)?,
                    plan_id: row.get(7)?,
                    plan_version: row.get::<_, Option<i64>>(8)?.map(|value| value as u32),
                    environment_fingerprint: row.get(9)?,
                    snapshot: snapshot_json
                        .as_deref()
                        .map(|value| json_from_db(value, 10))
                        .transpose()?,
                    markdown: row.get(11)?,
                    created_at: row.get(12)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn environment_snapshots(
        &self,
        repository_path: &str,
    ) -> Result<Vec<EnvironmentSnapshot>, StorageError> {
        let connection = self.connection()?;
        let project_id = project_id_for_path(&connection, repository_path)?;
        let mut statement = connection.prepare(
            "SELECT snapshot_json FROM environment_snapshots WHERE project_id=?1
             ORDER BY created_at DESC,id DESC",
        )?;
        let records = statement
            .query_map(params![project_id], |row| {
                let value: String = row.get(0)?;
                json_from_db(&value, 0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn record_explanation(&self, record: &ExplanationRecord) -> Result<(), StorageError> {
        let connection = self.connection()?;
        let known_project = connection.query_row(
            "SELECT COUNT(*) FROM projects WHERE id=?1",
            params![record.project_id],
            |row| row.get::<_, i64>(0),
        )?;
        if known_project == 0 {
            return Err(StorageError::ProjectIdNotFound(record.project_id.clone()));
        }
        let category = serde_json::to_value(record.category)?
            .as_str()
            .unwrap_or("decision")
            .to_owned();
        connection.execute(
            "INSERT INTO explanation_records(id,project_id,category,subject_type,subject_id,
             decision,outcome,snapshot_json,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                record.id,
                record.project_id,
                category,
                record.subject_type,
                record.subject_id,
                record.decision,
                record.outcome,
                serde_json::to_string(record)?,
                record.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn audit_trail(
        &self,
        repository_path: &str,
    ) -> Result<Vec<ExplanationRecord>, StorageError> {
        let connection = self.connection()?;
        let project_id = project_id_for_path(&connection, repository_path)?;
        let mut statement = connection.prepare(
            "SELECT snapshot_json FROM explanation_records WHERE project_id=?1
             ORDER BY created_at DESC,rowid DESC LIMIT 1000",
        )?;
        let records = statement
            .query_map(params![project_id], |row| {
                let value: String = row.get(0)?;
                json_from_db(&value, 0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(records)
    }

    pub fn evaluate_conflicts(
        &self,
        repository_path: &str,
    ) -> Result<Vec<ConflictRecord>, StorageError> {
        let memory = self.project_memory(repository_path)?;
        self.persist_analysis(&memory.project.id, analyze(&memory))?;
        Ok(self.project_memory(repository_path)?.conflicts)
    }

    pub(crate) fn evaluate_project(&self, project_id: &str) -> Result<(), StorageError> {
        let path = self.connection()?.query_row(
            "SELECT path FROM projects WHERE id=?1",
            params![project_id],
            |row| row.get::<_, String>(0),
        )?;
        self.evaluate_conflicts(&path)?;
        Ok(())
    }

    pub(crate) fn project_id_for_task(&self, task_id: &str) -> Result<String, StorageError> {
        self.connection()?
            .query_row(
                "SELECT goals.project_id FROM tasks
                 JOIN goals ON goals.id=tasks.goal_id WHERE tasks.id=?1",
                params![task_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::TaskNotFound(task_id.to_owned()))
    }

    pub(crate) fn repository_for_project_id(
        &self,
        project_id: &str,
    ) -> Result<String, StorageError> {
        self.connection()?
            .query_row(
                "SELECT path FROM projects WHERE id=?1",
                params![project_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::ProjectIdNotFound(project_id.to_owned()))
    }

    pub(crate) fn repository_for_task(&self, task_id: &str) -> Result<String, StorageError> {
        self.connection()?
            .query_row(
                "SELECT projects.path FROM tasks JOIN goals ON goals.id=tasks.goal_id
                 JOIN projects ON projects.id=goals.project_id WHERE tasks.id=?1",
                params![task_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::TaskNotFound(task_id.to_owned()))
    }

    pub fn acknowledge_conflict(&self, id: &str) -> Result<ConflictRecord, StorageError> {
        self.set_conflict_status(id, ConflictStatus::Acknowledged)
    }

    pub fn resolve_conflict(&self, id: &str) -> Result<ConflictRecord, StorageError> {
        self.set_conflict_status(id, ConflictStatus::Resolved)
    }

    fn set_conflict_status(
        &self,
        id: &str,
        status: ConflictStatus,
    ) -> Result<ConflictRecord, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let current = conflict_by_id(&transaction, id)?;
        let timestamp = now();
        transaction.execute(
            "UPDATE conflicts SET status=?2,updated_at=?3,resolved_at=?4 WHERE id=?1",
            params![
                id,
                status.as_db(),
                timestamp,
                (status == ConflictStatus::Resolved).then_some(timestamp.as_str())
            ],
        )?;
        let event_kind = match status {
            ConflictStatus::Acknowledged => EventKind::ConflictAcknowledged,
            ConflictStatus::Resolved => EventKind::ConflictResolved,
            ConflictStatus::Open => EventKind::ConflictDetected,
        };
        append_event(
            &transaction,
            &current.project_id,
            event_kind,
            EventSource::User,
            json!({"conflictId": id, "kind": current.kind}),
            &timestamp,
        )?;
        let conflict = conflict_by_id(&transaction, id)?;
        transaction.commit()?;
        Ok(conflict)
    }

    fn persist_analysis(&self, project_id: &str, analysis: Analysis) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let timestamp = now();
        let existing_conflicts = {
            let mut statement = transaction
                .prepare("SELECT fingerprint,id,status FROM conflicts WHERE project_id=?1")?;
            let records = statement
                .query_map(params![project_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            records
        };
        let detected = analysis
            .conflicts
            .iter()
            .map(|item| item.fingerprint.clone())
            .collect::<std::collections::BTreeSet<_>>();

        for conflict in analysis.conflicts {
            let existing = existing_conflicts
                .iter()
                .find(|item| item.0 == conflict.fingerprint);
            let id = existing.map(|item| item.1.clone()).unwrap_or_else(new_id);
            transaction.execute(
                "INSERT INTO conflicts(
                   id,project_id,fingerprint,primary_session_id,related_session_id,kind,severity,
                   title,description,affected_paths_json,related_decision_ids_json,status,
                   detected_at,updated_at,resolved_at
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,'open',?12,?12,NULL)
                 ON CONFLICT(fingerprint) DO UPDATE SET
                   primary_session_id=excluded.primary_session_id,
                   related_session_id=excluded.related_session_id,kind=excluded.kind,
                   severity=excluded.severity,title=excluded.title,description=excluded.description,
                   affected_paths_json=excluded.affected_paths_json,
                   related_decision_ids_json=excluded.related_decision_ids_json,
                   updated_at=excluded.updated_at",
                params![
                    id,
                    project_id,
                    conflict.fingerprint,
                    conflict.primary_session_id,
                    conflict.related_session_id,
                    conflict.kind.as_db(),
                    conflict.severity.as_db(),
                    conflict.title,
                    conflict.description,
                    serde_json::to_string(&conflict.affected_paths)?,
                    serde_json::to_string(&conflict.related_decision_ids)?,
                    timestamp,
                ],
            )?;
            if existing.is_none() {
                insert_event(
                    &transaction,
                    project_id,
                    conflict.primary_session_id.as_deref(),
                    EventKind::ConflictDetected,
                    EventSource::System,
                    json!({"conflictId": id, "kind": conflict.kind,
                        "severity": conflict.severity, "title": conflict.title}),
                    &timestamp,
                )?;
            }
        }

        for (fingerprint, id, status) in existing_conflicts {
            if !detected.contains(&fingerprint) && status != ConflictStatus::Resolved.as_db() {
                transaction.execute(
                    "UPDATE conflicts SET status='resolved',updated_at=?2,resolved_at=?2 WHERE id=?1",
                    params![id, timestamp],
                )?;
                insert_event(
                    &transaction,
                    project_id,
                    None,
                    EventKind::ConflictResolved,
                    EventSource::System,
                    json!({"conflictId": id, "resolution": "condition no longer present"}),
                    &timestamp,
                )?;
            }
        }

        for freshness in analysis.freshness {
            let previous = transaction
                .query_row(
                    "SELECT status FROM freshness
                     WHERE project_id=?1 AND subject_type=?2 AND subject_id=?3",
                    params![project_id, freshness.subject_type, freshness.subject_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            transaction.execute(
                "INSERT INTO freshness(project_id,subject_type,subject_id,status,reason,
                   superseded_by,evaluated_at) VALUES (?1,?2,?3,?4,?5,?6,?7)
                 ON CONFLICT(project_id,subject_type,subject_id) DO UPDATE SET
                   status=excluded.status,reason=excluded.reason,
                   superseded_by=excluded.superseded_by,evaluated_at=excluded.evaluated_at",
                params![
                    project_id,
                    freshness.subject_type,
                    freshness.subject_id,
                    freshness.status.as_db(),
                    freshness.reason,
                    freshness.superseded_by,
                    timestamp,
                ],
            )?;
            if previous.as_deref() != Some(freshness.status.as_db()) {
                append_event(
                    &transaction,
                    project_id,
                    EventKind::FreshnessChanged,
                    EventSource::System,
                    json!({"subjectType": freshness.subject_type,
                        "subjectId": freshness.subject_id, "status": freshness.status,
                        "reason": freshness.reason}),
                    &timestamp,
                )?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    fn initialize(&self) -> Result<(), StorageError> {
        let mut connection = Connection::open(&self.database_path)?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        if table_exists(&connection, "projects")? && !column_exists(&connection, "projects", "id")?
        {
            migrate_v1(&mut connection)?;
        }
        create_schema(&connection)?;
        migrate_v3(&connection)?;
        migrate_v5(&connection)?;
        migrate_v7(&connection)?;
        migrate_v8(&connection)?;
        migrate_v11(&connection)?;
        migrate_v12(&connection)?;
        migrate_v13(&connection)?;
        migrate_v15(&connection)?;
        migrate_v16(&connection)?;
        migrate_v17(&connection)?;
        migrate_v19(&connection)?;
        connection.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)?;
        Ok(())
    }

    fn connection(&self) -> Result<Connection, StorageError> {
        let connection = Connection::open(&self.database_path)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(connection)
    }
}

fn prepare_migration_backup(database_path: &Path) -> Result<Option<PathBuf>, StorageError> {
    if !database_path.is_file() || fs::metadata(database_path)?.len() == 0 {
        return Ok(None);
    }
    let connection = Connection::open(database_path)?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version >= CURRENT_SCHEMA_VERSION {
        return Ok(None);
    }
    let backup_directory = database_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("migration-backups");
    fs::create_dir_all(&backup_directory)?;
    let backup_path = backup_directory.join(format!(
        "agentrelay-pre-v{CURRENT_SCHEMA_VERSION}-{}.db",
        Utc::now().format("%Y%m%dT%H%M%S%.3fZ")
    ));
    connection.execute(
        "VACUUM INTO ?1",
        params![backup_path.to_string_lossy().into_owned()],
    )?;
    Ok(Some(backup_path))
}

fn restore_migration_backup(database_path: &Path, backup_path: &Path) -> Result<(), StorageError> {
    let backup_directory = backup_path.parent().unwrap_or_else(|| Path::new("."));
    let failed_path = backup_directory.join(format!(
        "agentrelay-failed-v{CURRENT_SCHEMA_VERSION}-{}.db",
        Utc::now().format("%Y%m%dT%H%M%S%.3fZ")
    ));
    if database_path.is_file() {
        fs::copy(database_path, &failed_path)?;
    }
    remove_database_sidecar(database_path, "-wal")?;
    remove_database_sidecar(database_path, "-shm")?;
    fs::copy(backup_path, database_path)?;
    Ok(())
}

fn remove_database_sidecar(database_path: &Path, suffix: &str) -> Result<(), StorageError> {
    let path = PathBuf::from(format!("{}{suffix}", database_path.to_string_lossy()));
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(StorageError::Io(error)),
    }
}

fn migrate_v1(connection: &mut Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "PRAGMA foreign_keys=OFF; DROP TABLE IF EXISTS context_events;
         DROP TABLE IF EXISTS project_context; ALTER TABLE projects RENAME TO projects_v1;",
    )?;
    create_schema(connection)?;
    connection.execute_batch(
        "INSERT INTO projects(id,path,name,branch,last_commit,change_count,last_scanned_at,
         status_signature,status_json)
         SELECT lower(hex(randomblob(4)))||'-'||lower(hex(randomblob(2)))||'-'||
                lower(hex(randomblob(2)))||'-'||lower(hex(randomblob(2)))||'-'||
                lower(hex(randomblob(6))),path,path,branch,last_commit,change_count,
                last_scanned_at,'','{}' FROM projects_v1;
         DROP TABLE projects_v1; PRAGMA foreign_keys=ON;",
    )?;
    Ok(())
}

fn migrate_v3(connection: &Connection) -> Result<(), StorageError> {
    let session_columns = [
        ("provider", "TEXT NOT NULL DEFAULT 'unknown'"),
        ("agent_name", "TEXT NOT NULL DEFAULT ''"),
        ("working_directory", "TEXT NOT NULL DEFAULT ''"),
        ("initial_git_head", "TEXT NOT NULL DEFAULT ''"),
        ("final_git_head", "TEXT"),
        ("initial_git_state_json", "TEXT NOT NULL DEFAULT '{}'"),
        ("final_git_state_json", "TEXT"),
        ("summary_status", "TEXT NOT NULL DEFAULT ''"),
    ];
    for (column, definition) in session_columns {
        if !column_exists(connection, "agent_sessions", column)? {
            connection.execute_batch(&format!(
                "ALTER TABLE agent_sessions ADD COLUMN {column} {definition};"
            ))?;
        }
    }
    connection.execute(
        "UPDATE agent_sessions SET agent_name=agent WHERE agent_name=''",
        [],
    )?;
    if !column_exists(connection, "events", "session_id")? {
        connection.execute_batch(
            "ALTER TABLE events ADD COLUMN session_id TEXT REFERENCES agent_sessions(id) ON DELETE SET NULL;",
        )?;
    }
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS handoffs(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           session_id TEXT NOT NULL UNIQUE REFERENCES agent_sessions(id) ON DELETE CASCADE,
           snapshot_json TEXT NOT NULL,created_at TEXT NOT NULL);
         CREATE INDEX IF NOT EXISTS idx_events_session ON events(session_id,created_at);
         CREATE INDEX IF NOT EXISTS idx_handoffs_project ON handoffs(project_id,created_at);",
    )?;
    Ok(())
}

fn migrate_v5(connection: &Connection) -> Result<(), StorageError> {
    if !column_exists(connection, "projects", "tracking_enabled")? {
        connection.execute_batch(
            "ALTER TABLE projects ADD COLUMN tracking_enabled INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    if !column_exists(connection, "agent_sessions", "detected_pid")? {
        connection.execute_batch("ALTER TABLE agent_sessions ADD COLUMN detected_pid INTEGER;")?;
    }
    if !column_exists(connection, "agent_sessions", "automatic")? {
        connection.execute_batch(
            "ALTER TABLE agent_sessions ADD COLUMN automatic INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    Ok(())
}

fn migrate_v7(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS project_permissions(
           project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
           read_repository INTEGER NOT NULL DEFAULT 1,read_git INTEGER NOT NULL DEFAULT 1,
           write_memory INTEGER NOT NULL DEFAULT 1,write_context_files INTEGER NOT NULL DEFAULT 1,
           run_tests INTEGER NOT NULL DEFAULT 0,run_build INTEGER NOT NULL DEFAULT 0,
           modify_source INTEGER NOT NULL DEFAULT 0,git_commit INTEGER NOT NULL DEFAULT 0,
           git_push INTEGER NOT NULL DEFAULT 0,delete_files INTEGER NOT NULL DEFAULT 0,
           updated_at TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS action_runs(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           session_id TEXT REFERENCES agent_sessions(id) ON DELETE SET NULL,requested_by TEXT NOT NULL,
           action_kind TEXT NOT NULL,status TEXT NOT NULL,command_json TEXT NOT NULL DEFAULT '[]',
           sandbox TEXT,exit_code INTEGER,stdout TEXT NOT NULL DEFAULT '',stderr TEXT NOT NULL DEFAULT '',
           error TEXT,requested_at TEXT NOT NULL,started_at TEXT,finished_at TEXT);
         CREATE INDEX IF NOT EXISTS idx_action_runs_project ON action_runs(project_id,requested_at);"
    )?;
    Ok(())
}

fn migrate_v8(connection: &Connection) -> Result<(), StorageError> {
    let task_columns = [
        ("assignment_status", "TEXT NOT NULL DEFAULT 'unassigned'"),
        ("suggested_provider", "TEXT"),
        ("assigned_provider", "TEXT"),
        ("route_reasons_json", "TEXT NOT NULL DEFAULT '[]'"),
        ("required_capabilities_json", "TEXT NOT NULL DEFAULT '[]'"),
        ("blocked_by_json", "TEXT NOT NULL DEFAULT '[]'"),
        ("depends_on_json", "TEXT NOT NULL DEFAULT '[]'"),
        ("produces_json", "TEXT NOT NULL DEFAULT '[]'"),
        ("consumes_json", "TEXT NOT NULL DEFAULT '[]'"),
        ("claimed_paths_json", "TEXT NOT NULL DEFAULT '[]'"),
    ];
    for (column, definition) in task_columns {
        if !column_exists(connection, "tasks", column)? {
            connection.execute_batch(&format!(
                "ALTER TABLE tasks ADD COLUMN {column} {definition};"
            ))?;
        }
    }
    if !column_exists(connection, "agent_sessions", "task_id")? {
        connection.execute_batch("ALTER TABLE agent_sessions ADD COLUMN task_id TEXT;")?;
    }
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_capability_profiles(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           provider TEXT NOT NULL,display_name TEXT NOT NULL,strengths_json TEXT NOT NULL,
           allowed_actions_json TEXT NOT NULL,max_context_tokens INTEGER NOT NULL,
           enabled INTEGER NOT NULL DEFAULT 1,updated_at TEXT NOT NULL,
           UNIQUE(project_id,provider));
         CREATE TABLE IF NOT EXISTS workspace_locks(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
           session_id TEXT REFERENCES agent_sessions(id) ON DELETE SET NULL,provider TEXT NOT NULL,
           path_pattern TEXT NOT NULL,status TEXT NOT NULL DEFAULT 'active',
           acquired_at TEXT NOT NULL,released_at TEXT);
         CREATE INDEX IF NOT EXISTS idx_profiles_project ON agent_capability_profiles(project_id,enabled);
         CREATE INDEX IF NOT EXISTS idx_locks_project ON workspace_locks(project_id,status,task_id);",
    )?;
    Ok(())
}

fn migrate_v11(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS delivery_snapshots(
           project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           provider TEXT NOT NULL,repository_slug TEXT NOT NULL,snapshot_json TEXT NOT NULL,
           synced_at TEXT NOT NULL,PRIMARY KEY(project_id,provider));
         CREATE TABLE IF NOT EXISTS external_links(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           local_entity_type TEXT NOT NULL,local_entity_id TEXT NOT NULL,provider TEXT NOT NULL,
           external_entity_type TEXT NOT NULL,external_id TEXT NOT NULL,external_url TEXT NOT NULL,
           created_at TEXT NOT NULL,updated_at TEXT NOT NULL,
           UNIQUE(project_id,local_entity_type,local_entity_id,provider,external_entity_type,external_id));
         CREATE TABLE IF NOT EXISTS delivery_blockers(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,provider TEXT NOT NULL,
           pull_request_number INTEGER NOT NULL,check_id TEXT NOT NULL,summary TEXT NOT NULL,
           url TEXT,active INTEGER NOT NULL DEFAULT 1,detected_at TEXT NOT NULL,resolved_at TEXT,
           UNIQUE(project_id,provider,pull_request_number,check_id,task_id));
         CREATE INDEX IF NOT EXISTS idx_external_links_project
           ON external_links(project_id,provider,external_entity_type,external_id);
         CREATE INDEX IF NOT EXISTS idx_delivery_blockers_project
           ON delivery_blockers(project_id,active,task_id);",
    )?;
    Ok(())
}

fn migrate_v12(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS task_workspaces(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
           assigned_provider TEXT NOT NULL,branch TEXT NOT NULL,worktree_path TEXT NOT NULL UNIQUE,
           base_commit TEXT NOT NULL,head_commit TEXT NOT NULL,status TEXT NOT NULL,
           session_id TEXT REFERENCES agent_sessions(id) ON DELETE SET NULL,
           latest_validation_id TEXT,created_at TEXT NOT NULL,updated_at TEXT NOT NULL,
           merged_at TEXT,archived_at TEXT);
         CREATE TABLE IF NOT EXISTS workspace_validations(
           id TEXT PRIMARY KEY,workspace_id TEXT NOT NULL REFERENCES task_workspaces(id) ON DELETE CASCADE,
           project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,status TEXT NOT NULL,
           validated_head TEXT NOT NULL,snapshot_json TEXT NOT NULL,created_at TEXT NOT NULL);
         CREATE INDEX IF NOT EXISTS idx_task_workspaces_project
           ON task_workspaces(project_id,status,task_id);
         CREATE INDEX IF NOT EXISTS idx_workspace_validations_project
           ON workspace_validations(project_id,workspace_id,created_at);",
    )?;
    Ok(())
}

fn migrate_v13(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS planner_versions(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           goal_id TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,version INTEGER NOT NULL,
           status TEXT NOT NULL,reason TEXT NOT NULL,parent_plan_id TEXT,
           snapshot_json TEXT NOT NULL,created_at TEXT NOT NULL,approved_at TEXT,
           UNIQUE(goal_id,version));
         CREATE INDEX IF NOT EXISTS idx_planner_versions_project
           ON planner_versions(project_id,goal_id,version DESC);",
    )?;
    Ok(())
}

fn migrate_v15(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS environment_snapshots(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           fingerprint TEXT NOT NULL,status TEXT NOT NULL,snapshot_json TEXT NOT NULL,
           created_at TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS environment_caches(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           kind TEXT NOT NULL,fingerprint TEXT NOT NULL,path TEXT NOT NULL,status TEXT NOT NULL,
           updated_at TEXT NOT NULL,UNIQUE(project_id,kind,fingerprint));
         CREATE TABLE IF NOT EXISTS verification_suites(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
           workspace_id TEXT NOT NULL REFERENCES task_workspaces(id) ON DELETE CASCADE,
           status TEXT NOT NULL,environment_id TEXT NOT NULL,environment_fingerprint TEXT NOT NULL,
           verified_head TEXT NOT NULL,snapshot_json TEXT NOT NULL,created_at TEXT NOT NULL,
           completed_at TEXT);
         CREATE INDEX IF NOT EXISTS idx_environment_snapshots_project
           ON environment_snapshots(project_id,created_at DESC);
         CREATE INDEX IF NOT EXISTS idx_environment_caches_project
           ON environment_caches(project_id,status,kind);
         CREATE INDEX IF NOT EXISTS idx_verification_suites_project
           ON verification_suites(project_id,workspace_id,created_at DESC);",
    )?;
    Ok(())
}

fn migrate_v16(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS context_versions(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           session_id TEXT REFERENCES agent_sessions(id) ON DELETE SET NULL,
           task_id TEXT REFERENCES tasks(id) ON DELETE SET NULL,context_hash TEXT NOT NULL,
           commit_sha TEXT NOT NULL,branch TEXT NOT NULL,plan_id TEXT,plan_version INTEGER,
           environment_fingerprint TEXT,snapshot_json TEXT,markdown TEXT NOT NULL,created_at TEXT NOT NULL);
         CREATE INDEX IF NOT EXISTS idx_context_versions_project
           ON context_versions(project_id,created_at DESC,id DESC);
         CREATE INDEX IF NOT EXISTS idx_context_versions_session
           ON context_versions(session_id,created_at DESC);
         CREATE TRIGGER IF NOT EXISTS context_versions_no_update BEFORE UPDATE ON context_versions
           BEGIN SELECT RAISE(ABORT,'context versions are immutable'); END;
         CREATE TRIGGER IF NOT EXISTS context_versions_no_delete BEFORE DELETE ON context_versions
           BEGIN SELECT RAISE(ABORT,'context versions are immutable'); END;",
    )?;
    Ok(())
}

fn migrate_v17(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS explanation_records(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           category TEXT NOT NULL,subject_type TEXT NOT NULL,subject_id TEXT NOT NULL,
           decision TEXT NOT NULL,outcome TEXT NOT NULL,snapshot_json TEXT NOT NULL,
           created_at TEXT NOT NULL);
         CREATE INDEX IF NOT EXISTS idx_explanation_records_project
           ON explanation_records(project_id,created_at DESC,id DESC);
         CREATE INDEX IF NOT EXISTS idx_explanation_records_subject
           ON explanation_records(project_id,subject_type,subject_id,created_at DESC);
         CREATE TRIGGER IF NOT EXISTS explanation_records_no_update
           BEFORE UPDATE ON explanation_records
           BEGIN SELECT RAISE(ABORT,'explanation records are immutable'); END;
         CREATE TRIGGER IF NOT EXISTS explanation_records_no_delete
           BEFORE DELETE ON explanation_records
           BEGIN SELECT RAISE(ABORT,'explanation records are immutable'); END;",
    )?;
    Ok(())
}

fn migrate_v19(connection: &Connection) -> Result<(), StorageError> {
    if !column_exists(connection, "project_permissions", "run_tests_mode")? {
        connection.execute_batch(
            "ALTER TABLE project_permissions ADD COLUMN run_tests_mode TEXT NOT NULL DEFAULT 'deny';
             UPDATE project_permissions SET run_tests_mode=CASE WHEN run_tests=1 THEN 'allow' ELSE 'deny' END;",
        )?;
    }
    if !column_exists(connection, "project_permissions", "run_build_mode")? {
        connection.execute_batch(
            "ALTER TABLE project_permissions ADD COLUMN run_build_mode TEXT NOT NULL DEFAULT 'deny';
             UPDATE project_permissions SET run_build_mode=CASE WHEN run_build=1 THEN 'allow' ELSE 'deny' END;",
        )?;
    }
    Ok(())
}

fn create_schema(connection: &Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS projects(
           id TEXT PRIMARY KEY,path TEXT NOT NULL UNIQUE,name TEXT NOT NULL,branch TEXT NOT NULL,
           last_commit TEXT NOT NULL,change_count INTEGER NOT NULL DEFAULT 0,
           last_scanned_at TEXT NOT NULL,status_signature TEXT NOT NULL,status_json TEXT NOT NULL,
           tracking_enabled INTEGER NOT NULL DEFAULT 0);
         CREATE TABLE IF NOT EXISTS goals(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           title TEXT NOT NULL,description TEXT NOT NULL DEFAULT '',status TEXT NOT NULL
           CHECK(status IN ('active','completed','archived')),created_at TEXT NOT NULL,updated_at TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS tasks(
           id TEXT PRIMARY KEY,goal_id TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
           title TEXT NOT NULL,description TEXT NOT NULL DEFAULT '',status TEXT NOT NULL
           CHECK(status IN ('todo','in_progress','completed','archived')),priority TEXT NOT NULL
           CHECK(priority IN ('low','medium','high','critical')),assigned_agent TEXT,source_agent TEXT,
           created_at TEXT NOT NULL,completed_at TEXT,
           assignment_status TEXT NOT NULL DEFAULT 'unassigned',suggested_provider TEXT,
           assigned_provider TEXT,route_reasons_json TEXT NOT NULL DEFAULT '[]',
           required_capabilities_json TEXT NOT NULL DEFAULT '[]',blocked_by_json TEXT NOT NULL DEFAULT '[]',
           depends_on_json TEXT NOT NULL DEFAULT '[]',produces_json TEXT NOT NULL DEFAULT '[]',
           consumes_json TEXT NOT NULL DEFAULT '[]',claimed_paths_json TEXT NOT NULL DEFAULT '[]');
         CREATE TABLE IF NOT EXISTS decisions(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           summary TEXT NOT NULL,reason TEXT NOT NULL,alternatives_json TEXT NOT NULL DEFAULT '[]',
           affected_json TEXT NOT NULL DEFAULT '[]',status TEXT NOT NULL
           CHECK(status IN ('active','superseded','reversed')),created_by TEXT NOT NULL,
           superseded_by TEXT REFERENCES decisions(id),created_at TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS agent_sessions(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           agent TEXT NOT NULL DEFAULT '',provider TEXT NOT NULL DEFAULT 'unknown',
           agent_name TEXT NOT NULL DEFAULT '',status TEXT NOT NULL
           CHECK(status IN ('active','completed','interrupted')),working_directory TEXT NOT NULL DEFAULT '',
           initial_git_head TEXT NOT NULL DEFAULT '',final_git_head TEXT,
           initial_git_state_json TEXT NOT NULL DEFAULT '{}',final_git_state_json TEXT,
           started_at TEXT NOT NULL,ended_at TEXT,summary TEXT NOT NULL DEFAULT '',
           summary_status TEXT NOT NULL DEFAULT '',detected_pid INTEGER,
           automatic INTEGER NOT NULL DEFAULT 0,task_id TEXT);
         CREATE TABLE IF NOT EXISTS events(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           session_id TEXT REFERENCES agent_sessions(id) ON DELETE SET NULL,kind TEXT NOT NULL,
           source TEXT NOT NULL,payload_json TEXT NOT NULL,created_at TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS handoffs(
            id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            session_id TEXT NOT NULL UNIQUE REFERENCES agent_sessions(id) ON DELETE CASCADE,
            snapshot_json TEXT NOT NULL,created_at TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS conflicts(
            id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            fingerprint TEXT NOT NULL UNIQUE,
            primary_session_id TEXT REFERENCES agent_sessions(id) ON DELETE SET NULL,
            related_session_id TEXT REFERENCES agent_sessions(id) ON DELETE SET NULL,
            kind TEXT NOT NULL CHECK(kind IN ('file_overlap','decision_contradiction',
              'stale_context','completed_task_drift')),
            severity TEXT NOT NULL CHECK(severity IN ('info','warning','critical')),
            title TEXT NOT NULL,description TEXT NOT NULL,
            affected_paths_json TEXT NOT NULL DEFAULT '[]',
            related_decision_ids_json TEXT NOT NULL DEFAULT '[]',
            status TEXT NOT NULL CHECK(status IN ('open','acknowledged','resolved')),
            detected_at TEXT NOT NULL,updated_at TEXT NOT NULL,resolved_at TEXT);
         CREATE TABLE IF NOT EXISTS freshness(
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            subject_type TEXT NOT NULL,subject_id TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('current','stale','superseded','invalidated')),
            reason TEXT NOT NULL,superseded_by TEXT,evaluated_at TEXT NOT NULL,
            PRIMARY KEY(project_id,subject_type,subject_id));
         CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS project_permissions(
           project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
           read_repository INTEGER NOT NULL DEFAULT 1,read_git INTEGER NOT NULL DEFAULT 1,
           write_memory INTEGER NOT NULL DEFAULT 1,write_context_files INTEGER NOT NULL DEFAULT 1,
           run_tests INTEGER NOT NULL DEFAULT 0,run_build INTEGER NOT NULL DEFAULT 0,
           modify_source INTEGER NOT NULL DEFAULT 0,git_commit INTEGER NOT NULL DEFAULT 0,
           git_push INTEGER NOT NULL DEFAULT 0,delete_files INTEGER NOT NULL DEFAULT 0,
           run_tests_mode TEXT NOT NULL DEFAULT 'ask',run_build_mode TEXT NOT NULL DEFAULT 'ask',
           updated_at TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS action_runs(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           session_id TEXT REFERENCES agent_sessions(id) ON DELETE SET NULL,requested_by TEXT NOT NULL,
           action_kind TEXT NOT NULL,status TEXT NOT NULL,command_json TEXT NOT NULL DEFAULT '[]',
           sandbox TEXT,exit_code INTEGER,stdout TEXT NOT NULL DEFAULT '',stderr TEXT NOT NULL DEFAULT '',
           error TEXT,requested_at TEXT NOT NULL,started_at TEXT,finished_at TEXT);
         CREATE TABLE IF NOT EXISTS agent_capability_profiles(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           provider TEXT NOT NULL,display_name TEXT NOT NULL,strengths_json TEXT NOT NULL,
           allowed_actions_json TEXT NOT NULL,max_context_tokens INTEGER NOT NULL,
           enabled INTEGER NOT NULL DEFAULT 1,updated_at TEXT NOT NULL,UNIQUE(project_id,provider));
         CREATE TABLE IF NOT EXISTS workspace_locks(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
           session_id TEXT REFERENCES agent_sessions(id) ON DELETE SET NULL,provider TEXT NOT NULL,
           path_pattern TEXT NOT NULL,status TEXT NOT NULL DEFAULT 'active',
           acquired_at TEXT NOT NULL,released_at TEXT);
         CREATE TABLE IF NOT EXISTS delivery_snapshots(
           project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           provider TEXT NOT NULL,repository_slug TEXT NOT NULL,snapshot_json TEXT NOT NULL,
           synced_at TEXT NOT NULL,PRIMARY KEY(project_id,provider));
         CREATE TABLE IF NOT EXISTS external_links(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           local_entity_type TEXT NOT NULL,local_entity_id TEXT NOT NULL,provider TEXT NOT NULL,
           external_entity_type TEXT NOT NULL,external_id TEXT NOT NULL,external_url TEXT NOT NULL,
           created_at TEXT NOT NULL,updated_at TEXT NOT NULL,
           UNIQUE(project_id,local_entity_type,local_entity_id,provider,external_entity_type,external_id));
         CREATE TABLE IF NOT EXISTS delivery_blockers(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,provider TEXT NOT NULL,
           pull_request_number INTEGER NOT NULL,check_id TEXT NOT NULL,summary TEXT NOT NULL,
           url TEXT,active INTEGER NOT NULL DEFAULT 1,detected_at TEXT NOT NULL,resolved_at TEXT,
           UNIQUE(project_id,provider,pull_request_number,check_id,task_id));
         CREATE TABLE IF NOT EXISTS task_workspaces(
           id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
           task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
           assigned_provider TEXT NOT NULL,branch TEXT NOT NULL,worktree_path TEXT NOT NULL UNIQUE,
           base_commit TEXT NOT NULL,head_commit TEXT NOT NULL,status TEXT NOT NULL,
           session_id TEXT REFERENCES agent_sessions(id) ON DELETE SET NULL,
           latest_validation_id TEXT,created_at TEXT NOT NULL,updated_at TEXT NOT NULL,
           merged_at TEXT,archived_at TEXT);
         CREATE TABLE IF NOT EXISTS workspace_validations(
           id TEXT PRIMARY KEY,workspace_id TEXT NOT NULL REFERENCES task_workspaces(id) ON DELETE CASCADE,
           project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,status TEXT NOT NULL,
           validated_head TEXT NOT NULL,snapshot_json TEXT NOT NULL,created_at TEXT NOT NULL);
         CREATE INDEX IF NOT EXISTS idx_goals_project ON goals(project_id,status,created_at);
         CREATE INDEX IF NOT EXISTS idx_tasks_goal ON tasks(goal_id,status,priority,created_at);
         CREATE INDEX IF NOT EXISTS idx_decisions_project ON decisions(project_id,created_at);
         CREATE INDEX IF NOT EXISTS idx_events_project ON events(project_id,created_at);
         CREATE INDEX IF NOT EXISTS idx_sessions_project ON agent_sessions(project_id,started_at);
         CREATE INDEX IF NOT EXISTS idx_handoffs_project ON handoffs(project_id,created_at);
         CREATE INDEX IF NOT EXISTS idx_conflicts_project ON conflicts(project_id,status,updated_at);
         CREATE INDEX IF NOT EXISTS idx_freshness_project ON freshness(project_id,status,subject_type);
         CREATE INDEX IF NOT EXISTS idx_action_runs_project ON action_runs(project_id,requested_at);
         CREATE INDEX IF NOT EXISTS idx_profiles_project ON agent_capability_profiles(project_id,enabled);
         CREATE INDEX IF NOT EXISTS idx_locks_project ON workspace_locks(project_id,status,task_id);
         CREATE INDEX IF NOT EXISTS idx_external_links_project
           ON external_links(project_id,provider,external_entity_type,external_id);
         CREATE INDEX IF NOT EXISTS idx_delivery_blockers_project
           ON delivery_blockers(project_id,active,task_id);
         CREATE INDEX IF NOT EXISTS idx_task_workspaces_project
           ON task_workspaces(project_id,status,task_id);
         CREATE INDEX IF NOT EXISTS idx_workspace_validations_project
           ON workspace_validations(project_id,workspace_id,created_at);
         CREATE TRIGGER IF NOT EXISTS events_append_only_update BEFORE UPDATE ON events BEGIN
           SELECT RAISE(ABORT,'events are append-only'); END;
         CREATE TRIGGER IF NOT EXISTS events_append_only_delete BEFORE DELETE ON events BEGIN
           SELECT RAISE(ABORT,'events are append-only'); END;")?;
    Ok(())
}

fn apply_delivery_sync(
    connection: &Connection,
    project_id: &str,
    repository: ExternalRepository,
    issues: Vec<ExternalIssue>,
    pull_requests: Vec<PullRequestState>,
) -> Result<DeliverySyncReport, StorageError> {
    let provider = repository.provider.clone();
    let timestamp = now();
    let previous = delivery_snapshot_for_project(connection, project_id, &provider)?;
    let previous_issues = previous
        .as_ref()
        .map(|snapshot| {
            snapshot
                .issues
                .iter()
                .map(|issue| (issue.number, issue))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let previous_pull_requests = previous
        .as_ref()
        .map(|snapshot| {
            snapshot
                .pull_requests
                .iter()
                .map(|pull_request| (pull_request.number, pull_request))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let goal_id = ensure_delivery_goal(connection, project_id, &provider, &timestamp)?;
    let mut created_tasks = 0;
    let mut updated_tasks = 0;
    let mut created_links = 0;
    let mut blocked_task_ids = BTreeSet::new();
    let mut recorded_events = 0;
    let mut issue_tasks = BTreeMap::new();

    for issue in &issues {
        let external_id = issue.number.to_string();
        let task_id = external_link_local_id(
            connection,
            project_id,
            LocalEntityType::Task,
            &provider,
            ExternalEntityType::Issue,
            &external_id,
        )?;
        let (task_id, created) = match task_id {
            Some(task_id) => {
                update_issue_task(connection, &task_id, issue)?;
                updated_tasks += usize::from(previous_issues.get(&issue.number) != Some(&issue));
                (task_id, false)
            }
            None => {
                let task_id = create_delivery_task(
                    connection,
                    &goal_id,
                    &issue.title,
                    &issue_task_description(issue),
                    issue_priority(&issue.labels),
                    issue.assignees.first().map(String::as_str),
                    "github-issue",
                    issue.state == ExternalIssueState::Closed,
                    &timestamp,
                )?;
                created_tasks += 1;
                (task_id, true)
            }
        };
        created_links += usize::from(upsert_external_link(
            connection,
            project_id,
            LocalEntityType::Task,
            &task_id,
            &provider,
            ExternalEntityType::Issue,
            &external_id,
            &issue.url,
            &timestamp,
        )?);
        issue_tasks.insert(issue.number, task_id.clone());
        if created {
            append_event(
                connection,
                project_id,
                EventKind::TaskCreated,
                EventSource::GitHub,
                json!({"taskId": task_id, "goalId": goal_id, "title": issue.title,
                    "source": "github_issue", "issueNumber": issue.number}),
                &timestamp,
            )?;
            recorded_events += 1;
        }
        if created || previous_issues.get(&issue.number) != Some(&issue) {
            append_event(
                connection,
                project_id,
                EventKind::ExternalIssueUpdated,
                EventSource::GitHub,
                json!({"taskId": task_id, "issueNumber": issue.number, "title": issue.title,
                    "state": issue.state, "labels": issue.labels, "url": issue.url}),
                &timestamp,
            )?;
            recorded_events += 1;
        }
    }

    for pull_request in &pull_requests {
        let previous_pull_request = previous_pull_requests.get(&pull_request.number).copied();
        let mut linked_tasks = pull_request
            .linked_issue_numbers
            .iter()
            .filter_map(|number| issue_tasks.get(number).cloned())
            .collect::<BTreeSet<_>>();
        let tasks = tasks_for_project_connection(connection, project_id)?;
        for task in &tasks {
            if task.claimed_paths.iter().any(|pattern| {
                pull_request
                    .files
                    .iter()
                    .any(|path| path_patterns_overlap(pattern, path))
            }) {
                linked_tasks.insert(task.id.clone());
            }
        }

        for comment in &pull_request.review_comments {
            if comment.author.ends_with("[bot]") {
                continue;
            }
            let existing = external_link_local_id(
                connection,
                project_id,
                LocalEntityType::Task,
                &provider,
                ExternalEntityType::ReviewComment,
                &comment.id,
            )?;
            let task_id = match existing {
                Some(task_id) => task_id,
                None => {
                    let task_id = create_delivery_task(
                        connection,
                        &goal_id,
                        &review_task_title(&comment.body),
                        &review_task_description(pull_request.number, comment),
                        TaskPriority::Medium,
                        None,
                        "github-review",
                        false,
                        &timestamp,
                    )?;
                    created_tasks += 1;
                    append_event(
                        connection,
                        project_id,
                        EventKind::TaskCreated,
                        EventSource::GitHub,
                        json!({"taskId": task_id, "goalId": goal_id,
                            "title": review_task_title(&comment.body),
                            "source": "github_review", "pullRequestNumber": pull_request.number}),
                        &timestamp,
                    )?;
                    recorded_events += 1;
                    append_event(
                        connection,
                        project_id,
                        EventKind::ReviewCommentReceived,
                        EventSource::GitHub,
                        json!({"taskId": task_id, "pullRequestNumber": pull_request.number,
                            "commentId": comment.id, "author": comment.author,
                            "path": comment.path, "url": comment.url}),
                        &timestamp,
                    )?;
                    recorded_events += 1;
                    task_id
                }
            };
            created_links += usize::from(upsert_external_link(
                connection,
                project_id,
                LocalEntityType::Task,
                &task_id,
                &provider,
                ExternalEntityType::ReviewComment,
                &comment.id,
                &comment.url,
                &timestamp,
            )?);
            linked_tasks.insert(task_id);
        }

        for task_id in &linked_tasks {
            created_links += usize::from(upsert_external_link(
                connection,
                project_id,
                LocalEntityType::Task,
                task_id,
                &provider,
                ExternalEntityType::PullRequest,
                &pull_request.number.to_string(),
                &pull_request.url,
                &timestamp,
            )?);
        }
        for session in sessions_for_project(connection, project_id)? {
            if session_matches_pull_request(&session, pull_request) {
                created_links += usize::from(upsert_external_link(
                    connection,
                    project_id,
                    LocalEntityType::Session,
                    &session.id,
                    &provider,
                    ExternalEntityType::PullRequest,
                    &pull_request.number.to_string(),
                    &pull_request.url,
                    &timestamp,
                )?);
            }
        }

        let failing_checks = pull_request.failing_checks();
        let failing_ids = failing_checks
            .iter()
            .map(|check| check.id.as_str())
            .collect::<BTreeSet<_>>();
        for task_id in &linked_tasks {
            for check in &failing_checks {
                upsert_delivery_blocker(
                    connection,
                    project_id,
                    task_id,
                    &provider,
                    pull_request.number,
                    check,
                    &timestamp,
                )?;
                created_links += usize::from(upsert_external_link(
                    connection,
                    project_id,
                    LocalEntityType::Task,
                    task_id,
                    &provider,
                    ExternalEntityType::CiCheck,
                    &check.id,
                    check.details_url.as_deref().unwrap_or(&pull_request.url),
                    &timestamp,
                )?);
                if task_is_active(connection, task_id)? {
                    connection.execute(
                        "UPDATE tasks SET assignment_status='blocked' WHERE id=?1",
                        params![task_id],
                    )?;
                    blocked_task_ids.insert(task_id.clone());
                }
            }
        }
        resolve_delivery_blockers(
            connection,
            project_id,
            &provider,
            pull_request.number,
            &failing_ids,
            &timestamp,
        )?;

        if previous_pull_request != Some(pull_request) {
            append_event(
                connection,
                project_id,
                EventKind::ExternalPullRequestUpdated,
                EventSource::GitHub,
                json!({"pullRequestNumber": pull_request.number, "title": pull_request.title,
                    "status": pull_request.status, "branch": pull_request.branch,
                    "headRevision": pull_request.head_revision, "url": pull_request.url}),
                &timestamp,
            )?;
            recorded_events += 1;
        }
        if pull_request.is_ready()
            && previous_pull_request.is_none_or(|previous| !previous.is_ready())
        {
            append_event(
                connection,
                project_id,
                EventKind::PrReady,
                EventSource::GitHub,
                json!({"pullRequestNumber": pull_request.number, "url": pull_request.url}),
                &timestamp,
            )?;
            recorded_events += 1;
        }
        if pull_request.status == PullRequestStatus::Merged
            && previous_pull_request
                .is_none_or(|previous| previous.status != PullRequestStatus::Merged)
        {
            append_event(
                connection,
                project_id,
                EventKind::PrMerged,
                EventSource::GitHub,
                json!({"pullRequestNumber": pull_request.number, "headRevision": pull_request.head_revision,
                    "url": pull_request.url}),
                &timestamp,
            )?;
            recorded_events += 1;
            for task_id in &linked_tasks {
                complete_delivery_task(connection, task_id, &timestamp)?;
            }
        }
        recorded_events += record_ci_events(
            connection,
            project_id,
            pull_request,
            previous_pull_request,
            &timestamp,
        )?;
    }

    restore_resolved_delivery_tasks(connection, project_id, &timestamp)?;
    append_event(
        connection,
        project_id,
        EventKind::DeliverySynced,
        EventSource::GitHub,
        json!({"repository": repository.slug(), "issues": issues.len(),
            "pullRequests": pull_requests.len(), "blockedTasks": blocked_task_ids.len()}),
        &timestamp,
    )?;
    recorded_events += 1;

    let links = external_links_for_project(connection, project_id, &provider)?;
    let blockers = delivery_blockers_for_project(connection, project_id, false)?;
    let snapshot = DeliverySnapshot {
        project_id: project_id.into(),
        provider: provider.clone(),
        repository,
        issues,
        pull_requests,
        links,
        blockers,
        synced_at: timestamp.clone(),
    };
    connection.execute(
        "INSERT INTO delivery_snapshots(project_id,provider,repository_slug,snapshot_json,synced_at)
         VALUES (?1,?2,?3,?4,?5) ON CONFLICT(project_id,provider) DO UPDATE SET
         repository_slug=excluded.repository_slug,snapshot_json=excluded.snapshot_json,
         synced_at=excluded.synced_at",
        params![
            project_id,
            provider,
            snapshot.repository.slug(),
            serde_json::to_string(&snapshot)?,
            timestamp
        ],
    )?;
    Ok(DeliverySyncReport {
        snapshot,
        created_tasks,
        updated_tasks,
        created_links,
        blocked_tasks: blocked_task_ids.len(),
        recorded_events,
    })
}

fn ensure_delivery_goal(
    connection: &Connection,
    project_id: &str,
    provider: &str,
    timestamp: &str,
) -> Result<String, StorageError> {
    let id = format!("{project_id}:{provider}-delivery");
    connection.execute(
        "INSERT OR IGNORE INTO goals(id,project_id,title,description,status,created_at,updated_at)
         VALUES (?1,?2,?3,?4,'active',?5,?5)",
        params![
            id,
            project_id,
            format!("{} delivery", provider_name(provider)),
            format!(
                "Tasks synchronized from {provider}; local coordination remains authoritative."
            ),
            timestamp
        ],
    )?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
fn create_delivery_task(
    connection: &Connection,
    goal_id: &str,
    title: &str,
    description: &str,
    priority: TaskPriority,
    assigned_agent: Option<&str>,
    source_agent: &str,
    completed: bool,
    timestamp: &str,
) -> Result<String, StorageError> {
    let id = new_id();
    connection.execute(
        "INSERT INTO tasks(id,goal_id,title,description,status,priority,assigned_agent,
         source_agent,created_at,completed_at,assignment_status)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            id,
            goal_id,
            title.trim(),
            description.trim(),
            if completed { "completed" } else { "todo" },
            priority.as_db(),
            assigned_agent,
            source_agent,
            timestamp,
            completed.then_some(timestamp),
            if completed { "completed" } else { "unassigned" }
        ],
    )?;
    Ok(id)
}

fn update_issue_task(
    connection: &Connection,
    task_id: &str,
    issue: &ExternalIssue,
) -> Result<(), StorageError> {
    let current_status: String = connection.query_row(
        "SELECT status FROM tasks WHERE id=?1",
        params![task_id],
        |row| row.get(0),
    )?;
    let status = if issue.state == ExternalIssueState::Closed {
        "completed"
    } else if current_status == "completed" {
        "todo"
    } else {
        &current_status
    };
    connection.execute(
        "UPDATE tasks SET title=?2,description=?3,status=?4,priority=?5,assigned_agent=?6,
         completed_at=CASE WHEN ?4='completed' THEN COALESCE(completed_at,?7) ELSE NULL END,
         assignment_status=CASE WHEN ?4='completed' THEN 'completed'
           WHEN status='completed' THEN 'unassigned' ELSE assignment_status END WHERE id=?1",
        params![
            task_id,
            issue.title.trim(),
            issue_task_description(issue),
            status,
            issue_priority(&issue.labels).as_db(),
            issue.assignees.first(),
            now()
        ],
    )?;
    Ok(())
}

fn issue_task_description(issue: &ExternalIssue) -> String {
    let labels = if issue.labels.is_empty() {
        "none".into()
    } else {
        issue.labels.join(", ")
    };
    format!(
        "{}\n\nGitHub issue #{} · labels: {}\n{}",
        issue.description.trim(),
        issue.number,
        labels,
        issue.url
    )
    .trim()
    .to_owned()
}

fn issue_priority(labels: &[String]) -> TaskPriority {
    let labels = labels
        .iter()
        .map(|label| label.to_ascii_lowercase())
        .collect::<Vec<_>>();
    if labels
        .iter()
        .any(|label| label.contains("critical") || label.contains("security"))
    {
        TaskPriority::Critical
    } else if labels
        .iter()
        .any(|label| label.contains("high") || label.contains("bug"))
    {
        TaskPriority::High
    } else if labels
        .iter()
        .any(|label| label.contains("low") || label.contains("chore"))
    {
        TaskPriority::Low
    } else {
        TaskPriority::Medium
    }
}

fn review_task_title(body: &str) -> String {
    let first_line = body
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(body);
    let title = first_line.chars().take(88).collect::<String>();
    if title.trim().is_empty() {
        "Address pull-request review feedback".into()
    } else {
        title.trim().into()
    }
}

fn review_task_description(pull_request: u64, comment: &crate::domain::ReviewComment) -> String {
    let location = comment
        .path
        .as_ref()
        .map(|path| match comment.line {
            Some(line) => format!("{path}:{line}"),
            None => path.clone(),
        })
        .unwrap_or_else(|| "general review".into());
    format!(
        "{}\n\nSource: PR #{pull_request} review by {} · {location}\n{}",
        comment.body.trim(),
        comment.author,
        comment.url
    )
}

fn provider_name(provider: &str) -> String {
    let mut chars = provider.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
fn upsert_external_link(
    connection: &Connection,
    project_id: &str,
    local_entity_type: LocalEntityType,
    local_entity_id: &str,
    provider: &str,
    external_entity_type: ExternalEntityType,
    external_id: &str,
    external_url: &str,
    timestamp: &str,
) -> Result<bool, StorageError> {
    let inserted = connection.execute(
        "INSERT OR IGNORE INTO external_links(id,project_id,local_entity_type,local_entity_id,
         provider,external_entity_type,external_id,external_url,created_at,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)",
        params![
            new_id(),
            project_id,
            local_entity_type.as_db(),
            local_entity_id,
            provider,
            external_entity_type.as_db(),
            external_id,
            external_url,
            timestamp
        ],
    )? > 0;
    if !inserted {
        connection.execute(
            "UPDATE external_links SET external_url=?7,updated_at=?8 WHERE project_id=?1
             AND local_entity_type=?2 AND local_entity_id=?3 AND provider=?4
             AND external_entity_type=?5 AND external_id=?6",
            params![
                project_id,
                local_entity_type.as_db(),
                local_entity_id,
                provider,
                external_entity_type.as_db(),
                external_id,
                external_url,
                timestamp
            ],
        )?;
    }
    Ok(inserted)
}

fn external_link_local_id(
    connection: &Connection,
    project_id: &str,
    local_entity_type: LocalEntityType,
    provider: &str,
    external_entity_type: ExternalEntityType,
    external_id: &str,
) -> Result<Option<String>, StorageError> {
    Ok(connection
        .query_row(
            "SELECT local_entity_id FROM external_links WHERE project_id=?1
             AND local_entity_type=?2 AND provider=?3 AND external_entity_type=?4
             AND external_id=?5 ORDER BY created_at LIMIT 1",
            params![
                project_id,
                local_entity_type.as_db(),
                provider,
                external_entity_type.as_db(),
                external_id
            ],
            |row| row.get(0),
        )
        .optional()?)
}

fn tasks_for_project_connection(
    connection: &Connection,
    project_id: &str,
) -> Result<Vec<WorkTask>, StorageError> {
    let mut statement = connection.prepare(&format!(
        "{} WHERE goal_id IN (SELECT id FROM goals WHERE project_id=?1) ORDER BY created_at,id",
        task_select()
    ))?;
    let records = statement
        .query_map(params![project_id], task_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(records)
}

fn sessions_for_project(
    connection: &Connection,
    project_id: &str,
) -> Result<Vec<AgentSession>, StorageError> {
    let mut statement = connection.prepare(&format!(
        "{} WHERE project_id=?1 ORDER BY started_at DESC,id DESC",
        session_select()
    ))?;
    let records = statement
        .query_map(params![project_id], session_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(records)
}

fn session_matches_pull_request(session: &AgentSession, pull_request: &PullRequestState) -> bool {
    let revisions = [
        Some(session.initial_git_head.as_str()),
        session.final_git_head.as_deref(),
    ];
    revisions.into_iter().flatten().any(|revision| {
        revision == pull_request.head_revision
            || pull_request.commits.iter().any(|sha| sha == revision)
    }) || session
        .initial_git_state
        .as_ref()
        .is_some_and(|state| state.branch == pull_request.branch)
        || session
            .final_git_state
            .as_ref()
            .is_some_and(|state| state.branch == pull_request.branch)
}

fn task_is_active(connection: &Connection, task_id: &str) -> Result<bool, StorageError> {
    let status: String = connection.query_row(
        "SELECT status FROM tasks WHERE id=?1",
        params![task_id],
        |row| row.get(0),
    )?;
    Ok(matches!(status.as_str(), "todo" | "in_progress"))
}

fn mark_missing_workspace(
    connection: &Connection,
    workspace_id: &str,
    project_id: &str,
    task_id: Option<&str>,
    path: &str,
    timestamp: &str,
) -> Result<(), StorageError> {
    connection.execute(
        "UPDATE task_workspaces SET status='validation_failed',updated_at=?2 WHERE id=?1",
        params![workspace_id, timestamp],
    )?;
    if let Some(task_id) = task_id {
        connection.execute(
            "UPDATE tasks SET assignment_status='blocked' WHERE id=?1
             AND status NOT IN ('completed','archived')",
            params![task_id],
        )?;
        release_task_locks(connection, project_id, task_id, timestamp)?;
    }
    insert_event(
        connection,
        project_id,
        None,
        EventKind::ValidationFailed,
        EventSource::System,
        json!({
            "workspaceId": workspace_id,
            "worktreePath": path,
            "recovered": true,
            "reason": "isolated workspace was missing at startup"
        }),
        timestamp,
    )?;
    Ok(())
}

fn complete_delivery_task(
    connection: &Connection,
    task_id: &str,
    timestamp: &str,
) -> Result<(), StorageError> {
    connection.execute(
        "UPDATE tasks SET status='completed',assignment_status='completed',
         completed_at=COALESCE(completed_at,?2) WHERE id=?1",
        params![task_id, timestamp],
    )?;
    Ok(())
}

fn upsert_delivery_blocker(
    connection: &Connection,
    project_id: &str,
    task_id: &str,
    provider: &str,
    pull_request_number: u64,
    check: &CiCheck,
    timestamp: &str,
) -> Result<(), StorageError> {
    connection.execute(
        "INSERT INTO delivery_blockers(id,project_id,task_id,provider,pull_request_number,
         check_id,summary,url,active,detected_at,resolved_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,1,?9,NULL)
         ON CONFLICT(project_id,provider,pull_request_number,check_id,task_id) DO UPDATE SET
         summary=excluded.summary,url=excluded.url,active=1,resolved_at=NULL",
        params![
            new_id(),
            project_id,
            task_id,
            provider,
            pull_request_number,
            check.id,
            format!("PR #{pull_request_number}: {} failed", check.name),
            check.details_url,
            timestamp
        ],
    )?;
    Ok(())
}

fn resolve_delivery_blockers(
    connection: &Connection,
    project_id: &str,
    provider: &str,
    pull_request_number: u64,
    failing_ids: &BTreeSet<&str>,
    timestamp: &str,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        "SELECT id,check_id FROM delivery_blockers WHERE project_id=?1 AND provider=?2
         AND pull_request_number=?3 AND active=1",
    )?;
    let existing = statement
        .query_map(params![project_id, provider, pull_request_number], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    for (id, check_id) in existing {
        if !failing_ids.contains(check_id.as_str()) {
            connection.execute(
                "UPDATE delivery_blockers SET active=0,resolved_at=?2 WHERE id=?1",
                params![id, timestamp],
            )?;
        }
    }
    Ok(())
}

fn restore_resolved_delivery_tasks(
    connection: &Connection,
    project_id: &str,
    timestamp: &str,
) -> Result<(), StorageError> {
    let tasks = tasks_for_project_connection(connection, project_id)?;
    for task in tasks
        .into_iter()
        .filter(|task| task.assignment_status == TaskAssignmentStatus::Blocked)
    {
        let delivery_blocker_count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM delivery_blockers WHERE project_id=?1 AND task_id=?2",
            params![project_id, task.id],
            |row| row.get(0),
        )?;
        if delivery_blocker_count == 0 {
            continue;
        }
        let blocker_count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM delivery_blockers WHERE project_id=?1 AND task_id=?2 AND active=1",
            params![project_id, task.id],
            |row| row.get(0),
        )?;
        if blocker_count != 0 {
            continue;
        }
        let dependency_status = dependency_assignment_status(connection, &task)?;
        let status = match dependency_status {
            TaskAssignmentStatus::Blocked | TaskAssignmentStatus::Waiting => dependency_status,
            _ if task.assigned_provider.is_some() => TaskAssignmentStatus::Assigned,
            _ => TaskAssignmentStatus::Unassigned,
        };
        connection.execute(
            "UPDATE tasks SET assignment_status=?2 WHERE id=?1",
            params![task.id, status.as_db()],
        )?;
        insert_event(
            connection,
            project_id,
            None,
            EventKind::TaskAssignmentUpdated,
            EventSource::GitHub,
            json!({"taskId": task.id, "status": status,
                "reason": "GitHub CI blocker resolved"}),
            timestamp,
        )?;
    }
    Ok(())
}

fn record_ci_events(
    connection: &Connection,
    project_id: &str,
    pull_request: &PullRequestState,
    previous: Option<&PullRequestState>,
    timestamp: &str,
) -> Result<usize, StorageError> {
    let previous_checks = previous
        .map(|pull_request| {
            pull_request
                .ci_checks
                .iter()
                .map(|check| (check.id.as_str(), check))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let mut count = 0;
    for check in &pull_request.ci_checks {
        if previous_checks.get(check.id.as_str()).copied() == Some(check) {
            continue;
        }
        let kind = match (check.status, check.conclusion, check.category) {
            (CiStatus::Queued | CiStatus::InProgress, _, _) | (_, CiConclusion::Pending, _) => {
                EventKind::BuildStarted
            }
            (_, CiConclusion::Success, _) => EventKind::BuildPassed,
            (_, conclusion, CiCategory::Test) if conclusion.is_failure() => EventKind::TestFailed,
            (_, conclusion, CiCategory::Lint) if conclusion.is_failure() => EventKind::LintFailed,
            (_, conclusion, CiCategory::Security) if conclusion.is_failure() => {
                EventKind::SecurityCheckFailed
            }
            (_, conclusion, _) if conclusion.is_failure() => EventKind::BuildFailed,
            _ => EventKind::ExternalCiUpdated,
        };
        append_event(
            connection,
            project_id,
            kind,
            EventSource::GitHub,
            json!({"pullRequestNumber": pull_request.number, "checkId": check.id,
                "name": check.name, "category": check.category, "status": check.status,
                "conclusion": check.conclusion, "revision": check.revision,
                "url": check.details_url}),
            timestamp,
        )?;
        count += 1;
    }
    Ok(count)
}

fn delivery_snapshot_for_project(
    connection: &Connection,
    project_id: &str,
    provider: &str,
) -> Result<Option<DeliverySnapshot>, StorageError> {
    let value = connection
        .query_row(
            "SELECT snapshot_json FROM delivery_snapshots WHERE project_id=?1 AND provider=?2",
            params![project_id, provider],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    value
        .map(|value| serde_json::from_str(&value).map_err(StorageError::from))
        .transpose()
}

fn external_links_for_project(
    connection: &Connection,
    project_id: &str,
    provider: &str,
) -> Result<Vec<ExternalLink>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT id,project_id,local_entity_type,local_entity_id,provider,external_entity_type,
         external_id,external_url,created_at,updated_at FROM external_links
         WHERE project_id=?1 AND provider=?2 ORDER BY external_entity_type,external_id,created_at",
    )?;
    let records = statement
        .query_map(params![project_id, provider], external_link_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(records)
}

fn external_link_from_row(row: &Row<'_>) -> rusqlite::Result<ExternalLink> {
    let local_type: String = row.get(2)?;
    let external_type: String = row.get(5)?;
    Ok(ExternalLink {
        id: row.get(0)?,
        project_id: row.get(1)?,
        local_entity_type: LocalEntityType::from_db(&local_type)
            .ok_or_else(|| invalid_db_value(2, &local_type))?,
        local_entity_id: row.get(3)?,
        provider: row.get(4)?,
        external_entity_type: ExternalEntityType::from_db(&external_type)
            .ok_or_else(|| invalid_db_value(5, &external_type))?,
        external_id: row.get(6)?,
        external_url: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn delivery_blockers_for_project(
    connection: &Connection,
    project_id: &str,
    active_only: bool,
) -> Result<Vec<DeliveryBlocker>, StorageError> {
    let condition = if active_only { " AND active=1" } else { "" };
    let mut statement = connection.prepare(&format!(
        "SELECT id,task_id,pull_request_number,check_id,summary,url,active,detected_at,resolved_at
         FROM delivery_blockers WHERE project_id=?1{condition}
         ORDER BY active DESC,detected_at DESC,id DESC"
    ))?;
    let records = statement
        .query_map(params![project_id], delivery_blocker_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(records)
}

fn delivery_blocker_from_row(row: &Row<'_>) -> rusqlite::Result<DeliveryBlocker> {
    Ok(DeliveryBlocker {
        id: row.get(0)?,
        task_id: row.get(1)?,
        pull_request_number: row.get(2)?,
        check_id: row.get(3)?,
        summary: row.get(4)?,
        url: row.get(5)?,
        active: row.get::<_, i64>(6)? != 0,
        detected_at: row.get(7)?,
        resolved_at: row.get(8)?,
    })
}

fn append_event(
    connection: &Connection,
    project_id: &str,
    kind: EventKind,
    source: EventSource,
    payload: Value,
    created_at: &str,
) -> Result<(), StorageError> {
    let session_id = active_session_id(connection, project_id)?;
    insert_event(
        connection,
        project_id,
        session_id.as_deref(),
        kind,
        source,
        payload,
        created_at,
    )
}

fn insert_event(
    connection: &Connection,
    project_id: &str,
    session_id: Option<&str>,
    kind: EventKind,
    source: EventSource,
    payload: Value,
    created_at: &str,
) -> Result<(), StorageError> {
    connection.execute(
        "INSERT INTO events(id,project_id,session_id,kind,source,payload_json,created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            new_id(),
            project_id,
            session_id,
            kind.as_db(),
            source.as_db(),
            serde_json::to_string(&payload)?,
            created_at
        ],
    )?;
    Ok(())
}

fn active_session_id(
    connection: &Connection,
    project_id: &str,
) -> Result<Option<String>, StorageError> {
    Ok(connection
        .query_row(
            "SELECT id FROM agent_sessions WHERE project_id=?1 AND status='active'
             ORDER BY started_at DESC LIMIT 1",
            params![project_id],
            |row| row.get(0),
        )
        .optional()?)
}

fn session_select() -> &'static str {
    "SELECT id,project_id,provider,agent_name,status,working_directory,initial_git_head,
     final_git_head,initial_git_state_json,final_git_state_json,started_at,ended_at,summary_status,
     detected_pid,automatic,task_id
     FROM agent_sessions"
}

fn task_select() -> &'static str {
    "SELECT id,goal_id,title,description,status,priority,assigned_agent,source_agent,
     created_at,completed_at,assignment_status,suggested_provider,assigned_provider,
     route_reasons_json,required_capabilities_json,blocked_by_json,depends_on_json,
     produces_json,consumes_json,claimed_paths_json FROM tasks"
}

fn task_workspace_select() -> &'static str {
    "SELECT id,project_id,task_id,assigned_provider,branch,worktree_path,base_commit,
     head_commit,status,session_id,latest_validation_id,created_at,updated_at,merged_at,
     archived_at FROM task_workspaces"
}

fn task_workspace_by_id(connection: &Connection, id: &str) -> Result<TaskWorkspace, StorageError> {
    connection
        .query_row(
            &format!("{} WHERE id=?1", task_workspace_select()),
            params![id],
            task_workspace_from_row,
        )
        .optional()?
        .ok_or_else(|| StorageError::TaskWorkspaceNotFound(id.to_owned()))
}

fn task_workspaces_for_project(
    connection: &Connection,
    project_id: &str,
) -> Result<Vec<TaskWorkspace>, StorageError> {
    let mut statement = connection.prepare(&format!(
        "{} WHERE project_id=?1 ORDER BY created_at DESC,id DESC",
        task_workspace_select()
    ))?;
    let records = statement
        .query_map(params![project_id], task_workspace_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(records)
}

fn workspace_validations_for_project(
    connection: &Connection,
    project_id: &str,
) -> Result<Vec<WorkspaceValidation>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT snapshot_json FROM workspace_validations WHERE project_id=?1
         ORDER BY created_at DESC,id DESC",
    )?;
    let records = statement
        .query_map(params![project_id], |row| {
            let value: String = row.get(0)?;
            json_from_db(&value, 0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(records)
}

fn session_by_id(connection: &Connection, id: &str) -> Result<AgentSession, StorageError> {
    connection
        .query_row(
            &format!("{} WHERE id=?1", session_select()),
            params![id],
            session_from_row,
        )
        .optional()?
        .ok_or_else(|| StorageError::SessionNotFound(id.to_owned()))
}

fn project_id_for_path(connection: &Connection, path: &str) -> Result<String, StorageError> {
    connection
        .query_row(
            "SELECT id FROM projects WHERE path=?1",
            params![path],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| StorageError::ProjectNotFound(path.to_owned()))
}

fn goal_by_id(connection: &Connection, id: &str) -> Result<Goal, StorageError> {
    let mut goal = connection.query_row(
        "SELECT id,project_id,title,description,status,created_at,updated_at FROM goals WHERE id=?1",
        params![id], goal_from_row,
    ).optional()?.ok_or_else(|| StorageError::GoalNotFound(id.to_owned()))?;
    goal.tasks = tasks_for_goal(connection, id)?;
    Ok(goal)
}

fn task_by_id(connection: &Connection, id: &str) -> Result<WorkTask, StorageError> {
    connection
        .query_row(
            &format!("{} WHERE id=?1", task_select()),
            params![id],
            task_from_row,
        )
        .optional()?
        .ok_or_else(|| StorageError::TaskNotFound(id.to_owned()))
}

fn project_id_for_task(connection: &Connection, id: &str) -> Result<String, StorageError> {
    connection
        .query_row(
            "SELECT goals.project_id FROM tasks JOIN goals ON goals.id=tasks.goal_id WHERE tasks.id=?1",
            params![id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| StorageError::TaskNotFound(id.to_owned()))
}

fn ensure_default_profiles(connection: &Connection, project_id: &str) -> Result<(), StorageError> {
    let timestamp = now();
    for template in builtin_provider_profile_templates() {
        connection.execute(
            "INSERT OR IGNORE INTO agent_capability_profiles(id,project_id,provider,display_name,
             strengths_json,allowed_actions_json,max_context_tokens,enabled,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,1,?8)",
            params![
                new_id(),
                project_id,
                template.provider.as_db(),
                template.display_name,
                serde_json::to_string(&template.capabilities.strengths)?,
                serde_json::to_string(&template.capabilities.allowed_actions)?,
                template.capabilities.max_context_tokens,
                timestamp,
            ],
        )?;
    }
    Ok(())
}

fn profiles_for_project(
    connection: &Connection,
    project_id: &str,
) -> Result<Vec<AgentCapabilityProfile>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT id,project_id,provider,display_name,strengths_json,allowed_actions_json,
         max_context_tokens,enabled,updated_at FROM agent_capability_profiles
         WHERE project_id=?1 ORDER BY display_name,provider",
    )?;
    let records = statement
        .query_map(params![project_id], profile_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(records)
}

fn profile_by_provider(
    connection: &Connection,
    project_id: &str,
    provider: &ProviderId,
) -> Result<AgentCapabilityProfile, StorageError> {
    connection
        .query_row(
            "SELECT id,project_id,provider,display_name,strengths_json,allowed_actions_json,
             max_context_tokens,enabled,updated_at FROM agent_capability_profiles
             WHERE project_id=?1 AND provider=?2",
            params![project_id, provider.as_db()],
            profile_from_row,
        )
        .optional()?
        .ok_or_else(|| StorageError::CapabilityProfileNotFound(provider.display_name()))
}

fn workspace_locks_for_project(
    connection: &Connection,
    project_id: &str,
    active_only: bool,
) -> Result<Vec<WorkspaceLock>, StorageError> {
    let filter = if active_only {
        " AND status='active'"
    } else {
        ""
    };
    let mut statement = connection.prepare(&format!(
        "SELECT id,project_id,task_id,session_id,provider,path_pattern,status,acquired_at,released_at
         FROM workspace_locks WHERE project_id=?1{filter}
         ORDER BY CASE status WHEN 'active' THEN 0 ELSE 1 END,acquired_at DESC,id"
    ))?;
    let records = statement
        .query_map(params![project_id], workspace_lock_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(records)
}

fn release_task_locks(
    connection: &Connection,
    project_id: &str,
    task_id: &str,
    timestamp: &str,
) -> Result<(), StorageError> {
    let active = workspace_locks_for_project(connection, project_id, true)?
        .into_iter()
        .filter(|lock| lock.task_id == task_id)
        .collect::<Vec<_>>();
    connection.execute(
        "UPDATE workspace_locks SET status='released',released_at=?2
         WHERE task_id=?1 AND status='active'",
        params![task_id, timestamp],
    )?;
    for lock in active {
        insert_event(
            connection,
            project_id,
            lock.session_id.as_deref(),
            EventKind::WorkspaceLockReleased,
            EventSource::System,
            json!({"taskId": task_id, "pathPattern": lock.path_pattern}),
            timestamp,
        )?;
    }
    Ok(())
}

fn dependency_assignment_status(
    connection: &Connection,
    task: &WorkTask,
) -> Result<TaskAssignmentStatus, StorageError> {
    for dependency in &task.blocked_by {
        let status: String = connection.query_row(
            "SELECT status FROM tasks WHERE id=?1",
            params![dependency],
            |row| row.get(0),
        )?;
        if status != TaskStatus::Completed.as_db() {
            return Ok(TaskAssignmentStatus::Blocked);
        }
    }
    for dependency in &task.depends_on {
        let status: String = connection.query_row(
            "SELECT status FROM tasks WHERE id=?1",
            params![dependency],
            |row| row.get(0),
        )?;
        if status != TaskStatus::Completed.as_db() {
            return Ok(TaskAssignmentStatus::Waiting);
        }
    }
    Ok(TaskAssignmentStatus::Assigned)
}

fn refresh_waiting_assignments(
    connection: &Connection,
    project_id: &str,
    timestamp: &str,
) -> Result<(), StorageError> {
    let mut statement = connection.prepare(&format!(
        "{} WHERE goal_id IN (SELECT id FROM goals WHERE project_id=?1)
         AND status NOT IN ('completed','archived')
         AND assignment_status IN ('waiting','blocked')",
        task_select()
    ))?;
    let tasks = statement
        .query_map(params![project_id], task_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    for task in tasks {
        let status = dependency_assignment_status(connection, &task)?;
        if status != task.assignment_status {
            connection.execute(
                "UPDATE tasks SET assignment_status=?2 WHERE id=?1",
                params![task.id, status.as_db()],
            )?;
            insert_event(
                connection,
                project_id,
                None,
                EventKind::TaskAssignmentUpdated,
                EventSource::System,
                json!({"taskId": task.id, "status": status,
                    "reason": "dependency state changed"}),
                timestamp,
            )?;
        }
    }
    Ok(())
}

fn validate_routed_task_start(
    connection: &Connection,
    project_id: &str,
    task_id: &str,
    provider: &ProviderId,
) -> Result<(), StorageError> {
    let task = task_by_id(connection, task_id)?;
    if project_id_for_task(connection, task_id)? != project_id {
        return Err(StorageError::Validation(
            "routed task does not belong to this repository".into(),
        ));
    }
    if task.assigned_provider.as_ref() != Some(provider)
        || task.assignment_status != TaskAssignmentStatus::Assigned
    {
        return Err(StorageError::Validation(
            "task must be explicitly assigned to this provider before its session starts".into(),
        ));
    }
    if dependency_assignment_status(connection, &task)? != TaskAssignmentStatus::Assigned {
        return Err(StorageError::Validation(
            "task dependencies are not complete".into(),
        ));
    }
    let active: Option<String> = connection
        .query_row(
            "SELECT id FROM agent_sessions WHERE task_id=?1 AND status='active' LIMIT 1",
            params![task_id],
            |row| row.get(0),
        )
        .optional()?;
    if active.is_some() {
        return Err(StorageError::Validation(
            "this task already has an active session".into(),
        ));
    }
    Ok(())
}

fn validate_dependencies(
    connection: &Connection,
    project_id: &str,
    task_id: &str,
    blocked_by: &[String],
    depends_on: &[String],
) -> Result<(), StorageError> {
    let dependencies = blocked_by.iter().chain(depends_on).collect::<BTreeSet<_>>();
    if dependencies.contains(&task_id.to_owned()) {
        return Err(StorageError::Validation(
            "a task cannot depend on itself".into(),
        ));
    }
    for dependency in &dependencies {
        if project_id_for_task(connection, dependency)? != project_id {
            return Err(StorageError::Validation(
                "task dependencies must belong to the same project".into(),
            ));
        }
    }
    let mut statement = connection.prepare(&format!(
        "{} WHERE goal_id IN (SELECT id FROM goals WHERE project_id=?1)",
        task_select()
    ))?;
    let tasks = statement
        .query_map(params![project_id], task_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    let mut graph = tasks
        .into_iter()
        .map(|task| {
            let edges = task
                .depends_on
                .into_iter()
                .chain(task.blocked_by)
                .collect::<Vec<_>>();
            (task.id, edges)
        })
        .collect::<BTreeMap<_, _>>();
    graph.insert(
        task_id.to_owned(),
        dependencies.into_iter().cloned().collect(),
    );
    if has_dependency_cycle(&graph) {
        return Err(StorageError::Validation(
            "task dependencies cannot contain a cycle".into(),
        ));
    }
    Ok(())
}

fn has_dependency_cycle(graph: &BTreeMap<String, Vec<String>>) -> bool {
    fn visit(
        node: &str,
        graph: &BTreeMap<String, Vec<String>>,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
    ) -> bool {
        if visiting.contains(node) {
            return true;
        }
        if visited.contains(node) {
            return false;
        }
        visiting.insert(node.to_owned());
        if graph
            .get(node)
            .into_iter()
            .flatten()
            .any(|dependency| visit(dependency, graph, visiting, visited))
        {
            return true;
        }
        visiting.remove(node);
        visited.insert(node.to_owned());
        false
    }
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    graph
        .keys()
        .any(|node| visit(node, graph, &mut visiting, &mut visited))
}

fn normalized_values(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn normalized_paths(values: Vec<String>) -> Result<Vec<String>, StorageError> {
    let values = normalized_values(values);
    if values.iter().any(|value| {
        let normalized = value.replace('\\', "/");
        normalized.starts_with('/')
            || normalized.contains(":/")
            || normalized.split('/').any(|part| part == "..")
    }) {
        return Err(StorageError::Validation(
            "workspace lock paths must be repository-relative and cannot contain '..'".into(),
        ));
    }
    Ok(values)
}

fn decision_by_id(connection: &Connection, id: &str) -> Result<Decision, StorageError> {
    connection
        .query_row(
            "SELECT id,project_id,summary,reason,alternatives_json,affected_json,status,created_by,
         superseded_by,created_at FROM decisions WHERE id=?1",
            params![id],
            decision_from_row,
        )
        .optional()?
        .ok_or_else(|| StorageError::DecisionNotFound(id.to_owned()))
}

fn permissions_for_project(
    connection: &Connection,
    project_id: &str,
) -> Result<ProjectPermissions, StorageError> {
    connection.execute(
        "INSERT OR IGNORE INTO project_permissions(project_id,updated_at) VALUES (?1,?2)",
        params![project_id, now()],
    )?;
    connection
        .query_row(
            "SELECT project_id,read_repository,read_git,write_memory,write_context_files,
             run_tests,run_build,modify_source,git_commit,git_push,delete_files,updated_at,
             run_tests_mode,run_build_mode
             FROM project_permissions WHERE project_id=?1",
            params![project_id],
            permissions_from_row,
        )
        .map_err(StorageError::from)
}

fn action_run_by_id(connection: &Connection, id: &str) -> Result<ActionRun, StorageError> {
    connection
        .query_row(
            "SELECT id,project_id,session_id,requested_by,action_kind,status,command_json,
             sandbox,exit_code,stdout,stderr,error,requested_at,started_at,finished_at
             FROM action_runs WHERE id=?1",
            params![id],
            action_run_from_row,
        )
        .optional()?
        .ok_or_else(|| StorageError::ActionNotFound(id.to_owned()))
}

fn tasks_for_goal(connection: &Connection, goal_id: &str) -> Result<Vec<WorkTask>, StorageError> {
    let mut statement = connection.prepare(&format!(
        "{} WHERE goal_id=?1
         ORDER BY CASE status WHEN 'in_progress' THEN 0 WHEN 'todo' THEN 1 WHEN 'completed'
         THEN 2 ELSE 3 END, CASE priority WHEN 'critical' THEN 0 WHEN 'high' THEN 1
         WHEN 'medium' THEN 2 ELSE 3 END, created_at,id",
        task_select()
    ))?;
    let tasks = statement
        .query_map(params![goal_id], task_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(tasks)
}

fn project_from_row(row: &Row<'_>) -> rusqlite::Result<ProjectRecord> {
    let status_json: String = row.get(8)?;
    Ok(ProjectRecord {
        id: row.get(0)?,
        path: row.get(1)?,
        name: row.get(2)?,
        branch: row.get(3)?,
        last_commit: row.get(4)?,
        change_count: row.get::<_, i64>(5)? as usize,
        last_scanned_at: row.get(6)?,
        tracking_enabled: row.get::<_, i64>(7)? != 0,
        latest_status: if status_json == "{}" {
            None
        } else {
            Some(json_from_db(&status_json, 8)?)
        },
    })
}

fn goal_from_row(row: &Row<'_>) -> rusqlite::Result<Goal> {
    let status: String = row.get(4)?;
    Ok(Goal {
        id: row.get(0)?,
        project_id: row.get(1)?,
        title: row.get(2)?,
        description: row.get(3)?,
        status: GoalStatus::from_db(&status).ok_or_else(|| invalid_db_value(4, &status))?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        tasks: Vec::new(),
    })
}

fn task_from_row(row: &Row<'_>) -> rusqlite::Result<WorkTask> {
    let status: String = row.get(4)?;
    let priority: String = row.get(5)?;
    let assignment_status: String = row.get(10)?;
    let suggested_provider: Option<String> = row.get(11)?;
    let assigned_provider: Option<String> = row.get(12)?;
    Ok(WorkTask {
        id: row.get(0)?,
        goal_id: row.get(1)?,
        title: row.get(2)?,
        description: row.get(3)?,
        status: TaskStatus::from_db(&status).ok_or_else(|| invalid_db_value(4, &status))?,
        priority: TaskPriority::from_db(&priority).ok_or_else(|| invalid_db_value(5, &priority))?,
        assigned_agent: row.get(6)?,
        source_agent: row.get(7)?,
        created_at: row.get(8)?,
        completed_at: row.get(9)?,
        assignment_status: TaskAssignmentStatus::from_db(&assignment_status)
            .ok_or_else(|| invalid_db_value(10, &assignment_status))?,
        suggested_provider: suggested_provider
            .map(|value| ProviderId::from_db(&value).ok_or_else(|| invalid_db_value(11, &value)))
            .transpose()?,
        assigned_provider: assigned_provider
            .map(|value| ProviderId::from_db(&value).ok_or_else(|| invalid_db_value(12, &value)))
            .transpose()?,
        route_reasons: json_from_db(&row.get::<_, String>(13)?, 13)?,
        required_capabilities: json_from_db(&row.get::<_, String>(14)?, 14)?,
        blocked_by: json_from_db(&row.get::<_, String>(15)?, 15)?,
        depends_on: json_from_db(&row.get::<_, String>(16)?, 16)?,
        produces: json_from_db(&row.get::<_, String>(17)?, 17)?,
        consumes: json_from_db(&row.get::<_, String>(18)?, 18)?,
        claimed_paths: json_from_db(&row.get::<_, String>(19)?, 19)?,
    })
}

fn plan_from_row(row: &Row<'_>) -> rusqlite::Result<PlanVersion> {
    let snapshot: String = row.get(0)?;
    let status: String = row.get(1)?;
    let mut plan: PlanVersion = json_from_db(&snapshot, 0)?;
    plan.status = PlanStatus::from_db(&status).ok_or_else(|| invalid_db_value(1, &status))?;
    plan.approved_at = row.get(2)?;
    Ok(plan)
}

fn verification_suite_from_row(row: &Row<'_>) -> rusqlite::Result<VerificationSuite> {
    let snapshot: String = row.get(0)?;
    let status: String = row.get(1)?;
    let mut suite: VerificationSuite = json_from_db(&snapshot, 0)?;
    suite.status =
        VerificationStatus::from_db(&status).ok_or_else(|| invalid_db_value(1, &status))?;
    suite.completed_at = row.get(2)?;
    Ok(suite)
}

fn environment_caches_for_project(
    connection: &Connection,
    project_id: &str,
) -> Result<Vec<EnvironmentCacheRecord>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT id,project_id,kind,fingerprint,path,status,updated_at
         FROM environment_caches WHERE project_id=?1
         ORDER BY status DESC,kind,fingerprint DESC",
    )?;
    let records = statement
        .query_map(params![project_id], |row| {
            let status: String = row.get(5)?;
            Ok(EnvironmentCacheRecord {
                id: row.get(0)?,
                project_id: row.get(1)?,
                kind: row.get(2)?,
                fingerprint: row.get(3)?,
                path: row.get(4)?,
                status: match status.as_str() {
                    "valid" => CacheStatus::Valid,
                    "invalidated" => CacheStatus::Invalidated,
                    _ => return Err(invalid_db_value(5, &status)),
                },
                updated_at: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(records)
}

fn environment_status_as_db(status: EnvironmentStatus) -> &'static str {
    match status {
        EnvironmentStatus::Reproducible => "reproducible",
        EnvironmentStatus::EnvironmentDrift => "environment_drift",
        EnvironmentStatus::Incomplete => "incomplete",
        EnvironmentStatus::Unmanaged => "unmanaged",
    }
}

fn cache_directory_name(kind: &str) -> &str {
    match kind {
        "cargoRegistry" => "cargo-registry",
        "cargoTarget" => "cargo-target",
        "npm" => "npm",
        other => other,
    }
}

fn task_workspace_from_row(row: &Row<'_>) -> rusqlite::Result<TaskWorkspace> {
    let provider: String = row.get(3)?;
    let status: String = row.get(8)?;
    Ok(TaskWorkspace {
        id: row.get(0)?,
        project_id: row.get(1)?,
        task_id: row.get(2)?,
        assigned_provider: ProviderId::from_db(&provider)
            .ok_or_else(|| invalid_db_value(3, &provider))?,
        branch: row.get(4)?,
        worktree_path: row.get(5)?,
        base_commit: row.get(6)?,
        head_commit: row.get(7)?,
        status: TaskWorkspaceStatus::from_db(&status)
            .ok_or_else(|| invalid_db_value(8, &status))?,
        session_id: row.get(9)?,
        latest_validation_id: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        merged_at: row.get(13)?,
        archived_at: row.get(14)?,
    })
}

fn decision_from_row(row: &Row<'_>) -> rusqlite::Result<Decision> {
    let alternatives: String = row.get(4)?;
    let affected: String = row.get(5)?;
    let status: String = row.get(6)?;
    Ok(Decision {
        id: row.get(0)?,
        project_id: row.get(1)?,
        summary: row.get(2)?,
        reason: row.get(3)?,
        alternatives: json_from_db(&alternatives, 4)?,
        affected: json_from_db(&affected, 5)?,
        status: DecisionStatus::from_db(&status).ok_or_else(|| invalid_db_value(6, &status))?,
        created_by: row.get(7)?,
        superseded_by: row.get(8)?,
        created_at: row.get(9)?,
    })
}

fn conflict_by_id(connection: &Connection, id: &str) -> Result<ConflictRecord, StorageError> {
    connection
        .query_row(
            "SELECT id,project_id,fingerprint,primary_session_id,related_session_id,kind,severity,
             title,description,affected_paths_json,related_decision_ids_json,status,detected_at,
             updated_at,resolved_at FROM conflicts WHERE id=?1",
            params![id],
            conflict_from_row,
        )
        .optional()?
        .ok_or_else(|| StorageError::ConflictNotFound(id.to_owned()))
}

fn conflict_from_row(row: &Row<'_>) -> rusqlite::Result<ConflictRecord> {
    let kind: String = row.get(5)?;
    let severity: String = row.get(6)?;
    let paths: String = row.get(9)?;
    let decisions: String = row.get(10)?;
    let status: String = row.get(11)?;
    Ok(ConflictRecord {
        id: row.get(0)?,
        project_id: row.get(1)?,
        fingerprint: row.get(2)?,
        primary_session_id: row.get(3)?,
        related_session_id: row.get(4)?,
        kind: ConflictKind::from_db(&kind).ok_or_else(|| invalid_db_value(5, &kind))?,
        severity: ConflictSeverity::from_db(&severity)
            .ok_or_else(|| invalid_db_value(6, &severity))?,
        title: row.get(7)?,
        description: row.get(8)?,
        affected_paths: json_from_db(&paths, 9)?,
        related_decision_ids: json_from_db(&decisions, 10)?,
        status: ConflictStatus::from_db(&status).ok_or_else(|| invalid_db_value(11, &status))?,
        detected_at: row.get(12)?,
        updated_at: row.get(13)?,
        resolved_at: row.get(14)?,
    })
}

fn freshness_from_row(row: &Row<'_>) -> rusqlite::Result<FreshnessRecord> {
    let status: String = row.get(3)?;
    Ok(FreshnessRecord {
        project_id: row.get(0)?,
        subject_type: row.get(1)?,
        subject_id: row.get(2)?,
        status: FreshnessStatus::from_db(&status).ok_or_else(|| invalid_db_value(3, &status))?,
        reason: row.get(4)?,
        superseded_by: row.get(5)?,
        evaluated_at: row.get(6)?,
    })
}

fn event_from_row(row: &Row<'_>) -> rusqlite::Result<ProjectEvent> {
    let kind: String = row.get(3)?;
    let source: String = row.get(4)?;
    let payload: String = row.get(5)?;
    Ok(ProjectEvent {
        id: row.get(0)?,
        project_id: row.get(1)?,
        session_id: row.get(2)?,
        kind: EventKind::from_db(&kind).ok_or_else(|| invalid_db_value(3, &kind))?,
        source: EventSource::from_db(&source).ok_or_else(|| invalid_db_value(4, &source))?,
        payload: json_from_db(&payload, 5)?,
        created_at: row.get(6)?,
    })
}

fn permissions_from_row(row: &Row<'_>) -> rusqlite::Result<ProjectPermissions> {
    Ok(ProjectPermissions {
        project_id: row.get(0)?,
        read_repository: row.get::<_, i64>(1)? != 0,
        read_git: row.get::<_, i64>(2)? != 0,
        write_memory: row.get::<_, i64>(3)? != 0,
        write_context_files: row.get::<_, i64>(4)? != 0,
        run_tests: PermissionMode::from_db(&row.get::<_, String>(12)?),
        run_build: PermissionMode::from_db(&row.get::<_, String>(13)?),
        modify_source: row.get::<_, i64>(7)? != 0,
        git_commit: row.get::<_, i64>(8)? != 0,
        git_push: row.get::<_, i64>(9)? != 0,
        delete_files: row.get::<_, i64>(10)? != 0,
        updated_at: row.get(11)?,
    })
}

fn action_run_from_row(row: &Row<'_>) -> rusqlite::Result<ActionRun> {
    let action: String = row.get(4)?;
    let status: String = row.get(5)?;
    let command: String = row.get(6)?;
    Ok(ActionRun {
        id: row.get(0)?,
        project_id: row.get(1)?,
        session_id: row.get(2)?,
        requested_by: row.get(3)?,
        action: ActionKind::from_db(&action).ok_or_else(|| invalid_db_value(4, &action))?,
        status: ActionStatus::from_db(&status).ok_or_else(|| invalid_db_value(5, &status))?,
        command: json_from_db(&command, 6)?,
        sandbox: row.get(7)?,
        exit_code: row.get(8)?,
        stdout: row.get(9)?,
        stderr: row.get(10)?,
        error: row.get(11)?,
        requested_at: row.get(12)?,
        started_at: row.get(13)?,
        finished_at: row.get(14)?,
    })
}

fn profile_from_row(row: &Row<'_>) -> rusqlite::Result<AgentCapabilityProfile> {
    let provider: String = row.get(2)?;
    let strengths: String = row.get(4)?;
    let allowed_actions: String = row.get(5)?;
    Ok(AgentCapabilityProfile {
        id: row.get(0)?,
        project_id: row.get(1)?,
        provider: ProviderId::from_db(&provider).ok_or_else(|| invalid_db_value(2, &provider))?,
        display_name: row.get(3)?,
        strengths: json_from_db(&strengths, 4)?,
        allowed_actions: json_from_db(&allowed_actions, 5)?,
        max_context_tokens: row.get::<_, i64>(6)? as usize,
        enabled: row.get::<_, i64>(7)? != 0,
        updated_at: row.get(8)?,
    })
}

fn workspace_lock_from_row(row: &Row<'_>) -> rusqlite::Result<WorkspaceLock> {
    let provider: String = row.get(4)?;
    let status: String = row.get(6)?;
    Ok(WorkspaceLock {
        id: row.get(0)?,
        project_id: row.get(1)?,
        task_id: row.get(2)?,
        session_id: row.get(3)?,
        provider: ProviderId::from_db(&provider).ok_or_else(|| invalid_db_value(4, &provider))?,
        path_pattern: row.get(5)?,
        status: WorkspaceLockStatus::from_db(&status)
            .ok_or_else(|| invalid_db_value(6, &status))?,
        acquired_at: row.get(7)?,
        released_at: row.get(8)?,
    })
}

fn session_from_row(row: &Row<'_>) -> rusqlite::Result<AgentSession> {
    let provider: String = row.get(2)?;
    let status: String = row.get(4)?;
    let initial_state: String = row.get(8)?;
    let final_state: Option<String> = row.get(9)?;
    Ok(AgentSession {
        id: row.get(0)?,
        project_id: row.get(1)?,
        provider: ProviderId::from_db(&provider).ok_or_else(|| invalid_db_value(2, &provider))?,
        agent_name: row.get(3)?,
        status: AgentSessionStatus::from_db(&status).ok_or_else(|| invalid_db_value(4, &status))?,
        working_directory: row.get(5)?,
        initial_git_head: row.get(6)?,
        final_git_head: row.get(7)?,
        initial_git_state: if initial_state == "{}" {
            None
        } else {
            Some(json_from_db(&initial_state, 8)?)
        },
        final_git_state: final_state
            .map(|value| json_from_db(&value, 9))
            .transpose()?,
        started_at: row.get(10)?,
        ended_at: row.get(11)?,
        summary_status: row.get(12)?,
        detected_pid: row.get::<_, Option<i64>>(13)?.map(|pid| pid as u32),
        automatic: row.get::<_, i64>(14)? != 0,
        task_id: row.get(15)?,
    })
}

fn json_from_db<T: serde::de::DeserializeOwned>(value: &str, column: usize) -> rusqlite::Result<T> {
    serde_json::from_str(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn invalid_db_value(column: usize, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        column,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid AgentRelay database value: {value}"),
        )),
    )
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool, StorageError> {
    Ok(connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
            params![table],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn column_exists(connection: &Connection, table: &str, column: &str) -> Result<bool, StorageError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(columns.iter().any(|candidate| candidate == column))
}

fn scan_signature(status: &ProjectStatus) -> Result<String, StorageError> {
    Ok(serde_json::to_string(&json!({
        "branch": status.branch, "modifiedFiles": status.modified_files,
        "addedFiles": status.added_files, "deletedFiles": status.deleted_files,
        "recentCommits": status.recent_commits, "diffStat": status.diff_stat,
    }))?)
}

fn validate_required(field: &str, value: &str) -> Result<(), StorageError> {
    if value.trim().is_empty() {
        Err(StorageError::Validation(format!("{field} is required")))
    } else {
        Ok(())
    }
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn new_id() -> String {
    Uuid::new_v4().to_string()
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("unable to create AgentRelay data directory: {0}")]
    Io(#[from] std::io::Error),
    #[error("AgentRelay database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("AgentRelay data could not be serialized: {0}")]
    Json(#[from] serde_json::Error),
    #[error("database integrity check failed: {0}")]
    IntegrityCheckFailed(String),
    #[error("database migration failed and was rolled back from {backup}: {detail}")]
    MigrationFailed { detail: String, backup: String },
    #[error("project has not been scanned yet: {0}")]
    ProjectNotFound(String),
    #[error("project id not found: {0}")]
    ProjectIdNotFound(String),
    #[error("goal not found: {0}")]
    GoalNotFound(String),
    #[error("task not found: {0}")]
    TaskNotFound(String),
    #[error("decision not found: {0}")]
    DecisionNotFound(String),
    #[error("conflict not found: {0}")]
    ConflictNotFound(String),
    #[error("action not found: {0}")]
    ActionNotFound(String),
    #[error("agent capability profile not found: {0}")]
    CapabilityProfileNotFound(String),
    #[error("workspace path {path} is already claimed by {provider}")]
    WorkspaceLocked { path: String, provider: String },
    #[error("isolated workspace not found: {0}")]
    TaskWorkspaceNotFound(String),
    #[error("planner version not found: {0}")]
    PlanNotFound(String),
    #[error("verification suite not found: {0}")]
    VerificationSuiteNotFound(String),
    #[error("agent session not found: {0}")]
    SessionNotFound(String),
    #[error("agent session is no longer active: {0}")]
    SessionNotActive(String),
    #[error("an agent session is already active for project: {0}")]
    ActiveSessionExists(String),
    #[error("{0}")]
    Validation(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        ApproveTaskAssignmentInput, Capability, ConflictKind, ConflictStatus, CreateDecisionInput,
        CreateGoalInput, CreateTaskInput, EvidenceReference, ExecutionProfile, ExplanationCategory,
        ExplanationReason, FreshnessStatus, ProviderId, TaskAssignmentStatus, TaskPriority,
        UpdateTaskOrchestrationInput, VerificationGate, VerificationGateKind, VerificationImpact,
    };
    use std::time::Instant;
    use tempfile::TempDir;

    fn status(repository: &str) -> ProjectStatus {
        ProjectStatus {
            repository: repository.into(),
            branch: "main".into(),
            modified_files: vec!["src/lib.rs".into()],
            added_files: Vec::new(),
            deleted_files: Vec::new(),
            last_commit: "Initial commit".into(),
            head_commit: "abc1234".into(),
            recent_commits: vec!["abc123 Initial commit".into()],
            diff_stat: Vec::new(),
            scanned_at: "2026-09-19T10:00:00.000Z".into(),
        }
    }

    #[test]
    fn persists_memory_and_protects_append_only_events() {
        let directory = TempDir::new().unwrap();
        let store = ContextStore::open(directory.path().join("agentrelay.db")).unwrap();
        let repository = directory.path().to_string_lossy().replace('\\', "/");
        store.record_scan(&status(&repository)).unwrap();
        let goal = store
            .create_goal(CreateGoalInput {
                repository_path: repository.clone(),
                title: "Ship memory".into(),
                description: "SQLite is truth".into(),
            })
            .unwrap();
        store
            .create_task(CreateTaskInput {
                goal_id: goal.id,
                title: "Add schema".into(),
                description: String::new(),
                priority: TaskPriority::High,
                assigned_agent: Some("Codex".into()),
                source_agent: Some("user".into()),
            })
            .unwrap();
        let memory = store.project_memory(&repository).unwrap();
        assert_eq!(memory.goals[0].tasks.len(), 1);
        assert!(memory.events.len() >= 4);
        let explanation = ExplanationRecord {
            id: "audit-1".into(),
            project_id: memory.project.id.clone(),
            category: ExplanationCategory::Decision,
            subject_type: "decision".into(),
            subject_id: "decision-1".into(),
            decision: "Preserve structured evidence".into(),
            outcome: "recorded".into(),
            reasons: vec![ExplanationReason {
                code: "deterministic-rule".into(),
                summary: "SQLite is the source of truth".into(),
                detail: "The explanation stores the reason and its evidence references.".into(),
                score: None,
            }],
            evidence_refs: vec![EvidenceReference {
                kind: "project".into(),
                id: memory.project.id,
                label: "Test project".into(),
            }],
            created_at: "2026-09-29T10:00:00.000Z".into(),
        };
        store.record_explanation(&explanation).unwrap();
        assert_eq!(store.audit_trail(&repository).unwrap(), vec![explanation]);
        let connection = store.connection().unwrap();
        let event_id: String = connection
            .query_row("SELECT id FROM events LIMIT 1", [], |row| row.get(0))
            .unwrap();
        assert!(connection
            .execute("DELETE FROM events WHERE id=?1", params![event_id])
            .is_err());
        assert!(connection
            .execute(
                "UPDATE explanation_records SET outcome='changed' WHERE id='audit-1'",
                [],
            )
            .is_err());
        assert!(connection
            .execute("DELETE FROM explanation_records WHERE id='audit-1'", [])
            .is_err());
    }

    #[test]
    fn remembers_the_last_repository() {
        let directory = TempDir::new().unwrap();
        let store = ContextStore::open(directory.path().join("agentrelay.db")).unwrap();
        store.record_scan(&status("C:/work/demo")).unwrap();
        assert_eq!(
            store.last_repository().unwrap().as_deref(),
            Some("C:/work/demo")
        );
    }

    #[test]
    fn tracks_projects_explicitly_and_bounds_the_idle_timeout() {
        let directory = TempDir::new().unwrap();
        let store = ContextStore::open(directory.path().join("agentrelay.db")).unwrap();
        store.record_scan(&status("C:/work/tracked")).unwrap();
        assert!(store.tracked_projects().unwrap().is_empty());

        let project = store.set_project_tracking("C:/work/tracked", true).unwrap();
        assert!(project.tracking_enabled);
        assert_eq!(store.tracked_projects().unwrap().len(), 1);
        assert_eq!(store.set_idle_timeout_minutes(0).unwrap(), 1);
        assert_eq!(store.idle_timeout_minutes().unwrap(), 1);
        assert_eq!(store.set_idle_timeout_minutes(999).unwrap(), 240);
    }

    #[test]
    fn delivery_sync_links_issues_reviews_and_ci_blockers_without_duplicates() {
        let directory = TempDir::new().unwrap();
        let store = ContextStore::open(directory.path().join("agentrelay.db")).unwrap();
        let repository_path = directory.path().to_string_lossy().replace('\\', "/");
        store.record_scan(&status(&repository_path)).unwrap();
        let repository = ExternalRepository {
            provider: "github".into(),
            owner: "agentrelay".into(),
            name: "agentrelay".into(),
            web_url: "https://github.com/agentrelay/agentrelay".into(),
        };
        let issue = ExternalIssue {
            id: "issue-node-42".into(),
            number: 42,
            title: "Persist session timeout".into(),
            description: "Keep timeout state durable.".into(),
            labels: vec!["bug".into()],
            assignees: vec!["octocat".into()],
            state: ExternalIssueState::Open,
            linked_branch: Some("feature/session-timeout".into()),
            url: "https://github.com/agentrelay/agentrelay/issues/42".into(),
            updated_at: "2026-09-21T10:00:00Z".into(),
        };
        let failed_check = CiCheck {
            id: "check-9".into(),
            name: "session timeout tests".into(),
            category: CiCategory::Test,
            status: CiStatus::Completed,
            conclusion: CiConclusion::Failure,
            revision: "def5678".into(),
            details_url: Some("https://github.com/check/9".into()),
            started_at: Some("2026-09-21T10:01:00Z".into()),
            completed_at: Some("2026-09-21T10:02:00Z".into()),
        };
        let mut pull_request = PullRequestState {
            id: "pr-node-55".into(),
            number: 55,
            title: "Add session timeout persistence".into(),
            description: "Fixes #42".into(),
            status: PullRequestStatus::Open,
            merge_state: crate::domain::MergeState::Blocked,
            branch: "feature/session-timeout".into(),
            base_branch: "main".into(),
            head_revision: "def5678".into(),
            author: "octocat".into(),
            reviewers: vec!["reviewer".into()],
            files: vec!["src/session.rs".into()],
            commits: vec!["def5678".into()],
            linked_issue_numbers: vec![42],
            ci_checks: vec![failed_check],
            review_comments: vec![crate::domain::ReviewComment {
                id: "comment:77".into(),
                author: "reviewer".into(),
                body: "Add a test for session timeout".into(),
                state: "commented".into(),
                path: Some("src/session.rs".into()),
                line: Some(91),
                url: "https://github.com/review/77".into(),
                created_at: "2026-09-21T10:03:00Z".into(),
            }],
            url: "https://github.com/agentrelay/agentrelay/pull/55".into(),
            updated_at: "2026-09-21T10:03:00Z".into(),
        };

        let first = store
            .apply_delivery_snapshot(
                &repository_path,
                repository.clone(),
                vec![issue.clone()],
                vec![pull_request.clone()],
            )
            .unwrap();
        assert_eq!(first.created_tasks, 2);
        assert_eq!(first.blocked_tasks, 2);
        assert_eq!(
            first
                .snapshot
                .blockers
                .iter()
                .filter(|blocker| blocker.active)
                .count(),
            2
        );
        assert!(first.snapshot.links.iter().any(|link| {
            link.local_entity_type == LocalEntityType::Task
                && link.external_entity_type == ExternalEntityType::Issue
        }));
        assert!(store
            .project_memory(&repository_path)
            .unwrap()
            .events
            .iter()
            .any(|event| event.kind == EventKind::TestFailed));

        let second = store
            .apply_delivery_snapshot(
                &repository_path,
                repository.clone(),
                vec![issue.clone()],
                vec![pull_request.clone()],
            )
            .unwrap();
        assert_eq!(second.created_tasks, 0);
        assert_eq!(second.created_links, 0);
        assert_eq!(second.recorded_events, 1);

        pull_request.status = PullRequestStatus::Merged;
        pull_request.merge_state = crate::domain::MergeState::Ready;
        pull_request.updated_at = "2026-09-21T10:10:00Z".into();
        pull_request.ci_checks[0].conclusion = CiConclusion::Success;
        let merged = store
            .apply_delivery_snapshot(
                &repository_path,
                repository,
                vec![issue],
                vec![pull_request],
            )
            .unwrap();
        assert!(merged
            .snapshot
            .blockers
            .iter()
            .all(|blocker| !blocker.active));
        let memory = store.project_memory(&repository_path).unwrap();
        assert!(memory
            .events
            .iter()
            .any(|event| event.kind == EventKind::PrMerged));
        assert!(memory
            .events
            .iter()
            .any(|event| event.kind == EventKind::BuildPassed));
        assert!(memory
            .goals
            .iter()
            .flat_map(|goal| &goal.tasks)
            .all(|task| task.status == TaskStatus::Completed));
    }

    #[test]
    fn routes_only_after_approval_and_rejects_cycles_and_overlapping_locks() {
        let directory = TempDir::new().unwrap();
        let store = ContextStore::open(directory.path().join("agentrelay.db")).unwrap();
        let repository = directory.path().to_string_lossy().replace('\\', "/");
        store.record_scan(&status(&repository)).unwrap();
        let goal = store
            .create_goal(CreateGoalInput {
                repository_path: repository.clone(),
                title: "Build authentication".into(),
                description: String::new(),
            })
            .unwrap();
        let architecture = store
            .create_task(CreateTaskInput {
                goal_id: goal.id.clone(),
                title: "Design auth architecture".into(),
                description: String::new(),
                priority: TaskPriority::High,
                assigned_agent: None,
                source_agent: Some("user".into()),
            })
            .unwrap();
        let implementation = store
            .create_task(CreateTaskInput {
                goal_id: goal.id,
                title: "Implement auth storage in Rust".into(),
                description: String::new(),
                priority: TaskPriority::High,
                assigned_agent: None,
                source_agent: Some("user".into()),
            })
            .unwrap();
        store
            .update_task_orchestration(UpdateTaskOrchestrationInput {
                task_id: architecture.id.clone(),
                required_capabilities: vec![Capability::Architecture],
                blocked_by: Vec::new(),
                depends_on: Vec::new(),
                produces: vec!["auth design".into()],
                consumes: Vec::new(),
                claimed_paths: vec!["src/auth/*".into()],
            })
            .unwrap();
        store
            .update_task_orchestration(UpdateTaskOrchestrationInput {
                task_id: implementation.id.clone(),
                required_capabilities: vec![Capability::Rust],
                blocked_by: Vec::new(),
                depends_on: vec![architecture.id.clone()],
                produces: Vec::new(),
                consumes: vec!["auth design".into()],
                claimed_paths: vec!["src/auth/service.rs".into()],
            })
            .unwrap();
        let profiles = store.capability_profiles(&repository).unwrap();
        let task = task_by_id(&store.connection().unwrap(), &architecture.id).unwrap();
        let recommendation = crate::orchestration::recommend(&task, &profiles).unwrap();
        let suggested = store.record_route_recommendation(&recommendation).unwrap();
        assert_eq!(suggested.assignment_status, TaskAssignmentStatus::Suggested);
        assert!(suggested.assigned_provider.is_none());

        let assigned = store
            .approve_task_assignment(ApproveTaskAssignmentInput {
                task_id: architecture.id.clone(),
                provider: recommendation.provider.clone(),
            })
            .unwrap();
        assert_eq!(assigned.assignment_status, TaskAssignmentStatus::Assigned);
        assert_eq!(assigned.assigned_provider, Some(recommendation.provider));
        assert_eq!(
            store
                .workspace_locks(&repository)
                .unwrap()
                .iter()
                .filter(|lock| lock.status == WorkspaceLockStatus::Active)
                .count(),
            1
        );
        assert!(matches!(
            store.approve_task_assignment(ApproveTaskAssignmentInput {
                task_id: implementation.id.clone(),
                provider: ProviderId::codex(),
            }),
            Err(StorageError::WorkspaceLocked { .. })
        ));
        assert!(store
            .update_task_orchestration(UpdateTaskOrchestrationInput {
                task_id: architecture.id,
                required_capabilities: vec![Capability::Architecture],
                blocked_by: Vec::new(),
                depends_on: vec![implementation.id],
                produces: Vec::new(),
                consumes: Vec::new(),
                claimed_paths: vec!["src/auth/*".into()],
            })
            .is_err());
    }

    #[test]
    fn decision_supersession_creates_conflict_and_freshness_records() {
        let directory = TempDir::new().unwrap();
        let store = ContextStore::open(directory.path().join("agentrelay.db")).unwrap();
        let repository = directory.path().to_string_lossy().replace('\\', "/");
        store.record_scan(&status(&repository)).unwrap();
        store
            .record_context_generated("not-a-project", "AGENT_CONTEXT.md")
            .expect_err("unknown project must fail");
        let project_id = store.project_memory(&repository).unwrap().project.id;
        store
            .record_context_generated(&project_id, "AGENT_CONTEXT.md")
            .unwrap();

        let previous = store
            .record_decision(CreateDecisionInput {
                repository_path: repository.clone(),
                summary: "Use session cookies".into(),
                reason: "The first implementation is server-rendered".into(),
                alternatives: vec!["JWT".into()],
                affected: vec!["src/auth.rs".into()],
                created_by: "Claude".into(),
                supersedes_decision_id: None,
            })
            .unwrap();
        let replacement = store
            .record_decision(CreateDecisionInput {
                repository_path: repository.clone(),
                summary: "Use short-lived JWTs".into(),
                reason: "The architecture now serves multiple clients".into(),
                alternatives: vec!["session cookies".into()],
                affected: vec!["src/auth.rs".into()],
                created_by: "Codex".into(),
                supersedes_decision_id: Some(previous.id.clone()),
            })
            .unwrap();
        store.evaluate_conflicts(&repository).unwrap();

        let memory = store.project_memory(&repository).unwrap();
        let old = memory
            .decisions
            .iter()
            .find(|decision| decision.id == previous.id)
            .unwrap();
        assert_eq!(old.status, DecisionStatus::Superseded);
        assert_eq!(old.superseded_by.as_deref(), Some(replacement.id.as_str()));
        assert!(memory.conflicts.iter().any(|conflict| {
            conflict.kind == ConflictKind::DecisionContradiction
                && conflict.status == ConflictStatus::Open
        }));
        assert!(memory.freshness.iter().any(|freshness| {
            freshness.subject_id == previous.id && freshness.status == FreshnessStatus::Superseded
        }));
        assert!(memory.freshness.iter().any(|freshness| {
            freshness.subject_type == "context" && freshness.status == FreshnessStatus::Stale
        }));
    }

    #[test]
    fn context_becomes_current_after_generation_and_stale_after_memory_changes() {
        let directory = TempDir::new().unwrap();
        let store = ContextStore::open(directory.path().join("agentrelay.db")).unwrap();
        let repository = directory.path().to_string_lossy().replace('\\', "/");
        store.record_scan(&status(&repository)).unwrap();
        let project_id = store.project_memory(&repository).unwrap().project.id;
        store
            .record_context_generated(&project_id, "AGENT_CONTEXT.md")
            .unwrap();
        store.evaluate_conflicts(&repository).unwrap();
        assert!(store
            .project_memory(&repository)
            .unwrap()
            .freshness
            .iter()
            .any(|item| item.subject_type == "context" && item.status == FreshnessStatus::Current));

        store
            .create_goal(CreateGoalInput {
                repository_path: repository.clone(),
                title: "Change project intent".into(),
                description: String::new(),
            })
            .unwrap();
        store.evaluate_conflicts(&repository).unwrap();
        let memory = store.project_memory(&repository).unwrap();
        assert!(memory.freshness.iter().any(|item| {
            item.subject_type == "context" && item.status == FreshnessStatus::Stale
        }));
        assert!(memory.conflicts.iter().any(|conflict| {
            conflict.kind == ConflictKind::StaleContext && conflict.status == ConflictStatus::Open
        }));
    }

    #[test]
    fn planner_versions_materialize_tasks_only_after_approval() {
        use crate::domain::{
            PlanStatus, PlanVersion, PlannedTask, PlannedTaskKind, PlannedTaskStatus, RiskLevel,
        };

        let directory = TempDir::new().unwrap();
        let store = ContextStore::open(directory.path().join("agentrelay.db")).unwrap();
        let repository = directory.path().to_string_lossy().replace('\\', "/");
        store.record_scan(&status(&repository)).unwrap();
        let goal = store
            .create_goal(CreateGoalInput {
                repository_path: repository.clone(),
                title: "Add provider support".into(),
                description: String::new(),
            })
            .unwrap();
        let timestamp = now();
        let plan = PlanVersion {
            id: new_id(),
            project_id: goal.project_id.clone(),
            goal_id: goal.id.clone(),
            version: 1,
            status: PlanStatus::Valid,
            reason: "Initial plan".into(),
            parent_plan_id: None,
            tasks: vec![PlannedTask {
                id: new_id(),
                title: "Define provider contract".into(),
                description: "Define the boundary.".into(),
                kind: PlannedTaskKind::Architecture,
                status: PlannedTaskStatus::Ready,
                priority: TaskPriority::High,
                required_capabilities: vec![Capability::Architecture],
                depends_on: vec![],
                produces: vec!["approved-contract".into()],
                consumes: vec![],
                affected_modules: vec!["providers".into()],
                expected_files: vec!["src/providers.rs".into()],
                conflict_risk: RiskLevel::Medium,
                risk_score: 35,
                risk_reasons: vec!["Touches a provider boundary.".into()],
                can_run_in_parallel: false,
                parallel_group: 0,
                suggested_provider: Some(ProviderId::codex()),
                suggestion_reasons: vec!["Configured architecture strength.".into()],
                materialized_task_id: None,
                recovery_for: None,
            }],
            validation_issues: vec![],
            changed_task_ids: vec![],
            changed_dependencies: vec![],
            created_at: timestamp,
            approved_at: None,
        };
        store.record_plan(&plan).unwrap();
        assert!(store.tasks_for_project(&repository).unwrap().is_empty());
        let approved = store.approve_plan(&plan.id).unwrap();
        assert_eq!(approved.plan.status, PlanStatus::Approved);
        assert_eq!(approved.tasks.len(), 1);
        assert_eq!(
            approved.tasks[0].assignment_status,
            TaskAssignmentStatus::Suggested
        );
        assert!(approved.plan.tasks[0].materialized_task_id.is_some());
        assert_eq!(
            store.approve_plan(&plan.id).unwrap_err().to_string(),
            "only a valid, unapproved plan can be approved"
        );
    }

    #[test]
    fn application_settings_are_normalized_and_backups_are_consistent() {
        let directory = TempDir::new().unwrap();
        let database = directory.path().join("agentrelay.db");
        let store = ContextStore::open(&database).unwrap();
        let repository = directory.path().join("project");
        fs::create_dir(&repository).unwrap();
        let repository = repository.to_string_lossy().replace('\\', "/");
        store.record_scan(&status(&repository)).unwrap();

        let codex = ProviderId::new("codex").unwrap();
        let saved = store
            .save_application_settings(ApplicationSettings {
                onboarding_completed: true,
                selected_agents: vec![codex.clone(), codex],
                default_context_depth: "unsupported".into(),
                default_context_profile: "unsupported".into(),
                backup_interval_hours: 0,
                ..ApplicationSettings::default()
            })
            .unwrap();
        assert_eq!(saved.selected_agents.len(), 1);
        assert_eq!(saved.default_context_depth, "normal");
        assert_eq!(saved.default_context_profile, "auto");
        assert_eq!(saved.backup_interval_hours, 1);
        assert_eq!(store.application_settings().unwrap(), saved);

        let backup = store.create_backup().unwrap();
        assert!(backup.size_bytes > 0);
        assert_eq!(store.backups().unwrap().len(), 1);
        let backup_connection = Connection::open(backup.path).unwrap();
        let project_count: i64 = backup_connection
            .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
            .unwrap();
        assert_eq!(project_count, 1);
    }

    #[test]
    fn migrates_the_v1_project_schema_without_losing_the_repository() {
        let directory = TempDir::new().unwrap();
        let database = directory.path().join("agentrelay.db");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE projects(
               path TEXT PRIMARY KEY, branch TEXT NOT NULL, last_commit TEXT NOT NULL,
               change_count INTEGER NOT NULL DEFAULT 0, last_scanned_at TEXT NOT NULL
             );
             CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO projects VALUES ('C:/legacy', 'main', 'Before v0.2', 0,
               '2026-09-19T10:00:00.000Z');
             INSERT INTO settings VALUES ('last_repository', 'C:/legacy');",
            )
            .unwrap();
        drop(connection);

        let store = ContextStore::open(&database).unwrap();
        assert_eq!(
            store.last_repository().unwrap().as_deref(),
            Some("C:/legacy")
        );
        store.record_scan(&status("C:/legacy")).unwrap();
        assert_eq!(
            store.project_memory("C:/legacy").unwrap().project.branch,
            "main"
        );
    }

    #[test]
    fn migrates_v2_events_and_sessions_for_handoffs() {
        let directory = TempDir::new().unwrap();
        let database = directory.path().join("agentrelay.db");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE projects(
                   id TEXT PRIMARY KEY,path TEXT NOT NULL UNIQUE,name TEXT NOT NULL,
                   branch TEXT NOT NULL,last_commit TEXT NOT NULL,change_count INTEGER NOT NULL,
                   last_scanned_at TEXT NOT NULL,status_signature TEXT NOT NULL,status_json TEXT NOT NULL
                 );
                 CREATE TABLE agent_sessions(
                   id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id),
                   agent TEXT NOT NULL,status TEXT NOT NULL,started_at TEXT NOT NULL,
                   ended_at TEXT,summary TEXT NOT NULL DEFAULT ''
                 );
                 CREATE TABLE events(
                   id TEXT PRIMARY KEY,project_id TEXT NOT NULL REFERENCES projects(id),
                   kind TEXT NOT NULL,source TEXT NOT NULL,payload_json TEXT NOT NULL,created_at TEXT NOT NULL
                 );
                 CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);",
            )
            .unwrap();
        drop(connection);

        let store = ContextStore::open(&database).unwrap();
        let connection = store.connection().unwrap();
        assert!(column_exists(&connection, "events", "session_id").unwrap());
        assert!(column_exists(&connection, "agent_sessions", "provider").unwrap());
        assert!(table_exists(&connection, "handoffs").unwrap());
        assert!(table_exists(&connection, "conflicts").unwrap());
        assert!(table_exists(&connection, "freshness").unwrap());
        assert!(column_exists(&connection, "projects", "tracking_enabled").unwrap());
        assert!(column_exists(&connection, "agent_sessions", "detected_pid").unwrap());
        assert!(column_exists(&connection, "agent_sessions", "automatic").unwrap());
        assert!(column_exists(&connection, "agent_sessions", "task_id").unwrap());
        assert!(column_exists(&connection, "tasks", "assignment_status").unwrap());
        assert!(column_exists(&connection, "tasks", "claimed_paths_json").unwrap());
        assert!(table_exists(&connection, "agent_capability_profiles").unwrap());
        assert!(table_exists(&connection, "workspace_locks").unwrap());
        assert!(table_exists(&connection, "delivery_snapshots").unwrap());
        assert!(table_exists(&connection, "external_links").unwrap());
        assert!(table_exists(&connection, "delivery_blockers").unwrap());
        assert!(table_exists(&connection, "task_workspaces").unwrap());
        assert!(table_exists(&connection, "workspace_validations").unwrap());
        assert!(table_exists(&connection, "planner_versions").unwrap());
        assert!(table_exists(&connection, "environment_snapshots").unwrap());
        assert!(table_exists(&connection, "environment_caches").unwrap());
        assert!(table_exists(&connection, "verification_suites").unwrap());
        assert!(table_exists(&connection, "context_versions").unwrap());
        assert!(table_exists(&connection, "explanation_records").unwrap());
        assert!(column_exists(&connection, "project_permissions", "run_tests_mode").unwrap());
        assert!(column_exists(&connection, "project_permissions", "run_build_mode").unwrap());
    }

    #[test]
    fn startup_recovery_fails_closed_for_interrupted_work() {
        let directory = TempDir::new().unwrap();
        let database = directory.path().join("agentrelay.db");
        let repository_path = directory.path().join("project");
        fs::create_dir(&repository_path).unwrap();
        let repository = repository_path.to_string_lossy().replace('\\', "/");
        let store = ContextStore::open(&database).unwrap();
        store.record_scan(&status(&repository)).unwrap();
        let goal = store
            .create_goal(CreateGoalInput {
                repository_path: repository.clone(),
                title: "Exercise restart recovery".into(),
                description: String::new(),
            })
            .unwrap();
        let project_id = goal.project_id.clone();
        let task = store
            .create_task(CreateTaskInput {
                goal_id: goal.id,
                title: "Interrupted task".into(),
                description: String::new(),
                priority: TaskPriority::High,
                assigned_agent: None,
                source_agent: Some("rc-test".into()),
            })
            .unwrap();
        let missing_workspace = directory.path().join("deleted-worktree");
        let missing_cache = directory.path().join("deleted-cache");
        let connection = store.connection().unwrap();
        let timestamp = now();
        let session_id = new_id();
        let workspace_id = new_id();
        connection
            .execute(
                "INSERT INTO agent_sessions(
                   id,project_id,agent,provider,agent_name,status,working_directory,initial_git_head,
                   final_git_head,initial_git_state_json,final_git_state_json,started_at,ended_at,
                   summary,summary_status,detected_pid,automatic,task_id
                 ) VALUES (?1,?2,'Codex','codex','Codex','active',?3,'abc1234',NULL,?4,NULL,
                   ?5,NULL,'','',1234,1,?6)",
                params![
                    session_id,
                    project_id,
                    repository,
                    serde_json::to_string(&status(&repository)).unwrap(),
                    timestamp,
                    task.id,
                ],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE tasks SET status='in_progress',assignment_status='running' WHERE id=?1",
                params![task.id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO task_workspaces(
                   id,project_id,task_id,assigned_provider,branch,worktree_path,base_commit,
                   head_commit,status,session_id,created_at,updated_at
                 ) VALUES (?1,?2,?3,'codex','codex/recovery',?4,'abc1234','abc1234','active',?5,?6,?6)",
                params![
                    workspace_id,
                    project_id,
                    task.id,
                    missing_workspace.to_string_lossy(),
                    session_id,
                    timestamp,
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO action_runs(
                   id,project_id,session_id,requested_by,action_kind,status,command_json,
                   requested_at,started_at
                 ) VALUES ('action-rc',?1,?2,'Codex','run_tests','running','[]',?3,?3)",
                params![project_id, session_id, timestamp],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO environment_caches(id,project_id,kind,fingerprint,path,status,updated_at)
                 VALUES ('cache-rc',?1,'cargo','fingerprint',?2,'valid',?3)",
                params![project_id, missing_cache.to_string_lossy(), timestamp],
            )
            .unwrap();
        let verification = VerificationSuite {
            id: "verification-rc".into(),
            project_id: project_id.clone(),
            task_id: task.id.clone(),
            workspace_id: workspace_id.clone(),
            status: VerificationStatus::Running,
            environment_id: "environment-rc".into(),
            environment_fingerprint: "fingerprint".into(),
            environment_profile: ExecutionProfile::NativeWindows,
            commit: "abc1234".into(),
            branch: "codex/recovery".into(),
            worktree: missing_workspace.to_string_lossy().into_owned(),
            acceptance_criteria: vec!["state survives restart".into()],
            impact: VerificationImpact {
                changed_files: vec![],
                highest_risk: None,
                affected_modules: vec![],
                tests_to_run: vec![],
            },
            gates: vec![VerificationGate {
                id: "gate-rc".into(),
                kind: VerificationGateKind::Tests,
                name: "Tests".into(),
                required: true,
                status: VerificationStatus::Running,
                command: vec!["cargo".into(), "test".into()],
                exit_code: None,
                duration_ms: None,
                stdout_summary: String::new(),
                stderr_summary: String::new(),
                detail: String::new(),
                started_at: Some(timestamp.clone()),
                finished_at: None,
                waiver_reason: None,
            }],
            repair_task_id: None,
            created_at: timestamp.clone(),
            completed_at: None,
        };
        connection
            .execute(
                "INSERT INTO verification_suites(
                   id,project_id,task_id,workspace_id,status,environment_id,
                   environment_fingerprint,verified_head,snapshot_json,created_at
                 ) VALUES (?1,?2,?3,?4,'running',?5,?6,?7,?8,?9)",
                params![
                    verification.id,
                    verification.project_id,
                    verification.task_id,
                    verification.workspace_id,
                    verification.environment_id,
                    verification.environment_fingerprint,
                    verification.commit,
                    serde_json::to_string(&verification).unwrap(),
                    timestamp,
                ],
            )
            .unwrap();
        drop(connection);
        drop(store);

        let recovered = ContextStore::open(&database).unwrap();
        let report = recovered.last_recovery_report().unwrap().unwrap();
        assert_eq!(report.database_integrity, "ok");
        assert_eq!(report.interrupted_sessions, 1);
        assert_eq!(report.interrupted_actions, 1);
        assert_eq!(report.interrupted_verifications, 1);
        assert_eq!(report.missing_workspaces, 1);
        assert_eq!(report.invalidated_caches, 1);
        let connection = recovered.connection().unwrap();
        let session_status: String = connection
            .query_row(
                "SELECT status FROM agent_sessions WHERE id=?1",
                params![session_id],
                |row| row.get(0),
            )
            .unwrap();
        let task_status: String = connection
            .query_row(
                "SELECT assignment_status FROM tasks WHERE id=?1",
                params![task.id],
                |row| row.get(0),
            )
            .unwrap();
        let workspace_status: String = connection
            .query_row(
                "SELECT status FROM task_workspaces WHERE id=?1",
                params![workspace_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(session_status, "interrupted");
        assert_eq!(task_status, "blocked");
        assert_eq!(workspace_status, "validation_failed");
        assert_eq!(
            recovered
                .verification_suite("verification-rc")
                .unwrap()
                .status,
            VerificationStatus::Failed
        );
    }

    #[test]
    fn startup_recovery_preserves_existing_workspace_for_revalidation() {
        let directory = TempDir::new().unwrap();
        let database = directory.path().join("agentrelay.db");
        let repository_path = directory.path().join("project");
        let workspace_path = directory.path().join("existing-worktree");
        fs::create_dir(&repository_path).unwrap();
        fs::create_dir(&workspace_path).unwrap();
        let repository = repository_path.to_string_lossy().replace('\\', "/");
        let store = ContextStore::open(&database).unwrap();
        store.record_scan(&status(&repository)).unwrap();
        let goal = store
            .create_goal(CreateGoalInput {
                repository_path: repository.clone(),
                title: "Resume safely".into(),
                description: String::new(),
            })
            .unwrap();
        let project_id = goal.project_id.clone();
        let task = store
            .create_task(CreateTaskInput {
                goal_id: goal.id,
                title: "Recoverable task".into(),
                description: String::new(),
                priority: TaskPriority::High,
                assigned_agent: None,
                source_agent: Some("rc-test".into()),
            })
            .unwrap();
        let session_id = new_id();
        let workspace_id = new_id();
        let timestamp = now();
        let connection = store.connection().unwrap();
        connection
            .execute(
                "INSERT INTO agent_sessions(
                   id,project_id,agent,provider,agent_name,status,working_directory,initial_git_head,
                   initial_git_state_json,started_at,summary,summary_status,automatic,task_id
                 ) VALUES (?1,?2,'Codex','codex','Codex','active',?3,'abc1234',?4,?5,'','',1,?6)",
                params![
                    session_id,
                    project_id,
                    workspace_path.to_string_lossy(),
                    serde_json::to_string(&status(&repository)).unwrap(),
                    timestamp,
                    task.id,
                ],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE tasks SET status='in_progress',assignment_status='running' WHERE id=?1",
                params![task.id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO task_workspaces(
                   id,project_id,task_id,assigned_provider,branch,worktree_path,base_commit,
                   head_commit,status,session_id,created_at,updated_at
                 ) VALUES (?1,?2,?3,'codex','codex/recovery',?4,'abc1234','abc1234','active',?5,?6,?6)",
                params![
                    workspace_id,
                    project_id,
                    task.id,
                    workspace_path.to_string_lossy(),
                    session_id,
                    timestamp,
                ],
            )
            .unwrap();
        drop(connection);
        drop(store);

        let recovered = ContextStore::open(&database).unwrap();
        let report = recovered.last_recovery_report().unwrap().unwrap();
        assert_eq!(report.interrupted_sessions, 1);
        assert_eq!(report.recoverable_workspaces, 1);
        assert_eq!(report.missing_workspaces, 0);
        let connection = recovered.connection().unwrap();
        let workspace_status: String = connection
            .query_row(
                "SELECT status FROM task_workspaces WHERE id=?1",
                params![workspace_id],
                |row| row.get(0),
            )
            .unwrap();
        let task_status: String = connection
            .query_row(
                "SELECT assignment_status FROM tasks WHERE id=?1",
                params![task.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(workspace_status, "validation_pending");
        assert_eq!(task_status, "waiting");
    }

    #[test]
    fn failed_migration_restores_the_original_database() {
        let directory = TempDir::new().unwrap();
        let database = directory.path().join("agentrelay.db");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE projects(path TEXT PRIMARY KEY);
                 INSERT INTO projects VALUES ('C:/malformed');
                 PRAGMA user_version=19;",
            )
            .unwrap();
        drop(connection);

        let error = match ContextStore::open(&database) {
            Ok(_) => panic!("malformed v19 schema unexpectedly migrated"),
            Err(error) => error,
        };
        let backup = match error {
            StorageError::MigrationFailed { backup, .. } => backup,
            other => panic!("expected migration rollback, got {other}"),
        };
        assert!(Path::new(&backup).is_file());
        let restored = Connection::open(&database).unwrap();
        let rows: i64 = restored
            .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1);
        assert!(column_exists(&restored, "projects", "path").unwrap());
        assert!(!column_exists(&restored, "projects", "id").unwrap());
        assert!(!table_exists(&restored, "projects_v1").unwrap());
    }

    #[test]
    #[ignore = "release-candidate performance baseline"]
    fn release_candidate_event_query_baseline() {
        let directory = TempDir::new().unwrap();
        let store = ContextStore::open(directory.path().join("agentrelay.db")).unwrap();
        let repository = directory.path().to_string_lossy().replace('\\', "/");
        store.record_scan(&status(&repository)).unwrap();
        let mut connection = store.connection().unwrap();
        let project_id = project_id_for_path(&connection, &repository).unwrap();
        let transaction = connection.transaction().unwrap();
        for index in 0..10_000 {
            append_event(
                &transaction,
                &project_id,
                EventKind::FileModified,
                EventSource::System,
                json!({"path": format!("src/generated/{index}.rs")}),
                &now(),
            )
            .unwrap();
        }
        transaction.commit().unwrap();
        let persisted_events: i64 = store
            .connection()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM events WHERE project_id=?1",
                params![project_id],
                |row| row.get(0),
            )
            .unwrap();
        let projection_started = Instant::now();
        let memory = store.project_memory(&repository).unwrap();
        let projection_elapsed = projection_started.elapsed();
        let replay_started = Instant::now();
        let replay = store.replay_events(&repository).unwrap();
        let replay_elapsed = replay_started.elapsed();
        eprintln!(
            "RC_PERF persisted_events={} projected_events={} project_memory_ms={} replay_ms={}",
            persisted_events,
            memory.events.len(),
            projection_elapsed.as_millis(),
            replay_elapsed.as_millis()
        );
        assert!(persisted_events >= 10_000);
        assert_eq!(
            memory.events.len(),
            200,
            "working projection must stay bounded"
        );
        assert!(replay.len() >= 10_000);
        assert!(
            projection_elapsed.as_secs() < 10 && replay_elapsed.as_secs() < 10,
            "10k event projection/replay exceeded 10 seconds"
        );
    }
}
