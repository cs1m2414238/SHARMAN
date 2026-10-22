mod assistant;
mod assistant_tools;
mod audit;
mod background;
mod commands;
mod detector;
mod gmail;
pub mod household;
mod native_bridge;
mod personal;
mod product;
mod supervisor;

use std::{fs, io, sync::Arc, time::Duration};

use agentrelay_core::AgentRelayCore;
use background::{BackgroundMonitor, BackgroundStatus};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

pub struct AppState {
    core: Arc<AgentRelayCore>,
    background: Arc<BackgroundMonitor>,
    assistant_memory: Arc<assistant::AssistantMemoryStore>,
    assistant_audit: Arc<audit::AssistantAuditStore>,
    gmail: Arc<gmail::GmailService>,
    personal: Arc<personal::PersonalStore>,
    household: Arc<household::HouseholdStore>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if native_bridge::run_if_requested() {
        return;
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !args.iter().any(|argument| argument == "--startup") {
                show_main_window(app);
            }
        }))
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let data_directory = std::env::var_os("SHARMAN_NATIVE_DATA_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or(app.path().app_data_dir()?);
            fs::create_dir_all(&data_directory)?;
            let core = Arc::new(
                AgentRelayCore::open(
                    data_directory.join("agentrelay.db"),
                    Duration::from_secs(10),
                )
                .map_err(|error| io::Error::other(error.to_string()))?,
            );
            let background = Arc::new(BackgroundMonitor::default());
            let assistant_memory = Arc::new(
                assistant::AssistantMemoryStore::open(data_directory.join("sharman-memory.db"))
                    .map_err(io::Error::other)?,
            );
            let personal = Arc::new(
                personal::PersonalStore::open(data_directory.join("sharman-memory.db"))
                    .map_err(io::Error::other)?,
            );
            let assistant_audit = Arc::new(
                audit::AssistantAuditStore::open(data_directory.join("sharman-memory.db"))
                    .map_err(io::Error::other)?,
            );
            let gmail = Arc::new(
                gmail::GmailService::open(data_directory.join("sharman-memory.db"))
                    .map_err(io::Error::other)?,
            );
            let household = Arc::new(
                household::HouseholdStore::open(data_directory.join("sharman-memory.db"))
                    .map_err(io::Error::other)?,
            );
            app.manage(AppState {
                core: Arc::clone(&core),
                background: Arc::clone(&background),
                assistant_memory,
                assistant_audit,
                gmail,
                personal: Arc::clone(&personal),
                household: Arc::clone(&household),
            });

            let mut tray = TrayIconBuilder::with_id("agentrelay-tray")
                .tooltip("Sharman local assistant")
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main_window(app),
                    "pause_tracking" | "resume_tracking" => {
                        let state = app.state::<AppState>();
                        if let Ok(mut settings) = state.core.application_settings() {
                            settings.tracking_paused = event.id.as_ref() == "pause_tracking";
                            let _ = state.core.save_application_settings(settings);
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        }
                    ) {
                        show_main_window(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            update_tray_menu(app.handle(), &BackgroundStatus::default());
            personal::start_scheduler(personal, app.handle().clone());
            background::start(core, app.handle().clone(), background);
            if std::env::args().any(|argument| argument == "--startup") {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    let close_to_tray = window
                        .app_handle()
                        .try_state::<AppState>()
                        .and_then(|state| state.core.application_settings().ok())
                        .is_none_or(|settings| settings.close_to_tray);
                    if close_to_tray {
                        api.prevent_close();
                        let _ = window.hide();
                    } else {
                        window.app_handle().exit(0);
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_assistant_home,
            commands::get_assistant_memory,
            commands::create_assistant_conversation,
            commands::forget_assistant_memory,
            commands::get_personal_dashboard,
            commands::set_personal_task_completed,
            commands::dismiss_personal_reminder,
            commands::get_inbox_dashboard,
            commands::connect_gmail,
            commands::disconnect_gmail,
            commands::refresh_gmail_inbox,
            commands::dismiss_email_action,
            commands::create_email_reminder,
            commands::open_gmail_message,
            commands::get_assistant_audit,
            commands::ask_sharman,
            commands::execute_assistant_action,
            commands::install_local_ai_runtime,
            commands::pull_recommended_local_model,
            commands::scan_project,
            commands::get_last_repository,
            commands::get_tracked_projects,
            commands::get_application_settings,
            commands::get_startup_recovery_report,
            commands::update_application_settings,
            commands::choose_repository,
            commands::inspect_onboarding,
            commands::complete_project_onboarding,
            commands::get_project_health,
            commands::list_backups,
            commands::create_backup,
            commands::export_diagnostics,
            commands::get_project_memory,
            commands::get_audit_trail,
            commands::get_timeline,
            commands::replay_project,
            commands::compare_replay_points,
            commands::explain_history,
            commands::get_project_permissions,
            commands::update_project_permissions,
            commands::get_sandbox_status,
            commands::get_integration_registry,
            commands::get_project_graph,
            commands::analyze_impact,
            commands::get_environment_dashboard,
            commands::save_environment_manifest,
            commands::get_verification_snapshot,
            commands::run_workspace_verification,
            commands::waive_verification_gate,
            commands::get_delivery_snapshot,
            commands::sync_github_delivery,
            commands::ingest_external_event,
            commands::request_plugin_action,
            commands::request_action,
            commands::create_goal,
            commands::update_goal,
            commands::create_task,
            commands::update_task,
            commands::update_task_orchestration,
            commands::get_orchestration_snapshot,
            commands::get_planner_snapshot,
            commands::generate_plan,
            commands::approve_plan,
            commands::replan,
            commands::update_capability_profile,
            commands::suggest_task_route,
            commands::approve_task_assignment,
            commands::clear_task_assignment,
            commands::start_routed_task,
            commands::get_collaborative_execution_snapshot,
            commands::prepare_task_workspace,
            commands::start_task_workspace,
            commands::validate_task_workspace,
            commands::prepare_workspace_merge,
            commands::approve_workspace_merge,
            commands::cleanup_task_workspace,
            commands::record_decision,
            commands::get_context_preview,
            commands::generate_context,
            commands::compile_context,
            commands::generate_smart_context,
            commands::start_agent_session,
            commands::end_agent_session,
            commands::get_active_session,
            commands::get_latest_handoff,
            commands::mark_session_idle,
            commands::evaluate_conflicts,
            commands::acknowledge_conflict,
            commands::resolve_conflict,
            commands::set_project_tracking,
            commands::get_idle_timeout_minutes,
            commands::set_idle_timeout_minutes,
            commands::get_background_status,
            commands::open_context_file,
            commands::get_household_overview,
            commands::ingest_household_event,
            commands::list_household_events,
            commands::reconstruct_household_state
        ])
        .run(tauri::generate_context!())
        .expect("error while running Sharman");
}

pub(crate) fn update_tray_menu(app: &tauri::AppHandle, status: &BackgroundStatus) {
    let Some(tray) = app.tray_by_id("agentrelay-tray") else {
        return;
    };
    let active = MenuItem::with_id(
        app,
        "active-agents-status",
        format!("{} active agent(s)", status.automatic_session_count),
        false,
        None::<&str>,
    );
    let blocked = MenuItem::with_id(
        app,
        "blocked-tasks-status",
        format!("{} blocked task(s)", status.blocked_task_count),
        false,
        None::<&str>,
    );
    let projects = MenuItem::with_id(
        app,
        "tracked-projects-status",
        format!("{} tracked project(s)", status.watched_projects.len()),
        false,
        None::<&str>,
    );
    let show = MenuItem::with_id(app, "show", "Open Sharman", true, None::<&str>);
    let tracking = MenuItem::with_id(
        app,
        if status.paused {
            "resume_tracking"
        } else {
            "pause_tracking"
        },
        if status.paused {
            "Resume Tracking"
        } else {
            "Pause Tracking"
        },
        true,
        None::<&str>,
    );
    let quit = MenuItem::with_id(app, "quit", "Exit", true, None::<&str>);
    let separator_one = PredefinedMenuItem::separator(app);
    let separator_two = PredefinedMenuItem::separator(app);
    let (
        Ok(active),
        Ok(blocked),
        Ok(projects),
        Ok(show),
        Ok(tracking),
        Ok(quit),
        Ok(separator_one),
        Ok(separator_two),
    ) = (
        active,
        blocked,
        projects,
        show,
        tracking,
        quit,
        separator_one,
        separator_two,
    )
    else {
        return;
    };
    let Ok(menu) = Menu::with_items(
        app,
        &[
            &active,
            &blocked,
            &projects,
            &separator_one,
            &show,
            &tracking,
            &separator_two,
            &quit,
        ],
    ) else {
        return;
    };
    let _ = tray.set_tooltip(Some(if status.paused {
        "Sharman project tracking paused".to_owned()
    } else {
        format!(
            "Sharman · {} active · {} blocked · {} projects",
            status.automatic_session_count,
            status.blocked_task_count,
            status.watched_projects.len()
        )
    }));
    let _ = tray.set_menu(Some(menu));
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
