use std::{path::PathBuf, time::Duration};

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantAuditRecord {
    pub id: i64,
    pub actor: String,
    pub intent: String,
    pub tool: String,
    pub permission: String,
    pub outcome: String,
    pub summary: String,
    pub occurred_at: String,
}

pub struct AssistantAuditStore {
    path: PathBuf,
}

impl AssistantAuditStore {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let store = Self { path };
        store
            .connection()?
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 CREATE TABLE IF NOT EXISTS assistant_tool_audit (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    actor TEXT NOT NULL,
                    intent TEXT NOT NULL,
                    tool TEXT NOT NULL,
                    permission TEXT NOT NULL,
                    outcome TEXT NOT NULL,
                    summary TEXT NOT NULL,
                    occurred_at TEXT NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS idx_assistant_tool_audit_time
                    ON assistant_tool_audit(occurred_at DESC);",
            )
            .map_err(|error| format!("Unable to initialize the assistant audit log: {error}"))?;
        Ok(store)
    }

    fn connection(&self) -> Result<Connection, String> {
        let connection = Connection::open(&self.path)
            .map_err(|error| format!("Unable to open the assistant audit log: {error}"))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| format!("Unable to configure the assistant audit log: {error}"))?;
        Ok(connection)
    }

    pub fn record(
        &self,
        actor: &str,
        intent: &str,
        tool: &str,
        permission: &str,
        outcome: &str,
        summary: &str,
    ) -> Result<(), String> {
        self.connection()?
            .execute(
                "INSERT INTO assistant_tool_audit
                 (actor, intent, tool, permission, outcome, summary, occurred_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    actor,
                    intent,
                    tool,
                    permission,
                    outcome,
                    bounded(summary, 500),
                    Utc::now().to_rfc3339()
                ],
            )
            .map_err(|error| format!("Unable to record the assistant audit event: {error}"))?;
        Ok(())
    }

    pub fn recent(&self, limit: usize) -> Result<Vec<AssistantAuditRecord>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, actor, intent, tool, permission, outcome, summary, occurred_at
                 FROM assistant_tool_audit ORDER BY id DESC LIMIT ?1",
            )
            .map_err(|error| format!("Unable to read the assistant audit log: {error}"))?;
        let records = statement
            .query_map([limit.clamp(1, 100) as i64], |row| {
                Ok(AssistantAuditRecord {
                    id: row.get(0)?,
                    actor: row.get(1)?,
                    intent: row.get(2)?,
                    tool: row.get(3)?,
                    permission: row.get(4)?,
                    outcome: row.get(5)?,
                    summary: row.get(6)?,
                    occurred_at: row.get(7)?,
                })
            })
            .map_err(|error| format!("Unable to read the assistant audit log: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to decode the assistant audit log: {error}"))?;
        Ok(records)
    }
}

fn bounded(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_records_are_bounded_and_newest_first() {
        let directory = tempfile::tempdir().unwrap();
        let store = AssistantAuditStore::open(directory.path().join("audit.db")).unwrap();
        store
            .record(
                "supervisor",
                "tasks",
                "tasks.today",
                "READ",
                "passed",
                "first",
            )
            .unwrap();
        store
            .record(
                "supervisor",
                "gmail",
                "gmail.search",
                "READ",
                "passed",
                &"x".repeat(900),
            )
            .unwrap();
        let records = store.recent(10).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].tool, "gmail.search");
        assert_eq!(records[0].summary.len(), 500);
    }
}
