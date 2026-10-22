use std::collections::{BTreeMap, BTreeSet};

use chrono::{SecondsFormat, Utc};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    domain::{
        AgentCapabilityProfile, AgentSession, Blocker, DecisionSummary, EndSessionInput, EventKind,
        FileChange, FileChangeType, HandoffSnapshot, StartSessionInput, TaskStatus, TaskSummary,
    },
    git::{GitError, GitScanner, ProjectStatus},
    storage::{ContextStore, StorageError},
};

/// Owns manual session lifecycle and deterministic handoff orchestration. Delivery adapters only
/// translate typed commands into these operations.
pub struct SessionService<'a> {
    git: &'a GitScanner,
    store: &'a ContextStore,
}

impl<'a> SessionService<'a> {
    pub fn new(git: &'a GitScanner, store: &'a ContextStore) -> Self {
        Self { git, store }
    }

    pub fn start_session(&self, input: StartSessionInput) -> Result<AgentSession, SessionError> {
        self.start(input, None, false)
    }

    pub fn start_detected_session(
        &self,
        repository_path: String,
        provider: crate::domain::ProviderId,
        agent_name: String,
        pid: u32,
    ) -> Result<AgentSession, SessionError> {
        self.start(
            StartSessionInput {
                repository_path,
                agent_name,
                provider,
                task_id: None,
            },
            Some(pid),
            true,
        )
    }

    fn start(
        &self,
        input: StartSessionInput,
        detected_pid: Option<u32>,
        automatic: bool,
    ) -> Result<AgentSession, SessionError> {
        let initial_state = self.git.scan(&input.repository_path)?;
        self.store.record_scan(&initial_state)?;
        let session = self.store.start_session(
            &initial_state.repository,
            &initial_state.repository,
            input.provider,
            &input.agent_name,
            &initial_state,
            detected_pid,
            automatic,
            input.task_id.as_deref(),
        )?;
        self.store.evaluate_project(&session.project_id)?;
        Ok(session)
    }

    pub fn start_isolated_session(
        &self,
        project_repository_path: String,
        working_directory: String,
        provider: crate::domain::ProviderId,
        agent_name: String,
        task_id: String,
    ) -> Result<AgentSession, SessionError> {
        let initial_state = self.git.scan(&working_directory)?;
        let session = self.store.start_session(
            &project_repository_path,
            &initial_state.repository,
            provider,
            &agent_name,
            &initial_state,
            None,
            false,
            Some(&task_id),
        )?;
        self.store.evaluate_project(&session.project_id)?;
        Ok(session)
    }

    pub fn active_session(
        &self,
        repository_path: &str,
    ) -> Result<Option<AgentSession>, SessionError> {
        Ok(self.store.active_session(repository_path)?)
    }

    pub fn mark_idle(&self, session_id: &str) -> Result<AgentSession, SessionError> {
        Ok(self.store.mark_session_idle(session_id)?)
    }

    pub fn end_session(&self, input: EndSessionInput) -> Result<HandoffSnapshot, SessionError> {
        let session = self.store.session(&input.session_id)?;
        let initial_state = session
            .initial_git_state
            .as_ref()
            .ok_or_else(|| SessionError::MissingBaseline(session.id.clone()))?;
        let final_state = self.git.scan(&session.working_directory)?;
        let project_repository = self.store.repository_for_project_id(&session.project_id)?;
        if final_state.repository == project_repository {
            self.store.record_scan(&final_state)?;
        }

        let commits = self.git.commits_between(
            &session.working_directory,
            &session.initial_git_head,
            &final_state.head_commit,
        )?;
        let committed_changes = self.git.changed_files_between(
            &session.working_directory,
            &session.initial_git_head,
            &final_state.head_commit,
        )?;
        let changed_files = merge_changes(
            committed_changes,
            worktree_delta(initial_state, &final_state),
        );
        self.store
            .record_session_delta(&session, &changed_files, &commits, &final_state)?;
        let completed_session =
            self.store
                .finish_session(&session.id, &final_state, &input.summary_status)?;

        let mut blockers = input.blockers;
        blockers.extend(
            self.store
                .active_delivery_blockers(&project_repository)?
                .into_iter()
                .map(|blocker| blocker.summary),
        );
        blockers.sort();
        blockers.dedup();
        let memory = self.store.project_memory(&project_repository)?;
        let profiles = self.store.capability_profiles(&project_repository)?;
        let handoff = build_handoff(
            &completed_session,
            &memory,
            &profiles,
            changed_files,
            commits,
            blockers,
        );
        self.store.save_handoff(&handoff)?;
        self.store.evaluate_project(&session.project_id)?;

        Ok(handoff)
    }
}

