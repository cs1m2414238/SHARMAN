use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::domain::{
    ActionKind, ActionStatus, ChangeRisk, ConflictStatus, EdgeType, GraphEdge, GraphNode,
    GraphStats, ImpactAnalysis, NodeType, ProjectGraph, ProjectMemory, RiskFactor, RiskLevel,
    TaskStatus, WorkTask,
};

const MAX_GRAPH_FILES: usize = 10_000;
const MAX_SOURCE_BYTES: u64 = 512 * 1024;

pub fn build(memory: &ProjectMemory, project_files: &[String]) -> ProjectGraph {
    let mut builder = GraphBuilder::new(memory);
    builder.add_memory_nodes();
    builder.add_files(project_files);
    builder.add_task_relationships();
    builder.add_decision_relationships();
    builder.add_session_relationships();
    builder.add_source_relationships();
    builder.finish()
}

pub fn analyze(
    graph: &ProjectGraph,
    memory: &ProjectMemory,
    target: &str,
) -> Result<ImpactAnalysis, GraphError> {
    let target = resolve_target(graph, target)
        .cloned()
        .ok_or_else(|| GraphError::TargetNotFound(target.to_owned()))?;
    if !matches!(target.node_type, NodeType::File | NodeType::Test) {
        return Err(GraphError::UnsupportedTarget(target.label));
    }

    let node_map = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    let mut impacted_ids = BTreeSet::from([target.id.clone()]);
    let mut dependent_ids = BTreeSet::new();
    let mut queue = VecDeque::from([target.id.clone()]);
    while let Some(current) = queue.pop_front() {
        for edge in graph
            .edges
            .iter()
            .filter(|edge| edge.edge_type == EdgeType::DependsOn && edge.target == current)
        {
            if impacted_ids.insert(edge.source.clone()) {
                queue.push_back(edge.source.clone());
            }
            if node_map
                .get(edge.source.as_str())
                .is_some_and(|node| node.node_type == NodeType::File)
            {
                dependent_ids.insert(edge.source.clone());
            }
        }
    }

    let mut module_ids = BTreeSet::new();
    for edge in graph
        .edges
        .iter()
        .filter(|edge| edge.edge_type == EdgeType::BelongsTo && impacted_ids.contains(&edge.source))
    {
        module_ids.insert(edge.target.clone());
    }
    let initial_modules = module_ids.iter().cloned().collect::<Vec<_>>();
    for module in initial_modules {
        for edge in graph
            .edges
            .iter()
            .filter(|edge| edge.edge_type == EdgeType::DependsOn && edge.target == module)
        {
            if node_map
                .get(edge.source.as_str())
                .is_some_and(|node| node.node_type == NodeType::Module)
            {
                module_ids.insert(edge.source.clone());
            }
        }
    }
    impacted_ids.extend(module_ids.iter().cloned());

    let affected_task_ids =
        related_sources(graph, EdgeType::Touches, &impacted_ids, NodeType::Task);
    let test_ids = related_sources(graph, EdgeType::Validates, &impacted_ids, NodeType::Test);
    let decision_ids = related_sources(graph, EdgeType::Affects, &impacted_ids, NodeType::Decision);
    let session_ids = graph
        .edges
        .iter()
        .filter(|edge| {
            edge.edge_type == EdgeType::ModifiedBy && impacted_ids.contains(&edge.source)
        })
        .filter_map(|edge| {
            node_map
                .get(edge.target.as_str())
                .filter(|node| node.node_type == NodeType::Session)
                .map(|_| edge.target.clone())
        })
        .collect::<BTreeSet<_>>();

    let affected_modules = nodes_for_ids(&node_map, &module_ids);
    let affected_tasks = nodes_for_ids(&node_map, &affected_task_ids);
    let dependent_files = nodes_for_ids(&node_map, &dependent_ids);
    let validating_tests = nodes_for_ids(&node_map, &test_ids);
    let relevant_decisions = nodes_for_ids(&node_map, &decision_ids);
    let related_sessions = nodes_for_ids(&node_map, &session_ids);

    let impacted_paths = impacted_ids
        .iter()
        .filter_map(|id| {
            node_map
                .get(id.as_str())
                .and_then(|node| node.path.as_ref())
        })
        .map(|path| fold(path))
        .collect::<BTreeSet<_>>();
    let active_conflicts = memory
        .conflicts
        .iter()
        .filter(|conflict| conflict.status != ConflictStatus::Resolved)
        .filter(|conflict| {
            conflict.affected_paths.iter().any(|path| {
                let path = fold(path);
                impacted_paths.iter().any(|impacted| {
                    impacted == &path
                        || impacted.starts_with(&format!("{path}/"))
                        || path.starts_with(&format!("{impacted}/"))
                })
            })
        })
        .cloned()
        .collect::<Vec<_>>();

    let evidence_ids = impacted_ids
        .iter()
        .chain(&affected_task_ids)
        .chain(&test_ids)
        .chain(&decision_ids)
        .chain(&session_ids)
        .cloned()
        .collect::<BTreeSet<_>>();
    let evidence_edges = graph
        .edges
        .iter()
        .filter(|edge| evidence_ids.contains(&edge.source) && evidence_ids.contains(&edge.target))
        .cloned()
        .collect::<Vec<_>>();

    let risk = score_risk(
        memory,
        dependent_files.len(),
        &affected_tasks,
        &validating_tests,
        &relevant_decisions,
        &related_sessions,
        active_conflicts.len(),
        target
            .path
            .as_ref()
            .is_some_and(|path| is_currently_changed(memory, path)),
    );
    let warning = risk_warning(risk.level, &target.label);
    let tests_to_run = if validating_tests.is_empty() {
        vec!["Full project test suite — no direct validation edge was found".into()]
    } else {
        validating_tests
            .iter()
            .filter_map(|test| test.path.clone())
            .collect()
    };

    Ok(ImpactAnalysis {
        target,
        risk,
        affected_modules,
        affected_tasks,
        dependent_files,
        validating_tests,
        relevant_decisions,
        related_sessions,
        active_conflicts,
        evidence_edges,
        tests_to_run,
        warning,
    })
}

