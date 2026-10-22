use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::domain::EnvironmentSnapshot;
use crate::{
    domain::{
        AgentSession, AgentSessionStatus, ContextVersion, Decision, DecisionStatus, EventKind,
        ExplainHistoryInput, Goal, GoalStatus, HistoricalBlocker, HistoricalFile,
        HistoryExplanation, PlanVersion, ProjectEvent, ProjectMemory, ReplayAccuracy,
        ReplayComparison, ReplayEvidence, ReplayGitState, ReplayPoint, ReplayRequest, ReplayState,
        TaskAssignmentStatus, TaskStatus, TimelineCategory, TimelineEntry, TimelineSnapshot,
        VerificationSuite, WorkTask,
    },
    git::ProjectStatus,
};

/// Complete read model used by the replay engine. Building it may read SQLite, but replaying it
/// never talks to Git and never writes to either the repository or the database.
#[derive(Debug, Clone)]
pub(crate) struct ReplaySource {
    pub memory: ProjectMemory,
    pub events: Vec<ProjectEvent>,
    pub sessions: Vec<AgentSession>,
    pub contexts: Vec<ContextVersion>,
    pub plans: Vec<PlanVersion>,
    pub verifications: Vec<VerificationSuite>,
    pub environments: Vec<EnvironmentSnapshot>,
}

pub(crate) fn timeline(source: &ReplaySource) -> TimelineSnapshot {
    let entries = source
        .events
        .iter()
        .rev()
        .map(timeline_entry)
        .collect::<Vec<_>>();
    TimelineSnapshot {
        project_id: source.memory.project.id.clone(),
        repository: source.memory.project.path.clone(),
        first_timestamp: entries.first().map(|entry| entry.timestamp.clone()),
        last_timestamp: entries.last().map(|entry| entry.timestamp.clone()),
        entries,
    }
}

pub(crate) fn replay(
    source: &ReplaySource,
    request: &ReplayRequest,
) -> Result<ReplayState, String> {
    let point = resolve_point(source, request)?;
    Ok(replay_at(source, point))
}

pub(crate) fn compare(
    source: &ReplaySource,
    left_event_id: &str,
    right_event_id: &str,
) -> Result<ReplayComparison, String> {
    let left = replay_at(source, point_for_event(source, left_event_id)?);
    let right = replay_at(source, point_for_event(source, right_event_id)?);

    let left_decisions = keyed(&left.decisions, |item| item.id.clone());
    let right_decisions = keyed(&right.decisions, |item| item.id.clone());
    let left_tasks = tasks_by_id(&left.goals);
    let right_tasks = tasks_by_id(&right.goals);
    let left_files = keyed(&left.files, |item| item.path.clone());
    let right_files = keyed(&right.files, |item| item.path.clone());
    let left_blockers = keyed(&left.blockers, |item| item.id.clone());
    let right_blockers = keyed(&right.blockers, |item| item.id.clone());

    Ok(ReplayComparison {
        left: left.point,
        right: right.point,
        new_decisions: right_decisions
            .keys()
            .filter(|id| !left_decisions.contains_key(*id))
            .cloned()
            .collect(),
        superseded_decisions: right_decisions
            .iter()
            .filter(|(id, decision)| {
                decision.status == DecisionStatus::Superseded
                    && left_decisions
                        .get(*id)
                        .is_none_or(|old| old.status != DecisionStatus::Superseded)
            })
            .map(|(id, _)| id.clone())
            .collect(),
        completed_tasks: right_tasks
            .iter()
            .filter(|(id, task)| {
                task.status == TaskStatus::Completed
                    && left_tasks
                        .get(*id)
                        .is_none_or(|old| old.status != TaskStatus::Completed)
            })
            .map(|(id, _)| id.clone())
            .collect(),
        changed_files: right_files
            .iter()
            .filter(|(path, file)| {
                left_files
                    .get(*path)
                    .is_none_or(|old| old.last_changed_at != file.last_changed_at)
            })
            .map(|(path, _)| path.clone())
            .collect(),
        new_blockers: right_blockers
            .keys()
            .filter(|id| !left_blockers.contains_key(*id))
            .cloned()
            .collect(),
        resolved_blockers: left_blockers
            .keys()
            .filter(|id| !right_blockers.contains_key(*id))
            .cloned()
            .collect(),
        context_hash_changed: left
            .context
            .as_ref()
            .map(|item| &item.context_snapshot_hash)
            != right
                .context
                .as_ref()
                .map(|item| &item.context_snapshot_hash),
        plan_version_changed: left.plan.as_ref().map(|item| (&item.id, item.version))
            != right.plan.as_ref().map(|item| (&item.id, item.version)),
        environment_changed: left.environment.as_ref().map(|item| &item.fingerprint)
            != right.environment.as_ref().map(|item| &item.fingerprint),
    })
}

