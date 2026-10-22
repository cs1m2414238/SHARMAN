use std::collections::{BTreeMap, BTreeSet};

use crate::domain::{
    ConflictKind, ConflictSeverity, DecisionStatus, EventKind, FreshnessStatus, ProjectMemory,
    TaskStatus,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConflictDetection {
    pub fingerprint: String,
    pub primary_session_id: Option<String>,
    pub related_session_id: Option<String>,
    pub kind: ConflictKind,
    pub severity: ConflictSeverity,
    pub title: String,
    pub description: String,
    pub affected_paths: Vec<String>,
    pub related_decision_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FreshnessEvaluation {
    pub subject_type: String,
    pub subject_id: String,
    pub status: FreshnessStatus,
    pub reason: String,
    pub superseded_by: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Analysis {
    pub conflicts: Vec<ConflictDetection>,
    pub freshness: Vec<FreshnessEvaluation>,
}

/// Deterministic conflict and freshness analysis over authoritative project memory.
/// No model output participates in detection or state transitions.
pub(crate) fn analyze(memory: &ProjectMemory) -> Analysis {
    let mut analysis = Analysis::default();
    analyze_handoffs(memory, &mut analysis);
    analyze_decisions(memory, &mut analysis);
    analyze_context(memory, &mut analysis);
    analyze_tasks(memory, &mut analysis);
    analysis
}

fn analyze_handoffs(memory: &ProjectMemory, analysis: &mut Analysis) {
    for (index, handoff) in memory.handoffs.iter().enumerate() {
        analysis.freshness.push(FreshnessEvaluation {
            subject_type: "handoff".into(),
            subject_id: handoff.id.clone(),
            status: if index == 0 {
                FreshnessStatus::Current
            } else {
                FreshnessStatus::Superseded
            },
            reason: if index == 0 {
                "Latest generated handoff.".into()
            } else {
                "A newer handoff is available.".into()
            },
            superseded_by: (index > 0).then(|| memory.handoffs[0].id.clone()),
        });
    }

    for (newer_index, newer) in memory.handoffs.iter().enumerate() {
        for older in memory.handoffs.iter().skip(newer_index + 1) {
            let newer_agent = newer.previous_agent.as_deref().unwrap_or("unknown");
            let older_agent = older.previous_agent.as_deref().unwrap_or("unknown");
            let overlap = intersect_paths(
                newer
                    .changed_files
                    .iter()
                    .map(|change| change.path.as_str()),
                older
                    .changed_files
                    .iter()
                    .map(|change| change.path.as_str()),
            );
            if newer_agent != older_agent && !overlap.is_empty() {
                analysis.conflicts.push(ConflictDetection {
                fingerprint: format!("file-overlap:{}:{}", older.id, newer.id),
                primary_session_id: Some(newer.session_id.clone()),
                related_session_id: Some(older.session_id.clone()),
                kind: ConflictKind::FileOverlap,
                severity: ConflictSeverity::Warning,
                title: format!("{newer_agent} revisited files changed by {older_agent}"),
                description: format!(
                    "Two consecutive agent sessions changed {} shared file(s). Review the newer changes against the previous handoff before continuing.",
                    overlap.len()
                ),
                affected_paths: overlap.clone(),
                related_decision_ids: Vec::new(),
            });
            }

            if !overlap.is_empty() {
                for task in &older.completed_tasks {
                    analysis.conflicts.push(ConflictDetection {
                    fingerprint: format!(
                        "completed-task-drift:{}:{}:{}",
                        task.id, older.id, newer.id
                    ),
                    primary_session_id: Some(newer.session_id.clone()),
                    related_session_id: Some(older.session_id.clone()),
                    kind: ConflictKind::CompletedTaskDrift,
                    severity: ConflictSeverity::Warning,
                    title: format!("Completed task may need revalidation: {}", task.title),
                    description: "Files associated with the session that completed this task were changed again in a later session.".into(),
                    affected_paths: overlap.clone(),
                    related_decision_ids: Vec::new(),
                });
                }
            }
        }
    }
}

fn analyze_decisions(memory: &ProjectMemory, analysis: &mut Analysis) {
    for decision in &memory.decisions {
        let (status, reason) = match decision.status {
            DecisionStatus::Active => (
                FreshnessStatus::Current,
                "Decision is active and is included in generated context.".into(),
            ),
            DecisionStatus::Superseded => (
                FreshnessStatus::Superseded,
                "A newer decision explicitly supersedes this decision.".into(),
            ),
            DecisionStatus::Reversed => (
                FreshnessStatus::Invalidated,
                "Decision was reversed and must not guide new work.".into(),
            ),
        };
        analysis.freshness.push(FreshnessEvaluation {
            subject_type: "decision".into(),
            subject_id: decision.id.clone(),
            status,
            reason,
            superseded_by: decision.superseded_by.clone(),
        });

        if let Some(replacement_id) = &decision.superseded_by {
            let replacement = memory
                .decisions
                .iter()
                .find(|candidate| &candidate.id == replacement_id);
            analysis.conflicts.push(ConflictDetection {
                fingerprint: format!("decision-contradiction:{}:{}", decision.id, replacement_id),
                primary_session_id: None,
                related_session_id: None,
                kind: ConflictKind::DecisionContradiction,
                severity: ConflictSeverity::Warning,
                title: "A project decision was superseded".into(),
                description: replacement.map_or_else(
                    || format!("The decision ‘{}’ has been superseded.", decision.summary),
                    |newer| {
                        format!(
                            "‘{}’ no longer applies. Use ‘{}’ instead.",
                            decision.summary, newer.summary
                        )
                    },
                ),
                affected_paths: decision.affected.clone(),
                related_decision_ids: vec![decision.id.clone(), replacement_id.clone()],
            });
        }
    }
}

fn analyze_context(memory: &ProjectMemory, analysis: &mut Analysis) {
    let generated = memory
        .events
        .iter()
        .enumerate()
        .find(|(_, event)| event.kind == EventKind::ContextGenerated);
    let authoritative = memory
        .events
        .iter()
        .enumerate()
        .find(|(_, event)| is_context_input(event.kind));
    let current_head = memory
        .project
        .latest_status
        .as_ref()
        .map(|status| status.head_commit.as_str())
        .unwrap_or("");

    let (status, reason, trigger) = match generated {
        None => (
            FreshnessStatus::Invalidated,
            "AGENT_CONTEXT.md has not been generated from project memory.".to_owned(),
            authoritative
                .map(|(_, event)| event.id.clone())
                .unwrap_or_else(|| "missing".into()),
        ),
        Some((context_index, context_event)) => {
            let generated_head = context_event
                .payload
                .get("head")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            if authoritative.is_some_and(|(index, _)| index < context_index) {
                let (_, source) = authoritative.expect("checked as some");
                (
                    FreshnessStatus::Stale,
                    format!(
                        "Project memory changed after context generation ({:?}).",
                        source.kind
                    ),
                    source.id.clone(),
                )
            } else if !generated_head.is_empty()
                && !current_head.is_empty()
                && generated_head != current_head
            {
                (
                    FreshnessStatus::Stale,
                    "The repository HEAD no longer matches the generated context.".into(),
                    current_head.to_owned(),
                )
            } else {
                (
                    FreshnessStatus::Current,
                    "Generated context matches the latest authoritative project memory.".into(),
                    context_event.id.clone(),
                )
            }
        }
    };

    analysis.freshness.push(FreshnessEvaluation {
        subject_type: "context".into(),
        subject_id: "AGENT_CONTEXT.md".into(),
        status,
        reason: reason.clone(),
        superseded_by: None,
    });
    if status != FreshnessStatus::Current {
        analysis.conflicts.push(ConflictDetection {
            fingerprint: format!("stale-context:{}:{trigger}", memory.project.id),
            primary_session_id: memory
                .agent_sessions
                .iter()
                .find(|session| session.status == crate::domain::AgentSessionStatus::Active)
                .map(|session| session.id.clone()),
            related_session_id: memory
                .latest_handoff
                .as_ref()
                .map(|item| item.session_id.clone()),
            kind: ConflictKind::StaleContext,
            severity: if status == FreshnessStatus::Invalidated {
                ConflictSeverity::Critical
            } else {
                ConflictSeverity::Warning
            },
            title: if status == FreshnessStatus::Invalidated {
                "Agent context is unavailable".into()
            } else {
                "Agent context is stale".into()
            },
            description: reason,
            affected_paths: vec!["AGENT_CONTEXT.md".into()],
            related_decision_ids: Vec::new(),
        });
    }
}

fn analyze_tasks(memory: &ProjectMemory, analysis: &mut Analysis) {
    let invalidated = analysis
        .conflicts
        .iter()
        .filter(|conflict| conflict.kind == ConflictKind::CompletedTaskDrift)
        .filter_map(|conflict| {
            conflict
                .fingerprint
                .strip_prefix("completed-task-drift:")?
                .split(':')
                .next()
                .map(str::to_owned)
        })
        .collect::<BTreeSet<_>>();
    for task in memory.goals.iter().flat_map(|goal| &goal.tasks) {
        if task.status != TaskStatus::Completed {
            continue;
        }
        let is_invalidated = invalidated.contains(&task.id);
        analysis.freshness.push(FreshnessEvaluation {
            subject_type: "task".into(),
            subject_id: task.id.clone(),
            status: if is_invalidated {
                FreshnessStatus::Invalidated
            } else {
                FreshnessStatus::Current
            },
            reason: if is_invalidated {
                "Files changed after this task was completed; revalidation is required.".into()
            } else {
                "No later handoff has invalidated this completed task.".into()
            },
            superseded_by: None,
        });
    }
}

fn intersect_paths<'a>(
    left: impl Iterator<Item = &'a str>,
    right: impl Iterator<Item = &'a str>,
) -> Vec<String> {
    let left = left.map(normalize_path).collect::<BTreeMap<_, _>>();
    let right = right
        .map(|path| normalize_path(path).0)
        .collect::<BTreeSet<_>>();
    left.into_iter()
        .filter(|(normalized, _)| right.contains(normalized))
        .map(|(_, original)| original)
        .collect()
}

fn normalize_path(path: &str) -> (String, String) {
    (path.replace('\\', "/").to_lowercase(), path.to_owned())
}

fn is_context_input(kind: EventKind) -> bool {
    matches!(
        kind,
        EventKind::FileModified
            | EventKind::FileChanged
            | EventKind::FileCreated
            | EventKind::FileDeleted
            | EventKind::CommitCreated
            | EventKind::CommitDetected
            | EventKind::BranchChanged
            | EventKind::GoalCreated
            | EventKind::GoalUpdated
            | EventKind::TaskCreated
            | EventKind::TaskUpdated
            | EventKind::TaskCompleted
            | EventKind::DecisionRecorded
            | EventKind::DecisionSuperseded
            | EventKind::HandoffGenerated
            | EventKind::ActionCompleted
            | EventKind::ActionFailed
            | EventKind::CapabilityProfileUpdated
            | EventKind::TaskRouteSuggested
            | EventKind::TaskAssigned
            | EventKind::TaskAssignmentUpdated
            | EventKind::WorkspaceLockAcquired
            | EventKind::WorkspaceLockReleased
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_intersection_is_case_and_separator_insensitive() {
        assert_eq!(
            intersect_paths(["src\\Auth.rs"].into_iter(), ["src/auth.rs"].into_iter()),
            vec!["src\\Auth.rs"]
        );
    }
}
