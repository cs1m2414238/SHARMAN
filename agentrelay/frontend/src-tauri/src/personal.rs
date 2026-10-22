use std::{path::PathBuf, sync::Arc, thread, time::Duration};

use chrono::{DateTime, Local, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::AppHandle;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalTask {
    pub id: String,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub due_at: Option<String>,
    pub source: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledReminder {
    pub id: String,
    pub task_id: Option<String>,
    pub title: String,
    pub remind_at: String,
    pub status: String,
    pub created_at: String,
    pub fired_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalDashboard {
    pub tasks: Vec<PersonalTask>,
    pub reminders: Vec<ScheduledReminder>,
    pub generated_at: String,
}

pub struct PersonalStore {
    path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskView {
    Open,
    Completed,
    Overdue,
    All,
}

impl TaskView {
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Completed => "completed",
            Self::Overdue => "overdue",
            Self::All => "saved",
        }
    }
}

fn normalized_title(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

impl PersonalStore {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let store = Self { path };
        store
            .connection()?
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 CREATE TABLE IF NOT EXISTS personal_tasks (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    status TEXT NOT NULL,
                    priority TEXT NOT NULL,
                    due_at TEXT,
                    source TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS idx_personal_tasks_status_due
                    ON personal_tasks(status, due_at);
                 CREATE TABLE IF NOT EXISTS scheduled_reminders (
                    id TEXT PRIMARY KEY,
                    task_id TEXT,
                    title TEXT NOT NULL,
                    remind_at TEXT NOT NULL,
                    status TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    fired_at TEXT,
                    FOREIGN KEY(task_id) REFERENCES personal_tasks(id) ON DELETE SET NULL
                 );
                 CREATE INDEX IF NOT EXISTS idx_scheduled_reminders_due
                    ON scheduled_reminders(status, remind_at);",
            )
            .map_err(|error| format!("Unable to initialize personal tasks: {error}"))?;
        Ok(store)
    }

    fn connection(&self) -> Result<Connection, String> {
        let connection = Connection::open(&self.path)
            .map_err(|error| format!("Unable to open personal task storage: {error}"))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| format!("Unable to configure personal task storage: {error}"))?;
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(|error| format!("Unable to enable personal task integrity: {error}"))?;
        Ok(connection)
    }

    pub fn create_task(
        &self,
        title: &str,
        due_at: Option<DateTime<Local>>,
        source: &str,
    ) -> Result<PersonalTask, String> {
        let id = local_id("task");
        let now = Utc::now().to_rfc3339();
        let due = due_at.map(|value| value.to_rfc3339());
        self.connection()?
            .execute(
                "INSERT INTO personal_tasks
                 (id, title, status, priority, due_at, source, created_at, updated_at)
                 VALUES (?1, ?2, 'todo', 'normal', ?3, ?4, ?5, ?5)",
                params![id, title, due, source, now],
            )
            .map_err(|error| format!("Unable to create the personal task: {error}"))?;
        Ok(PersonalTask {
            id,
            title: title.to_owned(),
            status: "todo".to_owned(),
            priority: "normal".to_owned(),
            due_at: due,
            source: source.to_owned(),
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn create_reminder_task(
        &self,
        title: &str,
        remind_at: DateTime<Local>,
    ) -> Result<(PersonalTask, ScheduledReminder), String> {
        let task = self.create_task(title, Some(remind_at), "reminder")?;
        let reminder = ScheduledReminder {
            id: local_id("reminder"),
            task_id: Some(task.id.clone()),
            title: title.to_owned(),
            remind_at: remind_at.with_timezone(&Utc).to_rfc3339(),
            status: "pending".to_owned(),
            created_at: Utc::now().to_rfc3339(),
            fired_at: None,
        };
        if let Err(error) = self.connection()?.execute(
            "INSERT INTO scheduled_reminders
             (id, task_id, title, remind_at, status, created_at, fired_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
            params![
                reminder.id,
                reminder.task_id,
                reminder.title,
                reminder.remind_at,
                reminder.status,
                reminder.created_at
            ],
        ) {
            let _ = self
                .connection()?
                .execute("DELETE FROM personal_tasks WHERE id = ?1", [&task.id]);
            return Err(format!("Unable to schedule the reminder: {error}"));
        }
        Ok((task, reminder))
    }

    pub fn create_bridge_item(
        &self,
        title: &str,
        due_at: Option<DateTime<Local>>,
        reminder: bool,
    ) -> Result<(PersonalTask, Option<ScheduledReminder>, bool), String> {
        let title = title.trim();
        if !(2..=240).contains(&title.chars().count()) || title.chars().any(char::is_control) {
            return Err("A title between 2 and 240 characters is required.".into());
        }
        if reminder && due_at.is_none() {
            return Err("A reminder time is required.".into());
        }
        let due = due_at.map(|date| date.with_timezone(&Utc).to_rfc3339());
        let identity = format!(
            "{:x}",
            Sha256::digest(format!(
                "{reminder}|{}|{}",
                title.to_lowercase(),
                due.as_deref().unwrap_or("")
            ))
        );
        let task_id = format!("mcp-task-{identity}");
        let reminder_id = format!("mcp-reminder-{identity}");
        let now = Utc::now().to_rfc3339();
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|error| format!("Unable to begin the native tool transaction: {error}"))?;
        let created = transaction.execute(
            "INSERT OR IGNORE INTO personal_tasks (id, title, status, priority, due_at, source, created_at, updated_at)
             VALUES (?1, ?2, 'todo', 'normal', ?3, 'alexa-mcp', ?4, ?4)",
            params![task_id, title, due, now],
        ).map_err(|error| format!("Unable to save the native task: {error}"))? > 0;
        if reminder {
            transaction.execute(
                "INSERT OR IGNORE INTO scheduled_reminders (id, task_id, title, remind_at, status, created_at, fired_at)
                 VALUES (?1, ?2, ?3, ?4, 'pending', ?5, NULL)",
                params![reminder_id, task_id, title, due, now],
            ).map_err(|error| format!("Unable to save the native reminder: {error}"))?;
        }
        let task = transaction.query_row(
            "SELECT id, title, status, priority, due_at, source, created_at, updated_at FROM personal_tasks WHERE id = ?1",
            [&task_id], task_from_row,
        ).map_err(|error| format!("Unable to read the native task: {error}"))?;
        let saved_reminder = if reminder {
            Some(transaction.query_row(
                "SELECT id, task_id, title, remind_at, status, created_at, fired_at FROM scheduled_reminders WHERE id = ?1",
                [&reminder_id], reminder_from_row,
            ).map_err(|error| format!("Unable to read the native reminder: {error}"))?)
        } else {
            None
        };
        transaction
            .commit()
            .map_err(|error| format!("Unable to commit the native tool transaction: {error}"))?;
        Ok((task, saved_reminder, created))
    }

    pub fn bridge_snapshot(&self) -> Result<PersonalDashboard, String> {
        let connection = self.connection()?;
        let mut tasks = connection.prepare(
            "SELECT id, title, status, priority, due_at, source, created_at, updated_at FROM personal_tasks
             ORDER BY updated_at DESC LIMIT 200",
        ).map_err(|error| error.to_string())?;
        let mut reminders = connection.prepare(
            "SELECT id, task_id, title, remind_at, status, created_at, fired_at FROM scheduled_reminders
             ORDER BY created_at DESC LIMIT 200",
        ).map_err(|error| error.to_string())?;
        let task_rows = tasks
            .query_map([], task_from_row)
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        let reminder_rows = reminders
            .query_map([], reminder_from_row)
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        Ok(PersonalDashboard {
            tasks: task_rows,
            reminders: reminder_rows,
            generated_at: Utc::now().to_rfc3339(),
        })
    }

    pub fn dashboard(&self, now: DateTime<Local>) -> Result<PersonalDashboard, String> {
        let connection = self.connection()?;
        let mut task_statement = connection
            .prepare(
                "SELECT id, title, status, priority, due_at, source, created_at, updated_at
                 FROM personal_tasks WHERE status = 'todo'
                 ORDER BY due_at IS NULL, due_at, created_at",
            )
            .map_err(|error| format!("Unable to read personal tasks: {error}"))?;
        let tasks = task_statement
            .query_map([], task_from_row)
            .map_err(|error| format!("Unable to read personal tasks: {error}"))?
            .filter_map(Result::ok)
            .filter(|task| task_is_relevant_today(task, now))
            .collect::<Vec<_>>();

        let mut reminder_statement = connection
            .prepare(
                "SELECT id, task_id, title, remind_at, status, created_at, fired_at
                 FROM scheduled_reminders WHERE status = 'pending'
                 ORDER BY remind_at LIMIT 30",
            )
            .map_err(|error| format!("Unable to read reminders: {error}"))?;
        let reminders = reminder_statement
            .query_map([], reminder_from_row)
            .map_err(|error| format!("Unable to read reminders: {error}"))?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        Ok(PersonalDashboard {
            tasks,
            reminders,
            generated_at: now.to_rfc3339(),
        })
    }

    /// Bounded views over actual SQLite records; overdue excludes undated tasks.
    pub fn list_tasks(
        &self,
        view: TaskView,
        now: DateTime<Local>,
    ) -> Result<Vec<PersonalTask>, String> {
        let predicate = match view {
            TaskView::Open => "status = 'todo'",
            TaskView::Completed => "status = 'completed'",
            TaskView::Overdue => "status = 'todo' AND julianday(due_at) < julianday(?1)",
            TaskView::All => "1 = 1",
        };
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(&format!(
                "SELECT id, title, status, priority, due_at, source, created_at, updated_at
             FROM personal_tasks WHERE {predicate} AND ?1 IS NOT NULL
             ORDER BY CASE WHEN status = 'todo' THEN 0 ELSE 1 END,
                      due_at IS NULL, julianday(due_at), updated_at DESC, id LIMIT 50"
            ))
            .map_err(|error| format!("Unable to list personal tasks: {error}"))?;
        let rows = statement
            .query_map([now.with_timezone(&Utc).to_rfc3339()], task_from_row)
            .map_err(|error| format!("Unable to list personal tasks: {error}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to read personal tasks: {error}"))
    }

    /// Exact, case/whitespace-insensitive title matching. Two results suffice to
    /// prove ambiguity; never pick the first fuzzy match or change any record.
    pub fn matching_open_tasks(&self, title: &str) -> Result<Vec<PersonalTask>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, title, status, priority, due_at, source, created_at, updated_at
             FROM personal_tasks WHERE status = 'todo' ORDER BY id",
            )
            .map_err(|error| error.to_string())?;
        let mut matches = Vec::new();
        let title = normalized_title(title);
        for row in statement
            .query_map([], task_from_row)
            .map_err(|error| error.to_string())?
        {
            let task = row.map_err(|error| error.to_string())?;
            if normalized_title(&task.title) == title {
                matches.push(task);
                if matches.len() == 2 {
                    break;
                }
            }
        }
        Ok(matches)
    }

    pub fn matching_pending_reminders(
        &self,
        title: &str,
    ) -> Result<Vec<ScheduledReminder>, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, task_id, title, remind_at, status, created_at, fired_at
             FROM scheduled_reminders WHERE status = 'pending' ORDER BY remind_at, id",
            )
            .map_err(|error| error.to_string())?;
        let mut matches = Vec::new();
        let title = normalized_title(title);
        for row in statement
            .query_map([], reminder_from_row)
            .map_err(|error| error.to_string())?
        {
            let reminder = row.map_err(|error| error.to_string())?;
            if normalized_title(&reminder.title) == title {
                matches.push(reminder);
                if matches.len() == 2 {
                    break;
                }
            }
        }
        Ok(matches)
    }

    pub fn set_task_completed(&self, task_id: &str, completed: bool) -> Result<(), String> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("Unable to start the personal task update: {error}"))?;
        let changed = transaction
            .execute(
                "UPDATE personal_tasks SET status = ?2, updated_at = ?3 WHERE id = ?1",
                params![
                    task_id,
                    if completed { "completed" } else { "todo" },
                    Utc::now().to_rfc3339()
                ],
            )
            .map_err(|error| format!("Unable to update the personal task: {error}"))?;
        if changed == 0 {
            return Err("The personal task no longer exists.".to_owned());
        }
        if completed {
            transaction
                .execute(
                    "UPDATE scheduled_reminders
                     SET status = 'dismissed'
                     WHERE task_id = ?1 AND status = 'pending'",
                    [task_id],
                )
                .map_err(|error| {
                    format!("Unable to cancel the completed task reminder: {error}")
                })?;
        }
        transaction
            .commit()
            .map_err(|error| format!("Unable to save the personal task update: {error}"))?;
        Ok(())
    }

    pub fn dismiss_reminder(&self, reminder_id: &str) -> Result<(), String> {
        let connection = self.connection()?;
        let changed = connection
            .execute(
                "UPDATE scheduled_reminders SET status = 'dismissed' WHERE id = ?1 AND status = 'pending'",
                [reminder_id],
            )
            .map_err(|error| format!("Unable to dismiss the reminder: {error}"))?;
        if changed == 0 {
            let mut statement = connection
                .prepare("SELECT status FROM scheduled_reminders WHERE id = ?1")
                .map_err(|error| error.to_string())?;
            let mut rows = statement
                .query([reminder_id])
                .map_err(|error| error.to_string())?;
            if let Some(row) = rows.next().map_err(|error| error.to_string())? {
                let status: String = row.get(0).map_err(|error| error.to_string())?;
                return if status == "dismissed" {
                    Ok(())
                } else {
                    Err("This reminder has already been delivered or is no longer pending; cancellation cannot undo a notification.".into())
                };
            }
            return Err("The reminder no longer exists.".to_owned());
        }
        Ok(())
    }

    fn claim_due(&self, now: DateTime<Local>) -> Result<Vec<ScheduledReminder>, String> {
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("Unable to claim due reminders: {error}"))?;
        let reminders = {
            let mut statement = transaction
                .prepare(
                    "SELECT id, task_id, title, remind_at, status, created_at, fired_at
                     FROM scheduled_reminders
                     WHERE status = 'pending' AND remind_at <= ?1
                     ORDER BY remind_at LIMIT 20",
                )
                .map_err(|error| format!("Unable to read due reminders: {error}"))?;
            let due = statement
                .query_map([now.with_timezone(&Utc).to_rfc3339()], reminder_from_row)
                .map_err(|error| format!("Unable to read due reminders: {error}"))?
                .filter_map(Result::ok)
                .collect::<Vec<_>>();
            due
        };
        for reminder in &reminders {
            transaction
                .execute(
                    "UPDATE scheduled_reminders
                     SET status = 'fired', fired_at = ?2
                     WHERE id = ?1 AND status = 'pending'",
                    params![reminder.id, Utc::now().to_rfc3339()],
                )
                .map_err(|error| format!("Unable to mark a reminder as delivered: {error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("Unable to commit delivered reminders: {error}"))?;
        Ok(reminders)
    }
}

pub fn start_scheduler(store: Arc<PersonalStore>, app: AppHandle) {
    thread::Builder::new()
        .name("sharman-reminder-scheduler".into())
        .spawn(move || loop {
            if let Ok(reminders) = store.claim_due(Local::now()) {
                for reminder in reminders {
                    crate::background::send_notification(&app, "Sharman reminder", &reminder.title);
                }
            }
            thread::sleep(Duration::from_secs(15));
        })
        .expect("unable to start Sharman reminder scheduler");
}

fn task_is_relevant_today(task: &PersonalTask, now: DateTime<Local>) -> bool {
    let Some(due_at) = task.due_at.as_deref() else {
        return true;
    };
    DateTime::parse_from_rfc3339(due_at)
        .map(|due| due.with_timezone(&Local).date_naive() <= now.date_naive())
        .unwrap_or(true)
}

fn task_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PersonalTask> {
    Ok(PersonalTask {
        id: row.get(0)?,
        title: row.get(1)?,
        status: row.get(2)?,
        priority: row.get(3)?,
        due_at: row.get(4)?,
        source: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn reminder_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScheduledReminder> {
    Ok(ScheduledReminder {
        id: row.get(0)?,
        task_id: row.get(1)?,
        title: row.get(2)?,
        remind_at: row.get(3)?,
        status: row.get(4)?,
        created_at: row.get(5)?,
        fired_at: row.get(6)?,
    })
}

fn local_id(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        std::process::id()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn task_views_use_real_deadlines_and_survive_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("personal.db");
        let store = PersonalStore::open(path.clone()).unwrap();
        let now = Local.with_ymd_and_hms(2026, 10, 21, 12, 0, 0).unwrap();
        let overdue = store
            .create_task("Past", Some(now - chrono::Duration::seconds(1)), "test")
            .unwrap();
        store.create_task("At boundary", Some(now), "test").unwrap();
        store
            .create_task("Future", Some(now + chrono::Duration::days(1)), "test")
            .unwrap();
        store.create_task("Undated", None, "test").unwrap();
        let done = store
            .create_task("Done", Some(now - chrono::Duration::days(2)), "test")
            .unwrap();
        store.set_task_completed(&done.id, true).unwrap();
        let reopened = PersonalStore::open(path).unwrap();
        assert_eq!(reopened.list_tasks(TaskView::All, now).unwrap().len(), 5);
        assert_eq!(reopened.list_tasks(TaskView::Open, now).unwrap().len(), 4);
        assert_eq!(
            reopened.list_tasks(TaskView::Completed, now).unwrap()[0].id,
            done.id
        );
        let late = reopened.list_tasks(TaskView::Overdue, now).unwrap();
        assert_eq!(late.len(), 1);
        assert_eq!(late[0].id, overdue.id);
    }

    #[test]
    fn stale_cancellation_cannot_rewrite_a_fired_reminder() {
        let directory = tempfile::tempdir().unwrap();
        let store = PersonalStore::open(directory.path().join("personal.db")).unwrap();
        let now = Local::now();
        let (_, fired) = store
            .create_reminder_task("Already delivered", now - chrono::Duration::seconds(1))
            .unwrap();
        store.claim_due(now).unwrap();
        assert!(store
            .dismiss_reminder(&fired.id)
            .unwrap_err()
            .contains("already been delivered"));
        assert_eq!(
            store.bridge_snapshot().unwrap().reminders[0].status,
            "fired"
        );
        let (_, pending) = store
            .create_reminder_task("Still pending", now + chrono::Duration::hours(1))
            .unwrap();
        store.dismiss_reminder(&pending.id).unwrap();
        store.dismiss_reminder(&pending.id).unwrap();
        assert!(store.dismiss_reminder("missing-id").is_err());
    }

    #[test]
    fn task_view_limit_does_not_hide_completed_or_overdue_matches() {
        let directory = tempfile::tempdir().unwrap();
        let store = PersonalStore::open(directory.path().join("personal.db")).unwrap();
        let now = Local::now();
        for i in 0..55 {
            store
                .create_task(&format!("Undated {i}"), None, "test")
                .unwrap();
        }
        let done = store.create_task("Completed item", None, "test").unwrap();
        store.set_task_completed(&done.id, true).unwrap();
        store
            .create_task(
                "Overdue item",
                Some(now - chrono::Duration::days(1)),
                "test",
            )
            .unwrap();
        assert_eq!(store.list_tasks(TaskView::Open, now).unwrap().len(), 50);
        assert_eq!(store.list_tasks(TaskView::Completed, now).unwrap().len(), 1);
        assert_eq!(store.list_tasks(TaskView::Overdue, now).unwrap().len(), 1);
    }

    #[test]
    fn exact_title_previews_never_mutate_and_detect_ambiguous_names() {
        let directory = tempfile::tempdir().unwrap();
        let store = PersonalStore::open(directory.path().join("personal.db")).unwrap();
        let now = Local.with_ymd_and_hms(2026, 10, 21, 12, 0, 0).unwrap();
        let (task, reminder) = store
            .create_reminder_task("Review ÉTÉ notes", now + chrono::Duration::hours(1))
            .unwrap();
        assert_eq!(
            store.matching_open_tasks("review été   notes").unwrap()[0].id,
            task.id
        );
        assert_eq!(
            store
                .matching_pending_reminders("Review été notes")
                .unwrap()[0]
                .id,
            reminder.id
        );
        assert!(store.matching_open_tasks("Review").unwrap().is_empty());
        assert!(store.matching_open_tasks("' OR 1=1 --").unwrap().is_empty());
        store
            .create_reminder_task("Review ÉTÉ notes", now + chrono::Duration::hours(2))
            .unwrap();
        assert_eq!(
            store.matching_open_tasks("review été notes").unwrap().len(),
            2
        );
        assert_eq!(
            store
                .matching_pending_reminders("review été notes")
                .unwrap()
                .len(),
            2
        );
        assert_eq!(store.dashboard(now).unwrap().tasks.len(), 2);
        assert_eq!(store.dashboard(now).unwrap().reminders.len(), 2);
        store.dismiss_reminder(&reminder.id).unwrap();
        assert_eq!(
            store
                .matching_pending_reminders("review été notes")
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            store.matching_open_tasks("review été notes").unwrap().len(),
            2,
            "cancelling a reminder must not complete its task"
        );
    }

    #[test]
    fn tasks_and_reminders_persist_and_due_reminders_are_claimed_once() {
        let directory = tempfile::tempdir().unwrap();
        let store = PersonalStore::open(directory.path().join("personal.db")).unwrap();
        let now = Local.with_ymd_and_hms(2026, 10, 11, 20, 0, 0).unwrap();
        let (task, reminder) = store
            .create_reminder_task("Solve LeetCode", now - chrono::Duration::minutes(1))
            .unwrap();
        let dashboard = store.dashboard(now).unwrap();
        assert_eq!(dashboard.tasks[0].id, task.id);
        assert_eq!(dashboard.reminders[0].id, reminder.id);
        assert_eq!(store.claim_due(now).unwrap().len(), 1);
        assert!(store.claim_due(now).unwrap().is_empty());
        store.set_task_completed(&task.id, true).unwrap();
        assert!(store.dashboard(now).unwrap().tasks.is_empty());
    }

    #[test]
    fn completing_a_task_cancels_its_pending_reminder() {
        let directory = tempfile::tempdir().unwrap();
        let store = PersonalStore::open(directory.path().join("personal.db")).unwrap();
        let now = Local.with_ymd_and_hms(2026, 10, 11, 20, 0, 0).unwrap();
        let (task, _) = store
            .create_reminder_task("Review applications", now + chrono::Duration::hours(1))
            .unwrap();
        store.set_task_completed(&task.id, true).unwrap();
        let dashboard = store.dashboard(now).unwrap();
        assert!(dashboard.tasks.is_empty());
        assert!(dashboard.reminders.is_empty());
    }

    #[test]
    fn mcp_reminder_is_atomic_idempotent_and_claimed_by_the_native_scheduler() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("personal.db");
        let store = PersonalStore::open(path.clone()).unwrap();
        let due = Local::now() + chrono::Duration::minutes(2);
        let (task, first, created) = store
            .create_bridge_item("Test SHARMAN", Some(due), true)
            .unwrap();
        assert!(created);
        let (_, duplicate, created) = store
            .create_bridge_item("Test SHARMAN", Some(due), true)
            .unwrap();
        assert!(!created);
        assert_eq!(first.as_ref().unwrap().id, duplicate.unwrap().id);
        let reopened = PersonalStore::open(path).unwrap();
        assert_eq!(reopened.bridge_snapshot().unwrap().tasks.len(), 1);
        assert_eq!(
            reopened.dashboard(Local::now()).unwrap().tasks[0].id,
            task.id
        );
        assert_eq!(
            reopened
                .claim_due(due + chrono::Duration::seconds(1))
                .unwrap()
                .len(),
            1
        );
        assert!(reopened
            .claim_due(due + chrono::Duration::seconds(2))
            .unwrap()
            .is_empty());
        let (_, replayed, _) = reopened
            .create_bridge_item("Test SHARMAN", Some(due), true)
            .unwrap();
        assert_eq!(replayed.unwrap().status, "fired");
        assert!(reopened
            .claim_due(due + chrono::Duration::seconds(3))
            .unwrap()
            .is_empty());
    }
}