pub(crate) fn explain(
    source: &ReplaySource,
    input: &ExplainHistoryInput,
) -> Result<HistoryExplanation, String> {
    if let Some(id) = input.at_event_id.as_deref() {
        point_for_event(source, id)?;
    }
    let mut eligible = Vec::new();
    for event in source.events.iter().rev() {
        eligible.push(event);
        if input.at_event_id.as_ref() == Some(&event.id) {
            break;
        }
    }
    let mut evidence = eligible
        .into_iter()
        .filter(|event| value_mentions(&event.payload, &input.entity_id))
        .map(|event| ReplayEvidence {
            event_id: event.id.clone(),
            timestamp: event.created_at.clone(),
            kind: format!("{:?}", event.kind),
            explanation: event_summary(event),
            session_id: event.session_id.clone(),
            task_id: string_field(&event.payload, "taskId"),
            verification_id: string_field(&event.payload, "verificationId")
                .or_else(|| string_field(&event.payload, "suiteId")),
        })
        .collect::<Vec<_>>();
    evidence.sort_by(|left, right| left.timestamp.cmp(&right.timestamp));

    let title = entity_title(source, &input.entity_type, &input.entity_id)
        .unwrap_or_else(|| input.entity_id.clone());
    let introduced = evidence.first();
    let introduced_by = introduced.and_then(|item| {
        item.session_id
            .as_ref()
            .and_then(|id| source.sessions.iter().find(|session| &session.id == id))
            .map(|session| session.agent_name.clone())
            .or_else(|| {
                source
                    .events
                    .iter()
                    .find(|event| event.id == item.event_id)
                    .map(|event| format!("{:?}", event.source))
            })
    });
    let superseded_at = evidence
        .iter()
        .find(|item| item.kind.contains("Superseded") || item.kind.contains("Resolved"))
        .map(|item| item.timestamp.clone());
    Ok(HistoryExplanation {
        entity_type: input.entity_type.clone(),
        entity_id: input.entity_id.clone(),
        title,
        summary: if evidence.is_empty() {
            "No matching append-only events were recorded for this entity.".into()
        } else {
            format!(
                "{} recorded event(s) explain this {}.",
                evidence.len(),
                input.entity_type
            )
        },
        introduced_at: introduced.map(|item| item.timestamp.clone()),
        introduced_by,
        superseded_at,
        evidence,
    })
}

fn replay_at(source: &ReplaySource, point: ReplayPoint) -> ReplayState {
    let mut events = Vec::new();
    if let Some(event_id) = &point.event_id {
        for event in source.events.iter().rev() {
            events.push(event.clone());
            if &event.id == event_id {
                break;
            }
        }
    } else {
        for event in source.events.iter().rev() {
            if event.created_at <= point.timestamp {
                events.push(event.clone());
            }
        }
    }
    let (goals, decisions, accuracy, mut warnings) = reconstruct_entities(source, &events);
    let sessions = historical_sessions(&source.sessions, &events, &point.timestamp);
    let files = reconstruct_files(&events, &sessions);
    let blockers = reconstruct_blockers(&events);
    let git = events.iter().rev().find_map(event_git_state);
    let context = source
        .contexts
        .iter()
        .filter(|item| visible_at(&item.id, &events))
        .filter(|item| {
            point
                .session_id
                .as_ref()
                .is_none_or(|id| item.session_id.as_ref() == Some(id))
        })
        .max_by(|left, right| left.created_at.cmp(&right.created_at))
        .cloned()
        .or_else(|| {
            source
                .contexts
                .iter()
                .filter(|item| visible_at(&item.id, &events))
                .max_by(|left, right| left.created_at.cmp(&right.created_at))
                .cloned()
        });
    let plan = source
        .plans
        .iter()
        .filter(|item| visible_at(&item.id, &events))
        .max_by(|left, right| left.created_at.cmp(&right.created_at))
        .cloned()
        .map(|item| historical_plan(item, &events));
    let verification = source
        .verifications
        .iter()
        .filter(|item| visible_at(&item.id, &events))
        .max_by(|left, right| left.created_at.cmp(&right.created_at))
        .cloned()
        .map(|item| historical_verification(item, &events));
    let environment = source
        .environments
        .iter()
        .filter(|item| visible_at(&item.id, &events))
        .max_by(|left, right| left.captured_at.cmp(&right.captured_at))
        .cloned();
    if context.is_none() {
        warnings.push("No versioned context snapshot had been captured at this point.".into());
    }
    ReplayState {
        project_id: source.memory.project.id.clone(),
        repository: source.memory.project.path.clone(),
        point,
        accuracy,
        git,
        goals,
        decisions,
        sessions,
        files,
        blockers,
        context,
        plan,
        verification,
        environment,
        events,
        warnings,
    }
}

