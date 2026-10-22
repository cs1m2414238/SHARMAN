use std::collections::BTreeSet;

use crate::domain::{
    ActionKind, AgentCapabilityProfile, Capability, Decision, DecisionStatus, ParallelAssessment,
    TaskRouteRecommendation, TaskStatus, WorkTask,
};

pub fn recommend(
    task: &WorkTask,
    profiles: &[AgentCapabilityProfile],
) -> Option<TaskRouteRecommendation> {
    let required = inferred_capabilities(task);
    let mut candidates = profiles
        .iter()
        .filter(|profile| profile.enabled)
        .map(|profile| {
            let matched = required
                .iter()
                .filter(|capability| profile.strengths.contains(capability))
                .copied()
                .collect::<Vec<_>>();
            let mut score = (matched.len() * 30) as u16;
            score += (profile.max_context_tokens.min(12_000) / 1_000) as u16;
            if required.contains(&Capability::Testing)
                && profile.allowed_actions.contains(&ActionKind::RunTests)
            {
                score += 12;
            }
            if matches!(task.priority, crate::domain::TaskPriority::Critical) {
                score += (matched.len() * 4) as u16;
            }
            let mut reasons = matched
                .iter()
                .map(|capability| {
                    format!(
                        "Profile strength `{}` matches this task.",
                        capability.as_key()
                    )
                })
                .collect::<Vec<_>>();
            if required.contains(&Capability::Testing)
                && profile.allowed_actions.contains(&ActionKind::RunTests)
            {
                reasons.push("Profile may request the controlled run-tests action.".into());
            }
            reasons.push(format!(
                "Context allowance is {} tokens.",
                profile.max_context_tokens
            ));
            TaskRouteRecommendation {
                task_id: task.id.clone(),
                provider: profile.provider.clone(),
                display_name: profile.display_name.clone(),
                score,
                reasons,
                required_capabilities: required.clone(),
            }
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.display_name.cmp(&right.display_name))
    });
    candidates.into_iter().next()
}

pub fn assess_parallel_with_decisions(
    tasks: &[WorkTask],
    decisions: &[Decision],
) -> Vec<ParallelAssessment> {
    let active = tasks
        .iter()
        .filter(|task| !matches!(task.status, TaskStatus::Completed | TaskStatus::Archived))
        .collect::<Vec<_>>();
    let mut assessments = Vec::new();
    for (index, left) in active.iter().enumerate() {
        for right in active.iter().skip(index + 1) {
            assessments.push(assess_pair_with_decisions(left, right, decisions));
        }
    }
    assessments
}

#[cfg(test)]
pub fn assess_pair(left: &WorkTask, right: &WorkTask) -> ParallelAssessment {
    assess_pair_with_decisions(left, right, &[])
}

pub fn assess_pair_with_decisions(
    left: &WorkTask,
    right: &WorkTask,
    decisions: &[Decision],
) -> ParallelAssessment {
    let mut conflicts = Vec::new();
    if left.depends_on.contains(&right.id)
        || left.blocked_by.contains(&right.id)
        || right.depends_on.contains(&left.id)
        || right.blocked_by.contains(&left.id)
    {
        conflicts.push("One task depends on the other and must wait.".into());
    }
    if intersects(&left.produces, &right.consumes) || intersects(&right.produces, &left.consumes) {
        conflicts.push("One task consumes an artifact produced by the other.".into());
    }
    if left.claimed_paths.iter().any(|left_path| {
        right
            .claimed_paths
            .iter()
            .any(|right_path| path_patterns_overlap(left_path, right_path))
    }) {
        conflicts.push("The tasks claim overlapping workspace paths.".into());
    }
    for decision in decisions
        .iter()
        .filter(|decision| decision.status == DecisionStatus::Active)
    {
        if task_touches_decision(left, decision) && task_touches_decision(right, decision) {
            conflicts.push(format!(
                "Active architectural decision `{}` affects both tasks.",
                decision.summary
            ));
        }
    }
    ParallelAssessment {
        left_task_id: left.id.clone(),
        right_task_id: right.id.clone(),
        safe: conflicts.is_empty(),
        reasons: if conflicts.is_empty() {
            vec!["No dependency, artifact-flow, or workspace-path overlap detected.".into()]
        } else {
            conflicts
        },
    }
}

fn task_touches_decision(task: &WorkTask, decision: &Decision) -> bool {
    decision.affected.iter().any(|affected| {
        task.claimed_paths
            .iter()
            .any(|path| path_patterns_overlap(path, affected))
            || task
                .produces
                .iter()
                .chain(&task.consumes)
                .any(|artifact| artifact.eq_ignore_ascii_case(affected))
    })
}

