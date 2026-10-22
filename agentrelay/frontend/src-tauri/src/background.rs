use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use agentrelay_core::{
    detection::{AgentDetector, DetectedAgent},
    domain::{EndSessionInput, EventKind, ProjectRecord},
    AgentRelayCore,
};
use notify::{Event, EventKind as NotifyEventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::detector::WindowsAgentDetector;

const PROCESS_POLL_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundStatus {
    pub running: bool,
    pub paused: bool,
    pub detected_agents: Vec<DetectedAgent>,
    pub watched_projects: Vec<String>,
    pub automatic_session_count: usize,
    pub idle_timeout_minutes: u64,
    pub blocked_task_count: usize,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct BackgroundMonitor {
    status: Mutex<BackgroundStatus>,
}

impl BackgroundMonitor {
    pub fn snapshot(&self) -> BackgroundStatus {
        self.status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_default()
    }

    fn update(&self, status: BackgroundStatus) {
        if let Ok(mut current) = self.status.lock() {
            *current = status;
        }
    }
}

struct AutomaticSession {
    session_id: String,
    repository_path: String,
    last_activity: Instant,
}

pub fn start(core: Arc<AgentRelayCore>, app: AppHandle, monitor: Arc<BackgroundMonitor>) {
    thread::Builder::new()
        .name("agentrelay-background".into())
        .spawn(move || run(core, app, monitor))
        .expect("unable to start AgentRelay background monitor");
}

fn run(core: Arc<AgentRelayCore>, app: AppHandle, monitor: Arc<BackgroundMonitor>) {
    let (watch_tx, watch_rx) = mpsc::channel();
    let mut watcher = match notify::recommended_watcher(move |event| {
        let _ = watch_tx.send(event);
    }) {
        Ok(watcher) => watcher,
        Err(error) => {
            monitor.update(BackgroundStatus {
                last_error: Some(format!("repository watcher failed to start: {error}")),
                ..BackgroundStatus::default()
            });
            return;
        }
    };
    let detector = WindowsAgentDetector::new(core.provider_detection_rules());
    let mut watched = HashSet::<PathBuf>::new();
    let mut sessions = HashMap::<(u32, String), AutomaticSession>::new();
    let mut idle_suppressed = HashSet::<(u32, String)>::new();
    let mut seen_events = HashSet::<String>::new();
    let mut initialized_projects = HashSet::<String>::new();

    loop {
        let mut status = BackgroundStatus {
            running: true,
            ..BackgroundStatus::default()
        };
        let settings = core.application_settings().unwrap_or_default();
        status.paused = settings.tracking_paused;
        let projects = match core.tracked_projects() {
            Ok(projects) => projects,
            Err(error) => {
                status.last_error = Some(error.to_string());
                monitor.update(status);
                thread::sleep(PROCESS_POLL_INTERVAL);
                continue;
            }
        };
        status.blocked_task_count = projects
            .iter()
            .filter_map(|project| core.project_memory(&project.path).ok())
            .map(|memory| {
                memory
                    .goals
                    .iter()
                    .flat_map(|goal| &goal.tasks)
                    .filter(|task| {
                        task.assignment_status
                            == agentrelay_core::domain::TaskAssignmentStatus::Blocked
                    })
                    .count()
            })
            .sum();
        if settings.tracking_paused {
            sync_watches(&mut watcher, &mut watched, &[]);
            status.watched_projects = projects
                .iter()
                .map(|project| project.path.clone())
                .collect();
            status.idle_timeout_minutes = core.idle_timeout_minutes().unwrap_or(15);
            monitor.update(status.clone());
            crate::update_tray_menu(&app, &status);
            thread::sleep(PROCESS_POLL_INTERVAL);
            continue;
        }
        sync_watches(&mut watcher, &mut watched, &projects);
        status.watched_projects = projects
            .iter()
            .map(|project| project.path.clone())
            .collect();
        status.idle_timeout_minutes = core.idle_timeout_minutes().unwrap_or(15);

        let dirty_projects = drain_file_activity(&watch_rx, &projects);
        for repository_path in &dirty_projects {
            let _ = core.scan_project(repository_path);
            sessions
                .values_mut()
                .filter(|session| same_path(&session.repository_path, repository_path))
                .for_each(|session| session.last_activity = Instant::now());
            idle_suppressed.retain(|(_, path)| !same_path(path, repository_path));
        }

        let detected_agents = match detector.detect_agents() {
            Ok(agents) => agents,
            Err(error) => {
                status.last_error = Some(error.to_string());
                Vec::new()
            }
        };
        status.detected_agents = detected_agents.clone();
        adopt_persisted_sessions(&core, &projects, &mut sessions);

        let mut matched = HashSet::<(u32, String)>::new();
        let mut selected_groups = HashSet::new();
        for agent in &detected_agents {
            if !settings.selected_agents.contains(&agent.provider) {
                continue;
            }
            let Some(project) = projects
                .iter()
                .find(|project| agent_matches_project(agent, &project.path))
            else {
                continue;
            };
            if !selected_groups.insert((agent.provider.clone(), project.path.clone())) {
                continue;
            }
            let key = (agent.pid, project.path.clone());
            matched.insert(key.clone());
            if sessions.contains_key(&key) || idle_suppressed.contains(&key) {
                continue;
            }
            let active = core.active_agent_session(&project.path).ok().flatten();
            if active.is_some() {
                continue;
            }
            match core.start_detected_agent_session(project.path.clone(), agent) {
                Ok(session) => {
                    if settings.notify_agent_sessions {
                        send_notification(
                            &app,
                            "Agent session detected",
                            &format!(
                                "{} started working in {}.",
                                agent.provider.display_name(),
                                project.name
                            ),
                        );
                    }
                    sessions.insert(
                        key,
                        AutomaticSession {
                            session_id: session.id,
                            repository_path: project.path.clone(),
                            last_activity: Instant::now(),
                        },
                    );
                }
                Err(error) => status.last_error = Some(error.to_string()),
            }
        }

        let live_pids = detected_agents
            .iter()
            .map(|agent| agent.pid)
            .collect::<HashSet<_>>();
        idle_suppressed.retain(|(pid, _)| live_pids.contains(pid));
        let idle_timeout = Duration::from_secs(status.idle_timeout_minutes.saturating_mul(60));
        let finished = sessions
            .iter()
            .filter_map(|(key, session)| {
                if !matched.contains(key) {
                    Some((key.clone(), "agent process exited"))
                } else if session.last_activity.elapsed() >= idle_timeout {
                    Some((key.clone(), "idle timeout reached"))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for (key, reason) in finished {
            if let Some(session) = sessions.remove(&key) {
                match core.end_agent_session(EndSessionInput {
                    session_id: session.session_id,
                    summary_status: format!("Automatic session ended: {reason}"),
                    blockers: Vec::new(),
                }) {
                    Ok(_) if settings.notify_handoffs => send_notification(
                        &app,
                        "Handoff ready",
                        &format!(
                            "Fresh context was generated for {}.",
                            session.repository_path
                        ),
                    ),
                    Ok(_) => {}
                    Err(error) => status.last_error = Some(error.to_string()),
                }
                if reason == "idle timeout reached" {
                    idle_suppressed.insert(key);
                }
            }
        }

        emit_memory_notifications(
            &core,
            &app,
            &projects,
            &mut initialized_projects,
            &mut seen_events,
        );
        status.automatic_session_count = sessions.len();
        monitor.update(status.clone());
        crate::update_tray_menu(&app, &status);
        thread::sleep(PROCESS_POLL_INTERVAL);
    }
}

fn sync_watches(
    watcher: &mut RecommendedWatcher,
    watched: &mut HashSet<PathBuf>,
    projects: &[ProjectRecord],
) {
    let desired = projects
        .iter()
        .map(|project| PathBuf::from(&project.path))
        .collect::<HashSet<_>>();
    for removed in watched.difference(&desired).cloned().collect::<Vec<_>>() {
        let _ = watcher.unwatch(&removed);
        watched.remove(&removed);
    }
    for added in desired.difference(watched).cloned().collect::<Vec<_>>() {
        if watcher.watch(&added, RecursiveMode::Recursive).is_ok() {
            watched.insert(added);
        }
    }
}

fn drain_file_activity(
    receiver: &mpsc::Receiver<notify::Result<Event>>,
    projects: &[ProjectRecord],
) -> HashSet<String> {
    let mut dirty = HashSet::new();
    while let Ok(event) = receiver.try_recv() {
        let Ok(event) = event else { continue };
        if matches!(event.kind, NotifyEventKind::Access(_)) {
            continue;
        }
        for path in event.paths.iter().filter(|path| is_relevant_path(path)) {
            for project in projects {
                if path_is_inside(path, Path::new(&project.path)) {
                    dirty.insert(project.path.clone());
                }
            }
        }
    }
    dirty
}

fn is_relevant_path(path: &Path) -> bool {
    if path
        .file_name()
        .is_some_and(|name| name == "AGENT_CONTEXT.md")
    {
        return false;
    }
    !path.components().any(|component| {
        matches!(
            component
                .as_os_str()
                .to_string_lossy()
                .to_lowercase()
                .as_str(),
            ".git" | "target" | "node_modules" | "dist"
        )
    })
}

fn adopt_persisted_sessions(
    core: &AgentRelayCore,
    projects: &[ProjectRecord],
    sessions: &mut HashMap<(u32, String), AutomaticSession>,
) {
    for project in projects {
        let Ok(Some(session)) = core.active_agent_session(&project.path) else {
            continue;
        };
        let Some(pid) = session.detected_pid.filter(|_| session.automatic) else {
            continue;
        };
        sessions
            .entry((pid, project.path.clone()))
            .or_insert_with(|| AutomaticSession {
                session_id: session.id,
                repository_path: project.path.clone(),
                last_activity: Instant::now(),
            });
    }
}

fn emit_memory_notifications(
    core: &AgentRelayCore,
    app: &AppHandle,
    projects: &[ProjectRecord],
    initialized_projects: &mut HashSet<String>,
    seen_events: &mut HashSet<String>,
) {
    let settings = core.application_settings().unwrap_or_default();
    for project in projects {
        let Ok(memory) = core.project_memory(&project.path) else {
            continue;
        };
        if initialized_projects.insert(project.id.clone()) {
            seen_events.extend(memory.events.iter().map(|event| event.id.clone()));
            continue;
        }
        for event in memory.events.iter().rev() {
            if !seen_events.insert(event.id.clone()) {
                continue;
            }
            match event.kind {
                EventKind::ConflictDetected if settings.notify_conflicts => send_notification(
                    app,
                    "Conflict detected",
                    event
                        .payload
                        .get("title")
                        .and_then(|value| value.as_str())
                        .unwrap_or("AgentRelay found a project conflict."),
                ),
                EventKind::FreshnessChanged
                    if event
                        .payload
                        .get("subjectType")
                        .and_then(|value| value.as_str())
                        == Some("context")
                        && settings.notify_conflicts
                        && matches!(
                            event.payload.get("status").and_then(|value| value.as_str()),
                            Some("stale" | "invalidated")
                        ) =>
                {
                    send_notification(
                        app,
                        "Project context became stale",
                        &format!("Regenerate AGENT_CONTEXT.md for {}.", project.name),
                    );
                }
                EventKind::BuildFailed
                | EventKind::TestFailed
                | EventKind::LintFailed
                | EventKind::SecurityCheckFailed
                | EventKind::VerificationFailed
                | EventKind::ValidationFailed
                    if settings.notify_blockers =>
                {
                    let kind = format!("{:?}", event.kind).to_lowercase();
                    send_notification(
                        app,
                        "Task needs attention",
                        &format!(
                            "{} reported {kind}. Open AgentRelay to review the blocker.",
                            project.name
                        ),
                    );
                }
                _ => {}
            }
        }
    }
}

fn agent_matches_project(agent: &DetectedAgent, repository_path: &str) -> bool {
    let repository = normalize(repository_path);
    agent
        .working_directory
        .as_ref()
        .is_some_and(|cwd| normalized_is_inside(&normalize(&cwd.to_string_lossy()), &repository))
        || agent
            .command_line
            .iter()
            .any(|part| normalize(part).contains(&repository))
}

fn path_is_inside(path: &Path, parent: &Path) -> bool {
    normalized_is_inside(
        &normalize(&path.to_string_lossy()),
        &normalize(&parent.to_string_lossy()),
    )
}

fn same_path(left: &str, right: &str) -> bool {
    normalize(left) == normalize(right)
}

fn normalize(path: &str) -> String {
    path.trim_matches('"')
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase()
}

fn normalized_is_inside(candidate: &str, parent: &str) -> bool {
    candidate == parent
        || candidate
            .strip_prefix(parent)
            .is_some_and(|remainder| remainder.starts_with('/'))
}

pub fn send_notification(app: &AppHandle, title: &str, body: &str) {
    if app
        .try_state::<crate::AppState>()
        .and_then(|state| state.core.application_settings().ok())
        .is_some_and(|settings| !settings.notifications_enabled)
    {
        return;
    }
    let _ = app.notification().builder().title(title).body(body).show();
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentrelay_core::domain::ProviderId;

    #[test]
    fn matches_working_directories_and_command_arguments_to_projects() {
        let mut agent = DetectedAgent {
            provider: ProviderId::codex(),
            pid: 42,
            executable: "codex.exe".into(),
            working_directory: Some(PathBuf::from("C:\\work\\demo\\src")),
            command_line: Vec::new(),
        };
        assert!(agent_matches_project(&agent, "C:\\work\\demo"));
        assert!(!agent_matches_project(&agent, "C:\\work\\other"));
        assert!(!agent_matches_project(&agent, "C:\\work\\dem"));
        agent.working_directory = None;
        agent.command_line = vec!["--cwd=C:\\work\\demo".into()];
        assert!(agent_matches_project(&agent, "C:\\work\\demo"));
    }

    #[test]
    fn ignores_generated_and_build_output_paths() {
        assert!(!is_relevant_path(Path::new("C:\\repo\\.git\\index")));
        assert!(!is_relevant_path(Path::new("C:\\repo\\AGENT_CONTEXT.md")));
        assert!(is_relevant_path(Path::new("C:\\repo\\src\\lib.rs")));
    }
}
