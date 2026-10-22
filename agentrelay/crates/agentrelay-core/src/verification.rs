use std::{collections::BTreeSet, fs, path::Path};

use chrono::{SecondsFormat, Utc};
use uuid::Uuid;

use crate::domain::{
    EnvironmentManifest, EnvironmentSnapshot, EnvironmentStatus, TaskWorkspace, VerificationGate,
    VerificationGateKind, VerificationImpact, VerificationStatus, VerificationSuite, WorkTask,
};

pub fn create_suite(
    project_id: &str,
    task: &WorkTask,
    workspace: &TaskWorkspace,
    environment: &EnvironmentSnapshot,
    manifest: &EnvironmentManifest,
    impact: VerificationImpact,
    goal_description: &str,
) -> VerificationSuite {
    let mut criteria = [goal_description, task.description.as_str()]
        .into_iter()
        .flat_map(|value| value.lines())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if criteria.is_empty() {
        criteria.push(format!("Complete task: {}", task.title));
    }
    let mut gates = vec![environment_gate(environment)];
    for (name, kind) in [
        ("format", VerificationGateKind::Format),
        ("lint", VerificationGateKind::Lint),
        ("test", VerificationGateKind::Tests),
        ("build", VerificationGateKind::Build),
        ("frontendBuild", VerificationGateKind::FrontendBuild),
        ("desktopBuild", VerificationGateKind::DesktopBuild),
    ] {
        if let Some(command) = manifest.commands.get(name) {
            gates.push(command_gate(kind, command.clone()));
        }
    }
    gates.push(pending_gate(
        VerificationGateKind::Architecture,
        "Architecture rules",
        !manifest.architecture_rules.is_empty(),
    ));
    gates.push(pending_gate(
        VerificationGateKind::AcceptanceCriteria,
        "Acceptance criteria",
        true,
    ));
    VerificationSuite {
        id: Uuid::new_v4().to_string(),
        project_id: project_id.into(),
        task_id: task.id.clone(),
        workspace_id: workspace.id.clone(),
        status: VerificationStatus::Running,
        environment_id: environment.id.clone(),
        environment_fingerprint: environment.fingerprint.clone(),
        environment_profile: environment.profile,
        commit: workspace.head_commit.clone(),
        branch: workspace.branch.clone(),
        worktree: workspace.worktree_path.clone(),
        acceptance_criteria: criteria,
        impact,
        gates,
        repair_task_id: None,
        created_at: now(),
        completed_at: None,
    }
}

pub fn verify_architecture(repository: &Path, rules: &[String]) -> (VerificationStatus, String) {
    if rules.is_empty() {
        return (
            VerificationStatus::Waived,
            "No architecture rules are configured for this project.".into(),
        );
    }
    let mut failures = Vec::new();
    for rule in rules {
        match rule.as_str() {
            "core-must-not-depend-on-tauri" => {
                let path = repository.join("crates/agentrelay-core/Cargo.toml");
                if path.is_file() {
                    match fs::read_to_string(&path)
                        .ok()
                        .and_then(|source| toml::from_str::<toml::Value>(&source).ok())
                    {
                        Some(value)
                            if dependency_tables(&value)
                                .iter()
                                .all(|table| !table.contains_key("tauri")) => {}
                        Some(_) => failures.push(
                            "agentrelay-core depends on Tauri; the core must remain reusable"
                                .into(),
                        ),
                        None => {
                            failures.push("could not inspect agentrelay-core Cargo.toml".into())
                        }
                    }
                }
            }
            "frontend-must-not-spawn-processes" => {
                let source = repository.join("frontend/src");
                let violations = scan_frontend_process_violations(&source);
                if !violations.is_empty() {
                    failures.push(format!(
                        "frontend process-spawn boundary violated in {}",
                        violations.join(", ")
                    ));
                }
            }
            unknown => failures.push(format!("unknown architecture rule `{unknown}`")),
        }
    }
    if failures.is_empty() {
        (
            VerificationStatus::Passed,
            format!("{} configured architecture rule(s) passed.", rules.len()),
        )
    } else {
        (VerificationStatus::Failed, failures.join(" "))
    }
}

pub fn verify_acceptance(
    task: &WorkTask,
    changed_files: &[String],
    criteria: &[String],
) -> (VerificationStatus, String) {
    if changed_files.is_empty() {
        return (
            VerificationStatus::Failed,
            "No committed diff exists to satisfy the task acceptance criteria.".into(),
        );
    }
    if !task.claimed_paths.is_empty()
        && !task.claimed_paths.iter().any(|claim| {
            changed_files
                .iter()
                .any(|changed| paths_overlap(claim, changed))
        })
    {
        return (
            VerificationStatus::Failed,
            "The committed diff does not touch any file or module claimed by the task.".into(),
        );
    }
    (
        VerificationStatus::Passed,
        format!(
            "{} recorded acceptance criterion/criteria are backed by {} committed changed file(s); command and architecture gates provide independent executable evidence.",
            criteria.len(),
            changed_files.len()
        ),
    )
}

