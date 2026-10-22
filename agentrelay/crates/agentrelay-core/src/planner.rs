use std::collections::{BTreeMap, BTreeSet};

use chrono::{SecondsFormat, Utc};
use uuid::Uuid;

use crate::{
    domain::{
        AgentCapabilityProfile, Capability, ConflictSeverity, ConflictStatus, DecisionStatus,
        FreshnessStatus, Goal, NodeType, PlanIssueSeverity, PlanStatus, PlanValidationIssue,
        PlanVersion, PlannedTask, PlannedTaskKind, PlannedTaskStatus, ProjectGraph, ProjectMemory,
        RiskLevel, TaskAssignmentStatus, TaskPriority, TaskStatus, WorkTask,
    },
    orchestration,
};

pub fn generate(
    goal: &Goal,
    graph: &ProjectGraph,
    memory: &ProjectMemory,
    profiles: &[AgentCapabilityProfile],
    version: u32,
) -> PlanVersion {
    let goal_text = format!("{} {}", goal.title, goal.description).to_ascii_lowercase();
    let mut kinds = vec![
        PlannedTaskKind::Architecture,
        PlannedTaskKind::Implementation,
    ];
    if contains_any(
        &goal_text,
        &[
            "persist", "database", "sqlite", "postgres", "storage", "sync",
        ],
    ) {
        kinds.push(PlannedTaskKind::Persistence);
    }
    if contains_any(
        &goal_text,
        &[
            "provider",
            "integration",
            "config",
            "migration",
            "schema",
            "manifest",
            "api",
        ],
    ) {
        kinds.push(PlannedTaskKind::MigrationConfig);
    }
    kinds.extend([PlannedTaskKind::Testing, PlannedTaskKind::Documentation]);

    let ids = kinds
        .iter()
        .map(|kind| (*kind, Uuid::new_v4().to_string()))
        .collect::<BTreeMap<_, _>>();
    let architecture = ids[&PlannedTaskKind::Architecture].clone();
    let implementation = ids[&PlannedTaskKind::Implementation].clone();
    let persistence = ids.get(&PlannedTaskKind::Persistence).cloned();
    let migration = ids.get(&PlannedTaskKind::MigrationConfig).cloned();
    let code_terminal = persistence
        .as_ref()
        .or(migration.as_ref())
        .cloned()
        .unwrap_or_else(|| implementation.clone());

    let mut tasks = kinds
        .into_iter()
        .map(|kind| {
            let depends_on = match kind {
                PlannedTaskKind::Architecture => vec![],
                PlannedTaskKind::Implementation => vec![architecture.clone()],
                PlannedTaskKind::Persistence => vec![implementation.clone()],
                PlannedTaskKind::MigrationConfig => persistence
                    .as_ref()
                    .map(|id| vec![id.clone()])
                    .unwrap_or_else(|| vec![architecture.clone()]),
                PlannedTaskKind::Testing => vec![code_terminal.clone()],
                // Documentation can proceed beside implementation once the contract is stable.
                PlannedTaskKind::Documentation => vec![architecture.clone()],
                PlannedTaskKind::Recovery => vec![],
            };
            make_task(&ids[&kind], kind, goal, graph, memory, profiles, depends_on)
        })
        .collect::<Vec<_>>();
    assign_parallel_groups(&mut tasks);
    mark_unsafe_parallel_overlap(&mut tasks);
    let issues = validate(&tasks, memory, profiles);
    let status = if issues
        .iter()
        .any(|issue| issue.severity == PlanIssueSeverity::Error)
    {
        PlanStatus::Invalid
    } else {
        PlanStatus::Valid
    };
    PlanVersion {
        id: Uuid::new_v4().to_string(),
        project_id: goal.project_id.clone(),
        goal_id: goal.id.clone(),
        version,
        status,
        reason: if version == 1 {
            "Initial deterministic decomposition of the approved high-level goal.".into()
        } else {
            "Regenerated plan.".into()
        },
        parent_plan_id: None,
        changed_task_ids: tasks.iter().map(|task| task.id.clone()).collect(),
        changed_dependencies: tasks
            .iter()
            .filter(|task| !task.depends_on.is_empty())
            .map(|task| format!("{} <- {}", task.id, task.depends_on.join(", ")))
            .collect(),
        tasks,
        validation_issues: issues,
        created_at: now(),
        approved_at: None,
    }
}

