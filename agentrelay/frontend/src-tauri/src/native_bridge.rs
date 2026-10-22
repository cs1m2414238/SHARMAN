use std::{
    env, fs,
    io::{self, Read, Write},
    path::PathBuf,
};

use chrono::{DateTime, Local};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    audit::AssistantAuditStore,
    gmail::{GmailSearchMode, GmailService},
    personal::PersonalStore,
};

const BRIDGE_PROTOCOL: &str = "SHARMAN_PERSONAL_BRIDGE_V2";

#[derive(Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Snapshot {},
    GmailStatus {},
    GmailMessages {
        unread: bool,
    },
    GmailActions {
        refresh: bool,
        mode: GmailSearchMode,
    },
    CreateTask {
        title: String,
        #[serde(rename = "dueAt")]
        due_at: Option<String>,
    },
    CreateReminder {
        title: String,
        #[serde(rename = "remindAt")]
        remind_at: String,
    },
}

pub fn run_if_requested() -> bool {
    if env::args().any(|arg| arg == "--personal-bridge-protocol") {
        let _ = writeln!(io::stdout().lock(), "{BRIDGE_PROTOCOL}");
        return true;
    }
    if !env::args().any(|arg| arg == "--personal-bridge") {
        return false;
    }
    let result = (|| {
        let mut input = String::new();
        io::stdin()
            .take(16_385)
            .read_to_string(&mut input)
            .map_err(|error| error.to_string())?;
        if input.len() > 16_384 {
            return Err("Native tool request exceeds its size limit.".into());
        }
        let request: Request = serde_json::from_str(&input)
            .map_err(|_| "Unsupported or invalid native personal tool request.".to_owned())?;
        let directory = env::var_os("SHARMAN_NATIVE_DATA_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("APPDATA")
                    .map(|path| PathBuf::from(path).join("com.agentrelay.desktop"))
            })
            .ok_or_else(|| "Unable to locate native SHARMAN application data.".to_owned())?;
        fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        execute(request, directory.join("sharman-memory.db"))
    })();
    let output = match result {
        Ok(value) => json!({ "ok": true, "result": value }),
        Err(error) => json!({ "ok": false, "error": error }),
    };
    let _ = writeln!(io::stdout().lock(), "{output}");
    true
}

fn execute(request: Request, database: PathBuf) -> Result<Value, String> {
    match &request {
        Request::GmailStatus {} => {
            let status = GmailService::open(database)?.status()?;
            return Ok(
                json!({ "connected": status.connected, "needsReconnect": status.sync.last_error.as_deref().is_some_and(|error| error.contains("authorization needs reconnecting")), "source": "native_gmail" }),
            );
        }
        Request::GmailMessages { unread } => {
            let gmail = GmailService::open(database)?;
            let result = if *unread {
                gmail.list_unread()?
            } else {
                gmail.list_recent()?
            };
            return serde_json::to_value(result).map_err(|error| error.to_string());
        }
        Request::GmailActions { refresh, mode } => {
            let gmail = GmailService::open(database)?;
            if !gmail.status()?.connected {
                return Err(
                    "Gmail is not connected. Connect Gmail in the desktop Action Inbox.".into(),
                );
            }
            let dashboard = if *refresh {
                gmail.refresh(*mode)?
            } else {
                gmail.dashboard()?
            };
            return Ok(json!({ "items": dashboard.needs_action, "scan": dashboard.scan }));
        }
        _ => {}
    }
    let store = PersonalStore::open(database.clone())?;
    match request {
        Request::Snapshot {} => {
            serde_json::to_value(store.bridge_snapshot()?).map_err(|error| error.to_string())
        }
        Request::CreateTask { title, due_at } => {
            let due = due_at.as_deref().map(parse_time).transpose()?;
            let (task, _, created) = store.create_bridge_item(&title, due, false)?;
            AssistantAuditStore::open(database)?.record(
                "alexa+",
                "mcp-task",
                "tasks.create",
                "LOW_RISK",
                "completed",
                "Native task persisted through MCP",
            )?;
            Ok(json!({ "task": task, "created": created, "storage": "native_sqlite" }))
        }
        Request::CreateReminder { title, remind_at } => {
            let (task, reminder, created) =
                store.create_bridge_item(&title, Some(parse_time(&remind_at)?), true)?;
            AssistantAuditStore::open(database)?.record(
                "alexa+",
                "mcp-reminder",
                "reminders.create",
                "LOW_RISK",
                "completed",
                "Native reminder persisted for the Windows scheduler",
            )?;
            Ok(
                json!({ "task": task, "reminder": reminder, "created": created, "storage": "native_sqlite" }),
            )
        }
        Request::GmailStatus {} | Request::GmailMessages { .. } | Request::GmailActions { .. } => {
            unreachable!()
        }
    }
}

fn parse_time(value: &str) -> Result<DateTime<Local>, String> {
    DateTime::parse_from_rfc3339(value)
        .map(|date| date.with_timezone(&Local))
        .map_err(|_| "A valid ISO timestamp is required.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_bridge_rejects_arbitrary_tools_and_extra_arguments() {
        assert!(
            serde_json::from_str::<Request>(r#"{"tool":"system.exec","command":"whoami"}"#)
                .is_err()
        );
        assert!(
            serde_json::from_str::<Request>(r#"{"tool":"snapshot","path":"other.db"}"#).is_err()
        );
    }
}