struct GraphBuilder<'a> {
    memory: &'a ProjectMemory,
    repository: PathBuf,
    nodes: BTreeMap<String, GraphNode>,
    edges: BTreeMap<(String, String, EdgeType), GraphEdge>,
    file_ids: BTreeMap<String, String>,
    module_for_file: BTreeMap<String, String>,
    warnings: Vec<String>,
}

impl<'a> GraphBuilder<'a> {
    fn new(memory: &'a ProjectMemory) -> Self {
        Self {
            memory,
            repository: PathBuf::from(&memory.project.path),
            nodes: BTreeMap::new(),
            edges: BTreeMap::new(),
            file_ids: BTreeMap::new(),
            module_for_file: BTreeMap::new(),
            warnings: Vec::new(),
        }
    }

    fn add_memory_nodes(&mut self) {
        let goals = self.memory.goals.clone();
        for goal in goals {
            let goal_id = format!("goal:{}", goal.id);
            self.node(
                goal_id.clone(),
                NodeType::Goal,
                goal.title,
                None,
                Some(format!("{:?}", goal.status).to_ascii_lowercase()),
                BTreeMap::new(),
            );
            for task in goal.tasks {
                let task_id = format!("task:{}", task.id);
                let mut metadata = BTreeMap::new();
                metadata.insert(
                    "priority".into(),
                    format!("{:?}", task.priority).to_ascii_lowercase(),
                );
                metadata.insert(
                    "assignmentStatus".into(),
                    format!("{:?}", task.assignment_status).to_ascii_lowercase(),
                );
                self.node(
                    task_id.clone(),
                    NodeType::Task,
                    task.title,
                    None,
                    Some(task_status(task.status).into()),
                    metadata,
                );
                self.edge(
                    goal_id.clone(),
                    task_id,
                    EdgeType::Contains,
                    "Goal contains this task.",
                );
            }
        }

        let tasks = self.tasks().into_iter().cloned().collect::<Vec<_>>();
        for task in tasks {
            let task_id = format!("task:{}", task.id);
            for dependency in task.depends_on {
                if self.nodes.contains_key(&format!("task:{dependency}")) {
                    self.edge(
                        task_id.clone(),
                        format!("task:{dependency}"),
                        EdgeType::DependsOn,
                        "Task dependency declared in orchestration metadata.",
                    );
                }
            }
            for blocker in task.blocked_by {
                if self.nodes.contains_key(&format!("task:{blocker}")) {
                    self.edge(
                        format!("task:{blocker}"),
                        task_id.clone(),
                        EdgeType::Blocks,
                        "Task is explicitly blocked by this task.",
                    );
                }
            }
        }

        let decisions = self.memory.decisions.clone();
        for decision in decisions {
            let mut metadata = BTreeMap::new();
            metadata.insert("createdBy".into(), decision.created_by);
            self.node(
                format!("decision:{}", decision.id),
                NodeType::Decision,
                decision.summary,
                None,
                Some(format!("{:?}", decision.status).to_ascii_lowercase()),
                metadata,
            );
        }
        let sessions = self.memory.agent_sessions.clone();
        for session in sessions {
            let mut metadata = BTreeMap::new();
            metadata.insert("provider".into(), session.provider.to_string());
            metadata.insert("agentName".into(), session.agent_name.clone());
            if let Some(task_id) = session.task_id {
                metadata.insert("taskId".into(), task_id);
            }
            self.node(
                format!("session:{}", session.id),
                NodeType::Session,
                session.agent_name,
                None,
                Some(format!("{:?}", session.status).to_ascii_lowercase()),
                metadata,
            );
        }
    }