pub fn replan(
    previous: &PlanVersion,
    failed_task_id: &str,
    failure_reason: &str,
    memory: &ProjectMemory,
    profiles: &[AgentCapabilityProfile],
) -> Result<PlanVersion, String> {
    let failed = previous
        .tasks
        .iter()
        .find(|task| task.id == failed_task_id)
        .cloned()
        .ok_or_else(|| "the failed planned task is not part of this plan".to_owned())?;
    let mut tasks = previous.tasks.clone();
    if let Some(task) = tasks.iter_mut().find(|task| task.id == failed_task_id) {
        task.status = PlannedTaskStatus::Failed;
        task.can_run_in_parallel = false;
    }
    let downstream = transitive_dependents(&tasks, failed_task_id);
    for task in tasks
        .iter_mut()
        .filter(|task| downstream.contains(&task.id))
    {
        task.status = PlannedTaskStatus::Blocked;
    }

    let recovery_id = Uuid::new_v4().to_string();
    let mut recovery = failed.clone();
    recovery.id = recovery_id.clone();
    recovery.title = format!("Recover: {}", failed.title);
    recovery.description = format!(
        "Recover the failed task after deterministic validation reported: {}",
        failure_reason.trim()
    );
    recovery.kind = PlannedTaskKind::Recovery;
    recovery.status = if recovery.depends_on.is_empty() {
        PlannedTaskStatus::Ready
    } else {
        PlannedTaskStatus::Blocked
    };
    recovery.priority = TaskPriority::Critical;
    recovery.conflict_risk = RiskLevel::High;
    recovery.risk_score = recovery.risk_score.max(70);
    recovery.risk_reasons = vec![format!(
        "Recovery required because: {}",
        failure_reason.trim()
    )];
    recovery.can_run_in_parallel = false;
    recovery.parallel_group = failed.parallel_group;
    recovery.materialized_task_id = None;
    recovery.recovery_for = Some(failed_task_id.to_owned());
    route(&mut recovery, &previous.goal_id, profiles);
    tasks.push(recovery);
    for task in tasks
        .iter_mut()
        .filter(|task| downstream.contains(&task.id))
    {
        for dependency in &mut task.depends_on {
            if dependency == failed_task_id {
                *dependency = recovery_id.clone();
            }
        }
    }
    assign_parallel_groups(&mut tasks);
    let issues = validate(&tasks, memory, profiles);
    let status = if issues
        .iter()
        .any(|issue| issue.severity == PlanIssueSeverity::Error)
    {
        PlanStatus::Invalid
    } else {
        PlanStatus::Valid
    };
    Ok(PlanVersion {
        id: Uuid::new_v4().to_string(),
        project_id: previous.project_id.clone(),
        goal_id: previous.goal_id.clone(),
        version: previous.version + 1,
        status,
        reason: format!(
            "Replanned after `{}` failed: {}",
            failed.title, failure_reason
        ),
        parent_plan_id: Some(previous.id.clone()),
        changed_task_ids: std::iter::once(failed_task_id.to_owned())
            .chain(downstream.iter().cloned())
            .chain(std::iter::once(recovery_id.clone()))
            .collect(),
        changed_dependencies: downstream
            .iter()
            .map(|id| format!("{id}: replaced failed dependency with recovery {recovery_id}"))
            .collect(),
        tasks,
        validation_issues: issues,
        created_at: now(),
        approved_at: None,
    })
}