fn reconstruct_entities(
    source: &ReplaySource,
    events: &[ProjectEvent],
) -> (Vec<Goal>, Vec<Decision>, ReplayAccuracy, Vec<String>) {
    let current_goals = keyed(&source.memory.goals, |item| item.id.clone());
    let current_tasks = tasks_by_id(&source.memory.goals);
    let current_decisions = keyed(&source.memory.decisions, |item| item.id.clone());
    let mut goals = BTreeMap::<String, Goal>::new();
    let mut tasks = BTreeMap::<String, WorkTask>::new();
    let mut decisions = BTreeMap::<String, Decision>::new();
    let mut legacy = false;

    for event in events {
        match event.kind {
            EventKind::GoalCreated | EventKind::GoalUpdated => {
                if let Some(snapshot) = event.payload.get("goal").and_then(deserialize) {
                    let mut snapshot: Goal = snapshot;
                    snapshot.tasks.clear();
                    goals.insert(snapshot.id.clone(), snapshot);
                } else if let Some(id) = string_field(&event.payload, "goalId") {
                    legacy = true;
                    if let Some(current) = current_goals.get(&id) {
                        let goal = goals.entry(id).or_insert_with(|| {
                            let mut value = (*current).clone();
                            value.status = GoalStatus::Active;
                            value.tasks.clear();
                            value
                        });
                        if let Some(title) = string_field(&event.payload, "title") {
                            goal.title = title;
                        }
                        if let Some(status) = event.payload.get("status").and_then(deserialize) {
                            goal.status = status;
                        }
                        goal.updated_at = event.created_at.clone();
                    }
                }
            }
            EventKind::TaskCreated | EventKind::TaskUpdated | EventKind::TaskCompleted => {
                if let Some(snapshot) = event.payload.get("task").and_then(deserialize::<WorkTask>)
                {
                    tasks.insert(snapshot.id.clone(), snapshot);
                } else if let Some(id) = string_field(&event.payload, "taskId") {
                    legacy = true;
                    if let Some(current) = current_tasks.get(&id) {
                        let task = tasks.entry(id).or_insert_with(|| {
                            let mut value = (*current).clone();
                            value.status = TaskStatus::Todo;
                            value.completed_at = None;
                            value.assignment_status = TaskAssignmentStatus::Unassigned;
                            value
                        });
                        if let Some(title) = string_field(&event.payload, "title") {
                            task.title = title;
                        }
                        if let Some(status) = event.payload.get("status").and_then(deserialize) {
                            task.status = status;
                        }
                        if event.kind == EventKind::TaskCompleted {
                            task.status = TaskStatus::Completed;
                            task.completed_at = Some(event.created_at.clone());
                        }
                    }
                }
            }
            EventKind::TaskAssigned
            | EventKind::TaskAssignmentUpdated
            | EventKind::TaskRouteSuggested => {
                if let Some(id) = string_field(&event.payload, "taskId") {
                    if let Some(task) = tasks.get_mut(&id) {
                        if let Some(status) = event.payload.get("status").and_then(deserialize) {
                            task.assignment_status = status;
                        }
                        task.assigned_agent = string_field(&event.payload, "agent")
                            .or_else(|| task.assigned_agent.clone());
                    }
                }
            }
            EventKind::DecisionRecorded => {
                if let Some(snapshot) = event
                    .payload
                    .get("decision")
                    .and_then(deserialize::<Decision>)
                {
                    decisions.insert(snapshot.id.clone(), snapshot);
                } else if let Some(id) = string_field(&event.payload, "decisionId") {
                    legacy = true;
                    if let Some(current) = current_decisions.get(&id) {
                        let mut decision = (*current).clone();
                        decision.status = DecisionStatus::Active;
                        decision.superseded_by = None;
                        if let Some(summary) = string_field(&event.payload, "summary") {
                            decision.summary = summary;
                        }
                        decisions.insert(id, decision);
                    }
                }
            }
            EventKind::DecisionSuperseded => {
                if let Some(id) = string_field(&event.payload, "decisionId") {
                    if let Some(decision) = decisions.get_mut(&id) {
                        decision.status = DecisionStatus::Superseded;
                        decision.superseded_by = string_field(&event.payload, "supersededBy");
                    }
                }
            }
            _ => {}
        }
    }

    for task in tasks.into_values() {
        if let Some(goal) = goals.get_mut(&task.goal_id) {
            goal.tasks.push(task);
        }
    }
    for goal in goals.values_mut() {
        goal.tasks.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then(left.id.cmp(&right.id))
        });
    }
    let warnings = if legacy {
        vec![
            "Some pre-v0.16 events contain compact payloads. Their historical titles and descriptions use current records, while statuses are replayed from events.".into()
        ]
    } else {
        Vec::new()
    };
    (
        goals.into_values().collect(),
        decisions.into_values().collect(),
        if legacy {
            ReplayAccuracy::LegacyBestEffort
        } else {
            ReplayAccuracy::Exact
        },
        warnings,
    )
}