fn worktree_delta(initial: &ProjectStatus, final_state: &ProjectStatus) -> Vec<FileChange> {
    let initial_files = worktree_files(initial);
    let final_files = worktree_files(final_state);
    final_files
        .into_iter()
        .filter(|(path, change)| initial_files.get(path) != Some(change))
        .map(|(path, change_type)| FileChange { path, change_type })
        .collect()
}

fn worktree_files(status: &ProjectStatus) -> BTreeMap<String, FileChangeType> {
    let mut files = BTreeMap::new();
    for path in &status.modified_files {
        files.insert(path.clone(), FileChangeType::Modified);
    }
    for path in &status.added_files {
        files.insert(path.clone(), FileChangeType::Created);
    }
    for path in &status.deleted_files {
        files.insert(path.clone(), FileChangeType::Deleted);
    }
    files
}

fn merge_changes(committed: Vec<FileChange>, working: Vec<FileChange>) -> Vec<FileChange> {
    let mut changes = BTreeMap::new();
    for change in committed.into_iter().chain(working) {
        changes.insert(change.path, change.change_type);
    }
    changes
        .into_iter()
        .map(|(path, change_type)| FileChange { path, change_type })
        .collect()
}

fn build_handoff(
    session: &AgentSession,
    memory: &crate::domain::ProjectMemory,
    profiles: &[AgentCapabilityProfile],
    changed_files: Vec<FileChange>,
    commits: Vec<String>,
    blockers: Vec<String>,
) -> HandoffSnapshot {
    let completed_ids = memory
        .events
        .iter()
        .filter(|event| {
            event.session_id.as_deref() == Some(&session.id)
                && event.kind == EventKind::TaskCompleted
        })
        .filter_map(|event| event.payload.get("taskId")?.as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let decision_ids = memory
        .events
        .iter()
        .filter(|event| {
            event.session_id.as_deref() == Some(&session.id)
                && event.kind == EventKind::DecisionRecorded
        })
        .filter_map(|event| event.payload.get("decisionId")?.as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let all_tasks = memory.goals.iter().flat_map(|goal| &goal.tasks);
    let task_summaries = all_tasks
        .map(|task| TaskSummary {
            id: task.id.clone(),
            title: task.title.clone(),
            status: task.status,
            priority: task.priority,
            assigned_agent: task.assigned_agent.clone(),
        })
        .collect::<Vec<_>>();
    let completed_tasks = task_summaries
        .iter()
        .filter(|task| completed_ids.contains(&task.id))
        .cloned()
        .collect();
    let active_tasks = task_summaries
        .iter()
        .filter(|task| matches!(task.status, TaskStatus::Todo | TaskStatus::InProgress))
        .cloned()
        .collect::<Vec<_>>();
    let recent_decisions = memory
        .decisions
        .iter()
        .filter(|decision| decision_ids.contains(&decision.id))
        .map(|decision| DecisionSummary {
            id: decision.id.clone(),
            summary: decision.summary.clone(),
            reason: decision.reason.clone(),
            status: decision.status,
            created_by: decision.created_by.clone(),
        })
        .collect();
    let blockers = blockers
        .into_iter()
        .map(|description| description.trim().to_owned())
        .filter(|description| !description.is_empty())
        .map(|description| Blocker { description })
        .collect::<Vec<_>>();
    let mut recommended_next_actions = active_tasks
        .iter()
        .take(3)
        .map(|task| task.title.clone())
        .collect::<Vec<_>>();
    if recommended_next_actions.is_empty() {
        recommended_next_actions
            .push("Review the handoff and create the next project task.".into());
    }
    let completed_task_ids = memory
        .goals
        .iter()
        .flat_map(|goal| &goal.tasks)
        .filter(|task| task.status == TaskStatus::Completed)
        .map(|task| task.id.as_str())
        .collect::<BTreeSet<_>>();
    let route = memory
        .goals
        .iter()
        .flat_map(|goal| &goal.tasks)
        .find(|task| {
            task.status == TaskStatus::Todo
                && task
                    .depends_on
                    .iter()
                    .chain(&task.blocked_by)
                    .all(|dependency| completed_task_ids.contains(dependency.as_str()))
        })
        .and_then(|task| crate::orchestration::recommend(task, profiles));
    let ended_at = session.ended_at.clone().unwrap_or_else(now);
    let final_state = session.final_git_state.as_ref();
    HandoffSnapshot {
        id: Uuid::new_v4().to_string(),
        project_id: session.project_id.clone(),
        session_id: session.id.clone(),
        previous_agent: Some(session.agent_name.clone()),
        branch: final_state
            .map(|state| state.branch.clone())
            .unwrap_or_default(),
        base_commit: session.initial_git_head.clone(),
        final_commit: session.final_git_head.clone().unwrap_or_default(),
        changed_files,
        commits,
        completed_tasks,
        active_tasks,
        recent_decisions,
        blockers,
        recommended_next_actions,
        next_recommended_provider: route.as_ref().map(|route| route.provider.clone()),
        routing_reasons: route.map(|route| route.reasons).unwrap_or_default(),
        started_at: session.started_at.clone(),
        ended_at: ended_at.clone(),
        created_at: ended_at,
    }
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error(transparent)]
    Git(#[from] GitError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("session has no captured Git baseline: {0}")]
    MissingBaseline(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::Path, process::Command, time::Duration};

    use crate::{
        detection::DetectedAgent,
        domain::{
            ApproveTaskAssignmentInput, Capability, CreateDecisionInput, CreateGoalInput,
            CreateTaskInput, EndSessionInput, ProviderId, StartRoutedTaskInput, StartSessionInput,
            TaskAssignmentStatus, TaskPriority, TaskStatus, UpdateTaskInput,
            UpdateTaskOrchestrationInput, WorkspaceLockStatus,
        },
        AgentRelayCore,
    };
    use tempfile::TempDir;

    fn status(modified: &[&str], added: &[&str]) -> ProjectStatus {
        ProjectStatus {
            repository: "C:/demo".into(),
            branch: "main".into(),
            modified_files: modified.iter().map(|value| (*value).to_owned()).collect(),
            added_files: added.iter().map(|value| (*value).to_owned()).collect(),
            deleted_files: Vec::new(),
            last_commit: "Initial".into(),
            head_commit: "abc1234".into(),
            recent_commits: Vec::new(),
            diff_stat: Vec::new(),
            scanned_at: "2026-09-20T00:00:00.000Z".into(),
        }
    }

    #[test]
    fn worktree_delta_excludes_changes_that_predate_the_session() {
        let initial = status(&["existing.rs"], &[]);
        let final_state = status(&["existing.rs", "new.rs"], &["created.rs"]);
        assert_eq!(
            worktree_delta(&initial, &final_state),
            vec![
                FileChange {
                    path: "created.rs".into(),
                    change_type: FileChangeType::Created,
                },
                FileChange {
                    path: "new.rs".into(),
                    change_type: FileChangeType::Modified,
                }
            ]
        );
    }

    #[test]
    fn session_lifecycle_persists_a_deterministic_handoff() {
        let directory = TempDir::new().unwrap();
        let repository = directory.path().join("repository");
        std::fs::create_dir(&repository).unwrap();
        git(&repository, &["init", "-b", "main"]);
        git(&repository, &["config", "user.name", "AgentRelay Test"]);
        git(
            &repository,
            &["config", "user.email", "agentrelay@example.test"],
        );
        std::fs::write(repository.join("tracked.txt"), "before\n").unwrap();
        git(&repository, &["add", "tracked.txt"]);
        git(&repository, &["commit", "-m", "Initial project"]);

        let database = directory.path().join("agentrelay.db");
        let core = AgentRelayCore::open(&database, Duration::from_secs(5)).unwrap();
        let project = core.scan_project(&repository).unwrap();
        let goal = core
            .create_goal(CreateGoalInput {
                repository_path: project.repository.clone(),
                title: "Ship session handoff".into(),
                description: String::new(),
            })
            .unwrap();
        let task = core
            .create_task(CreateTaskInput {
                goal_id: goal.id,
                title: "Persist the handoff".into(),
                description: String::new(),
                priority: TaskPriority::High,
                assigned_agent: Some("Codex".into()),
                source_agent: Some("user".into()),
            })
            .unwrap();
        let task_id = task.id.clone();
        let session = core
            .start_agent_session(StartSessionInput {
                repository_path: project.repository.clone(),
                provider: ProviderId::codex(),
                agent_name: "Codex".into(),
                task_id: None,
            })
            .unwrap();

        std::fs::write(repository.join("tracked.txt"), "after\n").unwrap();
        std::fs::write(repository.join("session.txt"), "new\n").unwrap();
        git(&repository, &["add", "tracked.txt", "session.txt"]);
        git(&repository, &["commit", "-m", "Complete session work"]);
        core.update_task(UpdateTaskInput {
            id: task.id,
            title: task.title,
            description: task.description,
            status: TaskStatus::Completed,
            priority: task.priority,
            assigned_agent: task.assigned_agent,
        })
        .unwrap();
        core.record_decision(CreateDecisionInput {
            repository_path: project.repository.clone(),
            summary: "Use deterministic handoffs".into(),
            reason: "SQLite remains authoritative".into(),
            alternatives: Vec::new(),
            affected: vec!["AGENT_CONTEXT.md".into()],
            created_by: "Codex".into(),
            supersedes_decision_id: None,
        })
        .unwrap();

        let handoff = core
            .end_agent_session(EndSessionInput {
                session_id: session.id,
                summary_status: "Session lifecycle implemented".into(),
                blockers: vec!["Process detection remains for v0.4".into()],
            })
            .unwrap();
        assert_eq!(handoff.previous_agent.as_deref(), Some("Codex"));
        assert_eq!(handoff.completed_tasks.len(), 1);
        assert_eq!(handoff.recent_decisions.len(), 1);
        assert_eq!(handoff.commits.len(), 1);
        assert!(handoff
            .changed_files
            .iter()
            .any(|change| change.path == "session.txt"));
        let context = std::fs::read_to_string(repository.join("AGENT_CONTEXT.md")).unwrap();
        assert!(context.contains("## Selected Grounded Context"));
        assert!(context.contains("Complete session work"));

        let reopened = AgentRelayCore::open(&database, Duration::from_secs(5)).unwrap();
        let memory = reopened.project_memory(&project.repository).unwrap();
        assert_eq!(memory.agent_sessions.len(), 1);
        assert_eq!(memory.latest_handoff.unwrap().id, handoff.id);
        assert!(reopened
            .active_agent_session(&project.repository)
            .unwrap()
            .is_none());

        let second_session = reopened
            .start_detected_agent_session(
                project.repository.clone(),
                &DetectedAgent {
                    provider: ProviderId::claude(),
                    pid: 4242,
                    executable: "claude.exe".into(),
                    working_directory: Some(repository.clone()),
                    command_line: Vec::new(),
                },
            )
            .unwrap();
        assert!(second_session.automatic);
        assert_eq!(second_session.detected_pid, Some(4242));
        std::fs::write(repository.join("tracked.txt"), "changed again\n").unwrap();
        git(&repository, &["add", "tracked.txt"]);
        git(&repository, &["commit", "-m", "Revisit shared auth file"]);
        reopened
            .end_agent_session(EndSessionInput {
                session_id: second_session.id,
                summary_status: "Revisited work from the prior agent".into(),
                blockers: Vec::new(),
            })
            .unwrap();
        let memory = reopened.project_memory(&project.repository).unwrap();
        assert!(memory.conflicts.iter().any(|conflict| {
            conflict.kind == crate::domain::ConflictKind::FileOverlap
                && conflict.status == crate::domain::ConflictStatus::Open
                && conflict
                    .affected_paths
                    .iter()
                    .any(|path| path == "tracked.txt")
        }));
        assert!(memory.conflicts.iter().any(|conflict| {
            conflict.kind == crate::domain::ConflictKind::CompletedTaskDrift
                && conflict.status == crate::domain::ConflictStatus::Open
        }));
        assert!(memory.freshness.iter().any(|freshness| {
            freshness.subject_id == task_id
                && freshness.status == crate::domain::FreshnessStatus::Invalidated
        }));
    }

    #[test]
    fn routed_session_requires_approval_and_retains_locks_until_merge() {
        let directory = TempDir::new().unwrap();
        let repository = directory.path().join("repository");
        std::fs::create_dir(&repository).unwrap();
        git(&repository, &["init", "-b", "main"]);
        git(&repository, &["config", "user.name", "AgentRelay Test"]);
        git(
            &repository,
            &["config", "user.email", "agentrelay@example.test"],
        );
        std::fs::write(repository.join("tracked.txt"), "before\n").unwrap();
        git(&repository, &["add", "tracked.txt"]);
        git(&repository, &["commit", "-m", "Initial project"]);

        let core = AgentRelayCore::open(
            directory.path().join("agentrelay.db"),
            Duration::from_secs(5),
        )
        .unwrap();
        let project = core.scan_project(&repository).unwrap();
        let goal = core
            .create_goal(CreateGoalInput {
                repository_path: project.repository.clone(),
                title: "Implement routing".into(),
                description: String::new(),
            })
            .unwrap();
        let task = core
            .create_task(CreateTaskInput {
                goal_id: goal.id,
                title: "Implement Rust task router".into(),
                description: String::new(),
                priority: TaskPriority::High,
                assigned_agent: None,
                source_agent: Some("user".into()),
            })
            .unwrap();
        core.update_task_orchestration(UpdateTaskOrchestrationInput {
            task_id: task.id.clone(),
            required_capabilities: vec![Capability::Rust, Capability::Backend],
            blocked_by: Vec::new(),
            depends_on: Vec::new(),
            produces: vec!["task routing".into()],
            consumes: Vec::new(),
            claimed_paths: vec!["src/router/*".into()],
        })
        .unwrap();
        core.generate_context(&project.repository).unwrap();
        git(&repository, &["add", "AGENT_CONTEXT.md"]);
        git(&repository, &["commit", "-m", "refresh agent context"]);
        assert!(core
            .start_routed_task(StartRoutedTaskInput {
                repository_path: project.repository.clone(),
                task_id: task.id.clone(),
            })
            .is_err());
        let suggested = core.suggest_task_route(&task.id).unwrap();
        let provider = suggested.suggested_provider.unwrap();
        core.approve_task_assignment(ApproveTaskAssignmentInput {
            task_id: task.id.clone(),
            provider,
        })
        .unwrap();
        let session = core
            .start_routed_task(StartRoutedTaskInput {
                repository_path: project.repository.clone(),
                task_id: task.id.clone(),
            })
            .unwrap();
        assert_eq!(session.task_id.as_deref(), Some(task.id.as_str()));
        assert_eq!(
            core.orchestration_snapshot(&project.repository)
                .unwrap()
                .tasks[0]
                .assignment_status,
            TaskAssignmentStatus::Running
        );
        core.end_agent_session(EndSessionInput {
            session_id: session.id,
            summary_status: "Routing work paused".into(),
            blockers: Vec::new(),
        })
        .unwrap();
        let snapshot = core.orchestration_snapshot(&project.repository).unwrap();
        assert_eq!(
            snapshot.tasks[0].assignment_status,
            TaskAssignmentStatus::Waiting
        );
        assert!(snapshot
            .locks
            .iter()
            .all(|lock| lock.status == WorkspaceLockStatus::Active));
    }

    fn git(repository: &Path, arguments: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