fn make_task(
    id: &str,
    kind: PlannedTaskKind,
    goal: &Goal,
    graph: &ProjectGraph,
    memory: &ProjectMemory,
    profiles: &[AgentCapabilityProfile],
    depends_on: Vec<String>,
) -> PlannedTask {
    let (label, description, capabilities, produces, consumes) = template(kind, goal);
    let expected_files = select_files(kind, goal, graph);
    let affected_modules = modules_for_files(graph, &expected_files);
    let (risk_score, conflict_risk, risk_reasons) =
        risk(&expected_files, &affected_modules, depends_on.len(), memory);
    let mut task = PlannedTask {
        id: id.to_owned(),
        title: label,
        description,
        kind,
        status: if depends_on.is_empty() {
            PlannedTaskStatus::Ready
        } else {
            PlannedTaskStatus::Blocked
        },
        priority: if matches!(conflict_risk, RiskLevel::High | RiskLevel::Critical) {
            TaskPriority::High
        } else {
            TaskPriority::Medium
        },
        required_capabilities: capabilities,
        depends_on,
        produces,
        consumes,
        affected_modules,
        expected_files,
        conflict_risk,
        risk_score,
        risk_reasons,
        can_run_in_parallel: !matches!(
            kind,
            PlannedTaskKind::Architecture | PlannedTaskKind::Recovery
        ) && !matches!(conflict_risk, RiskLevel::Critical),
        parallel_group: 0,
        suggested_provider: None,
        suggestion_reasons: vec![],
        materialized_task_id: None,
        recovery_for: None,
    };
    route(&mut task, &goal.id, profiles);
    task
}

fn template(
    kind: PlannedTaskKind,
    goal: &Goal,
) -> (String, String, Vec<Capability>, Vec<String>, Vec<String>) {
    let subject = goal.title.trim();
    match kind {
        PlannedTaskKind::Architecture => (
            format!("Define contract: {subject}"),
            "Confirm interfaces, boundaries, decisions, and acceptance criteria before implementation.".into(),
            vec![Capability::Architecture, Capability::Research],
            vec!["approved-contract".into()],
            vec![],
        ),
        PlannedTaskKind::Implementation => (
            format!("Implement: {subject}"),
            "Implement the approved contract without expanding scope.".into(),
            vec![Capability::Backend, Capability::Rust],
            vec!["implementation".into()],
            vec!["approved-contract".into()],
        ),
        PlannedTaskKind::Persistence => (
            format!("Persist and synchronize: {subject}"),
            "Add the required persistence and synchronization path using the existing source of truth.".into(),
            vec![Capability::Backend, Capability::Rust],
            vec!["persisted-state".into()],
            vec!["implementation".into()],
        ),
        PlannedTaskKind::MigrationConfig => (
            format!("Configure and migrate: {subject}"),
            "Apply manifest, configuration, schema, or migration changes required by the feature.".into(),
            vec![Capability::DevOps, Capability::Refactoring],
            vec!["runtime-configuration".into()],
            vec!["approved-contract".into()],
        ),
        PlannedTaskKind::Testing => (
            format!("Verify: {subject}"),
            "Add deterministic tests and verify the acceptance criteria.".into(),
            vec![Capability::Testing, Capability::Rust],
            vec!["verified-feature".into()],
            vec!["implementation".into()],
        ),
        PlannedTaskKind::Documentation => (
            format!("Document: {subject}"),
            "Update user and developer documentation from the approved contract.".into(),
            vec![Capability::Documentation],
            vec!["updated-documentation".into()],
            vec!["approved-contract".into()],
        ),
        PlannedTaskKind::Recovery => unreachable!("recovery tasks are derived from failures"),
    }
}