    fn add_files(&mut self, project_files: &[String]) {
        let mut paths = project_files
            .iter()
            .map(|path| normalize(path))
            .filter(|path| !path.is_empty() && !path.starts_with("../"))
            .collect::<BTreeSet<_>>();
        if let Some(status) = &self.memory.project.latest_status {
            paths.extend(
                status
                    .modified_files
                    .iter()
                    .chain(&status.added_files)
                    .chain(&status.deleted_files)
                    .map(|path| normalize(path)),
            );
        }
        if paths.len() > MAX_GRAPH_FILES {
            self.warnings.push(format!(
                "Graph limited to {MAX_GRAPH_FILES} files out of {} discovered paths.",
                paths.len()
            ));
        }
        for path in paths.into_iter().take(MAX_GRAPH_FILES) {
            let node_type = if is_test_file(&path) {
                NodeType::Test
            } else {
                NodeType::File
            };
            let id = format!("{}:{path}", node_type.as_key());
            let mut metadata = BTreeMap::new();
            if let Some(change_state) = change_state(self.memory, &path) {
                metadata.insert("changeState".into(), change_state.into());
            }
            self.node(
                id.clone(),
                node_type,
                path.clone(),
                Some(path.clone()),
                metadata.get("changeState").cloned(),
                metadata,
            );
            self.file_ids.insert(path.clone(), id.clone());

            let module = infer_module(&path);
            let module_id = format!("module:{module}");
            self.node(
                module_id.clone(),
                NodeType::Module,
                module,
                None,
                None,
                BTreeMap::new(),
            );
            self.module_for_file.insert(id.clone(), module_id.clone());
            self.edge(
                id,
                module_id,
                EdgeType::BelongsTo,
                "Module inferred deterministically from the repository path.",
            );
        }
    }

    fn add_task_relationships(&mut self) {
        let files = self
            .file_ids
            .iter()
            .map(|(path, id)| (path.clone(), id.clone()))
            .collect::<Vec<_>>();
        let tasks = self.tasks().into_iter().cloned().collect::<Vec<_>>();
        for task in tasks {
            let task_id = format!("task:{}", task.id);
            for (path, file_id) in &files {
                let declared = task
                    .claimed_paths
                    .iter()
                    .chain(&task.produces)
                    .chain(&task.consumes)
                    .any(|pattern| path_matches(pattern, path));
                let mentioned =
                    mentions_path(&task.title, path) || mentions_path(&task.description, path);
                if declared || mentioned {
                    self.edge(
                        task_id.clone(),
                        file_id.clone(),
                        EdgeType::Touches,
                        if declared {
                            "Task path/artifact metadata maps to this file."
                        } else {
                            "Task title or description references this file."
                        },
                    );
                }
            }
        }
    }