fn historical_sessions(
    sessions: &[AgentSession],
    events: &[ProjectEvent],
    timestamp: &str,
) -> Vec<AgentSession> {
    let selected_sessions = events
        .iter()
        .filter_map(|event| event.session_id.as_deref())
        .collect::<BTreeSet<_>>();
    let ended_sessions = events
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                EventKind::SessionEnded | EventKind::AgentSessionEnded
            )
        })
        .filter_map(|event| event.session_id.as_deref())
        .collect::<BTreeSet<_>>();
    sessions
        .iter()
        .filter(|item| {
            item.started_at.as_str() < timestamp || selected_sessions.contains(item.id.as_str())
        })
        .map(|item| {
            let mut historical = item.clone();
            if !ended_sessions.contains(item.id.as_str()) {
                historical.status = AgentSessionStatus::Active;
                historical.ended_at = None;
                historical.final_git_head = None;
                historical.final_git_state = None;
            }
            historical
        })
        .collect()
}

fn reconstruct_files(events: &[ProjectEvent], sessions: &[AgentSession]) -> Vec<HistoricalFile> {
    let session_tasks = sessions
        .iter()
        .map(|item| (item.id.as_str(), item.task_id.as_deref()))
        .collect::<BTreeMap<_, _>>();
    let mut files = BTreeMap::<String, HistoricalFile>::new();
    for event in events {
        if !matches!(
            event.kind,
            EventKind::FileChanged
                | EventKind::FileModified
                | EventKind::FileCreated
                | EventKind::FileDeleted
        ) {
            continue;
        }
        let Some(path) = string_field(&event.payload, "path") else {
            continue;
        };
        let change_type = string_field(&event.payload, "changeType").unwrap_or_else(|| {
            match event.kind {
                EventKind::FileCreated => "created",
                EventKind::FileDeleted => "deleted",
                _ => "modified",
            }
            .into()
        });
        let file = files.entry(path.clone()).or_insert_with(|| HistoricalFile {
            path,
            change_type: change_type.clone(),
            introduced_at: event.created_at.clone(),
            last_changed_at: event.created_at.clone(),
            session_ids: Vec::new(),
            task_ids: Vec::new(),
        });
        file.change_type = change_type;
        file.last_changed_at = event.created_at.clone();
        if let Some(session_id) = &event.session_id {
            if !file.session_ids.contains(session_id) {
                file.session_ids.push(session_id.clone());
            }
            if let Some(Some(task_id)) = session_tasks.get(session_id.as_str()) {
                if !file.task_ids.iter().any(|item| item == *task_id) {
                    file.task_ids.push((*task_id).to_owned());
                }
            }
        }
    }
    files.into_values().collect()
}