pub fn finish_gate(
    gate: &mut VerificationGate,
    status: VerificationStatus,
    exit_code: Option<i32>,
    duration_ms: Option<u64>,
    stdout: &str,
    stderr: &str,
    detail: String,
) {
    gate.status = status;
    gate.exit_code = exit_code;
    gate.duration_ms = duration_ms;
    gate.stdout_summary = summarize(stdout);
    gate.stderr_summary = summarize(stderr);
    gate.detail = detail;
    gate.finished_at = Some(now());
}

pub fn complete_suite(suite: &mut VerificationSuite) {
    let required = suite
        .gates
        .iter()
        .filter(|gate| gate.required)
        .collect::<Vec<_>>();
    suite.status = if required
        .iter()
        .any(|gate| gate.status == VerificationStatus::Failed)
    {
        VerificationStatus::Failed
    } else if required
        .iter()
        .all(|gate| gate.status == VerificationStatus::Waived)
    {
        VerificationStatus::Waived
    } else if required.iter().all(|gate| gate.status.satisfies_gate()) {
        VerificationStatus::Passed
    } else {
        VerificationStatus::Running
    };
    if suite.status != VerificationStatus::Running {
        suite.completed_at = Some(now());
    }
}

pub fn waive_gate(
    suite: &mut VerificationSuite,
    gate_id: &str,
    reason: &str,
) -> Result<(), String> {
    if reason.trim().is_empty() {
        return Err("a waiver needs a recorded reason".into());
    }
    let gate = suite
        .gates
        .iter_mut()
        .find(|gate| gate.id == gate_id)
        .ok_or_else(|| "verification gate not found".to_owned())?;
    if gate.status != VerificationStatus::Failed {
        return Err("only a failed verification gate may be waived".into());
    }
    gate.status = VerificationStatus::Waived;
    gate.waiver_reason = Some(reason.trim().into());
    gate.detail = format!("Explicitly waived: {}", reason.trim());
    gate.finished_at = Some(now());
    complete_suite(suite);
    Ok(())
}

fn environment_gate(snapshot: &EnvironmentSnapshot) -> VerificationGate {
    let (status, detail) = match snapshot.status {
        EnvironmentStatus::Reproducible => (
            VerificationStatus::Passed,
            format!(
                "Reproducible environment {} matched.",
                &snapshot.fingerprint[..12]
            ),
        ),
        EnvironmentStatus::EnvironmentDrift => (
            VerificationStatus::Failed,
            "Environment drift was detected before verification.".into(),
        ),
        EnvironmentStatus::Incomplete => (
            VerificationStatus::Failed,
            "The environment is missing a required tool or variable.".into(),
        ),
        EnvironmentStatus::Unmanaged => (
            VerificationStatus::Failed,
            "Save the inferred environment manifest before verification.".into(),
        ),
    };
    VerificationGate {
        id: Uuid::new_v4().to_string(),
        kind: VerificationGateKind::Environment,
        name: "Reproducible environment".into(),
        required: true,
        status,
        command: vec![],
        exit_code: None,
        duration_ms: Some(0),
        stdout_summary: String::new(),
        stderr_summary: String::new(),
        detail,
        started_at: Some(now()),
        finished_at: Some(now()),
        waiver_reason: None,
    }
}

fn command_gate(kind: VerificationGateKind, command: Vec<String>) -> VerificationGate {
    let mut gate = pending_gate(kind, gate_name(kind), true);
    gate.command = command;
    gate
}

fn pending_gate(kind: VerificationGateKind, name: &str, required: bool) -> VerificationGate {
    VerificationGate {
        id: Uuid::new_v4().to_string(),
        kind,
        name: name.into(),
        required,
        status: VerificationStatus::NotRun,
        command: vec![],
        exit_code: None,
        duration_ms: None,
        stdout_summary: String::new(),
        stderr_summary: String::new(),
        detail: String::new(),
        started_at: None,
        finished_at: None,
        waiver_reason: None,
    }
}

fn gate_name(kind: VerificationGateKind) -> &'static str {
    match kind {
        VerificationGateKind::Format => "Formatting",
        VerificationGateKind::Lint => "Lint",
        VerificationGateKind::Tests => "Tests",
        VerificationGateKind::Build => "Build",
        VerificationGateKind::FrontendBuild => "Frontend build",
        VerificationGateKind::DesktopBuild => "Desktop build",
        VerificationGateKind::Environment => "Environment",
        VerificationGateKind::Architecture => "Architecture",
        VerificationGateKind::AcceptanceCriteria => "Acceptance criteria",
    }
}

fn dependency_tables(value: &toml::Value) -> Vec<&toml::map::Map<String, toml::Value>> {
    ["dependencies", "dev-dependencies", "build-dependencies"]
        .into_iter()
        .filter_map(|name| value.get(name).and_then(toml::Value::as_table))
        .collect()
}