    fn add_decision_relationships(&mut self) {
        let targets = self
            .nodes
            .values()
            .filter(|node| {
                matches!(
                    node.node_type,
                    NodeType::File | NodeType::Test | NodeType::Module
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        let decisions = self.memory.decisions.clone();
        for decision in decisions {
            let source = format!("decision:{}", decision.id);
            for affected in decision.affected {
                for target in &targets {
                    if reference_matches_node(&affected, target) {
                        self.edge(
                            source.clone(),
                            target.id.clone(),
                            EdgeType::Affects,
                            format!("Decision declares affected component {affected}."),
                        );
                    }
                }
            }
        }
    }

    fn add_session_relationships(&mut self) {
        let handoffs = self.memory.handoffs.clone();
        for handoff in handoffs {
            let session_id = format!("session:{}", handoff.session_id);
            if !self.nodes.contains_key(&session_id) {
                continue;
            }
            for changed in handoff.changed_files {
                if let Some(file_id) = self.file_ids.get(&normalize(&changed.path)).cloned() {
                    self.edge(
                        file_id,
                        session_id.clone(),
                        EdgeType::ModifiedBy,
                        "Completed handoff records this file as changed.",
                    );
                }
            }
        }

        let current_paths = self
            .memory
            .project
            .latest_status
            .as_ref()
            .map(|status| {
                status
                    .modified_files
                    .iter()
                    .chain(&status.added_files)
                    .chain(&status.deleted_files)
                    .map(|path| normalize(path))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let tasks = self.tasks().into_iter().cloned().collect::<Vec<_>>();
        let sessions = self.memory.agent_sessions.clone();
        for session in sessions
            .into_iter()
            .filter(|session| session.status == crate::domain::AgentSessionStatus::Active)
        {
            let task = session
                .task_id
                .as_ref()
                .and_then(|id| tasks.iter().find(|task| &task.id == id));
            for path in &current_paths {
                let related = task.is_none_or(|task| {
                    task.claimed_paths.is_empty()
                        || task
                            .claimed_paths
                            .iter()
                            .any(|pattern| path_matches(pattern, path))
                });
                if related {
                    if let Some(file_id) = self.file_ids.get(path).cloned() {
                        self.edge(
                            file_id,
                            format!("session:{}", session.id),
                            EdgeType::ModifiedBy,
                            "Active session overlaps a current worktree change or path claim.",
                        );
                    }
                }
            }
        }
    }

    fn add_source_relationships(&mut self) {
        let file_entries = self
            .file_ids
            .iter()
            .map(|(path, id)| (path.clone(), id.clone()))
            .collect::<Vec<_>>();
        for (path, source_id) in &file_entries {
            let full_path = self.repository.join(path);
            if !is_supported_source(path) || !full_path.is_file() {
                continue;
            }
            let Ok(metadata) = fs::metadata(&full_path) else {
                continue;
            };
            if metadata.len() > MAX_SOURCE_BYTES {
                self.warnings.push(format!(
                    "Skipped dependency analysis for {path} because it exceeds {} KiB.",
                    MAX_SOURCE_BYTES / 1024
                ));
                continue;
            }
            let Ok(contents) = fs::read_to_string(&full_path) else {
                self.warnings
                    .push(format!("Skipped non-text source file {path}."));
                continue;
            };
            let targets = source_dependencies(path, &contents, &self.file_ids);
            for target_path in targets {
                let Some(target_id) = self.file_ids.get(&target_path).cloned() else {
                    continue;
                };
                let source_is_test = self
                    .nodes
                    .get(source_id)
                    .is_some_and(|node| node.node_type == NodeType::Test);
                self.edge(
                    source_id.clone(),
                    target_id.clone(),
                    if source_is_test {
                        EdgeType::Validates
                    } else {
                        EdgeType::DependsOn
                    },
                    if source_is_test {
                        "Test imports or references this source file."
                    } else {
                        "Static import/module reference resolved to this file."
                    },
                );
                self.add_module_dependency(source_id, &target_id);
            }
        }

        let source_files = file_entries
            .iter()
            .filter(|(_, id)| {
                self.nodes
                    .get(id)
                    .is_some_and(|node| node.node_type == NodeType::File)
            })
            .cloned()
            .collect::<Vec<_>>();
        let tests = file_entries
            .iter()
            .filter(|(_, id)| {
                self.nodes
                    .get(id)
                    .is_some_and(|node| node.node_type == NodeType::Test)
            })
            .cloned()
            .collect::<Vec<_>>();
        for (test_path, test_id) in tests {
            let test_stem = semantic_stem(&test_path);
            for (source_path, source_id) in &source_files {
                if test_stem == semantic_stem(source_path) {
                    self.edge(
                        test_id.clone(),
                        source_id.clone(),
                        EdgeType::Validates,
                        "Test and source filenames share the same semantic stem.",
                    );
                }
            }
            if let Some(module_id) = self.module_for_file.get(&test_id).cloned() {
                self.edge(
                    test_id,
                    module_id,
                    EdgeType::Validates,
                    "Test validates its inferred module.",
                );
            }
        }
    }

    fn add_module_dependency(&mut self, source_file: &str, target_file: &str) {
        let Some(source_module) = self.module_for_file.get(source_file).cloned() else {
            return;
        };
        let Some(target_module) = self.module_for_file.get(target_file).cloned() else {
            return;
        };
        if source_module != target_module {
            self.edge(
                source_module,
                target_module,
                EdgeType::DependsOn,
                "A file in the source module depends on a file in the target module.",
            );
        }
    }

    fn tasks(&self) -> Vec<&WorkTask> {
        self.memory
            .goals
            .iter()
            .flat_map(|goal| goal.tasks.iter())
            .collect()
    }

    fn node(
        &mut self,
        id: String,
        node_type: NodeType,
        label: String,
        path: Option<String>,
        status: Option<String>,
        metadata: BTreeMap<String, String>,
    ) {
        self.nodes.entry(id.clone()).or_insert(GraphNode {
            id,
            node_type,
            label,
            path,
            status,
            metadata,
        });
    }

    fn edge(
        &mut self,
        source: String,
        target: String,
        edge_type: EdgeType,
        evidence: impl Into<String>,
    ) {
        if source == target
            || !self.nodes.contains_key(&source)
            || !self.nodes.contains_key(&target)
        {
            return;
        }
        self.edges
            .entry((source.clone(), target.clone(), edge_type))
            .or_insert(GraphEdge {
                source,
                target,
                edge_type,
                evidence: evidence.into(),
            });
    }

    fn finish(self) -> ProjectGraph {
        let project_id = self.memory.project.id.clone();
        let repository = self.memory.project.path.clone();
        let nodes = self.nodes.into_values().collect::<Vec<_>>();
        let edges = self.edges.into_values().collect::<Vec<_>>();
        let count = |node_type| {
            nodes
                .iter()
                .filter(|node| node.node_type == node_type)
                .count()
        };
        let stats = GraphStats {
            goals: count(NodeType::Goal),
            tasks: count(NodeType::Task),
            files: count(NodeType::File),
            modules: count(NodeType::Module),
            tests: count(NodeType::Test),
            decisions: count(NodeType::Decision),
            sessions: count(NodeType::Session),
            edges: edges.len(),
        };
        ProjectGraph {
            project_id,
            repository,
            nodes,
            edges,
            stats,
            warnings: self.warnings,
        }
    }
}

fn related_sources(
    graph: &ProjectGraph,
    edge_type: EdgeType,
    targets: &BTreeSet<String>,
    node_type: NodeType,
) -> BTreeSet<String> {
    let types = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node.node_type))
        .collect::<BTreeMap<_, _>>();
    graph
        .edges
        .iter()
        .filter(|edge| edge.edge_type == edge_type && targets.contains(&edge.target))
        .filter(|edge| types.get(edge.source.as_str()) == Some(&node_type))
        .map(|edge| edge.source.clone())
        .collect()
}

fn nodes_for_ids(nodes: &BTreeMap<&str, &GraphNode>, ids: &BTreeSet<String>) -> Vec<GraphNode> {
    ids.iter()
        .filter_map(|id| nodes.get(id.as_str()).copied().cloned())
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn score_risk(
    memory: &ProjectMemory,
    dependent_count: usize,
    tasks: &[GraphNode],
    tests: &[GraphNode],
    decisions: &[GraphNode],
    sessions: &[GraphNode],
    conflict_count: usize,
    currently_changed: bool,
) -> ChangeRisk {
    let active_tasks = tasks
        .iter()
        .filter(|node| !matches!(node.status.as_deref(), Some("completed" | "archived")))
        .count();
    let active_sessions = sessions
        .iter()
        .filter(|node| node.status.as_deref() == Some("active"))
        .count();
    let active_decisions = decisions
        .iter()
        .filter(|node| node.status.as_deref() == Some("active"))
        .count();
    let recent_failures = memory
        .action_runs
        .iter()
        .take(20)
        .filter(|run| {
            run.status == ActionStatus::Failed
                && matches!(run.action, ActionKind::RunTests | ActionKind::RunBuild)
        })
        .count();

    let factors = vec![
        factor(
            "dependents",
            "Dependent files",
            (dependent_count * 8).min(32) as u8,
            format!("{dependent_count} source file(s) transitively depend on this target."),
        ),
        factor(
            "activeTasks",
            "Active tasks",
            (active_tasks * 12).min(24) as u8,
            format!("{active_tasks} active task(s) touch the affected graph."),
        ),
        factor(
            "testCoverage",
            "Mapped test coverage",
            match tests.len() {
                0 => 24,
                1 => 8,
                _ => 0,
            },
            if tests.is_empty() {
                "No validating test edge was discovered.".into()
            } else {
                format!("{} validating test(s) were discovered.", tests.len())
            },
        ),
        factor(
            "concurrentSessions",
            "Concurrent sessions",
            (active_sessions * 18).min(36) as u8,
            format!("{active_sessions} active agent session(s) touch related code."),
        ),
        factor(
            "architecturalDecisions",
            "Architectural decisions",
            (active_decisions * 8).min(24) as u8,
            format!("{active_decisions} active decision(s) affect this area."),
        ),
        factor(
            "recentFailures",
            "Recent failure history",
            (recent_failures * 10).min(20) as u8,
            format!("{recent_failures} recent controlled test/build failure(s) exist."),
        ),
        factor(
            "openConflicts",
            "Open conflicts",
            (conflict_count * 20).min(40) as u8,
            format!("{conflict_count} unresolved conflict(s) overlap the impact set."),
        ),
        factor(
            "worktreeState",
            "Current worktree",
            if currently_changed { 5 } else { 0 },
            if currently_changed {
                "The target already has an uncommitted worktree change.".into()
            } else {
                "The target is not currently modified.".into()
            },
        ),
    ];
    let score = factors
        .iter()
        .map(|factor| u16::from(factor.points))
        .sum::<u16>()
        .min(100) as u8;
    let level = match score {
        0..=24 => RiskLevel::Low,
        25..=49 => RiskLevel::Medium,
        50..=74 => RiskLevel::High,
        _ => RiskLevel::Critical,
    };
    ChangeRisk {
        score,
        level,
        factors,
    }
}

fn factor(key: &str, label: &str, points: u8, detail: String) -> RiskFactor {
    RiskFactor {
        key: key.into(),
        label: label.into(),
        points,
        detail,
    }
}

fn risk_warning(level: RiskLevel, label: &str) -> String {
    match level {
        RiskLevel::Low => {
            format!("Low mapped impact for {label}. Review the listed edges before editing.")
        }
        RiskLevel::Medium => {
            format!("{label} has connected project impact. Run the mapped tests after editing.")
        }
        RiskLevel::High => {
            format!("High-risk edit: {label} affects active work or several dependents. Coordinate before changing it.")
        }
        RiskLevel::Critical => {
            format!("Critical-risk edit: pause and resolve active sessions/conflicts around {label} before changing it.")
        }
    }
}

fn resolve_target<'a>(graph: &'a ProjectGraph, target: &str) -> Option<&'a GraphNode> {
    let normalized = normalize(target);
    graph.nodes.iter().find(|node| {
        node.id.eq_ignore_ascii_case(target)
            || node
                .path
                .as_ref()
                .is_some_and(|path| normalize(path).eq_ignore_ascii_case(&normalized))
    })
}

fn infer_module(path: &str) -> String {
    let segments = path.split('/').collect::<Vec<_>>();
    if matches!(segments.first(), Some(&"crates" | &"apps" | &"packages")) && segments.len() > 1 {
        format!("{}/{}", segments[0], segments[1])
    } else if matches!(segments.first(), Some(&"frontend" | &"backend")) {
        segments[0].into()
    } else if segments.first() == Some(&"src") && segments.len() > 2 {
        format!("src/{}", segments[1])
    } else if segments.first() == Some(&"src") {
        "src".into()
    } else if segments.first() == Some(&"tests") {
        "tests".into()
    } else if segments.len() > 1 {
        segments[0].into()
    } else {
        "root".into()
    }
}

fn is_test_file(path: &str) -> bool {
    let lower = fold(path);
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    lower.starts_with("tests/")
        || lower.contains("/tests/")
        || lower.contains("/__tests__/")
        || name.starts_with("test_")
        || name.contains("_test.")
        || name.contains("_tests.")
        || name.contains(".test.")
        || name.contains(".spec.")
}

fn is_supported_source(path: &str) -> bool {
    matches!(
        Path::new(path).extension().and_then(|value| value.to_str()),
        Some("rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "py")
    )
}

fn source_dependencies(
    current: &str,
    contents: &str,
    files: &BTreeMap<String, String>,
) -> BTreeSet<String> {
    let mut resolved = BTreeSet::new();
    for line in contents.lines() {
        for specifier in quoted_relative_specifiers(line) {
            if let Some(path) = resolve_relative_import(current, &specifier, files) {
                resolved.insert(path);
            }
        }
        if current.ends_with(".rs") {
            if let Some(module) = rust_mod_declaration(line) {
                if let Some(path) = resolve_rust_module(current, &module, files) {
                    resolved.insert(path);
                }
            }
            if let Some(path) = resolve_rust_use(current, line, files) {
                resolved.insert(path);
            }
        }
        if current.ends_with(".py") {
            if let Some(specifier) = python_relative_import(line) {
                if let Some(path) = resolve_relative_import(current, &specifier, files) {
                    resolved.insert(path);
                }
            }
        }
    }
    resolved.remove(&normalize(current));
    resolved
}

fn quoted_relative_specifiers(line: &str) -> Vec<String> {
    if !["import", "export", "require(", "from "]
        .iter()
        .any(|marker| line.contains(marker))
    {
        return Vec::new();
    }
    let bytes = line.as_bytes();
    let mut values = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if matches!(bytes[index], b'\'' | b'"') {
            let quote = bytes[index];
            let start = index + 1;
            index = start;
            while index < bytes.len() && bytes[index] != quote {
                index += 1;
            }
            if let Some(value) = line
                .get(start..index)
                .filter(|value| value.starts_with('.'))
            {
                values.push(value.into());
            }
        }
        index += 1;
    }
    values
}

fn resolve_relative_import(
    current: &str,
    specifier: &str,
    files: &BTreeMap<String, String>,
) -> Option<String> {
    let parent = current.rsplit_once('/').map_or("", |(parent, _)| parent);
    let specifier = specifier.split(['?', '#']).next().unwrap_or(specifier);
    let joined = if parent.is_empty() {
        specifier.into()
    } else {
        format!("{parent}/{specifier}")
    };
    resolve_candidate(&normalize_segments(&joined), files)
}

fn rust_mod_declaration(line: &str) -> Option<String> {
    let line = line.trim().strip_prefix("pub ").unwrap_or(line.trim());
    let module = line.strip_prefix("mod ")?.strip_suffix(';')?.trim();
    (!module.is_empty()
        && module
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'))
    .then(|| module.into())
}

fn resolve_rust_module(
    current: &str,
    module: &str,
    files: &BTreeMap<String, String>,
) -> Option<String> {
    let parent = current.rsplit_once('/').map_or("", |(parent, _)| parent);
    resolve_candidate(&format!("{parent}/{module}"), files)
}

fn resolve_rust_use(current: &str, line: &str, files: &BTreeMap<String, String>) -> Option<String> {
    let line = line.trim().strip_prefix("pub ").unwrap_or(line.trim());
    let path = line.strip_prefix("use ")?.split(';').next()?.trim();
    let path = path.split(['{', ' ', '*']).next()?.trim_end_matches("::");
    let segments = path.split("::").collect::<Vec<_>>();
    let candidate = match segments.first().copied()? {
        "crate" => {
            let root = current
                .find("/src/")
                .map(|index| &current[..index + 4])
                .or_else(|| current.starts_with("src/").then_some("src"))?;
            format!("{root}/{}", segments[1..].join("/"))
        }
        "self" => {
            let parent = current.rsplit_once('/').map_or("", |(parent, _)| parent);
            format!("{parent}/{}", segments[1..].join("/"))
        }
        "super" => {
            let parent = current.rsplit_once('/').map_or("", |(parent, _)| parent);
            let upper = parent.rsplit_once('/').map_or("", |(upper, _)| upper);
            format!("{upper}/{}", segments[1..].join("/"))
        }
        _ => return None,
    };
    let mut candidate = normalize_segments(&candidate);
    loop {
        if let Some(resolved) = resolve_candidate(&candidate, files) {
            return Some(resolved);
        }
        let (parent, _) = candidate.rsplit_once('/')?;
        candidate = parent.into();
    }
}

fn python_relative_import(line: &str) -> Option<String> {
    let line = line.trim();
    let module = line.strip_prefix("from ")?.split_whitespace().next()?;
    module.starts_with('.').then(|| module.replace('.', "./"))
}

fn resolve_candidate(candidate: &str, files: &BTreeMap<String, String>) -> Option<String> {
    let candidate = normalize(candidate).trim_start_matches('/').to_owned();
    let candidates = [
        candidate.clone(),
        format!("{candidate}.rs"),
        format!("{candidate}/mod.rs"),
        format!("{candidate}.ts"),
        format!("{candidate}.tsx"),
        format!("{candidate}.js"),
        format!("{candidate}.jsx"),
        format!("{candidate}/index.ts"),
        format!("{candidate}/index.tsx"),
        format!("{candidate}/index.js"),
        format!("{candidate}.py"),
        format!("{candidate}/__init__.py"),
    ];
    candidates.into_iter().find(|path| files.contains_key(path))
}

fn normalize_segments(path: &str) -> String {
    let normalized = normalize(path);
    let mut segments = Vec::new();
    for segment in normalized.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            value => segments.push(value),
        }
    }
    segments.join("/")
}

fn semantic_stem(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = name.split('.').next().unwrap_or(name).to_ascii_lowercase();
    stem.trim_start_matches("test_")
        .trim_end_matches("_tests")
        .trim_end_matches("_test")
        .to_owned()
}

fn path_matches(pattern: &str, path: &str) -> bool {
    let pattern = fold(pattern)
        .trim_start_matches("./")
        .trim_end_matches('/')
        .to_owned();
    let path = fold(path);
    if let Some(prefix) = pattern.strip_suffix('*') {
        path.starts_with(prefix.trim_end_matches('/'))
    } else {
        path == pattern
            || path.starts_with(&format!("{pattern}/"))
            || pattern.starts_with(&format!("{path}/"))
    }
}

fn mentions_path(text: &str, path: &str) -> bool {
    let text = text.to_ascii_lowercase();
    let path = fold(path);
    let file = path.rsplit('/').next().unwrap_or(&path);
    text.contains(&path) || (file.len() >= 4 && text.contains(file))
}

fn reference_matches_node(reference: &str, node: &GraphNode) -> bool {
    let reference = fold(reference).trim_matches('/').to_owned();
    let label = fold(&node.label).trim_matches('/').to_owned();
    let path = node.path.as_deref().map(fold);
    !reference.is_empty()
        && (reference == label
            || label.contains(&reference)
            || reference.contains(&label)
            || path.as_ref().is_some_and(|path| {
                path == &reference
                    || path.ends_with(&format!("/{reference}"))
                    || reference.ends_with(&format!("/{path}"))
            }))
}

fn change_state<'a>(memory: &'a ProjectMemory, path: &str) -> Option<&'a str> {
    let status = memory.project.latest_status.as_ref()?;
    if status
        .modified_files
        .iter()
        .any(|item| normalize(item).eq_ignore_ascii_case(path))
    {
        Some("modified")
    } else if status
        .added_files
        .iter()
        .any(|item| normalize(item).eq_ignore_ascii_case(path))
    {
        Some("added")
    } else if status
        .deleted_files
        .iter()
        .any(|item| normalize(item).eq_ignore_ascii_case(path))
    {
        Some("deleted")
    } else {
        None
    }
}