fn reconstruct_blockers(events: &[ProjectEvent]) -> Vec<HistoricalBlocker> {
    let mut blockers = BTreeMap::<String, HistoricalBlocker>::new();
    for event in events {
        match event.kind {
            EventKind::ConflictDetected
            | EventKind::VerificationFailed
            | EventKind::BuildFailed
            | EventKind::TestFailed
            | EventKind::LintFailed
            | EventKind::SecurityCheckFailed => {
                let id = string_field(&event.payload, "conflictId")
                    .or_else(|| string_field(&event.payload, "verificationId"))
                    .unwrap_or_else(|| event.id.clone());
                blockers.insert(
                    id.clone(),
                    HistoricalBlocker {
                        id,
                        kind: format!("{:?}", event.kind),
                        summary: event_summary(event),
                        introduced_at: event.created_at.clone(),
                        resolved_at: None,
                        session_id: event.session_id.clone(),
                        task_id: string_field(&event.payload, "taskId"),
                    },
                );
            }
            EventKind::ConflictResolved => {
                if let Some(id) = string_field(&event.payload, "conflictId") {
                    blockers.remove(&id);
                }
            }
            EventKind::VerificationPassed => {
                if let Some(task_id) = string_field(&event.payload, "taskId") {
                    blockers.retain(|_, item| item.task_id.as_ref() != Some(&task_id));
                }
            }
            _ => {}
        }
    }
    blockers.into_values().collect()
}

fn event_git_state(event: &ProjectEvent) -> Option<ReplayGitState> {
    if event.kind != EventKind::RepositoryScanned {
        return None;
    }
    let status: ProjectStatus = serde_json::from_value(event.payload.clone()).ok()?;
    let dirty = status.change_count() > 0;
    Some(ReplayGitState {
        commit: if status.head_commit.is_empty() {
            status.last_commit
        } else {
            status.head_commit
        },
        branch: status.branch,
        worktree: status.repository,
        dirty,
        modified_files: status.modified_files,
        added_files: status.added_files,
        deleted_files: status.deleted_files,
        observed_at: status.scanned_at,
    })
}

fn visible_at(id: &str, events: &[ProjectEvent]) -> bool {
    events
        .iter()
        .any(|event| value_mentions(&event.payload, id))
}

fn historical_plan(mut plan: PlanVersion, events: &[ProjectEvent]) -> PlanVersion {
    if let Some(status) = events
        .iter()
        .filter(|event| value_mentions(&event.payload, &plan.id))
        .filter_map(|event| event.payload.get("status").and_then(deserialize))
        .next_back()
    {
        plan.status = status;
    }
    if !events.iter().any(|event| {
        event.kind == EventKind::PlanApproved && value_mentions(&event.payload, &plan.id)
    }) {
        plan.approved_at = None;
    }
    plan
}

fn historical_verification(
    mut suite: VerificationSuite,
    events: &[ProjectEvent],
) -> VerificationSuite {
    let selected = events
        .iter()
        .filter(|event| value_mentions(&event.payload, &suite.id))
        .collect::<Vec<_>>();
    if let Some(status) = selected
        .iter()
        .filter_map(|event| event.payload.get("status").and_then(deserialize))
        .next_back()
    {
        suite.status = status;
    }
    if !selected.iter().any(|event| {
        matches!(
            event.kind,
            EventKind::VerificationPassed
                | EventKind::VerificationFailed
                | EventKind::VerificationWaived
        )
    }) {
        suite.completed_at = None;
    }
    for gate in &mut suite.gates {
        let completed = selected.iter().find(|event| {
            event.kind == EventKind::VerificationGateCompleted
                && value_mentions(&event.payload, &gate.id)
        });
        if let Some(event) = completed {
            if let Some(status) = event.payload.get("status").and_then(deserialize) {
                gate.status = status;
            }
        } else {
            gate.status = crate::domain::VerificationStatus::NotRun;
            gate.exit_code = None;
            gate.duration_ms = None;
            gate.stdout_summary.clear();
            gate.stderr_summary.clear();
            gate.finished_at = None;
        }
    }
    suite
}