pub(crate) fn path_patterns_overlap(left: &str, right: &str) -> bool {
    let left = normalize_pattern(left);
    let right = normalize_pattern(right);
    !left.is_empty()
        && !right.is_empty()
        && (left == right
            || left.starts_with(&format!("{right}/"))
            || right.starts_with(&format!("{left}/")))
}

fn inferred_capabilities(task: &WorkTask) -> Vec<Capability> {
    let mut capabilities = task
        .required_capabilities
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let text = format!(
        "{} {} {} {} {}",
        task.title,
        task.description,
        task.produces.join(" "),
        task.consumes.join(" "),
        task.claimed_paths.join(" ")
    )
    .to_ascii_lowercase();
    for (words, capability) in [
        (
            &["architecture", "design", "schema"] as &[&str],
            Capability::Architecture,
        ),
        (
            &["backend", "api", "database", "storage"],
            Capability::Backend,
        ),
        (&["frontend", "ui", "react", "css"], Capability::Frontend),
        (
            &["refactor", "migration", "restructure"],
            Capability::Refactoring,
        ),
        (
            &["research", "investigate", "compare"],
            Capability::Research,
        ),
        (&["test", "coverage", "verify"], Capability::Testing),
        (
            &["docs", "documentation", "readme"],
            Capability::Documentation,
        ),
        (&["rust", ".rs", "cargo"], Capability::Rust),
        (
            &["typescript", ".ts", ".tsx", "react"],
            Capability::TypeScript,
        ),
        (&["security", "auth", "permission"], Capability::Security),
        (&["deploy", "docker", "ci", "build"], Capability::DevOps),
    ] {
        if words.iter().any(|word| text.contains(word)) {
            capabilities.insert(capability);
        }
    }
    if capabilities.is_empty() {
        capabilities.insert(Capability::Backend);
    }
    capabilities.into_iter().collect()
}

fn intersects(left: &[String], right: &[String]) -> bool {
    left.iter()
        .any(|left| right.iter().any(|right| left.eq_ignore_ascii_case(right)))
}

fn normalize_pattern(value: &str) -> String {
    value
        .trim()
        .replace('\\', "/")
        .trim_end_matches('*')
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ProviderId, TaskAssignmentStatus, TaskPriority};

    fn task(id: &str, title: &str, paths: &[&str]) -> WorkTask {
        WorkTask {
            id: id.into(),
            goal_id: "goal".into(),
            title: title.into(),
            description: String::new(),
            status: TaskStatus::Todo,
            priority: TaskPriority::Medium,
            assigned_agent: None,
            source_agent: None,
            created_at: String::new(),
            completed_at: None,
            assignment_status: TaskAssignmentStatus::Unassigned,
            suggested_provider: None,
            assigned_provider: None,
            route_reasons: Vec::new(),
            required_capabilities: Vec::new(),
            blocked_by: Vec::new(),
            depends_on: Vec::new(),
            produces: Vec::new(),
            consumes: Vec::new(),
            claimed_paths: paths.iter().map(|path| (*path).into()).collect(),
        }
    }

    #[test]
    fn overlapping_paths_prevent_parallel_work() {
        let result = assess_pair(
            &task("a", "Auth architecture", &["src/auth/*"]),
            &task("b", "Auth tests", &["src/auth/service.rs"]),
        );
        assert!(!result.safe);
    }

    #[test]
    fn routing_uses_configured_strengths_not_provider_names() {
        let profile = AgentCapabilityProfile {
            id: "custom".into(),
            project_id: "project".into(),
            provider: ProviderId::gemini(),
            display_name: "Custom worker".into(),
            strengths: vec![Capability::Rust, Capability::Testing],
            allowed_actions: vec![ActionKind::RunTests],
            max_context_tokens: 6_000,
            enabled: true,
            updated_at: String::new(),
        };
        let recommendation = recommend(&task("a", "Test Rust storage", &[]), &[profile]).unwrap();
        assert_eq!(recommendation.provider, ProviderId::gemini());
        assert!(recommendation.score > 60);
    }

    #[test]
    fn shared_architectural_decision_serializes_related_tasks() {
        let decision = Decision {
            id: "decision".into(),
            project_id: "project".into(),
            summary: "Auth contract is stable".into(),
            reason: String::new(),
            alternatives: Vec::new(),
            affected: vec!["src/auth".into()],
            status: DecisionStatus::Active,
            created_by: "Claude".into(),
            superseded_by: None,
            created_at: String::new(),
        };
        let result = assess_pair_with_decisions(
            &task("a", "Auth architecture", &["src/auth/types.rs"]),
            &task("b", "Auth implementation", &["src/auth/service.rs"]),
            &[decision],
        );
        assert!(!result.safe);
        assert!(result
            .reasons
            .iter()
            .any(|reason| reason.contains("decision")));
    }
}