fn is_currently_changed(memory: &ProjectMemory, path: &str) -> bool {
    change_state(memory, &normalize(path)).is_some()
}

fn task_status(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Todo => "todo",
        TaskStatus::InProgress => "inProgress",
        TaskStatus::Completed => "completed",
        TaskStatus::Archived => "archived",
    }
}

fn normalize(value: &str) -> String {
    value.trim().replace('\\', "/")
}

fn fold(value: &str) -> String {
    normalize(value).to_ascii_lowercase()
}

#[derive(Debug, Error)]
pub enum GraphError {
    #[error("impact target was not found in the project graph: {0}")]
    TargetNotFound(String),
    #[error("impact analysis currently accepts file or test nodes, not: {0}")]
    UnsupportedTarget(String),
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::{
        domain::{
            Decision, DecisionStatus, Goal, GoalStatus, PermissionMode, ProjectPermissions,
            ProjectRecord, TaskAssignmentStatus, TaskPriority,
        },
        git::ProjectStatus,
    };

    #[test]
    fn graph_connects_source_work_and_tests_then_scores_impact() {
        let repository = TempDir::new().expect("temporary repository");
        fs::create_dir_all(repository.path().join("src")).expect("source directory");
        fs::create_dir_all(repository.path().join("tests")).expect("test directory");
        fs::write(
            repository.path().join("src/storage.rs"),
            "pub fn save() {}\n",
        )
        .expect("storage source");
        fs::write(
            repository.path().join("src/context.rs"),
            "use crate::storage;\npub fn compile() { storage::save(); }\n",
        )
        .expect("dependent source");
        fs::write(
            repository.path().join("tests/storage_tests.rs"),
            "#[test]\nfn storage_round_trip() {}\n",
        )
        .expect("test source");

        let memory = fixture_memory(repository.path());
        let graph = build(
            &memory,
            &[
                "src/storage.rs".into(),
                "src/context.rs".into(),
                "tests/storage_tests.rs".into(),
            ],
        );

        assert!(has_edge(
            &graph,
            "file:src/context.rs",
            "file:src/storage.rs",
            EdgeType::DependsOn
        ));
        assert!(has_edge(
            &graph,
            "test:tests/storage_tests.rs",
            "file:src/storage.rs",
            EdgeType::Validates
        ));
        assert!(has_edge(
            &graph,
            "task:task-storage",
            "file:src/storage.rs",
            EdgeType::Touches
        ));
        assert!(has_edge(
            &graph,
            "decision:decision-storage",
            "file:src/storage.rs",
            EdgeType::Affects
        ));

        let impact = analyze(&graph, &memory, "src/storage.rs").expect("impact analysis");
        assert_eq!(impact.target.path.as_deref(), Some("src/storage.rs"));
        assert!(impact
            .dependent_files
            .iter()
            .any(|node| node.path.as_deref() == Some("src/context.rs")));
        assert!(impact
            .validating_tests
            .iter()
            .any(|node| node.path.as_deref() == Some("tests/storage_tests.rs")));
        assert_eq!(impact.affected_tasks.len(), 1);
        assert_eq!(impact.relevant_decisions.len(), 1);
        assert!(impact.risk.score >= 25);
        assert!(matches!(
            impact.risk.level,
            RiskLevel::Medium | RiskLevel::High | RiskLevel::Critical
        ));
    }