fn resolve_point(source: &ReplaySource, request: &ReplayRequest) -> Result<ReplayPoint, String> {
    if let Some(id) = &request.event_id {
        return point_for_event(source, id);
    }
    if let Some(id) = &request.session_id {
        let session = source
            .sessions
            .iter()
            .find(|item| &item.id == id)
            .ok_or_else(|| format!("session {id} was not found"))?;
        let timestamp = session
            .ended_at
            .clone()
            .unwrap_or_else(|| session.started_at.clone());
        let event_id = source
            .events
            .iter()
            .find(|event| {
                event.session_id.as_ref() == Some(id)
                    && if session.ended_at.is_some() {
                        matches!(
                            event.kind,
                            EventKind::SessionEnded | EventKind::AgentSessionEnded
                        )
                    } else {
                        matches!(
                            event.kind,
                            EventKind::SessionStarted | EventKind::AgentSessionStarted
                        )
                    }
            })
            .map(|event| event.id.clone());
        return Ok(ReplayPoint {
            event_id,
            session_id: Some(id.clone()),
            timestamp,
            label: format!("{} session", session.agent_name),
        });
    }
    if let Some(timestamp) = &request.timestamp {
        return Ok(ReplayPoint {
            event_id: None,
            session_id: None,
            timestamp: timestamp.clone(),
            label: "Selected time".into(),
        });
    }
    let event = source
        .events
        .first()
        .ok_or_else(|| "the project has no recorded events".to_owned())?;
    point_for_event(source, &event.id)
}

fn point_for_event(source: &ReplaySource, id: &str) -> Result<ReplayPoint, String> {
    let event = source
        .events
        .iter()
        .find(|event| event.id == id)
        .ok_or_else(|| format!("event {id} was not found"))?;
    Ok(ReplayPoint {
        event_id: Some(event.id.clone()),
        session_id: event.session_id.clone(),
        timestamp: event.created_at.clone(),
        label: event_title(event),
    })
}

fn timeline_entry(event: &ProjectEvent) -> TimelineEntry {
    TimelineEntry {
        id: event.id.clone(),
        event_id: event.id.clone(),
        session_id: event.session_id.clone(),
        timestamp: event.created_at.clone(),
        category: event_category(event.kind),
        title: event_title(event),
        summary: event_summary(event),
        source: format!("{:?}", event.source),
        commit: event_commit(event),
        branch: string_field(&event.payload, "branch"),
    }
}

fn event_category(kind: EventKind) -> TimelineCategory {
    match kind {
        EventKind::RepositoryScanned
        | EventKind::FileChanged
        | EventKind::CommitDetected
        | EventKind::FileModified
        | EventKind::FileCreated
        | EventKind::FileDeleted
        | EventKind::CommitCreated
        | EventKind::BranchChanged => TimelineCategory::Git,
        EventKind::GoalCreated | EventKind::GoalUpdated => TimelineCategory::Goal,
        EventKind::TaskCreated
        | EventKind::TaskUpdated
        | EventKind::TaskCompleted
        | EventKind::TaskAssigned
        | EventKind::TaskAssignmentUpdated
        | EventKind::TaskRouteSuggested => TimelineCategory::Task,
        EventKind::DecisionRecorded | EventKind::DecisionSuperseded => TimelineCategory::Decision,
        EventKind::AgentSessionStarted
        | EventKind::AgentSessionEnded
        | EventKind::SessionStarted
        | EventKind::SessionIdle
        | EventKind::SessionEnded => TimelineCategory::Session,
        EventKind::ContextGenerated | EventKind::HandoffGenerated | EventKind::FreshnessChanged => {
            TimelineCategory::Context
        }
        EventKind::PlanGenerated
        | EventKind::PlanValidationFailed
        | EventKind::PlanApproved
        | EventKind::PlanReplanned
        | EventKind::RecoveryTaskProposed => TimelineCategory::Plan,
        EventKind::VerificationStarted
        | EventKind::VerificationGateCompleted
        | EventKind::VerificationPassed
        | EventKind::VerificationFailed
        | EventKind::VerificationWaived
        | EventKind::RepairTaskCreated => TimelineCategory::Verification,
        EventKind::EnvironmentManifestSaved
        | EventKind::EnvironmentCaptured
        | EventKind::EnvironmentDriftDetected
        | EventKind::EnvironmentCacheInvalidated => TimelineCategory::Environment,
        EventKind::ConflictDetected
        | EventKind::ConflictAcknowledged
        | EventKind::ConflictResolved => TimelineCategory::Conflict,
        EventKind::WorktreeReady
        | EventKind::ValidationPassed
        | EventKind::ValidationFailed
        | EventKind::MergeReady
        | EventKind::MergeConflictDetected
        | EventKind::MergeCompleted
        | EventKind::WorktreeArchived => TimelineCategory::Workspace,
        EventKind::ExternalIssueUpdated
        | EventKind::ExternalPullRequestUpdated
        | EventKind::ExternalCiUpdated
        | EventKind::ReviewCommentReceived
        | EventKind::DeliverySynced
        | EventKind::PrReady
        | EventKind::PrMerged => TimelineCategory::Delivery,
        _ => TimelineCategory::Other,
    }
}

