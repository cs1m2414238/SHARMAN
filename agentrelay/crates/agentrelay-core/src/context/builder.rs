use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    domain::{
        Capability, ConflictKind, ConflictSeverity, ConflictStatus, DecisionStatus, EventKind,
        FreshnessStatus, GoalStatus, ProjectMemory, TaskPriority, TaskStatus,
    },
    ports::{CompressionRequest, CompressionResponse, GroundedFact},
};

const CONTEXT_OVERHEAD_TOKENS: usize = 260;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ContextDepth {
    Compact,
    Normal,
    Deep,
}

impl ContextDepth {
    pub const fn token_budget(self) -> usize {
        match self {
            Self::Compact => 2_000,
            Self::Normal => 6_000,
            Self::Deep => 12_000,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ContextProfile {
    Balanced,
    Architecture,
    Implementation,
    Product,
}

impl ContextProfile {
    pub fn for_capabilities(capabilities: &[Capability]) -> Self {
        if capabilities.contains(&Capability::Architecture)
            && capabilities.contains(&Capability::Refactoring)
        {
            Self::Architecture
        } else if capabilities.contains(&Capability::Backend)
            || capabilities.contains(&Capability::Frontend)
            || capabilities.contains(&Capability::Testing)
        {
            Self::Implementation
        } else if capabilities.contains(&Capability::Research)
            || capabilities.contains(&Capability::Documentation)
        {
            Self::Product
        } else {
            Self::Balanced
        }
    }

    pub const fn as_label(self) -> &'static str {
        match self {
            Self::Balanced => "balanced",
            Self::Architecture => "architecture",
            Self::Implementation => "implementation",
            Self::Product => "product",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContextRequest {
    pub project_id: String,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub token_budget: usize,
    pub profile: ContextProfile,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContextCompileInput {
    pub repository_path: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    pub depth: ContextDepth,
    #[serde(default)]
    pub profile: Option<ContextProfile>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum ContextTier {
    Critical,
    Active,
    Recent,
    Historical,
    Archived,
}

impl ContextTier {
    pub const fn as_label(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::Active => "active",
            Self::Recent => "recent",
            Self::Historical => "historical",
            Self::Archived => "archived",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ContextItemKind {
    Repository,
    Goal,
    Task,
    Decision,
    Conflict,
    File,
    Commit,
    Blocker,
    Session,
    Event,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    Project,
    Goal,
    Task,
    Decision,
    Conflict,
    Event,
    Session,
    Handoff,
    GitCommit,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourceReference {
    pub key: String,
    pub kind: SourceKind,
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContextItem {
    pub id: String,
    pub kind: ContextItemKind,
    pub title: String,
    pub content: String,
    pub tier: ContextTier,
    pub relevance_score: u16,
    pub freshness: Option<FreshnessStatus>,
    pub recorded_at: Option<String>,
    pub sources: Vec<SourceReference>,
    pub inclusion_reasons: Vec<String>,
    pub estimated_tokens: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CompressionMode {
    DeterministicOnly,
    NemotronAvailable,
    NemotronValidated,
    NemotronFallback,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompressionState {
    pub mode: CompressionMode,
    pub model: Option<String>,
    pub detail: String,
}

impl CompressionState {
    pub fn deterministic(detail: impl Into<String>) -> Self {
        Self {
            mode: CompressionMode::DeterministicOnly,
            model: None,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SummaryBullet {
    pub text: String,
    pub source_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedSummary {
    pub provider: String,
    pub model: String,
    pub bullets: Vec<SummaryBullet>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContextSnapshot {
    pub project_name: String,
    pub repository: String,
    pub request: ContextRequest,
    pub items: Vec<ContextItem>,
    pub summary: Option<ValidatedSummary>,
    pub compression: CompressionState,
    pub estimated_tokens: usize,
    pub omitted_item_count: usize,
    pub recommended_next_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContextPackage {
    pub snapshot: ContextSnapshot,
    pub markdown: String,
}

pub fn default_request(memory: &ProjectMemory) -> ContextRequest {
    ContextRequest {
        project_id: memory.project.id.clone(),
        session_id: memory
            .agent_sessions
            .iter()
            .find(|session| session.status == crate::domain::AgentSessionStatus::Active)
            .map(|session| session.id.clone()),
        task_id: None,
        token_budget: ContextDepth::Normal.token_budget(),
        profile: ContextProfile::Balanced,
    }
}

pub fn build_snapshot(memory: &ProjectMemory) -> ContextSnapshot {
    compile_snapshot(memory, default_request(memory))
}

pub fn compile_snapshot(memory: &ProjectMemory, mut request: ContextRequest) -> ContextSnapshot {
    request.token_budget = request.token_budget.clamp(512, 32_000);
    let mut candidates = collect_candidates(memory, &request);
    apply_recency_and_dependencies(memory, &request, &mut candidates);
    candidates.sort_by(|left, right| {
        left.tier
            .cmp(&right.tier)
            .then_with(|| right.relevance_score.cmp(&left.relevance_score))
            .then_with(|| left.id.cmp(&right.id))
    });

    let candidate_count = candidates.len();
    let max_tier = if request.token_budget <= ContextDepth::Compact.token_budget() {
        ContextTier::Recent
    } else if request.token_budget <= ContextDepth::Normal.token_budget() {
        ContextTier::Historical
    } else {
        ContextTier::Archived
    };
    let selection_budget = request
        .token_budget
        .saturating_sub(CONTEXT_OVERHEAD_TOKENS)
        .max(128);
    let mut used = 0;
    let mut items = Vec::new();
    for mut item in candidates {
        if item.tier > max_tier {
            continue;
        }
        item.estimated_tokens = estimate_item_tokens(&item);
        if used + item.estimated_tokens <= selection_budget {
            used += item.estimated_tokens;
            items.push(item);
            continue;
        }
        let remaining = selection_budget.saturating_sub(used);
        if item.tier <= ContextTier::Active && remaining >= 48 {
            item.content = truncate_to_tokens(&item.content, remaining.saturating_sub(32));
            item.estimated_tokens = estimate_item_tokens(&item);
            if item.estimated_tokens <= remaining {
                used += item.estimated_tokens;
                items.push(item);
            }
        }
    }

    let recommended_next_action = memory
        .goals
        .iter()
        .flat_map(|goal| &goal.tasks)
        .find(|task| {
            request.task_id.as_deref() == Some(task.id.as_str())
                || matches!(task.status, TaskStatus::InProgress | TaskStatus::Todo)
        })
        .map(|task| task.title.clone())
        .unwrap_or_else(|| "Create the next concrete task for the active goal.".to_owned());

    ContextSnapshot {
        project_name: memory.project.name.clone(),
        repository: memory.project.path.clone(),
        request,
        omitted_item_count: candidate_count.saturating_sub(items.len()),
        estimated_tokens: used + CONTEXT_OVERHEAD_TOKENS,
        items,
        summary: None,
        compression: CompressionState::deterministic(
            "Grounded items selected without model-generated text.",
        ),
        recommended_next_action,
    }
}

pub fn compression_request(snapshot: &ContextSnapshot) -> CompressionRequest {
    let facts = snapshot
        .items
        .iter()
        .map(|item| GroundedFact {
            source_key: item.sources[0].key.clone(),
            tier: item.tier.as_label().into(),
            title: item.title.clone(),
            content: item.content.clone(),
        })
        .collect();
    CompressionRequest {
        project_name: snapshot.project_name.clone(),
        profile: snapshot.request.profile.as_label().into(),
        max_output_tokens: (snapshot.request.token_budget / 5).clamp(128, 1_024),
        facts,
    }
}

pub fn validate_compression(
    snapshot: &ContextSnapshot,
    response: CompressionResponse,
) -> Result<ValidatedSummary, String> {
    if response.bullets.is_empty() {
        return Err("Nemotron returned no summary bullets".into());
    }
    let allowed = snapshot
        .items
        .iter()
        .flat_map(|item| item.sources.iter().map(|source| source.key.as_str()))
        .collect::<BTreeSet<_>>();
    let mut summary_tokens = 0;
    let mut bullets = Vec::new();
    for bullet in response.bullets {
        let text = bullet.text.trim();
        if text.is_empty() || bullet.source_keys.is_empty() {
            return Err("every Nemotron bullet must contain text and at least one source".into());
        }
        if let Some(unknown) = bullet
            .source_keys
            .iter()
            .find(|source| !allowed.contains(source.as_str()))
        {
            return Err(format!("Nemotron cited an unknown source key: {unknown}"));
        }
        summary_tokens += estimate_tokens(text);
        bullets.push(SummaryBullet {
            text: text.to_owned(),
            source_keys: bullet.source_keys,
        });
    }
    let summary_limit = (snapshot.request.token_budget / 5).clamp(128, 1_024);
    if summary_tokens > summary_limit {
        return Err(format!(
            "Nemotron summary exceeded its {summary_limit}-token allowance"
        ));
    }
    Ok(ValidatedSummary {
        provider: "Nebius Token Factory".into(),
        model: response.model,
        bullets,
    })
}

fn collect_candidates(memory: &ProjectMemory, request: &ContextRequest) -> Vec<ContextItem> {
    let mut items = Vec::new();
    if let Some(git) = &memory.project.latest_status {
        let changed = git.modified_files.len() + git.added_files.len() + git.deleted_files.len();
        items.push(item(
            format!("repository:{}", memory.project.id),
            ContextItemKind::Repository,
            "Current repository state".into(),
            format!(
                "Branch `{}` at `{}` with {changed} working-tree change(s). Last commit: {}.",
                git.branch, git.head_commit, git.last_commit
            ),
            ContextTier::Active,
            84,
            Some(FreshnessStatus::Current),
            Some(git.scanned_at.clone()),
            vec![source(
                SourceKind::Project,
                &memory.project.id,
                format!("Project {}", memory.project.name),
            )],
            vec!["Defines the current Git baseline.".into()],
        ));
    }

    for goal in &memory.goals {
        let (tier, score) = match goal.status {
            GoalStatus::Active => (ContextTier::Active, 82),
            GoalStatus::Completed => (ContextTier::Historical, 38),
            GoalStatus::Archived => (ContextTier::Archived, 8),
        };
        items.push(item(
            format!("goal:{}", goal.id),
            ContextItemKind::Goal,
            goal.title.clone(),
            if goal.description.is_empty() {
                format!("Goal status: {:?}.", goal.status)
            } else {
                goal.description.clone()
            },
            tier,
            score,
            Some(if goal.status == GoalStatus::Archived {
                FreshnessStatus::Invalidated
            } else {
                FreshnessStatus::Current
            }),
            Some(goal.updated_at.clone()),
            vec![source(
                SourceKind::Goal,
                &goal.id,
                reference_label("Goal", &goal.id),
            )],
            vec![if goal.status == GoalStatus::Active {
                "This is an active project goal.".into()
            } else {
                "Retained for project history.".into()
            }],
        ));
        for task in &goal.tasks {
            let selected = request.task_id.as_deref() == Some(task.id.as_str());
            let (tier, mut score) = match task.status {
                TaskStatus::InProgress => (ContextTier::Active, 92),
                TaskStatus::Todo => (ContextTier::Active, 78),
                TaskStatus::Completed => (ContextTier::Recent, 48),
                TaskStatus::Archived => (ContextTier::Archived, 5),
            };
            if selected {
                score += 40;
            }
            if task.priority == TaskPriority::Critical {
                score += 12;
            }
            let mut reasons = vec![format!("Task belongs to goal “{}”.", goal.title)];
            if selected {
                reasons.push("Explicitly selected for this context request.".into());
            } else if task.status == TaskStatus::InProgress {
                reasons.push("Task is currently in progress.".into());
            } else if task.status == TaskStatus::Todo {
                reasons.push("Task is an available next step.".into());
            }
            let mut sources = vec![source(
                SourceKind::Task,
                &task.id,
                reference_label("Task", &task.id),
            )];
            sources.push(source(
                SourceKind::Goal,
                &goal.id,
                reference_label("Goal", &goal.id),
            ));
            items.push(item(
                format!("task:{}", task.id),
                ContextItemKind::Task,
                task.title.clone(),
                format!(
                    "Status: {:?}. Assignment: {:?}. Priority: {:?}.{}{}{}{}",
                    task.status,
                    task.assignment_status,
                    task.priority,
                    task.assigned_agent
                        .as_ref()
                        .map(|agent| format!(" Assigned to {agent}."))
                        .unwrap_or_default(),
                    if task.description.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", task.description)
                    },
                    if task.depends_on.is_empty() && task.blocked_by.is_empty() {
                        String::new()
                    } else {
                        format!(
                            " Dependencies: {}.",
                            task.depends_on
                                .iter()
                                .chain(&task.blocked_by)
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    },
                    if task.claimed_paths.is_empty() {
                        String::new()
                    } else {
                        format!(" Workspace claims: {}.", task.claimed_paths.join(", "))
                    },
                ),
                if selected {
                    ContextTier::Critical
                } else {
                    tier
                },
                score,
                Some(if task.status == TaskStatus::Archived {
                    FreshnessStatus::Invalidated
                } else {
                    FreshnessStatus::Current
                }),
                task.completed_at
                    .clone()
                    .or_else(|| Some(task.created_at.clone())),
                sources,
                reasons,
            ));
        }
    }

    for decision in &memory.decisions {
        let freshness = memory
            .freshness
            .iter()
            .find(|entry| entry.subject_type == "decision" && entry.subject_id == decision.id)
            .map(|entry| entry.status)
            .unwrap_or(match decision.status {
                DecisionStatus::Active => FreshnessStatus::Current,
                DecisionStatus::Superseded => FreshnessStatus::Superseded,
                DecisionStatus::Reversed => FreshnessStatus::Invalidated,
            });
        let active = decision.status == DecisionStatus::Active
            && !matches!(
                freshness,
                FreshnessStatus::Superseded | FreshnessStatus::Invalidated
            );
        let (tier, score, prefix) = if active {
            (ContextTier::Active, 86, String::new())
        } else {
            (
                ContextTier::Archived,
                4,
                format!("[{:?} — not current guidance] ", decision.status),
            )
        };
        let mut reasons = vec![if active {
            "This architectural decision is current.".into()
        } else {
            "Retained only as explicitly invalidated history.".into()
        }];
        if let Some(replacement) = &decision.superseded_by {
            reasons.push(format!(
                "Superseded by Decision #{}.",
                short_id(replacement)
            ));
        }
        let mut sources = vec![source(
            SourceKind::Decision,
            &decision.id,
            reference_label("Decision", &decision.id),
        )];
        sources.extend(
            decision
                .affected
                .iter()
                .map(|path| source(SourceKind::File, path, format!("File {path}"))),
        );
        items.push(item(
            format!("decision:{}", decision.id),
            ContextItemKind::Decision,
            decision.summary.clone(),
            format!("{prefix}Reason: {}", decision.reason),
            tier,
            score,
            Some(freshness),
            Some(decision.created_at.clone()),
            sources,
            reasons,
        ));
    }

    for conflict in &memory.conflicts {
        // This warning describes the projection being replaced. Carrying it into the newly
        // compiled context would immediately make that context misleading.
        if conflict.kind == ConflictKind::StaleContext {
            continue;
        }
        let open = conflict.status != ConflictStatus::Resolved;
        let tier = if open {
            ContextTier::Critical
        } else {
            ContextTier::Archived
        };
        let severity_score = match conflict.severity {
            ConflictSeverity::Critical => 18,
            ConflictSeverity::Warning => 10,
            ConflictSeverity::Info => 2,
        };
        let mut sources = vec![source(
            SourceKind::Conflict,
            &conflict.id,
            reference_label("Conflict", &conflict.id),
        )];
        sources.extend(
            conflict
                .affected_paths
                .iter()
                .map(|path| source(SourceKind::File, path, format!("File {path}"))),
        );
        items.push(item(
            format!("conflict:{}", conflict.id),
            ContextItemKind::Conflict,
            conflict.title.clone(),
            conflict.description.clone(),
            tier,
            if open { 100 + severity_score } else { 3 },
            Some(if open {
                FreshnessStatus::Stale
            } else {
                FreshnessStatus::Current
            }),
            Some(conflict.updated_at.clone()),
            sources,
            vec![if open {
                "Unresolved conflict can make the next agent's assumptions unsafe.".into()
            } else {
                "Resolved conflict retained for audit history.".into()
            }],
        ));
    }

    collect_handoff_candidates(memory, &mut items);
    collect_file_candidates(memory, &mut items);
    collect_event_candidates(memory, &mut items);
    collect_session_candidate(memory, request, &mut items);
    items
}

fn collect_handoff_candidates(memory: &ProjectMemory, items: &mut Vec<ContextItem>) {
    let Some(handoff) = &memory.latest_handoff else {
        return;
    };
    for (index, blocker) in handoff.blockers.iter().enumerate() {
        items.push(item(
            format!("blocker:{}:{index}", handoff.id),
            ContextItemKind::Blocker,
            "Unresolved handoff blocker".into(),
            blocker.description.clone(),
            ContextTier::Critical,
            116,
            Some(FreshnessStatus::Current),
            Some(handoff.created_at.clone()),
            vec![source(
                SourceKind::Handoff,
                &handoff.id,
                reference_label("Handoff", &handoff.id),
            )],
            vec!["The previous agent explicitly reported this blocker.".into()],
        ));
    }
    for commit in &handoff.commits {
        let commit_id = commit.split_whitespace().next().unwrap_or(commit);
        items.push(item(
            format!("commit:{commit_id}"),
            ContextItemKind::Commit,
            format!("Commit {commit_id}"),
            commit.clone(),
            ContextTier::Recent,
            58,
            Some(FreshnessStatus::Current),
            Some(handoff.created_at.clone()),
            vec![source(
                SourceKind::GitCommit,
                commit_id,
                format!("git commit {commit_id}"),
            )],
            vec!["Created during the latest completed agent session.".into()],
        ));
    }
}

fn collect_file_candidates(memory: &ProjectMemory, items: &mut Vec<ContextItem>) {
    let mut files = BTreeMap::<String, (String, Option<String>, Vec<String>)>::new();
    if let Some(git) = &memory.project.latest_status {
        for (paths, state) in [
            (&git.modified_files, "modified"),
            (&git.added_files, "created"),
            (&git.deleted_files, "deleted"),
        ] {
            for path in paths {
                files.insert(
                    path.clone(),
                    (
                        state.into(),
                        Some(git.scanned_at.clone()),
                        vec!["Present in the latest working-tree scan.".into()],
                    ),
                );
            }
        }
    }
    if let Some(handoff) = &memory.latest_handoff {
        for changed in &handoff.changed_files {
            let entry = files.entry(changed.path.clone()).or_insert_with(|| {
                (
                    format!("{:?}", changed.change_type).to_ascii_lowercase(),
                    Some(handoff.created_at.clone()),
                    Vec::new(),
                )
            });
            entry
                .2
                .push("Touched by the previous agent's latest handoff.".into());
        }
    }
    for (path, (state, recorded_at, reasons)) in files {
        items.push(item(
            format!("file:{path}"),
            ContextItemKind::File,
            path.clone(),
            format!("File is currently {state}."),
            ContextTier::Active,
            80,
            Some(FreshnessStatus::Current),
            recorded_at,
            vec![source(SourceKind::File, &path, format!("File {path}"))],
            reasons,
        ));
    }
}

fn collect_event_candidates(memory: &ProjectMemory, items: &mut Vec<ContextItem>) {
    for (index, event) in memory.events.iter().take(80).enumerate() {
        if matches!(
            event.kind,
            EventKind::RepositoryScanned
                | EventKind::FreshnessChanged
                | EventKind::ContextGenerated
        ) {
            continue;
        }
        let verification = matches!(
            event.kind,
            EventKind::ActionCompleted | EventKind::ActionFailed
        );
        let tier = if verification {
            ContextTier::Active
        } else if index < 12 {
            ContextTier::Recent
        } else {
            ContextTier::Historical
        };
        items.push(item(
            format!("event:{}", event.id),
            ContextItemKind::Event,
            event_title(event.kind),
            compact_json(&event.payload),
            tier,
            if verification {
                86
            } else if index < 5 {
                46
            } else {
                24
            },
            Some(FreshnessStatus::Current),
            Some(event.created_at.clone()),
            vec![source(
                SourceKind::Event,
                &event.id,
                reference_label("Event", &event.id),
            )],
            vec![if verification {
                "Latest sandbox verification result for the current project state.".into()
            } else if index < 12 {
                "One of the most recent durable project events.".into()
            } else {
                "Included as lower-priority project history.".into()
            }],
        ));
    }
}

fn collect_session_candidate(
    memory: &ProjectMemory,
    request: &ContextRequest,
    items: &mut Vec<ContextItem>,
) {
    let session = request
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
                .find(|session| session.status == crate::domain::AgentSessionStatus::Active)
        })
        .or(memory.last_agent_session.as_ref());
    let Some(session) = session else {
        return;
    };
    items.push(item(
        format!("session:{}", session.id),
        ContextItemKind::Session,
        format!("{} agent session", session.agent_name),
        format!(
            "Status: {:?}. Summary: {}",
            session.status, session.summary_status
        ),
        if session.status == crate::domain::AgentSessionStatus::Active {
            ContextTier::Active
        } else {
            ContextTier::Recent
        },
        74,
        Some(FreshnessStatus::Current),
        session
            .ended_at
            .clone()
            .or_else(|| Some(session.started_at.clone())),
        vec![source(
            SourceKind::Session,
            &session.id,
            reference_label("Session", &session.id),
        )],
        vec![
            if session.status == crate::domain::AgentSessionStatus::Active {
                "This is the active agent session.".into()
            } else {
                "This is the latest completed agent session.".into()
            },
        ],
    ));
}

fn apply_recency_and_dependencies(
    memory: &ProjectMemory,
    request: &ContextRequest,
    items: &mut [ContextItem],
) {
    let mut timestamps = items
        .iter()
        .filter_map(|item| item.recorded_at.clone())
        .collect::<Vec<_>>();
    timestamps.sort_by(|left, right| right.cmp(left));
    timestamps.dedup();
    let active_decision_paths = memory
        .decisions
        .iter()
        .filter(|decision| decision.status == DecisionStatus::Active)
        .flat_map(|decision| {
            decision
                .affected
                .iter()
                .map(|path| path.to_ascii_lowercase())
        })
        .collect::<BTreeSet<_>>();
    for item in items {
        if let Some(timestamp) = &item.recorded_at {
            let rank = timestamps
                .iter()
                .position(|candidate| candidate == timestamp)
                .unwrap_or(usize::MAX);
            let bonus = match rank {
                0..=4 => 15,
                5..=11 => 10,
                12..=24 => 5,
                _ => 0,
            };
            if bonus > 0 {
                item.relevance_score += bonus;
                item.inclusion_reasons
                    .push("Ranks among the most recent relevant records.".into());
            }
        }
        if item.kind == ContextItemKind::File
            && item
                .sources
                .iter()
                .any(|source| active_decision_paths.contains(&source.id.to_ascii_lowercase()))
        {
            item.relevance_score += 12;
            item.inclusion_reasons
                .push("Referenced by an active architectural decision.".into());
        }
        let profile_bonus = match (request.profile, item.kind) {
            (
                ContextProfile::Architecture,
                ContextItemKind::Decision | ContextItemKind::Conflict,
            ) => 16,
            (
                ContextProfile::Implementation,
                ContextItemKind::Task
                | ContextItemKind::File
                | ContextItemKind::Commit
                | ContextItemKind::Blocker,
            ) => 16,
            (ContextProfile::Product, ContextItemKind::Goal | ContextItemKind::Decision) => 14,
            (ContextProfile::Balanced, _) => 4,
            _ => 0,
        };
        if profile_bonus > 0 {
            item.relevance_score += profile_bonus;
            item.inclusion_reasons.push(format!(
                "Prioritized by the {} context profile.",
                request.profile.as_label()
            ));
        }
        item.relevance_score = item.relevance_score.min(150);
    }
}

#[allow(clippy::too_many_arguments)]
fn item(
    id: String,
    kind: ContextItemKind,
    title: String,
    content: String,
    tier: ContextTier,
    relevance_score: u16,
    freshness: Option<FreshnessStatus>,
    recorded_at: Option<String>,
    sources: Vec<SourceReference>,
    inclusion_reasons: Vec<String>,
) -> ContextItem {
    ContextItem {
        id,
        kind,
        title,
        content,
        tier,
        relevance_score,
        freshness,
        recorded_at,
        sources,
        inclusion_reasons,
        estimated_tokens: 0,
    }
}

fn source(kind: SourceKind, id: &str, label: String) -> SourceReference {
    let prefix = match kind {
        SourceKind::Project => "project",
        SourceKind::Goal => "goal",
        SourceKind::Task => "task",
        SourceKind::Decision => "decision",
        SourceKind::Conflict => "conflict",
        SourceKind::Event => "event",
        SourceKind::Session => "session",
        SourceKind::Handoff => "handoff",
        SourceKind::GitCommit => "commit",
        SourceKind::File => "file",
    };
    SourceReference {
        key: format!("{prefix}:{id}"),
        kind,
        id: id.to_owned(),
        label,
    }
}

fn reference_label(kind: &str, id: &str) -> String {
    format!("{kind} #{}", short_id(id))
}

fn short_id(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

fn compact_json(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".into())
}

fn event_title(kind: EventKind) -> String {
    let debug = format!("{kind:?}");
    let mut output = String::new();
    for (index, character) in debug.chars().enumerate() {
        if index > 0 && character.is_ascii_uppercase() {
            output.push(' ');
        }
        output.push(character);
    }
    output
}

pub fn estimate_tokens(value: &str) -> usize {
    value.chars().count().div_ceil(4).max(1)
}

fn estimate_item_tokens(item: &ContextItem) -> usize {
    estimate_tokens(&item.title)
        + estimate_tokens(&item.content)
        + item
            .inclusion_reasons
            .iter()
            .map(|reason| estimate_tokens(reason))
            .sum::<usize>()
        + item
            .sources
            .iter()
            .map(|source| estimate_tokens(&source.label))
            .sum::<usize>()
        + 18
}

fn truncate_to_tokens(value: &str, token_limit: usize) -> String {
    let character_limit = token_limit.saturating_mul(4);
    if value.chars().count() <= character_limit {
        return value.to_owned();
    }
    let mut truncated = value
        .chars()
        .take(character_limit.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{
            AgentSession, ConflictKind, ConflictRecord, Decision, Goal, HandoffSnapshot,
            PermissionMode, ProjectPermissions, ProjectRecord, WorkTask,
        },
        git::ProjectStatus,
        ports::{CompressedBullet, CompressionResponse},
    };

    fn memory() -> ProjectMemory {
        let project_id = "project-1".to_owned();
        ProjectMemory {
            project: ProjectRecord {
                id: project_id.clone(),
                path: "C:/demo".into(),
                name: "demo".into(),
                branch: "main".into(),
                last_commit: "Current".into(),
                change_count: 1,
                last_scanned_at: "2026-09-20T10:00:00Z".into(),
                tracking_enabled: true,
                latest_status: Some(ProjectStatus {
                    repository: "C:/demo".into(),
                    branch: "main".into(),
                    modified_files: vec!["src/auth.rs".into()],
                    added_files: Vec::new(),
                    deleted_files: Vec::new(),
                    last_commit: "Current".into(),
                    head_commit: "abc123".into(),
                    recent_commits: Vec::new(),
                    diff_stat: Vec::new(),
                    scanned_at: "2026-09-20T10:00:00Z".into(),
                }),
            },
            goals: vec![Goal {
                id: "goal-1".into(),
                project_id: project_id.clone(),
                title: "Ship auth".into(),
                description: "Finish authentication".into(),
                status: GoalStatus::Active,
                created_at: "2026-09-20T08:00:00Z".into(),
                updated_at: "2026-09-20T09:00:00Z".into(),
                tasks: vec![WorkTask {
                    id: "task-1".into(),
                    goal_id: "goal-1".into(),
                    title: "Implement tokens".into(),
                    description: String::new(),
                    status: TaskStatus::InProgress,
                    priority: TaskPriority::High,
                    assigned_agent: Some("Codex".into()),
                    source_agent: Some("user".into()),
                    created_at: "2026-09-20T09:00:00Z".into(),
                    completed_at: None,
                    assignment_status: crate::domain::TaskAssignmentStatus::Unassigned,
                    suggested_provider: None,
                    assigned_provider: None,
                    route_reasons: Vec::new(),
                    required_capabilities: Vec::new(),
                    blocked_by: Vec::new(),
                    depends_on: Vec::new(),
                    produces: Vec::new(),
                    consumes: Vec::new(),
                    claimed_paths: Vec::new(),
                }],
            }],
            decisions: vec![
                Decision {
                    id: "decision-current".into(),
                    project_id: project_id.clone(),
                    summary: "Use PostgreSQL".into(),
                    reason: "Shared deployment".into(),
                    alternatives: vec!["SQLite".into()],
                    affected: vec!["src/auth.rs".into()],
                    status: DecisionStatus::Active,
                    created_by: "Claude".into(),
                    superseded_by: None,
                    created_at: "2026-09-20T09:30:00Z".into(),
                },
                Decision {
                    id: "decision-old".into(),
                    project_id: project_id.clone(),
                    summary: "Use SQLite".into(),
                    reason: "Old design".into(),
                    alternatives: Vec::new(),
                    affected: vec!["src/auth.rs".into()],
                    status: DecisionStatus::Superseded,
                    created_by: "Claude".into(),
                    superseded_by: Some("decision-current".into()),
                    created_at: "2026-09-19T09:30:00Z".into(),
                },
            ],
            events: Vec::new(),
            agent_sessions: Vec::<AgentSession>::new(),
            last_agent_session: None,
            latest_handoff: None::<HandoffSnapshot>,
            handoffs: Vec::new(),
            conflicts: vec![ConflictRecord {
                id: "conflict-1".into(),
                project_id: project_id.clone(),
                fingerprint: "fingerprint".into(),
                primary_session_id: None,
                related_session_id: None,
                kind: ConflictKind::DecisionContradiction,
                severity: ConflictSeverity::Warning,
                title: "Decision changed".into(),
                description: "An older decision was superseded.".into(),
                affected_paths: vec!["src/auth.rs".into()],
                related_decision_ids: vec!["decision-old".into(), "decision-current".into()],
                status: ConflictStatus::Open,
                detected_at: "2026-09-20T09:31:00Z".into(),
                updated_at: "2026-09-20T09:31:00Z".into(),
                resolved_at: None,
            }],
            freshness: Vec::new(),
            permissions: ProjectPermissions {
                project_id,
                read_repository: true,
                read_git: true,
                write_memory: true,
                write_context_files: true,
                run_tests: PermissionMode::Deny,
                run_build: PermissionMode::Deny,
                modify_source: false,
                git_commit: false,
                git_push: false,
                delete_files: false,
                updated_at: "2026-09-20T09:31:00Z".into(),
            },
            action_runs: Vec::new(),
        }
    }

    #[test]
    fn normal_context_excludes_superseded_statements_and_explains_selection() {
        let snapshot = compile_snapshot(
            &memory(),
            ContextRequest {
                project_id: "project-1".into(),
                session_id: None,
                task_id: Some("task-1".into()),
                token_budget: ContextDepth::Normal.token_budget(),
                profile: ContextProfile::Implementation,
            },
        );
        assert!(snapshot.items.iter().any(|item| item.id == "task:task-1"
            && item.tier == ContextTier::Critical
            && !item.inclusion_reasons.is_empty()));
        assert!(snapshot
            .items
            .iter()
            .any(|item| item.id == "decision:decision-current"));
        assert!(!snapshot
            .items
            .iter()
            .any(|item| item.id == "decision:decision-old"));
        assert!(snapshot.estimated_tokens <= snapshot.request.token_budget);
    }

    #[test]
    fn deep_context_marks_superseded_decisions_as_archived() {
        let snapshot = compile_snapshot(
            &memory(),
            ContextRequest {
                project_id: "project-1".into(),
                session_id: None,
                task_id: None,
                token_budget: ContextDepth::Deep.token_budget(),
                profile: ContextProfile::Architecture,
            },
        );
        let old = snapshot
            .items
            .iter()
            .find(|item| item.id == "decision:decision-old")
            .unwrap();
        assert_eq!(old.tier, ContextTier::Archived);
        assert!(old.content.contains("not current guidance"));
    }

    #[test]
    fn summary_validation_rejects_unverifiable_sources() {
        let snapshot = build_snapshot(&memory());
        let result = validate_compression(
            &snapshot,
            CompressionResponse {
                model: "nvidia/Nemotron".into(),
                bullets: vec![CompressedBullet {
                    text: "Invented claim".into(),
                    source_keys: vec!["decision:not-real".into()],
                }],
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn summary_validation_accepts_bullets_grounded_in_selected_sources() {
        let snapshot = build_snapshot(&memory());
        let source_key = snapshot.items[0].sources[0].key.clone();
        let result = validate_compression(
            &snapshot,
            CompressionResponse {
                model: "nvidia/Nemotron".into(),
                bullets: vec![CompressedBullet {
                    text: "Work remains focused on the selected project state.".into(),
                    source_keys: vec![source_key],
                }],
            },
        )
        .unwrap();
        assert_eq!(result.provider, "Nebius Token Factory");
        assert_eq!(result.bullets.len(), 1);
    }

    #[test]
    fn profiles_derive_from_capabilities_not_provider_ids() {
        assert_eq!(
            ContextProfile::for_capabilities(&[Capability::Architecture, Capability::Refactoring]),
            ContextProfile::Architecture
        );
        assert_eq!(
            ContextProfile::for_capabilities(&[Capability::Rust, Capability::Testing]),
            ContextProfile::Implementation
        );
        assert_eq!(
            ContextProfile::for_capabilities(&[Capability::Research, Capability::Documentation]),
            ContextProfile::Product
        );
    }
}