    #[test]
    fn impact_rejects_non_file_nodes_and_unknown_targets() {
        let repository = TempDir::new().expect("temporary repository");
        let memory = fixture_memory(repository.path());
        let graph = build(&memory, &["src/storage.rs".into()]);

        assert!(matches!(
            analyze(&graph, &memory, "goal:goal-storage"),
            Err(GraphError::UnsupportedTarget(_))
        ));
        assert!(matches!(
            analyze(&graph, &memory, "src/missing.rs"),
            Err(GraphError::TargetNotFound(_))
        ));
    }

    fn has_edge(graph: &ProjectGraph, source: &str, target: &str, edge_type: EdgeType) -> bool {
        graph.edges.iter().any(|edge| {
            edge.source == source && edge.target == target && edge.edge_type == edge_type
        })
    }

    fn fixture_memory(repository: &Path) -> ProjectMemory {
        let repository_path = repository.to_string_lossy().into_owned();
        let project_id = "project-graph".to_owned();
        ProjectMemory {
            project: ProjectRecord {
                id: project_id.clone(),
                path: repository_path.clone(),
                name: "graph-fixture".into(),
                branch: "main".into(),
                last_commit: "Fixture".into(),
                change_count: 1,
                last_scanned_at: "2026-09-21T10:00:00Z".into(),
                tracking_enabled: true,
                latest_status: Some(ProjectStatus {
                    repository: repository_path,
                    branch: "main".into(),
                    modified_files: vec!["src/storage.rs".into()],
                    added_files: Vec::new(),
                    deleted_files: Vec::new(),
                    last_commit: "Fixture".into(),
                    head_commit: "abc123".into(),
                    recent_commits: Vec::new(),
                    diff_stat: Vec::new(),
                    scanned_at: "2026-09-21T10:00:00Z".into(),
                }),
            },
            goals: vec![Goal {
                id: "goal-storage".into(),
                project_id: project_id.clone(),
                title: "Reliable persistence".into(),
                description: "Keep project memory durable.".into(),
                status: GoalStatus::Active,
                created_at: "2026-09-21T09:00:00Z".into(),
                updated_at: "2026-09-21T09:00:00Z".into(),
                tasks: vec![WorkTask {
                    id: "task-storage".into(),
                    goal_id: "goal-storage".into(),
                    title: "Change storage".into(),
                    description: "Update the persistence layer.".into(),
                    status: TaskStatus::InProgress,
                    priority: TaskPriority::High,
                    assigned_agent: Some("Codex".into()),
                    source_agent: Some("user".into()),
                    created_at: "2026-09-21T09:00:00Z".into(),
                    completed_at: None,
                    assignment_status: TaskAssignmentStatus::Running,
                    suggested_provider: None,
                    assigned_provider: None,
                    route_reasons: Vec::new(),
                    required_capabilities: Vec::new(),
                    blocked_by: Vec::new(),
                    depends_on: Vec::new(),
                    produces: Vec::new(),
                    consumes: Vec::new(),
                    claimed_paths: vec!["src/storage.rs".into()],
                }],
            }],
            decisions: vec![Decision {
                id: "decision-storage".into(),
                project_id: project_id.clone(),
                summary: "SQLite remains the source of truth".into(),
                reason: "Local durability is required.".into(),
                alternatives: Vec::new(),
                affected: vec!["src/storage.rs".into()],
                status: DecisionStatus::Active,
                created_by: "Claude".into(),
                superseded_by: None,
                created_at: "2026-09-21T09:15:00Z".into(),
            }],
            events: Vec::new(),
            agent_sessions: Vec::new(),
            last_agent_session: None,
            latest_handoff: None,
            handoffs: Vec::new(),
            conflicts: Vec::new(),
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
                updated_at: "2026-09-21T09:00:00Z".into(),
            },
            action_runs: Vec::new(),
        }
    }
}
