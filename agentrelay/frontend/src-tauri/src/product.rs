use std::{fs, io::Write, path::Path, process::Command};

use agentrelay_core::{
    detection::{AgentDetector, DetectedAgent},
    domain::{EnvironmentStatus, FreshnessStatus, RepositorySetupInspection, VerificationStatus},
    AgentRelayCore,
};
use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use serde_json::json;
use tauri::AppHandle;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

use crate::{background::BackgroundMonitor, detector::WindowsAgentDetector};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemToolStatus {
    pub id: String,
    pub name: String,
    pub required: bool,
    pub detected: bool,
    pub version: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingInspection {
    pub tools: Vec<SystemToolStatus>,
    pub detected_agents: Vec<DetectedAgent>,
    pub repository: Option<RepositorySetupInspection>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthItem {
    pub id: String,
    pub label: String,
    pub status: String,
    pub value: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectHealthSnapshot {
    pub repository: String,
    pub score: u8,
    pub status: String,
    pub items: Vec<HealthItem>,
    pub checked_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticExportResult {
    pub path: String,
    pub size_bytes: u64,
    pub included_files: Vec<String>,
    pub exclusions: Vec<String>,
}

pub fn choose_repository() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Add an AgentRelay repository")
        .pick_folder()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
}

pub fn onboarding_inspection(
    core: &AgentRelayCore,
    repository_path: Option<&str>,
) -> Result<OnboardingInspection, String> {
    let detector = WindowsAgentDetector::new(core.provider_detection_rules());
    let detected_agents = detector
        .detect_agents()
        .map_err(|error| error.to_string())?;
    let repository = repository_path
        .filter(|path| !path.trim().is_empty())
        .map(|path| {
            core.inspect_repository_setup(path)
                .map_err(|error| error.to_string())
        })
        .transpose()?;
    Ok(OnboardingInspection {
        tools: inspect_tools(),
        detected_agents,
        repository,
    })
}

pub fn inspect_tools() -> Vec<SystemToolStatus> {
    vec![
        tool("git", "Git", true, "git", &["--version"]),
        tool("rust", "Rust", false, "rustc", &["--version"]),
        tool("node", "Node.js", false, "node", &["--version"]),
        tool(
            "tauri",
            "Tauri CLI",
            false,
            "cargo",
            &["tauri", "--version"],
        ),
    ]
}

fn tool(id: &str, name: &str, required: bool, program: &str, args: &[&str]) -> SystemToolStatus {
    let mut command = Command::new(program);
    command.args(args);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    match command.output() {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            SystemToolStatus {
                id: id.into(),
                name: name.into(),
                required,
                detected: true,
                version: Some(if stdout.is_empty() { stderr } else { stdout }),
                detail: "Available on PATH".into(),
            }
        }
        Ok(output) => SystemToolStatus {
            id: id.into(),
            name: name.into(),
            required,
            detected: false,
            version: None,
            detail: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        },
        Err(error) => SystemToolStatus {
            id: id.into(),
            name: name.into(),
            required,
            detected: false,
            version: None,
            detail: error.to_string(),
        },
    }
}

pub fn project_health(
    core: &AgentRelayCore,
    monitor: &BackgroundMonitor,
    repository_path: &str,
) -> ProjectHealthSnapshot {
    let mut items = Vec::new();
    let repository = core.scan_project(repository_path);
    items.push(match &repository {
        Ok(status) => health(
            "repository",
            "Repository",
            "healthy",
            "Healthy",
            &format!(
                "{} · {} local change(s)",
                status.branch,
                status.change_count()
            ),
        ),
        Err(error) => health(
            "repository",
            "Repository",
            "actionRequired",
            "Needs attention",
            &error.to_string(),
        ),
    });

    if let Ok(memory) = core.project_memory(repository_path) {
        let freshness = memory
            .freshness
            .iter()
            .find(|item| item.subject_type == "context");
        items.push(match freshness {
            Some(item) if item.status == FreshnessStatus::Current => {
                health("context", "Context", "healthy", "Fresh", &item.reason)
            }
            Some(item) => health(
                "context",
                "Context",
                "warning",
                &format!("{:?}", item.status),
                &item.reason,
            ),
            None => health(
                "context",
                "Context",
                "warning",
                "Not generated",
                "Generate AGENT_CONTEXT.md to establish a freshness baseline.",
            ),
        });
        let conflicts = memory
            .conflicts
            .iter()
            .filter(|conflict| conflict.status != agentrelay_core::domain::ConflictStatus::Resolved)
            .count();
        items.push(if conflicts == 0 {
            health(
                "conflicts",
                "Conflicts",
                "healthy",
                "None",
                "No unresolved coordination conflicts.",
            )
        } else {
            health(
                "conflicts",
                "Conflicts",
                "actionRequired",
                &conflicts.to_string(),
                "Review unresolved conflicts before the next handoff.",
            )
        });
    } else {
        items.push(health(
            "context",
            "Context",
            "warning",
            "Unavailable",
            "Project memory is not initialized.",
        ));
        items.push(health(
            "conflicts",
            "Conflicts",
            "warning",
            "Unknown",
            "Project memory is not initialized.",
        ));
    }

    let background = monitor.snapshot();
    let detected = background
        .detected_agents
        .iter()
        .filter(|agent| {
            agent
                .working_directory
                .as_deref()
                .is_some_and(|path| path_is_inside(path, Path::new(repository_path)))
        })
        .count();
    items.push(health(
        "agents",
        "Agents",
        if background.paused {
            "warning"
        } else {
            "healthy"
        },
        if background.paused {
            "Tracking paused".into()
        } else {
            format!("{detected} detected")
        }
        .as_str(),
        if background.paused {
            "Resume background tracking from the tray or Settings."
        } else {
            "Configured agent processes are monitored for this repository."
        },
    ));

    items.push(match core.last_recovery_report() {
        Ok(Some(report)) if report.missing_workspaces > 0 => health(
            "recovery",
            "Recovery",
            "actionRequired",
            "Workspace missing",
            &format!(
                "{} workspace(s) were missing; affected tasks were blocked safely.",
                report.missing_workspaces
            ),
        ),
        Ok(Some(report)) if !report.is_clean() => health(
            "recovery",
            "Recovery",
            "warning",
            "Recovered",
            &format!(
                "Recovered {} session(s), {} action(s), and {} verification run(s) after restart.",
                report.interrupted_sessions,
                report.interrupted_actions,
                report.interrupted_verifications
            ),
        ),
        Ok(Some(_)) => health(
            "recovery",
            "Recovery",
            "healthy",
            "Clean",
            "Database integrity passed and no interrupted work required recovery.",
        ),
        Ok(None) => health(
            "recovery",
            "Recovery",
            "warning",
            "Not checked",
            "No startup recovery evidence is available yet.",
        ),
        Err(error) => health(
            "recovery",
            "Recovery",
            "actionRequired",
            "Check failed",
            &error.to_string(),
        ),
    });

    let verification = core
        .verification_snapshot(repository_path)
        .ok()
        .and_then(|snapshot| snapshot.latest);
    items.push(match verification {
        Some(suite) if suite.status == VerificationStatus::Passed => health(
            "verification",
            "Verification",
            "healthy",
            "Passed",
            &format!("Commit {}", short(&suite.commit)),
        ),
        Some(suite) if suite.status == VerificationStatus::Waived => health(
            "verification",
            "Verification",
            "warning",
            "Waived",
            "One or more gates were explicitly waived.",
        ),
        Some(suite) => health(
            "verification",
            "Verification",
            "actionRequired",
            &format!("{:?}", suite.status),
            "The latest verification suite is not passing.",
        ),
        None => health(
            "verification",
            "Verification",
            "warning",
            "Not run",
            "No independent verification evidence exists yet.",
        ),
    });

    items.push(match core.environment_dashboard(repository_path) {
        Ok(environment) if environment.snapshot.status == EnvironmentStatus::Reproducible => {
            health(
                "environment",
                "Environment",
                "healthy",
                "Reproducible",
                &short(&environment.snapshot.fingerprint),
            )
        }
        Ok(environment) => health(
            "environment",
            "Environment",
            "warning",
            &format!("{:?}", environment.snapshot.status),
            "Review toolchain or configuration drift.",
        ),
        Err(error) => health(
            "environment",
            "Environment",
            "warning",
            "Not checked",
            &error.to_string(),
        ),
    });

    let settings = core.application_settings().unwrap_or_default();
    let backups = core.backups().unwrap_or_default();
    let backup_current = backups.first().is_some_and(|backup| {
        chrono::DateTime::parse_from_rfc3339(&backup.created_at)
            .ok()
            .is_some_and(|created| {
                Utc::now()
                    .signed_duration_since(created.with_timezone(&Utc))
                    .num_hours()
                    <= settings.backup_interval_hours as i64
            })
    });
    items.push(if backup_current {
        health(
            "backups",
            "Backups",
            "healthy",
            "Current",
            "The SQLite memory backup is within the configured interval.",
        )
    } else {
        health(
            "backups",
            "Backups",
            "warning",
            "Due",
            "Create a current project-memory backup from Settings.",
        )
    });

    items.push(match core.delivery_snapshot(repository_path) {
        Ok(Some(snapshot)) => health(
            "github",
            "GitHub",
            "healthy",
            "Connected",
            &format!(
                "{} · {} pull request(s)",
                snapshot.repository.slug(),
                snapshot.pull_requests.len()
            ),
        ),
        _ => health(
            "github",
            "GitHub",
            "warning",
            "Not connected",
            "Connect GitHub from Integrations when delivery synchronization is needed.",
        ),
    });

    let healthy = items.iter().filter(|item| item.status == "healthy").count();
    let action_required = items.iter().any(|item| item.status == "actionRequired");
    ProjectHealthSnapshot {
        repository: repository_path.into(),
        score: ((healthy * 100) / items.len().max(1)) as u8,
        status: if action_required {
            "actionRequired"
        } else if healthy == items.len() {
            "healthy"
        } else {
            "warning"
        }
        .into(),
        items,
        checked_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
    }
}

fn health(id: &str, label: &str, status: &str, value: &str, detail: &str) -> HealthItem {
    HealthItem {
        id: id.into(),
        label: label.into(),
        status: status.into(),
        value: value.into(),
        detail: detail.into(),
    }
}

fn short(value: &str) -> String {
    value.chars().take(12).collect()
}

fn path_is_inside(candidate: &Path, repository: &Path) -> bool {
    let candidate = candidate
        .canonicalize()
        .unwrap_or_else(|_| candidate.to_path_buf());
    let repository = repository
        .canonicalize()
        .unwrap_or_else(|_| repository.to_path_buf());
    candidate.starts_with(repository)
}

pub fn export_diagnostics(
    app: &AppHandle,
    core: &AgentRelayCore,
    monitor: &BackgroundMonitor,
    repository_path: Option<&str>,
) -> Result<DiagnosticExportResult, String> {
    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let Some(path) = rfd::FileDialog::new()
        .set_title("Export AgentRelay diagnostics")
        .set_file_name(format!("agentrelay-diagnostics-{timestamp}.zip"))
        .add_filter("ZIP archive", &["zip"])
        .save_file()
    else {
        return Err("diagnostics export was cancelled".into());
    };
    let settings = core
        .application_settings()
        .map_err(|error| error.to_string())?;
    let tools = inspect_tools();
    let background = monitor.snapshot();
    let projects = core.tracked_projects().map_err(|error| error.to_string())?;
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let file = fs::File::create(&path).map_err(|error| error.to_string())?;
    let mut zip = ZipWriter::new(file);
    let mut included = Vec::new();
    let manifest = json!({
        "generatedAt": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        "application": "AgentRelay",
        "version": app.package_info().version.to_string(),
        "os": std::env::consts::OS,
        "architecture": std::env::consts::ARCH,
        "privacy": {
            "sourceCodeIncluded": false,
            "secretValuesIncluded": false,
            "databaseIncluded": false,
            "repositoryPathsIncluded": settings.diagnostics_include_paths
        }
    });
    write_zip_json(&mut zip, options, "manifest.json", &manifest, &mut included)?;
    write_zip_json(
        &mut zip,
        options,
        "system-tools.json",
        &tools,
        &mut included,
    )?;
    write_zip_json(&mut zip, options, "settings.json", &settings, &mut included)?;
    let project_summary = projects
        .iter()
        .map(|project| {
            json!({
                "name": project.name,
                "path": settings.diagnostics_include_paths.then_some(project.path.as_str()),
                "branch": project.branch,
                "changeCount": project.change_count,
                "trackingEnabled": project.tracking_enabled,
                "lastScannedAt": project.last_scanned_at,
            })
        })
        .collect::<Vec<_>>();
    write_zip_json(
        &mut zip,
        options,
        "projects.json",
        &project_summary,
        &mut included,
    )?;
    let runtime = json!({
        "backgroundRunning": background.running,
        "trackingPaused": background.paused,
        "detectedAgentCount": background.detected_agents.len(),
        "automaticSessionCount": background.automatic_session_count,
        "trackedProjectCount": background.watched_projects.len(),
        "blockedTaskCount": background.blocked_task_count,
        "idleTimeoutMinutes": background.idle_timeout_minutes,
        "lastError": redact_paths(background.last_error.as_deref(), &projects, settings.diagnostics_include_paths),
    });
    write_zip_json(&mut zip, options, "runtime.json", &runtime, &mut included)?;
    if let Ok(Some(report)) = core.last_recovery_report() {
        let mut recovery = serde_json::to_value(report).map_err(|error| error.to_string())?;
        if !settings.diagnostics_include_paths {
            redact_json_paths(&mut recovery, &projects);
        }
        write_zip_json(
            &mut zip,
            options,
            "startup-recovery.json",
            &recovery,
            &mut included,
        )?;
    }
    if let Some(repository_path) = repository_path {
        let health = project_health(core, monitor, repository_path);
        let mut health = serde_json::to_value(health).map_err(|error| error.to_string())?;
        if !settings.diagnostics_include_paths {
            redact_json_paths(&mut health, &projects);
        }
        write_zip_json(
            &mut zip,
            options,
            "project-health.json",
            &health,
            &mut included,
        )?;
        if let Ok(memory) = core.project_memory(repository_path) {
            let summary = json!({
                "projectName": memory.project.name,
                "goalCount": memory.goals.len(),
                "taskCount": memory.goals.iter().map(|goal| goal.tasks.len()).sum::<usize>(),
                "decisionCount": memory.decisions.len(),
                "sessionCount": memory.agent_sessions.len(),
                "openConflictCount": memory.conflicts.iter().filter(|item| item.status != agentrelay_core::domain::ConflictStatus::Resolved).count(),
                "actionRunCount": memory.action_runs.len(),
                "repositoryPath": settings.diagnostics_include_paths.then_some(repository_path),
            });
            write_zip_json(
                &mut zip,
                options,
                "project-summary.json",
                &summary,
                &mut included,
            )?;
        }
    }
    zip.finish().map_err(|error| error.to_string())?;
    let size_bytes = fs::metadata(&path)
        .map_err(|error| error.to_string())?
        .len();
    Ok(DiagnosticExportResult {
        path: path.to_string_lossy().replace('\\', "/"),
        size_bytes,
        included_files: included,
        exclusions: vec![
            "Repository source code".into(),
            "Environment-variable and credential values".into(),
            "SQLite project-memory database".into(),
            "Command stdout/stderr payloads".into(),
        ],
    })
}

fn write_zip_json<T: Serialize>(
    zip: &mut ZipWriter<fs::File>,
    options: SimpleFileOptions,
    name: &str,
    value: &T,
    included: &mut Vec<String>,
) -> Result<(), String> {
    zip.start_file(name, options)
        .map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    zip.write_all(&bytes).map_err(|error| error.to_string())?;
    included.push(name.into());
    Ok(())
}

fn redact_paths(
    value: Option<&str>,
    projects: &[agentrelay_core::domain::ProjectRecord],
    include_paths: bool,
) -> Option<String> {
    let mut value = value?.to_owned();
    if !include_paths {
        for project in projects {
            value = value.replace(&project.path, "<repository>");
        }
    }
    Some(value)
}

fn redact_json_paths(
    value: &mut serde_json::Value,
    projects: &[agentrelay_core::domain::ProjectRecord],
) {
    match value {
        serde_json::Value::String(text) => {
            for project in projects {
                *text = text.replace(&project.path, "<repository>");
                *text = text.replace(&project.path.replace('/', "\\"), "<repository>");
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                redact_json_paths(value, projects);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values_mut() {
                redact_json_paths(value, projects);
            }
        }
        _ => {}
    }
}

pub fn startup_enabled() -> bool {
    #[cfg(windows)]
    {
        use winreg::{enums::HKEY_CURRENT_USER, RegKey};
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
            .ok()
            .is_some_and(|key| {
                key.get_value::<String, _>("Sharman").is_ok()
                    || key.get_value::<String, _>("AgentRelay").is_ok()
            })
    }
    #[cfg(not(windows))]
    false
}

pub fn apply_startup_setting(enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use winreg::{enums::HKEY_CURRENT_USER, RegKey};
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
            .map_err(|error| error.to_string())?;
        if enabled {
            let executable = std::env::current_exe().map_err(|error| error.to_string())?;
            key.set_value(
                "Sharman",
                &format!("\"{}\" --startup", executable.display()),
            )
            .map_err(|error| error.to_string())?;
        } else {
            let _ = key.delete_value("Sharman");
            let _ = key.delete_value("AgentRelay");
        }
    }
    Ok(())
}