fn scan_frontend_process_violations(root: &Path) -> Vec<String> {
    if !root.is_dir() {
        return Vec::new();
    }
    let mut stack = vec![root.to_path_buf()];
    let mut violations = Vec::new();
    let mut inspected = 0usize;
    while let Some(directory) = stack.pop() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if inspected >= 2_000
                || !matches!(
                    path.extension().and_then(|value| value.to_str()),
                    Some("ts" | "tsx" | "js" | "jsx")
                )
            {
                continue;
            }
            inspected += 1;
            if fs::read_to_string(&path).is_ok_and(|source| {
                ["child_process", "@tauri-apps/plugin-shell", "Command::new"]
                    .iter()
                    .any(|needle| source.contains(needle))
            }) {
                violations.push(path.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    violations
}

fn summarize(value: &str) -> String {
    const LIMIT: usize = 8 * 1024;
    let value = value.trim();
    if value.len() <= LIMIT {
        return value.into();
    }
    let end = value
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= LIMIT)
        .last()
        .unwrap_or(0);
    format!("{}\n… output truncated …", &value[..end])
}

fn paths_overlap(left: &str, right: &str) -> bool {
    let left = normalize_path(left);
    let right = normalize_path(right);
    left == right
        || left.starts_with(&format!("{right}/"))
        || right.starts_with(&format!("{left}/"))
}

fn normalize_path(value: &str) -> String {
    value
        .replace('\\', "/")
        .trim_end_matches('*')
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        EnvironmentStatus, ExecutionProfile, TaskAssignmentStatus, TaskPriority, TaskStatus,
        TaskWorkspaceStatus,
    };

    fn task() -> WorkTask {
        WorkTask {
            id: "task".into(),
            goal_id: "goal".into(),
            title: "Implement".into(),
            description: "The build passes".into(),
            status: TaskStatus::Todo,
            priority: TaskPriority::High,
            assigned_agent: None,
            source_agent: None,
            created_at: now(),
            completed_at: None,
            assignment_status: TaskAssignmentStatus::Running,
            suggested_provider: None,
            assigned_provider: None,
            route_reasons: vec![],
            required_capabilities: vec![],
            blocked_by: vec![],
            depends_on: vec![],
            produces: vec![],
            consumes: vec![],
            claimed_paths: vec!["src".into()],
        }
    }

    #[test]
    fn acceptance_requires_a_committed_diff_in_scope() {
        let task = task();
        assert_eq!(
            verify_acceptance(&task, &[], &["done".into()]).0,
            VerificationStatus::Failed
        );
        assert_eq!(
            verify_acceptance(&task, &["src/lib.rs".into()], &["done".into()]).0,
            VerificationStatus::Passed
        );
    }

    #[test]
    fn environment_drift_is_a_blocking_gate() {
        let workspace = TaskWorkspace {
            id: "workspace".into(),
            project_id: "project".into(),
            task_id: "task".into(),
            assigned_provider: crate::domain::ProviderId::codex(),
            branch: "branch".into(),
            worktree_path: "repo".into(),
            base_commit: "a".into(),
            head_commit: "b".into(),
            status: TaskWorkspaceStatus::ValidationPassed,
            session_id: None,
            latest_validation_id: None,
            created_at: now(),
            updated_at: now(),
            merged_at: None,
            archived_at: None,
        };
        let environment = EnvironmentSnapshot {
            id: "environment".into(),
            project_id: "project".into(),
            profile: ExecutionProfile::NativeWindows,
            manifest_source: ".agentrelay/environment.toml".into(),
            fingerprint: "1234567890123456".into(),
            status: EnvironmentStatus::EnvironmentDrift,
            tools: vec![],
            environment: vec![],
            fingerprint_inputs: vec![],
            drift: vec![],
            os: "windows".into(),
            architecture: "x86_64".into(),
            commit: "b".into(),
            branch: "branch".into(),
            worktree: "repo".into(),
            captured_at: now(),
        };
        let manifest = EnvironmentManifest {
            schema_version: 1,
            profile: ExecutionProfile::NativeWindows,
            working_directory: ".".into(),
            container_image: None,
            disposable_verification: true,
            runtimes: Default::default(),
            package_manager: None,
            system_tools: vec![],
            commands: Default::default(),
            environment: vec![],
            cache_kinds: vec![],
            architecture_rules: vec![],
        };
        let mut suite = create_suite(
            "project",
            &task(),
            &workspace,
            &environment,
            &manifest,
            VerificationImpact {
                changed_files: vec!["src/lib.rs".into()],
                highest_risk: None,
                affected_modules: vec![],
                tests_to_run: vec![],
            },
            "done",
        );
        for gate in suite
            .gates
            .iter_mut()
            .filter(|gate| gate.kind != VerificationGateKind::Environment)
        {
            gate.status = VerificationStatus::Waived;
        }
        complete_suite(&mut suite);
        assert_eq!(suite.status, VerificationStatus::Failed);
    }
}