fn select_files(kind: PlannedTaskKind, goal: &Goal, graph: &ProjectGraph) -> Vec<String> {
    let goal_tokens = format!("{} {}", goal.title, goal.description)
        .to_ascii_lowercase()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| token.len() >= 4)
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let kind_words: &[&str] = match kind {
        PlannedTaskKind::Architecture => &["domain", "type", "trait", "interface", "mod", "lib"],
        PlannedTaskKind::Implementation => &["src", "core", "provider", "integration", "adapter"],
        PlannedTaskKind::Persistence => &["memory", "storage", "database", "sqlite", "migration"],
        PlannedTaskKind::Testing => &["test", "spec"],
        PlannedTaskKind::Documentation => &["readme", "docs", ".md"],
        PlannedTaskKind::MigrationConfig => {
            &["cargo", "package", "config", "manifest", "migration"]
        }
        PlannedTaskKind::Recovery => &[],
    };
    let mut scored = graph
        .nodes
        .iter()
        .filter(|node| matches!(node.node_type, NodeType::File | NodeType::Test))
        .filter_map(|node| {
            let path = node.path.as_ref()?.replace('\\', "/");
            let folded = path.to_ascii_lowercase();
            let kind_score = kind_words
                .iter()
                .filter(|word| folded.contains(**word))
                .count();
            let goal_score = goal_tokens
                .iter()
                .filter(|token| folded.contains(token.as_str()))
                .count();
            let score = kind_score * 3 + goal_score * 5;
            (score > 0).then_some((score, path))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    scored.into_iter().take(8).map(|(_, path)| path).collect()
}

fn modules_for_files(graph: &ProjectGraph, files: &[String]) -> Vec<String> {
    let file_ids = graph
        .nodes
        .iter()
        .filter(|node| node.path.as_ref().is_some_and(|path| files.contains(path)))
        .map(|node| node.id.as_str())
        .collect::<BTreeSet<_>>();
    let module_ids = graph
        .edges
        .iter()
        .filter(|edge| file_ids.contains(edge.source.as_str()))
        .map(|edge| edge.target.as_str())
        .collect::<BTreeSet<_>>();
    graph
        .nodes
        .iter()
        .filter(|node| node.node_type == NodeType::Module && module_ids.contains(node.id.as_str()))
        .map(|node| node.label.clone())
        .collect()
}

fn risk(
    files: &[String],
    modules: &[String],
    dependency_count: usize,
    memory: &ProjectMemory,
) -> (u8, RiskLevel, Vec<String>) {
    let mut score =
        (files.len().min(8) * 3 + modules.len().min(3) * 7 + dependency_count * 6) as u8;
    let mut reasons = vec![format!(
        "Touches {} expected files across {} modules with {} dependencies.",
        files.len(),
        modules.len(),
        dependency_count
    )];
    if files.is_empty() {
        score = score.saturating_add(18);
        reasons.push("The graph could not identify an expected file; scope needs review.".into());
    }
    let open_conflicts = memory
        .conflicts
        .iter()
        .filter(|conflict| conflict.status != ConflictStatus::Resolved)
        .filter(|conflict| {
            conflict
                .affected_paths
                .iter()
                .any(|affected| files.iter().any(|file| overlap(file, affected)))
        })
        .count();
    if open_conflicts > 0 {
        score = score.saturating_add((open_conflicts.min(2) * 20) as u8);
        reasons.push(format!(
            "{open_conflicts} unresolved conflict(s) overlap the expected files."
        ));
    }
    let critical_decisions = memory
        .decisions
        .iter()
        .filter(|decision| decision.status == DecisionStatus::Active)
        .filter(|decision| {
            decision.affected.iter().any(|affected| {
                files.iter().any(|file| overlap(file, affected))
                    || modules.iter().any(|module| overlap(module, affected))
            })
        })
        .count();
    if critical_decisions > 0 {
        score = score.saturating_add(12);
        reasons.push(format!(
            "{critical_decisions} active decision(s) constrain this scope."
        ));
    }
    score = score.min(100);
    let level = match score {
        0..=29 => RiskLevel::Low,
        30..=54 => RiskLevel::Medium,
        55..=79 => RiskLevel::High,
        _ => RiskLevel::Critical,
    };
    (score, level, reasons)
}

fn route(task: &mut PlannedTask, goal_id: &str, profiles: &[AgentCapabilityProfile]) {
    let candidate = WorkTask {
        id: task.id.clone(),
        goal_id: goal_id.to_owned(),
        title: task.title.clone(),
        description: task.description.clone(),
        status: TaskStatus::Todo,
        priority: task.priority,
        assigned_agent: None,
        source_agent: Some("planner".into()),
        created_at: now(),
        completed_at: None,
        assignment_status: TaskAssignmentStatus::Unassigned,
        suggested_provider: None,
        assigned_provider: None,
        route_reasons: vec![],
        required_capabilities: task.required_capabilities.clone(),
        blocked_by: task.depends_on.clone(),
        depends_on: task.depends_on.clone(),
        produces: task.produces.clone(),
        consumes: task.consumes.clone(),
        claimed_paths: task.expected_files.clone(),
    };
    if let Some(recommendation) = orchestration::recommend(&candidate, profiles) {
        task.suggested_provider = Some(recommendation.provider);
        task.suggestion_reasons = recommendation.reasons;
    }
}

fn validate(
    tasks: &[PlannedTask],
    memory: &ProjectMemory,
    profiles: &[AgentCapabilityProfile],
) -> Vec<PlanValidationIssue> {
    let mut issues = Vec::new();
    let ids = tasks
        .iter()
        .map(|task| task.id.as_str())
        .collect::<BTreeSet<_>>();
    for task in tasks {
        let missing = task
            .depends_on
            .iter()
            .filter(|dependency| !ids.contains(dependency.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            issues.push(issue(
                "missing-dependency",
                PlanIssueSeverity::Error,
                vec![task.id.clone()],
                format!(
                    "Task references missing dependencies: {}.",
                    missing.join(", ")
                ),
            ));
        }
        if task.suggested_provider.is_none() || profiles.iter().all(|profile| !profile.enabled) {
            issues.push(issue(
                "no-capable-agent",
                PlanIssueSeverity::Error,
                vec![task.id.clone()],
                "No enabled configurable agent profile can satisfy this task.".into(),
            ));
        }
        if task.expected_files.is_empty() {
            issues.push(issue(
                "scope-review-required",
                PlanIssueSeverity::Warning,
                vec![task.id.clone()],
                "No existing file matched this proposed task; review its scope before approval."
                    .into(),
            ));
        }
        for consumed in &task.consumes {
            let produced_in_plan = tasks.iter().any(|producer| {
                producer.id != task.id
                    && producer
                        .produces
                        .iter()
                        .any(|artifact| artifact.eq_ignore_ascii_case(consumed))
            });
            let grounded_in_decision = memory
                .decisions
                .iter()
                .filter(|decision| decision.status == DecisionStatus::Active)
                .any(|decision| {
                    decision
                        .summary
                        .to_ascii_lowercase()
                        .contains(&consumed.to_ascii_lowercase())
                        || decision
                            .affected
                            .iter()
                            .any(|affected| affected.eq_ignore_ascii_case(consumed))
                });
            if !produced_in_plan && !grounded_in_decision {
                issues.push(issue(
                    "required-decision-missing",
                    PlanIssueSeverity::Error,
                    vec![task.id.clone()],
                    format!(
                        "Task consumes `{consumed}`, but no planned predecessor or active decision provides it."
                    ),
                ));
            }
        }
    }
    if has_cycle(tasks) {
        issues.push(issue(
            "circular-dependency",
            PlanIssueSeverity::Error,
            tasks.iter().map(|task| task.id.clone()).collect(),
            "The generated task graph contains a circular dependency.".into(),
        ));
    }
    for (index, left) in tasks.iter().enumerate() {
        for right in tasks.iter().skip(index + 1) {
            if left.parallel_group == right.parallel_group
                && left.can_run_in_parallel
                && right.can_run_in_parallel
                && (left.expected_files.iter().any(|left_file| {
                    right
                        .expected_files
                        .iter()
                        .any(|right_file| overlap(left_file, right_file))
                }) || left
                    .affected_modules
                    .iter()
                    .any(|module| right.affected_modules.contains(module)))
            {
                issues.push(issue(
                    "parallel-critical-overlap",
                    PlanIssueSeverity::Error,
                    vec![left.id.clone(), right.id.clone()],
                    "Parallel tasks overlap files or modules and must be serialized.".into(),
                ));
            }
        }
    }
    if memory.freshness.iter().any(|record| {
        record.subject_type.eq_ignore_ascii_case("context")
            && matches!(
                record.status,
                FreshnessStatus::Stale | FreshnessStatus::Invalidated
            )
    }) {
        issues.push(issue(
            "stale-context",
            PlanIssueSeverity::Error,
            vec![],
            "Project context is explicitly stale or invalidated; regenerate it before approval."
                .into(),
        ));
    }
    if memory.conflicts.iter().any(|conflict| {
        conflict.status == ConflictStatus::Open && conflict.severity == ConflictSeverity::Critical
    }) {
        issues.push(issue(
            "unresolved-critical-conflict",
            PlanIssueSeverity::Error,
            vec![],
            "Resolve or acknowledge the open critical conflict before execution.".into(),
        ));
    }
    issues
}

fn mark_unsafe_parallel_overlap(tasks: &mut [PlannedTask]) {
    let mut unsafe_ids = BTreeSet::new();
    for (index, left) in tasks.iter().enumerate() {
        for right in tasks.iter().skip(index + 1) {
            if left.parallel_group == right.parallel_group
                && (left.expected_files.iter().any(|left_file| {
                    right
                        .expected_files
                        .iter()
                        .any(|right_file| overlap(left_file, right_file))
                }) || left
                    .affected_modules
                    .iter()
                    .any(|module| right.affected_modules.contains(module)))
            {
                unsafe_ids.insert(left.id.clone());
                unsafe_ids.insert(right.id.clone());
            }
        }
    }
    for task in tasks
        .iter_mut()
        .filter(|task| unsafe_ids.contains(&task.id))
    {
        task.can_run_in_parallel = false;
        task.risk_score = task.risk_score.saturating_add(18).min(100);
        task.conflict_risk = if task.risk_score >= 80 {
            RiskLevel::Critical
        } else {
            RiskLevel::High
        };
        task.risk_reasons
            .push("A same-stage task overlaps this file or module scope.".into());
    }
}

fn assign_parallel_groups(tasks: &mut [PlannedTask]) {
    let mut groups = BTreeMap::<String, usize>::new();
    for _ in 0..tasks.len().max(1) {
        let mut changed = false;
        for task in tasks.iter() {
            let group = task
                .depends_on
                .iter()
                .filter_map(|dependency| groups.get(dependency))
                .max()
                .map_or(0, |group| group + 1);
            if groups.get(&task.id).copied() != Some(group) {
                groups.insert(task.id.clone(), group);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    for task in tasks {
        task.parallel_group = groups.get(&task.id).copied().unwrap_or(0);
    }
}

fn has_cycle(tasks: &[PlannedTask]) -> bool {
    fn visit<'a>(
        id: &'a str,
        map: &BTreeMap<&'a str, &'a PlannedTask>,
        visiting: &mut BTreeSet<&'a str>,
        visited: &mut BTreeSet<&'a str>,
    ) -> bool {
        if visiting.contains(id) {
            return true;
        }
        if !visited.insert(id) {
            return false;
        }
        visiting.insert(id);
        let cycle = map.get(id).is_some_and(|task| {
            task.depends_on
                .iter()
                .any(|dependency| visit(dependency, map, visiting, visited))
        });
        visiting.remove(id);
        cycle
    }
    let map = tasks
        .iter()
        .map(|task| (task.id.as_str(), task))
        .collect::<BTreeMap<_, _>>();
    let mut visited = BTreeSet::new();
    tasks
        .iter()
        .any(|task| visit(&task.id, &map, &mut BTreeSet::new(), &mut visited))
}

fn transitive_dependents(tasks: &[PlannedTask], source: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    loop {
        let next = tasks
            .iter()
            .filter(|task| {
                !found.contains(&task.id)
                    && task
                        .depends_on
                        .iter()
                        .any(|dependency| dependency == source || found.contains(dependency))
            })
            .map(|task| task.id.clone())
            .collect::<Vec<_>>();
        if next.is_empty() {
            break;
        }
        found.extend(next);
    }
    found
}

fn issue(
    code: &str,
    severity: PlanIssueSeverity,
    task_ids: Vec<String>,
    detail: String,
) -> PlanValidationIssue {
    PlanValidationIssue {
        code: code.into(),
        severity,
        task_ids,
        detail,
    }
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn overlap(left: &str, right: &str) -> bool {
    let left = left.replace('\\', "/").to_ascii_lowercase();
    let right = right.replace('\\', "/").to_ascii_lowercase();
    left == right
        || left.starts_with(&format!("{right}/"))
        || right.starts_with(&format!("{left}/"))
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        ActionKind, GoalStatus, GraphNode, GraphStats, PermissionMode, ProjectPermissions,
        ProjectRecord, ProviderId,
    };

    fn fixture() -> (
        Goal,
        ProjectGraph,
        ProjectMemory,
        Vec<AgentCapabilityProfile>,
    ) {
        let project = ProjectRecord {
            id: "project".into(),
            path: "repo".into(),
            name: "repo".into(),
            branch: "main".into(),
            last_commit: "abc".into(),
            change_count: 0,
            last_scanned_at: now(),
            tracking_enabled: true,
            latest_status: None,
        };
        let goal = Goal {
            id: "goal".into(),
            project_id: "project".into(),
            title: "Add GitHub provider support with PR status sync".into(),
            description: String::new(),
            status: GoalStatus::Active,
            created_at: now(),
            updated_at: now(),
            tasks: vec![],
        };
        let graph = ProjectGraph {
            project_id: "project".into(),
            repository: "repo".into(),
            nodes: vec![GraphNode {
                id: "file:provider".into(),
                node_type: NodeType::File,
                label: "github.rs".into(),
                path: Some("src/providers/github.rs".into()),
                status: None,
                metadata: BTreeMap::new(),
            }],
            edges: vec![],
            stats: GraphStats::default(),
            warnings: vec![],
        };
        let memory = ProjectMemory {
            project,
            goals: vec![goal.clone()],
            decisions: vec![],
            events: vec![],
            agent_sessions: vec![],
            last_agent_session: None,
            latest_handoff: None,
            handoffs: vec![],
            conflicts: vec![],
            freshness: vec![],
            permissions: ProjectPermissions {
                project_id: "project".into(),
                read_repository: true,
                read_git: true,
                write_memory: true,
                write_context_files: true,
                run_tests: PermissionMode::Allow,
                run_build: PermissionMode::Deny,
                modify_source: false,
                git_commit: false,
                git_push: false,
                delete_files: false,
                updated_at: now(),
            },
            action_runs: vec![],
        };
        let profiles = vec![AgentCapabilityProfile {
            id: "profile".into(),
            project_id: "project".into(),
            provider: ProviderId::codex(),
            display_name: "Codex".into(),
            strengths: vec![
                Capability::Architecture,
                Capability::Backend,
                Capability::Rust,
                Capability::Testing,
                Capability::Documentation,
                Capability::DevOps,
                Capability::Refactoring,
                Capability::Research,
            ],
            allowed_actions: vec![ActionKind::RunTests],
            max_context_tokens: 12_000,
            enabled: true,
            updated_at: now(),
        }];
        (goal, graph, memory, profiles)
    }

    #[test]
    fn creates_dependency_aware_plan_and_recovery_version() {
        let (goal, graph, memory, profiles) = fixture();
        let plan = generate(&goal, &graph, &memory, &profiles, 1);
        assert_eq!(plan.status, PlanStatus::Valid);
        assert!(plan.tasks.len() >= 5);
        assert!(plan
            .tasks
            .iter()
            .all(|task| task.suggested_provider.is_some()));
        let implementation = plan
            .tasks
            .iter()
            .find(|task| task.kind == PlannedTaskKind::Implementation)
            .unwrap();
        let replanned = replan(
            &plan,
            &implementation.id,
            "tests failed",
            &memory,
            &profiles,
        )
        .unwrap();
        assert_eq!(replanned.version, 2);
        assert!(replanned
            .tasks
            .iter()
            .any(|task| task.kind == PlannedTaskKind::Recovery));
        assert_eq!(replanned.parent_plan_id.as_deref(), Some(plan.id.as_str()));
    }

    #[test]
    fn detects_cycles() {
        let (goal, graph, memory, profiles) = fixture();
        let mut plan = generate(&goal, &graph, &memory, &profiles, 1);
        let first = plan.tasks[0].id.clone();
        let last = plan.tasks.last().unwrap().id.clone();
        plan.tasks[0].depends_on.push(last);
        plan.tasks.last_mut().unwrap().depends_on.push(first);
        assert!(has_cycle(&plan.tasks));
    }
}