fn event_title(event: &ProjectEvent) -> String {
    match event.kind {
        EventKind::AgentSessionStarted | EventKind::SessionStarted => {
            "Agent session started".into()
        }
        EventKind::AgentSessionEnded | EventKind::SessionEnded => "Agent session ended".into(),
        EventKind::DecisionRecorded => "Decision recorded".into(),
        EventKind::DecisionSuperseded => "Decision superseded".into(),
        EventKind::TaskCreated => "Task created".into(),
        EventKind::TaskCompleted => "Task completed".into(),
        EventKind::VerificationPassed => "Verification passed".into(),
        EventKind::VerificationFailed => "Verification failed".into(),
        EventKind::MergeReady => "Merge ready".into(),
        _ => words(&format!("{:?}", event.kind)),
    }
}

fn event_summary(event: &ProjectEvent) -> String {
    for key in ["summary", "title", "path", "reason", "command", "status"] {
        if let Some(value) = string_field(&event.payload, key) {
            return value;
        }
    }
    let mut ids = Vec::new();
    for key in [
        "goalId",
        "taskId",
        "decisionId",
        "conflictId",
        "workspaceId",
        "verificationId",
    ] {
        if let Some(value) = string_field(&event.payload, key) {
            ids.push(format!("{key}: {value}"));
        }
    }
    if ids.is_empty() {
        event_title(event)
    } else {
        ids.join(" · ")
    }
}

fn event_commit(event: &ProjectEvent) -> Option<String> {
    string_field(&event.payload, "head")
        .or_else(|| string_field(&event.payload, "headCommit"))
        .or_else(|| string_field(&event.payload, "commit"))
        .or_else(|| {
            event
                .payload
                .get("recentCommits")
                .and_then(Value::as_array)
                .and_then(|values| values.first())
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

fn entity_title(source: &ReplaySource, entity_type: &str, id: &str) -> Option<String> {
    match entity_type.to_ascii_lowercase().as_str() {
        "goal" => source
            .memory
            .goals
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.title.clone()),
        "task" => source
            .memory
            .goals
            .iter()
            .flat_map(|goal| &goal.tasks)
            .find(|item| item.id == id)
            .map(|item| item.title.clone()),
        "decision" => source
            .memory
            .decisions
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.summary.clone()),
        "session" => source
            .sessions
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.agent_name.clone()),
        "file" => Some(id.to_owned()),
        _ => None,
    }
}

fn value_mentions(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(text) => text == needle,
        Value::Array(values) => values.iter().any(|value| value_mentions(value, needle)),
        Value::Object(values) => values.values().any(|value| value_mentions(value, needle)),
        _ => false,
    }
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn deserialize<T: serde::de::DeserializeOwned>(value: &Value) -> Option<T> {
    serde_json::from_value(value.clone()).ok()
}

fn keyed<T>(items: &[T], key: impl Fn(&T) -> String) -> BTreeMap<String, &T> {
    items.iter().map(|item| (key(item), item)).collect()
}

fn tasks_by_id(goals: &[Goal]) -> BTreeMap<String, &WorkTask> {
    goals
        .iter()
        .flat_map(|goal| &goal.tasks)
        .map(|task| (task.id.clone(), task))
        .collect()
}

fn words(value: &str) -> String {
    let mut result = String::new();
    for character in value.chars() {
        if character.is_uppercase() && !result.is_empty() {
            result.push(' ');
        }
        result.push(character);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_separates_event_names() {
        assert_eq!(words("VerificationFailed"), "Verification Failed");
    }

    #[test]
    fn value_matching_is_exact_for_strings() {
        let value = serde_json::json!({"taskId":"task-1","nested":["file.rs"]});
        assert!(value_mentions(&value, "task-1"));
        assert!(!value_mentions(&value, "task"));
    }
}
