use std::{
    cmp::Reverse,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, SystemTime},
};

use agentrelay_core::{
    domain::{ConflictStatus, ProjectMemory, TaskStatus},
    AgentRelayCore,
};
use chrono::{DateTime, Local, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sysinfo::System;

use crate::{
    assistant_tools::{self, is_trusted_browser_url},
    audit::AssistantAuditStore,
    gmail::{EmailAction, GmailMessageSummary, GmailService},
    personal::{PersonalStore, PersonalTask, ScheduledReminder},
    supervisor::{PlanStep, SupervisorDecision, SupervisorPlan, ToolCall},
};

const MAX_VISITED_ENTRIES: usize = 20_000;
const MAX_RESULTS: usize = 18;
const MAX_SEARCH_DEPTH: usize = 5;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantHomeSnapshot {
    pub projects: Vec<ProjectSummary>,
    pub device: DeviceProfile,
    pub capabilities: Vec<AssistantCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub path: String,
    pub branch: String,
    pub progress_percent: u8,
    pub completed_tasks: usize,
    pub total_tasks: usize,
    pub active_tasks: usize,
    pub blockers: usize,
    pub active_agents: usize,
    pub tracking_enabled: bool,
    pub last_scanned_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceProfile {
    pub total_memory_gb: u64,
    pub logical_cores: usize,
    pub cpu: String,
    pub operating_system: String,
    pub architecture: String,
    pub tier: String,
    pub recommended_model_size: String,
    pub recommendation: String,
    pub recommended_model_name: String,
    pub recommended_download_gb: f32,
    pub local_runtime_installed: bool,
    pub local_model_installed: bool,
    pub local_model_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantCapability {
    pub id: String,
    pub name: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantRequest {
    pub message: String,
    pub conversation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantResponse {
    pub conversation_id: String,
    pub intent: String,
    pub title: String,
    pub answer: String,
    pub projects: Vec<ProjectSummary>,
    pub files: Vec<FileSearchResult>,
    pub actions: Vec<AssistantAction>,
    pub suggestions: Vec<String>,
    pub local_only: bool,
    #[serde(default)]
    pub plan_steps: Vec<PlanStep>,
    #[serde(default)]
    pub personal_tasks: Vec<PersonalTask>,
    #[serde(default)]
    pub reminders: Vec<ScheduledReminder>,
    #[serde(default)]
    pub email_actions: Vec<EmailAction>,
    #[serde(default)]
    pub email_messages: Vec<GmailMessageSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSearchResult {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub modified_at: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantAction {
    pub id: String,
    pub label: String,
    pub kind: AssistantActionKind,
    pub target: String,
    pub requires_confirmation: bool,
}

#[derive(Debug, Deserialize)]
struct OllamaTags {
    #[serde(default)]
    models: Vec<OllamaModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaModel {
    name: String,
}

#[derive(Debug, Deserialize)]
struct OllamaGenerateResponse {
    response: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AssistantActionKind {
    #[serde(rename = "openProject")]
    Project,
    #[serde(rename = "openPath")]
    Path,
    #[serde(rename = "openUrl")]
    Url,
    #[serde(rename = "openTrustedUrl")]
    TrustedUrl,
    #[serde(rename = "playYoutube")]
    PlayYoutube,
    #[serde(rename = "setupLocalAi")]
    SetupLocalAi,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMemorySnapshot {
    pub active: AssistantConversation,
    pub conversations: Vec<AssistantConversationSummary>,
    pub memories: Vec<AssistantMemoryItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantConversation {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    pub turns: Vec<AssistantTurn>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantConversationSummary {
    pub id: String,
    pub title: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantTurn {
    pub id: i64,
    pub request: String,
    pub response: AssistantResponse,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantMemoryItem {
    pub id: i64,
    pub content: String,
    pub created_at: String,
}

pub struct AssistantMemoryStore {
    path: PathBuf,
}

impl AssistantMemoryStore {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let store = Self { path };
        let connection = store.connection()?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA foreign_keys = ON;
                 CREATE TABLE IF NOT EXISTS assistant_conversations (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS assistant_turns (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    conversation_id TEXT NOT NULL,
                    user_message TEXT NOT NULL,
                    response_json TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    FOREIGN KEY(conversation_id) REFERENCES assistant_conversations(id) ON DELETE CASCADE
                 );
                 CREATE INDEX IF NOT EXISTS idx_assistant_turns_conversation
                    ON assistant_turns(conversation_id, id);
                 CREATE TABLE IF NOT EXISTS assistant_memories (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    content TEXT NOT NULL UNIQUE COLLATE NOCASE,
                    created_at TEXT NOT NULL
                 );",
            )
            .map_err(|error| format!("Unable to initialize local assistant memory: {error}"))?;
        Ok(store)
    }

    fn connection(&self) -> Result<Connection, String> {
        let connection = Connection::open(&self.path)
            .map_err(|error| format!("Unable to open local assistant memory: {error}"))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| format!("Unable to configure local assistant memory: {error}"))?;
        Ok(connection)
    }

    pub fn create_conversation(&self) -> Result<String, String> {
        let id = unique_local_id("chat");
        let now = Utc::now().to_rfc3339();
        self.connection()?
            .execute(
                "INSERT INTO assistant_conversations (id, title, created_at, updated_at)
                 VALUES (?1, 'New conversation', ?2, ?2)",
                params![id, now],
            )
            .map_err(|error| format!("Unable to create a local conversation: {error}"))?;
        Ok(id)
    }

    fn ensure_conversation(&self, requested: Option<&str>) -> Result<String, String> {
        if let Some(id) = requested.filter(|value| !value.trim().is_empty()) {
            let exists = self
                .connection()?
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM assistant_conversations WHERE id = ?1)",
                    [id],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(|error| format!("Unable to read local conversations: {error}"))?;
            if exists {
                return Ok(id.to_owned());
            }
        }
        self.connection()?
            .query_row(
                "SELECT id FROM assistant_conversations ORDER BY updated_at DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("Unable to read local conversations: {error}"))?
            .map_or_else(|| self.create_conversation(), Ok)
    }

    pub fn snapshot(&self, requested: Option<&str>) -> Result<AssistantMemorySnapshot, String> {
        let active_id = self.ensure_conversation(requested)?;
        let connection = self.connection()?;
        let (title, updated_at) = connection
            .query_row(
                "SELECT title, updated_at FROM assistant_conversations WHERE id = ?1",
                [&active_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .map_err(|error| format!("Unable to load the local conversation: {error}"))?;

        let mut turn_statement = connection
            .prepare(
                "SELECT id, user_message, response_json, created_at
                 FROM (SELECT id, user_message, response_json, created_at
                       FROM assistant_turns WHERE conversation_id = ?1
                       ORDER BY id DESC LIMIT 80)
                 ORDER BY id ASC",
            )
            .map_err(|error| format!("Unable to load local chat history: {error}"))?;
        let turns = turn_statement
            .query_map([&active_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(|error| format!("Unable to load local chat history: {error}"))?
            .map(|row| {
                let (id, request, response_json, created_at) =
                    row.map_err(|error| format!("Unable to read local chat history: {error}"))?;
                let response = serde_json::from_str(&response_json)
                    .map_err(|error| format!("Unable to restore a local response: {error}"))?;
                Ok(AssistantTurn {
                    id,
                    request,
                    response,
                    created_at,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;

        let mut conversation_statement = connection
            .prepare(
                "SELECT id, title, updated_at FROM assistant_conversations
                 ORDER BY updated_at DESC LIMIT 20",
            )
            .map_err(|error| format!("Unable to list local conversations: {error}"))?;
        let conversations = conversation_statement
            .query_map([], |row| {
                Ok(AssistantConversationSummary {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    updated_at: row.get(2)?,
                })
            })
            .map_err(|error| format!("Unable to list local conversations: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to read local conversations: {error}"))?;

        let memories = read_memories(&connection)?;
        Ok(AssistantMemorySnapshot {
            active: AssistantConversation {
                id: active_id,
                title,
                updated_at,
                turns,
            },
            conversations,
            memories,
        })
    }

    fn record_turn(
        &self,
        conversation_id: &str,
        request: &str,
        response: &AssistantResponse,
    ) -> Result<(), String> {
        let connection = self.connection()?;
        let now = Utc::now().to_rfc3339();
        let response_json = serde_json::to_string(response)
            .map_err(|error| format!("Unable to encode the local response: {error}"))?;
        connection
            .execute(
                "INSERT INTO assistant_turns (conversation_id, user_message, response_json, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![conversation_id, request, response_json, now],
            )
            .map_err(|error| format!("Unable to save local chat history: {error}"))?;
        let title = truncate_text(request, 56);
        connection
            .execute(
                "UPDATE assistant_conversations
                 SET title = CASE WHEN title = 'New conversation' THEN ?2 ELSE title END,
                     updated_at = ?3
                 WHERE id = ?1",
                params![conversation_id, title, now],
            )
            .map_err(|error| format!("Unable to update local chat history: {error}"))?;
        Ok(())
    }

    fn remember(&self, content: &str) -> Result<(), String> {
        self.connection()?
            .execute(
                "INSERT INTO assistant_memories (content, created_at) VALUES (?1, ?2)
                 ON CONFLICT(content) DO NOTHING",
                params![content, Utc::now().to_rfc3339()],
            )
            .map_err(|error| format!("Unable to save local memory: {error}"))?;
        Ok(())
    }

    pub fn forget(&self, memory_id: Option<i64>) -> Result<(), String> {
        let connection = self.connection()?;
        match memory_id {
            Some(id) => connection.execute("DELETE FROM assistant_memories WHERE id = ?1", [id]),
            None => connection.execute("DELETE FROM assistant_memories", []),
        }
        .map_err(|error| format!("Unable to update local memory: {error}"))?;
        Ok(())
    }

    fn context(&self, conversation_id: &str) -> Result<String, String> {
        let connection = self.connection()?;
        let memories = read_memories(&connection)?;
        let mut statement = connection
            .prepare(
                "SELECT user_message, response_json FROM assistant_turns
                 WHERE conversation_id = ?1 ORDER BY id DESC LIMIT 8",
            )
            .map_err(|error| format!("Unable to prepare local context: {error}"))?;
        let mut turns = statement
            .query_map([conversation_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| format!("Unable to load local context: {error}"))?
            .filter_map(Result::ok)
            .filter_map(|(request, json)| {
                serde_json::from_str::<AssistantResponse>(&json)
                    .ok()
                    .map(|response| (request, response.answer))
            })
            .collect::<Vec<_>>();
        turns.reverse();
        let remembered = memories
            .iter()
            .map(|item| format!("- {}", item.content))
            .collect::<Vec<_>>()
            .join("\n");
        let recent = turns
            .iter()
            .map(|(request, answer)| format!("User: {request}\nSharman: {answer}"))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(truncate_text(
            &format!(
                "Remembered facts explicitly saved by the user:\n{}\n\nRecent conversation:\n{}",
                if remembered.is_empty() {
                    "- none"
                } else {
                    &remembered
                },
                if recent.is_empty() { "none" } else { &recent }
            ),
            7_000,
        ))
    }
}

fn read_memories(connection: &Connection) -> Result<Vec<AssistantMemoryItem>, String> {
    let mut statement = connection
        .prepare("SELECT id, content, created_at FROM assistant_memories ORDER BY id DESC LIMIT 50")
        .map_err(|error| format!("Unable to list local memories: {error}"))?;
    let memories = statement
        .query_map([], |row| {
            Ok(AssistantMemoryItem {
                id: row.get(0)?,
                content: row.get(1)?,
                created_at: row.get(2)?,
            })
        })
        .map_err(|error| format!("Unable to list local memories: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to read local memories: {error}"))?;
    Ok(memories)
}

fn unique_local_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{prefix}-{nanos}-{}", std::process::id())
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    let mut output = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        output.push('…');
    }
    output
}

pub fn home_snapshot(core: &AgentRelayCore) -> Result<AssistantHomeSnapshot, String> {
    let projects = project_summaries(core)?;
    let device = inspect_device();
    let local_ai_detail = device
        .local_model_name
        .as_ref()
        .map(|model| format!("Ollama model {model} is ready for private local reasoning"))
        .unwrap_or_else(|| {
            if device.local_runtime_installed {
                format!(
                    "Ollama is installed; download {} to enable local reasoning",
                    device.recommended_model_name
                )
            } else {
                "Install the local runtime, then download the hardware-matched model".to_owned()
            }
        });
    Ok(AssistantHomeSnapshot {
        capabilities: vec![
            capability(
                "supervisor",
                "Supervisor",
                "ready",
                format!(
                    "Routes requests through {} registered tools with explicit permissions",
                    assistant_tools::registry().len()
                ),
            ),
            capability(
                "tasks",
                "Personal tasks",
                "ready",
                "Local task list, due dates, and completion tracking",
            ),
            capability(
                "reminders",
                "Reminders",
                "ready",
                "Persistent scheduled reminders with Windows notifications",
            ),
            capability(
                "projects",
                "Projects",
                "ready",
                "AgentRelay memory, planning, handoffs, and progress",
            ),
            capability(
                "files",
                "Files",
                "ready",
                "Private filename search in your common folders",
            ),
            capability(
                "web",
                "Web & YouTube",
                "ready",
                "Open approved sites and play explicit YouTube requests",
            ),
            capability(
                "media",
                "Media",
                "ready",
                "Find and play local audio through the default Windows app",
            ),
            capability(
                "email",
                "Email",
                "notConnected",
                "Connect an email provider before Sharman can triage messages",
            ),
            capability(
                "voice",
                "Voice",
                "optional",
                "Uses the Windows webview speech service when available",
            ),
            capability(
                "localAi",
                "Local AI",
                if device.local_model_installed {
                    "ready"
                } else {
                    "setupRequired"
                },
                local_ai_detail,
            ),
        ],
        projects,
        device,
    })
}

pub fn answer(
    core: &AgentRelayCore,
    memory: &AssistantMemoryStore,
    personal: &PersonalStore,
    gmail: &GmailService,
    audit: &AssistantAuditStore,
    request: AssistantRequest,
) -> Result<AssistantResponse, String> {
    let message = request.message.trim();
    if message.is_empty() {
        return Err("Tell Sharman what you want to do.".to_owned());
    }
    let conversation_id = memory.ensure_conversation(request.conversation_id.as_deref())?;
    let lower = message.to_lowercase();
    let context = memory.context(&conversation_id)?;

    let mut output = if let Some(fact) = explicit_memory_fact(message) {
        if looks_sensitive(&fact) {
            response(
                "memory",
                "I did not save that",
                "That looks like a password, token, API key, or other secret. Sharman’s personal memory intentionally refuses to store secrets.",
                vec![],
                vec![],
                vec![],
                vec!["What do you remember about me?".into()],
            )
        } else {
            memory.remember(&fact)?;
            response(
                "memory",
                "Saved to local memory",
                format!("I’ll remember locally: {fact}"),
                vec![],
                vec![],
                vec![],
                vec!["What do you remember about me?".into()],
            )
        }
    } else if contains_any(
        &lower,
        &["what do you remember", "show my memory", "show memories"],
    ) {
        let memories = memory.snapshot(Some(&conversation_id))?.memories;
        let answer = if memories.is_empty() {
            "You have not asked me to remember anything yet. Say “Remember that …” to save a fact locally."
                .to_owned()
        } else {
            format!(
                "Here is what you explicitly asked me to remember:\n{}",
                memories
                    .iter()
                    .map(|item| format!("• {}", item.content))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        };
        response(
            "memory",
            "Local memory",
            answer,
            vec![],
            vec![],
            vec![],
            vec!["Remember that my preferred editor is VS Code".into()],
        )
    } else if let Some(decision) = crate::supervisor::decide(message, Local::now()) {
        execute_supervisor(personal, gmail, audit, decision)?
    } else if contains_any(
        &lower,
        &["email", "emails", "mail", "mails", "gmail", "inbox"],
    ) {
        let status = gmail.status()?;
        if status.connected {
            response(
                "email",
                "Gmail is connected",
                "Gmail is connected with read-only access. Try “Show my unread emails”, “Show my important emails”, or “What emails need action today?”.",
                vec![],
                vec![],
                vec![],
                vec![
                    "Show my unread emails".into(),
                    "Show my important emails".into(),
                    "What emails need action today?".into(),
                ],
            )
        } else {
            response(
                "email",
                "Connect Gmail",
                "Connect Gmail from the Action Inbox to use read-only inbox tools.",
                vec![],
                vec![],
                vec![],
                vec!["Open the Action Inbox".into()],
            )
        }
    } else {
        route_answer(core, message, &context)?
    };

    output.conversation_id = conversation_id.clone();
    memory.record_turn(&conversation_id, message, &output)?;
    Ok(output)
}

fn execute_supervisor(
    personal: &PersonalStore,
    gmail: &GmailService,
    audit: &AssistantAuditStore,
    decision: SupervisorDecision,
) -> Result<AssistantResponse, String> {
    let SupervisorPlan {
        intent,
        title,
        calls,
    } = match decision {
        SupervisorDecision::Execute(plan) => plan,
        SupervisorDecision::Clarify {
            title,
            prompt,
            suggestions,
        } => {
            return Ok(response(
                "supervisor",
                title,
                prompt,
                vec![],
                vec![],
                vec![],
                suggestions,
            ));
        }
    };

    let mut answer_parts = Vec::new();
    let mut actions = Vec::new();
    let mut tasks = Vec::new();
    let mut reminders = Vec::new();
    let mut email_actions = Vec::new();
    let mut email_messages = Vec::new();
    let mut steps = Vec::new();
    for call in calls {
        match &call {
            ToolCall::OpenTrustedUrl { url, label } => {
                actions.push(AssistantAction {
                    id: format!("trusted-url:{url}"),
                    label: label.clone(),
                    kind: AssistantActionKind::TrustedUrl,
                    target: url.clone(),
                    requires_confirmation: false,
                });
                answer_parts.push(format!("I prepared {label} in your default browser."));
                steps.push(call.step("ready"));
            }
            ToolCall::OpenLeetcodeDaily => {
                let (url, label, detail) = match assistant_tools::resolve_leetcode_daily() {
                    Ok(daily) => (
                        daily.url,
                        format!("Open: {}", daily.title),
                        format!(
                            "Today’s LeetCode challenge is “{}” ({}).",
                            daily.title, daily.difficulty
                        ),
                    ),
                    Err(_) => (
                        "https://leetcode.com/problemset/".to_owned(),
                        "Open LeetCode problemset".to_owned(),
                        "LeetCode’s daily endpoint was unavailable, so I prepared the official problemset where the Daily Challenge appears.".to_owned(),
                    ),
                };
                actions.push(AssistantAction {
                    id: format!("trusted-url:{url}"),
                    label,
                    kind: AssistantActionKind::TrustedUrl,
                    target: url,
                    requires_confirmation: false,
                });
                answer_parts.push(detail);
                steps.push(call.step("ready"));
            }
            ToolCall::SearchGmail { mode } => {
                if !gmail.status()?.connected {
                    answer_parts.push(
                        "Gmail is not connected yet. Open the Action Inbox and choose Connect Gmail; Sharman will request read-only access."
                            .to_owned(),
                    );
                    steps.push(call.step("setup-required"));
                } else {
                    match gmail.refresh(*mode) {
                        Ok(dashboard) => {
                            let scan = dashboard.scan.as_ref();
                            let scanned = scan.map_or(0, |summary| summary.scanned);
                            let matched = scan.map_or(0, |summary| summary.matched);
                            email_actions.extend(dashboard.needs_action);
                            answer_parts.push(if *mode == crate::gmail::GmailSearchMode::Internship {
                                format!(
                                    "Scanned: {scanned} career/OA candidate message{}. Actionable items matched this check: {matched}. Action Inbox total: {}.",
                                    if scanned == 1 { "" } else { "s" },
                                    email_actions.len()
                                )
                            } else {
                                format!(
                                    "Scanned: {scanned} {} message{}. Matched deterministic action signals: {matched}. Action Inbox total: {}. Verify extracted deadlines before creating reminders.",
                                    mode.label(),
                                    if scanned == 1 { "" } else { "s" },
                                    email_actions.len()
                                )
                            });
                            steps.push(call.step("completed"));
                        }
                        Err(error) => {
                            answer_parts.push(gmail_failure_message(&error));
                            steps.push(call.step("failed"));
                        }
                    }
                }
            }
            ToolCall::ListUnreadGmail | ToolCall::ListRecentGmail => {
                let unread = matches!(call, ToolCall::ListUnreadGmail);
                let label = if unread { "unread" } else { "recent inbox" };
                if !gmail.status()?.connected {
                    answer_parts.push(
                        format!("Connect Gmail from the Action Inbox to list {label} messages with read-only access."),
                    );
                    steps.push(call.step("setup-required"));
                } else {
                    match if unread {
                        gmail.list_unread()
                    } else {
                        gmail.list_recent()
                    } {
                        Ok(result) => {
                            email_messages.extend(result.messages);
                            answer_parts.push(if email_messages.is_empty() {
                                format!("No {label} messages found.")
                            } else {
                                format!(
                                    "Found {} {label} message{} using Gmail query `{}`. Only bounded metadata and snippets were returned.",
                                    result.scanned,
                                    if result.scanned == 1 { "" } else { "s" },
                                    result.query
                                )
                            });
                            steps.push(call.step("completed"));
                        }
                        Err(error) => {
                            answer_parts.push(gmail_failure_message(&error));
                            steps.push(call.step("failed"));
                        }
                    }
                }
            }
            ToolCall::GmailConnectionStatus => {
                let status = gmail.status()?;
                if !status.connected {
                    answer_parts.push("Connect Gmail from the Action Inbox.".to_owned());
                    steps.push(call.step("setup-required"));
                } else if status
                    .sync
                    .last_error
                    .as_deref()
                    .is_some_and(|error| error.contains("authorization needs reconnecting"))
                {
                    answer_parts.push(
                        "Gmail authorization needs reconnecting. Disconnect Gmail in the Action Inbox, then connect it again."
                            .to_owned(),
                    );
                    steps.push(call.step("reconnect-required"));
                } else {
                    answer_parts.push(format!(
                        "Gmail is connected{} with read-only access.{}",
                        status
                            .account_email
                            .as_deref()
                            .map(|email| format!(" as {email}"))
                            .unwrap_or_default(),
                        status
                            .sync
                            .last_error
                            .as_deref()
                            .map(|error| format!(
                                " The last check had a recoverable error: {error}"
                            ))
                            .unwrap_or_default()
                    ));
                    steps.push(call.step("completed"));
                }
            }
            ToolCall::ListTodayTasks => {
                let dashboard = personal.dashboard(Local::now())?;
                tasks.extend(dashboard.tasks);
                answer_parts.push(if tasks.is_empty() {
                    "You have no incomplete personal tasks due today or earlier.".to_owned()
                } else {
                    format!(
                        "You have {} incomplete personal task{} for today.",
                        tasks.len(),
                        if tasks.len() == 1 { "" } else { "s" }
                    )
                });
                steps.push(call.step("completed"));
            }
            ToolCall::CreateTask { title } => {
                let task = personal.create_task(title, None, "supervisor")?;
                answer_parts.push(format!("Created the local task “{}”.", task.title));
                tasks.push(task);
                steps.push(call.step("completed"));
            }
            ToolCall::ListTasks { view } => {
                tasks = personal.list_tasks(*view, Local::now())?;
                answer_parts.push(if tasks.is_empty() {
                    format!("No {} personal tasks found.", view.label())
                } else {
                    format!("Showing {} {} personal task(s), up to 50, from your local database. Overdue means the deadline has passed; undated tasks are not overdue.", tasks.len(), view.label())
                });
                steps.push(call.step("completed"));
            }
            ToolCall::ListReminders => {
                reminders = personal.dashboard(Local::now())?.reminders;
                answer_parts.push(if reminders.is_empty() {
                    "No pending reminders found.".into()
                } else {
                    format!("Your next {} pending reminder(s), up to 30, ordered by time. Delivery requires SHARMAN to be running and Windows notifications to be enabled.", reminders.len())
                });
                steps.push(call.step("completed"));
            }
            ToolCall::PreviewCompleteTask { title } => {
                let matches = personal.matching_open_tasks(title)?;
                match matches.len() {
                    0 => answer_parts.push("No open task has that exact title. Nothing was changed. Try ‘Show my tasks’, then use the exact title.".into()),
                    1 => {
                        tasks = matches;
                        answer_parts.push("Review the task below, then click ‘Complete task’ to confirm. Completing it also cancels its pending reminders. Nothing has changed yet.".into());
                    }
                    _ => answer_parts.push("More than one open task has that title. Nothing was changed or selected. Open the Action Inbox and choose the specific task by its deadline.".into()),
                }
                steps.push(call.step(if tasks.is_empty() {
                    "clarification-required"
                } else {
                    "awaiting-confirmation"
                }));
            }
            ToolCall::PreviewCancelReminder { title } => {
                let matches = personal.matching_pending_reminders(title)?;
                match matches.len() {
                    0 => answer_parts.push("No pending reminder has that exact title. Nothing was changed. Try ‘Show my reminders’.".into()),
                    1 => {
                        reminders = matches;
                        answer_parts.push("Review the reminder below, then click ‘Cancel reminder’ to confirm. The task itself will stay open. Nothing has changed yet.".into());
                    }
                    _ => answer_parts.push("More than one pending reminder has that title. Nothing was changed or selected. Open the Action Inbox and choose the specific reminder by its time.".into()),
                }
                steps.push(call.step(if reminders.is_empty() {
                    "clarification-required"
                } else {
                    "awaiting-confirmation"
                }));
            }
            ToolCall::CreateReminder { title, remind_at } => {
                let (task, reminder) = personal.create_reminder_task(title, *remind_at)?;
                answer_parts.push(format!(
                    "I’ll remind you to “{}” on {} at {}.",
                    title,
                    remind_at.format("%d %b %Y"),
                    remind_at.format("%I:%M %p")
                ));
                tasks.push(task);
                reminders.push(reminder);
                steps.push(call.step("completed"));
            }
        }
    }
    let mut result = response(
        intent,
        title,
        answer_parts.join("\n"),
        vec![],
        vec![],
        actions,
        vec![
            "Show my tasks".into(),
            "Show my reminders".into(),
            "Remind me in 20 minutes to take a break".into(),
        ],
    );
    result.plan_steps = steps;
    result.personal_tasks = tasks;
    result.reminders = reminders;
    result.email_actions = email_actions;
    result.email_messages = email_messages;
    result.local_only = !matches!(
        result.intent.as_str(),
        "email" | "gmail_unread" | "gmail_recent"
    );
    for step in &result.plan_steps {
        let _ = audit.record(
            "supervisor",
            &result.intent,
            &step.tool,
            if step.permission_level == 0 {
                "READ"
            } else {
                "LOW_RISK"
            },
            &step.status,
            &step.summary,
        );
    }
    Ok(result)
}

fn route_answer(
    core: &AgentRelayCore,
    message: &str,
    context: &str,
) -> Result<AssistantResponse, String> {
    let lower = message.to_lowercase();

    if contains_any(
        &lower,
        &[
            "project",
            "progress",
            "vidyut",
            "completed",
            "status",
            "work on next",
            "next task",
        ],
    ) && !contains_any(
        &lower,
        &[
            "file",
            "download",
            "document",
            "photo",
            "image",
            "pdf",
            "yesterday",
        ],
    ) {
        let mut projects = project_summaries(core)?;
        let requested = projects
            .iter()
            .find(|project| lower.contains(&project.name.to_lowercase()))
            .cloned();
        if let Some(project) = requested {
            let answer = if project.total_tasks == 0 {
                format!(
                    "{} is tracked, but it has no AgentRelay goals or tasks yet, so a truthful completion percentage is unavailable. It is on branch {} with {} blocker(s).",
                    project.name, project.branch, project.blockers
                )
            } else {
                format!(
                    "{} is {}% complete from its recorded tasks: {} of {} completed, {} active, and {} blocker(s).",
                    project.name,
                    project.progress_percent,
                    project.completed_tasks,
                    project.total_tasks,
                    project.active_tasks,
                    project.blockers
                )
            };
            let action = open_project_action(&project);
            return Ok(response(
                "projects",
                format!("{} progress", project.name),
                answer,
                vec![project],
                vec![],
                vec![action],
                vec![
                    "What should I work on next?".into(),
                    "Show all project progress".into(),
                ],
            ));
        }
        let answer = if projects.is_empty() {
            "No projects are tracked yet. Add a project when you want AgentRelay planning and progress; Sharman itself still works without one.".to_owned()
        } else {
            let complete = projects
                .iter()
                .map(|project| project.completed_tasks)
                .sum::<usize>();
            let total = projects
                .iter()
                .map(|project| project.total_tasks)
                .sum::<usize>();
            if total == 0 {
                format!(
                    "I found {} tracked project(s), but none has an AgentRelay task plan yet, so I will not invent completion percentages. Add goals and tasks to enable measurable progress.",
                    projects.len()
                )
            } else {
                format!(
                    "I found {} tracked project(s) with {} of {} recorded tasks completed. Progress is calculated from local AgentRelay task data, not guessed by a model.",
                    projects.len(), complete, total
                )
            }
        };
        let actions = projects.iter().map(open_project_action).collect();
        return Ok(response(
            "projects",
            "Project portfolio",
            answer,
            std::mem::take(&mut projects),
            vec![],
            actions,
            vec![
                "Show Vidyut progress".into(),
                "What should I work on next?".into(),
            ],
        ));
    }

    if contains_any(
        &lower,
        &[
            "file",
            "download",
            "document",
            "photo",
            "image",
            "pdf",
            "yesterday",
        ],
    ) {
        let files = search_files(message, SearchMode::Files)?;
        let answer = if files.is_empty() {
            "I searched filenames in Downloads, Desktop, Documents, and Pictures but found no local match. I did not inspect file contents.".to_owned()
        } else {
            format!(
                "I found {} local match(es). Choose one to open it. This search used filenames and timestamps only.",
                files.len()
            )
        };
        let actions = files
            .iter()
            .map(|file| AssistantAction {
                id: format!("open-file:{}", file.path),
                label: "Open".to_owned(),
                kind: AssistantActionKind::Path,
                target: file.path.clone(),
                requires_confirmation: false,
            })
            .collect();
        return Ok(response(
            "files",
            "Local file search",
            answer,
            vec![],
            files,
            actions,
            vec![
                "Find PDFs in Downloads".into(),
                "Find images from yesterday".into(),
            ],
        ));
    }

    if is_youtube_play_request(&lower) {
        let query = youtube_search_query(message);
        if query.is_empty() {
            return Ok(response(
                "youtube",
                "What should I play?",
                "Tell me the song, artist, or video name, for example: “Play Kasatka on YouTube.”",
                vec![],
                vec![],
                vec![],
                vec!["Play relaxing music on YouTube".into()],
            ));
        }
        return Ok(response(
            "youtube",
            "Opening YouTube",
            format!("I’m finding the best YouTube match for “{query}” and will start it in your default browser."),
            vec![],
            vec![],
            vec![AssistantAction {
                id: format!("play-youtube:{}", query.to_lowercase()),
                label: "Play on YouTube".to_owned(),
                kind: AssistantActionKind::PlayYoutube,
                target: query,
                requires_confirmation: false,
            }],
            vec!["Play another song on YouTube".into()],
        ));
    }

    if contains_any(&lower, &["play", "music", "song", "audio"]) {
        let files = search_files(message, SearchMode::Music)?;
        let answer = if files.is_empty() {
            "I could not find a matching local audio file in Music or Downloads. Streaming-service control is not connected yet.".to_owned()
        } else {
            "I found local audio you can play with your default Windows media app.".to_owned()
        };
        let actions = files
            .iter()
            .map(|file| AssistantAction {
                id: format!("play-file:{}", file.path),
                label: "Play".to_owned(),
                kind: AssistantActionKind::Path,
                target: file.path.clone(),
                requires_confirmation: false,
            })
            .collect();
        return Ok(response(
            "media",
            "Local music",
            answer,
            vec![],
            files,
            actions,
            vec!["Find music downloaded yesterday".into()],
        ));
    }

    if contains_any(
        &lower,
        &[
            "website",
            "open github",
            "open youtube",
            "open google",
            "open gmail",
            "http://",
            "https://",
        ],
    ) {
        if let Some(url) = requested_url(&lower) {
            return Ok(response(
                "web",
                "Website ready",
                "I prepared the validated website. Opening it requires your confirmation.",
                vec![],
                vec![],
                vec![AssistantAction {
                    id: format!("open-url:{url}"),
                    label: "Open website".to_owned(),
                    kind: AssistantActionKind::Url,
                    target: url,
                    requires_confirmation: true,
                }],
                vec!["List progress of all my projects".into()],
            ));
        }
    }

    if let Some((model, answer)) = local_reasoning(message, context) {
        return Ok(response(
            "assistant",
            format!("Local reasoning · {model}"),
            answer,
            vec![],
            vec![],
            vec![],
            vec![
                "List progress of all my projects".into(),
                "Find files downloaded yesterday".into(),
            ],
        ));
    }

    if contains_any(
        &lower,
        &[
            "hello",
            "hi sharman",
            "hey sharman",
            "what can you do",
            "help",
        ],
    ) {
        return Ok(response(
            "assistant",
            "Here’s what I can do now",
            "I can report AgentRelay project progress, find local files by name or date, open approved websites, and play local audio. Email needs a connected provider. General conversation becomes available after local AI setup.",
            vec![],
            vec![],
            vec![],
            vec![
                "List progress of all my projects".into(),
                "Find files downloaded yesterday".into(),
                "Open GitHub".into(),
                "Play music".into(),
            ],
        ));
    }

    Ok(response(
        "localAi",
        "Local AI needs setup",
        "This request needs general language reasoning. The deterministic tool router did not guess an action, and no local model is ready. Set up the recommended model once, then ask naturally.",
        vec![],
        vec![],
        vec![AssistantAction {
            id: "setup-local-ai".to_owned(),
            label: "Set up local AI".to_owned(),
            kind: AssistantActionKind::SetupLocalAi,
            target: String::new(),
            requires_confirmation: false,
        }],
        vec![
            "Find files downloaded yesterday".into(),
            "List progress of all my projects".into(),
        ],
    ))
}

pub fn execute_action(core: &AgentRelayCore, action: AssistantAction) -> Result<(), String> {
    match action.kind {
        AssistantActionKind::Project => {
            let permitted = core
                .tracked_projects()
                .map_err(|error| error.to_string())?
                .iter()
                .any(|project| same_path(&project.path, &action.target));
            if !permitted {
                return Err("That project is not tracked by Sharman.".to_owned());
            }
            Ok(())
        }
        AssistantActionKind::Path => {
            let target = fs::canonicalize(&action.target)
                .map_err(|error| format!("The selected file is no longer available: {error}"))?;
            if !is_allowed_path(core, &target)? {
                return Err(
                    "Sharman may only open files from your common folders or tracked projects."
                        .to_owned(),
                );
            }
            open_path(&target)
        }
        AssistantActionKind::Url => {
            if !is_safe_url(&action.target) {
                return Err("Only http:// and https:// websites may be opened.".to_owned());
            }
            open_url(&action.target)
        }
        AssistantActionKind::TrustedUrl => {
            if !is_trusted_browser_url(&action.target) {
                return Err(
                    "That address is outside Sharman’s trusted browser-tool scope.".to_owned(),
                );
            }
            open_url(&action.target)
        }
        AssistantActionKind::PlayYoutube => play_youtube(&action.target),
        AssistantActionKind::SetupLocalAi => Ok(()),
    }
}

fn project_summaries(core: &AgentRelayCore) -> Result<Vec<ProjectSummary>, String> {
    let mut summaries = core
        .tracked_projects()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter_map(|project| core.project_memory(&project.path).ok())
        .map(project_summary)
        .collect::<Vec<_>>();
    summaries.sort_by_key(|project| Reverse(project.last_scanned_at.clone()));
    Ok(summaries)
}

fn project_summary(memory: ProjectMemory) -> ProjectSummary {
    let tasks = memory
        .goals
        .iter()
        .flat_map(|goal| goal.tasks.iter())
        .collect::<Vec<_>>();
    let total_tasks = tasks
        .iter()
        .filter(|task| task.status != TaskStatus::Archived)
        .count();
    let completed_tasks = tasks
        .iter()
        .filter(|task| task.status == TaskStatus::Completed)
        .count();
    let active_tasks = tasks
        .iter()
        .filter(|task| matches!(task.status, TaskStatus::Todo | TaskStatus::InProgress))
        .count();
    let task_blockers = tasks
        .iter()
        .filter(|task| !task.blocked_by.is_empty())
        .count();
    let conflict_blockers = memory
        .conflicts
        .iter()
        .filter(|conflict| conflict.status != ConflictStatus::Resolved)
        .count();
    let progress_percent = completed_tasks
        .saturating_mul(100)
        .checked_div(total_tasks)
        .unwrap_or(0) as u8;
    ProjectSummary {
        id: memory.project.id,
        name: memory.project.name,
        path: memory.project.path,
        branch: memory.project.branch,
        progress_percent,
        completed_tasks,
        total_tasks,
        active_tasks,
        blockers: task_blockers + conflict_blockers,
        active_agents: memory
            .agent_sessions
            .iter()
            .filter(|session| session.status == agentrelay_core::domain::AgentSessionStatus::Active)
            .count(),
        tracking_enabled: memory.project.tracking_enabled,
        last_scanned_at: memory.project.last_scanned_at,
    }
}

fn inspect_device() -> DeviceProfile {
    let mut system = System::new_all();
    system.refresh_all();
    let total_memory_gb = system.total_memory() / 1024 / 1024 / 1024;
    let logical_cores = system.cpus().len();
    let cpu = system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Unknown CPU".to_owned());
    let recommendation = recommended_model(total_memory_gb);
    let local_model_name = detect_local_models().into_iter().next();
    DeviceProfile {
        total_memory_gb,
        logical_cores,
        cpu,
        operating_system: System::long_os_version()
            .or_else(System::name)
            .unwrap_or_else(|| "Windows".to_owned()),
        architecture: env::consts::ARCH.to_owned(),
        tier: recommendation.tier.to_owned(),
        recommended_model_size: recommendation.parameter_range.to_owned(),
        recommendation: recommendation.detail.to_owned(),
        recommended_model_name: recommendation.name.to_owned(),
        recommended_download_gb: recommendation.download_gb,
        local_runtime_installed: ollama_path().is_some(),
        local_model_installed: local_model_name.is_some(),
        local_model_name,
    }
}

#[derive(Debug, Clone, Copy)]
struct RecommendedModel {
    tier: &'static str,
    parameter_range: &'static str,
    name: &'static str,
    download_gb: f32,
    detail: &'static str,
}

fn recommended_model(total_memory_gb: u64) -> RecommendedModel {
    match total_memory_gb {
        0..=7 => RecommendedModel {
            tier: "tiny",
            parameter_range: "1–3B Q4",
            name: "qwen3:1.7b",
            download_gb: 1.4,
            detail: "Use a small local model so the assistant remains responsive on limited-memory hardware.",
        },
        8..=15 => RecommendedModel {
            tier: "compact",
            parameter_range: "3–4B Q4",
            name: "qwen3:4b",
            download_gb: 2.5,
            detail: "Use a compact local model and keep specialist project work in AgentRelay.",
        },
        16..=31 => RecommendedModel {
            tier: "balanced",
            parameter_range: "7–8B Q4",
            name: "qwen3:8b",
            download_gb: 5.2,
            detail: "A quantized 8B model is the best balance for local assistant reasoning on this computer.",
        },
        32..=63 => RecommendedModel {
            tier: "capable",
            parameter_range: "12–14B Q4",
            name: "qwen3:14b",
            download_gb: 9.3,
            detail: "This computer can run a stronger 14B local model while keeping the deterministic router responsive.",
        },
        _ => RecommendedModel {
            tier: "workstation",
            parameter_range: "20–30B Q4",
            name: "qwen3:30b",
            download_gb: 19.0,
            detail: "This workstation can target a larger local model while retaining a fast deterministic tool router.",
        },
    }
}

#[derive(Debug, Clone, Copy)]
enum SearchMode {
    Files,
    Music,
}

fn search_files(message: &str, mode: SearchMode) -> Result<Vec<FileSearchResult>, String> {
    let lower = message.to_lowercase();
    let mut roots = common_roots(mode);
    if lower.contains("download") {
        roots.retain(|path| {
            path.file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("Downloads"))
        });
    } else if lower.contains("desktop") {
        roots.retain(|path| {
            path.file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("Desktop"))
        });
    } else if lower.contains("document") {
        roots.retain(|path| {
            path.file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("Documents"))
        });
    } else if lower.contains("picture") || lower.contains("photo") || lower.contains("image") {
        roots.retain(|path| {
            path.file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("Pictures"))
        });
    }
    let yesterday_only = lower.contains("yesterday");
    let terms = search_terms(&lower);
    let extension_filter = requested_extensions(&lower);
    let criteria = SearchCriteria {
        terms: &terms,
        yesterday_only,
        mode,
        extension_filter: &extension_filter,
    };
    let mut matches = Vec::<(SystemTime, FileSearchResult)>::new();
    let mut visited = 0usize;
    for root in roots {
        walk_files(&root, 0, &criteria, &mut visited, &mut matches);
        if visited >= MAX_VISITED_ENTRIES {
            break;
        }
    }
    matches.sort_by_key(|(modified, _)| Reverse(*modified));
    matches.truncate(MAX_RESULTS);
    Ok(matches.into_iter().map(|(_, file)| file).collect())
}

struct SearchCriteria<'a> {
    terms: &'a [String],
    yesterday_only: bool,
    mode: SearchMode,
    extension_filter: &'a [&'a str],
}

fn common_roots(mode: SearchMode) -> Vec<PathBuf> {
    let Some(home) = env::var_os("USERPROFILE").map(PathBuf::from) else {
        return Vec::new();
    };
    let names: &[&str] = match mode {
        SearchMode::Files => &["Downloads", "Desktop", "Documents", "Pictures"],
        SearchMode::Music => &["Music", "Downloads"],
    };
    names
        .iter()
        .map(|name| home.join(name))
        .filter(|path| path.is_dir())
        .collect()
}

fn walk_files(
    directory: &Path,
    depth: usize,
    criteria: &SearchCriteria<'_>,
    visited: &mut usize,
    matches: &mut Vec<(SystemTime, FileSearchResult)>,
) {
    if depth > MAX_SEARCH_DEPTH || *visited >= MAX_VISITED_ENTRIES {
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        *visited += 1;
        if *visited >= MAX_VISITED_ENTRIES {
            break;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !skip_directory(&name) {
                walk_files(&path, depth + 1, criteria, visited, matches);
            }
            continue;
        }
        if !path.is_file() || (matches!(criteria.mode, SearchMode::Music) && !is_audio(&path)) {
            continue;
        }
        if !criteria.extension_filter.is_empty()
            && !path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    criteria
                        .extension_filter
                        .iter()
                        .any(|allowed| extension.eq_ignore_ascii_case(allowed))
                })
        {
            continue;
        }
        let name_lower = name.to_lowercase();
        if !criteria.terms.iter().all(|term| name_lower.contains(term)) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let modified = metadata
            .modified()
            .or_else(|_| metadata.created())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let modified_local: DateTime<Local> = modified.into();
        if criteria.yesterday_only {
            let today = Local::now().date_naive();
            let Some(yesterday) = today.pred_opt() else {
                continue;
            };
            if modified_local.date_naive() != yesterday {
                continue;
            }
        }
        matches.push((
            modified,
            FileSearchResult {
                name,
                path: path.to_string_lossy().into_owned(),
                kind: path
                    .extension()
                    .map(|extension| extension.to_string_lossy().to_ascii_uppercase())
                    .unwrap_or_else(|| "FILE".to_owned()),
                modified_at: modified_local.to_rfc3339(),
                size_bytes: metadata.len(),
            },
        ));
    }
}

fn search_terms(lower: &str) -> Vec<String> {
    let stop = [
        "find",
        "get",
        "me",
        "my",
        "the",
        "a",
        "an",
        "file",
        "files",
        "that",
        "i",
        "downloaded",
        "download",
        "downloads",
        "yesterday",
        "from",
        "in",
        "show",
        "open",
        "please",
        "image",
        "images",
        "photo",
        "photos",
        "picture",
        "pictures",
        "document",
        "documents",
        "desktop",
        "pdf",
        "pdfs",
        "play",
        "music",
        "song",
        "audio",
    ];
    lower
        .split(|character: char| {
            !character.is_alphanumeric() && character != '.' && character != '-'
        })
        .filter(|word| word.len() > 1 && !stop.contains(word))
        .map(ToOwned::to_owned)
        .collect()
}

fn requested_extensions(lower: &str) -> Vec<&'static str> {
    if lower.contains("pdf") {
        vec!["pdf"]
    } else if lower.contains("image") || lower.contains("photo") || lower.contains("picture") {
        vec!["png", "jpg", "jpeg", "gif", "webp", "bmp"]
    } else {
        Vec::new()
    }
}

fn skip_directory(name: &str) -> bool {
    name.starts_with('.')
        || ["node_modules", "target", "AppData", "$RECYCLE.BIN"]
            .iter()
            .any(|skip| name.eq_ignore_ascii_case(skip))
}

fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["mp3", "wav", "m4a", "flac", "aac", "ogg", "wma"]
                .iter()
                .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        })
}

fn is_allowed_path(core: &AgentRelayCore, target: &Path) -> Result<bool, String> {
    let common = common_roots(SearchMode::Files)
        .into_iter()
        .chain(common_roots(SearchMode::Music));
    for root in common {
        if let Ok(root) = fs::canonicalize(root) {
            if target.starts_with(root) {
                return Ok(true);
            }
        }
    }
    for project in core.tracked_projects().map_err(|error| error.to_string())? {
        if let Ok(root) = fs::canonicalize(project.path) {
            if target.starts_with(root) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn requested_url(lower: &str) -> Option<String> {
    if let Some(token) = lower
        .split_whitespace()
        .find(|part| part.starts_with("https://") || part.starts_with("http://"))
    {
        let url = token.trim_matches(|character: char| {
            matches!(character, ',' | '.' | ')' | ']' | '"' | '\'')
        });
        return is_safe_url(url).then(|| url.to_owned());
    }
    [
        ("github", "https://github.com"),
        ("youtube", "https://youtube.com"),
        ("gmail", "https://mail.google.com"),
        ("google", "https://google.com"),
    ]
    .into_iter()
    .find(|(name, _)| lower.contains(name))
    .map(|(_, url)| url.to_owned())
}

fn is_safe_url(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://"))
        && !url.chars().any(char::is_whitespace)
}

fn open_project_action(project: &ProjectSummary) -> AssistantAction {
    AssistantAction {
        id: format!("open-project:{}", project.id),
        label: "Open project".to_owned(),
        kind: AssistantActionKind::Project,
        target: project.path.clone(),
        requires_confirmation: false,
    }
}

fn capability(
    id: &str,
    name: &str,
    status: &str,
    detail: impl Into<String>,
) -> AssistantCapability {
    AssistantCapability {
        id: id.to_owned(),
        name: name.to_owned(),
        status: status.to_owned(),
        detail: detail.into(),
    }
}

fn response(
    intent: &str,
    title: impl Into<String>,
    answer: impl Into<String>,
    projects: Vec<ProjectSummary>,
    files: Vec<FileSearchResult>,
    actions: Vec<AssistantAction>,
    suggestions: Vec<String>,
) -> AssistantResponse {
    AssistantResponse {
        conversation_id: String::new(),
        intent: intent.to_owned(),
        title: title.into(),
        answer: answer.into(),
        projects,
        files,
        actions,
        suggestions,
        local_only: true,
        plan_steps: Vec::new(),
        personal_tasks: Vec::new(),
        reminders: Vec::new(),
        email_actions: Vec::new(),
        email_messages: Vec::new(),
    }
}

fn gmail_failure_message(error: &str) -> String {
    if error.contains("authorization needs reconnecting") {
        "Gmail authorization needs reconnecting. Disconnect Gmail in the Action Inbox, then connect it again."
            .to_owned()
    } else if error.contains("not connected") {
        "Connect Gmail from the Action Inbox to use read-only inbox tools.".to_owned()
    } else {
        format!("Gmail could not be checked safely: {error}")
    }
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn explicit_memory_fact(message: &str) -> Option<String> {
    let lower = message.to_lowercase();
    ["please remember that ", "remember that ", "remember "]
        .into_iter()
        .find_map(|prefix| {
            lower.strip_prefix(prefix).map(|_| {
                message[prefix.len()..]
                    .trim()
                    .trim_end_matches('.')
                    .to_owned()
            })
        })
        .filter(|fact| !fact.is_empty())
        .map(|fact| truncate_text(&fact, 500))
}

fn looks_sensitive(value: &str) -> bool {
    contains_any(
        &value.to_lowercase(),
        &[
            "password",
            "passcode",
            "api key",
            "api_key",
            "access token",
            "private key",
            "secret key",
            "-----begin",
        ],
    )
}

fn is_youtube_play_request(lower: &str) -> bool {
    lower.contains("youtube")
        && contains_any(
            lower,
            &["play", "listen", "watch", "song", "music", "video"],
        )
}

fn youtube_search_query(message: &str) -> String {
    let lower = message.to_lowercase();
    let start = if lower.starts_with("please play ") {
        "please play ".len()
    } else if lower.starts_with("play ") {
        "play ".len()
    } else if lower.starts_with("youtube play ") {
        "youtube play ".len()
    } else {
        0
    };
    let mut query = message[start..].trim();
    let query_lower = query.to_lowercase();
    for suffix in [" on youtube", " in youtube", " using youtube", " youtube"] {
        if query_lower.ends_with(suffix) {
            query = query[..query.len() - suffix.len()].trim();
            break;
        }
    }
    query
        .trim_matches(|character: char| matches!(character, '"' | '\'' | '.' | '?' | '!'))
        .to_owned()
}

fn encode_query(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                output.push(char::from(byte));
            }
            b' ' => output.push('+'),
            _ => output.push_str(&format!("%{byte:02X}")),
        }
    }
    output
}

fn first_youtube_video_id(html: &str) -> Option<&str> {
    const MARKER: &str = "\"videoId\":\"";
    html.match_indices(MARKER).find_map(|(index, _)| {
        let start = index + MARKER.len();
        let candidate = html.get(start..start + 11)?;
        candidate
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            .then_some(candidate)
    })
}

fn play_youtube(query: &str) -> Result<(), String> {
    if query.trim().is_empty() {
        return Err("Tell Sharman what to play on YouTube.".to_owned());
    }
    let search_url = format!(
        "https://www.youtube.com/results?search_query={}",
        encode_query(query.trim())
    );
    let playback_url = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(8))
        .timeout_write(Duration::from_secs(2))
        .build()
        .get(&search_url)
        .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .call()
        .ok()
        .and_then(|response| response.into_string().ok())
        .and_then(|html| first_youtube_video_id(&html).map(str::to_owned))
        .map(|video_id| format!("https://www.youtube.com/watch?v={video_id}&autoplay=1"))
        .unwrap_or(search_url);
    open_url(&playback_url)
}

fn ollama_agent(read_timeout: std::time::Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_millis(250))
        .timeout_read(read_timeout)
        .timeout_write(std::time::Duration::from_secs(2))
        .build()
}

fn detect_local_models() -> Vec<String> {
    ollama_agent(std::time::Duration::from_secs(1))
        .get("http://127.0.0.1:11434/api/tags")
        .call()
        .ok()
        .and_then(|response| response.into_json::<OllamaTags>().ok())
        .map(|tags| {
            tags.models
                .into_iter()
                .map(|model| model.name)
                .filter(|name| {
                    let lower = name.to_lowercase();
                    !lower.contains("embed") && !lower.contains("vision")
                })
                .collect()
        })
        .unwrap_or_default()
}

fn local_reasoning(message: &str, context: &str) -> Option<(String, String)> {
    let model = detect_local_models().into_iter().next()?;
    let prompt = format!(
        "You are Sharman, a concise local Windows assistant. Answer the user's request in plain language. Use the local context only when relevant. Do not reveal the context verbatim unless asked. Do not claim that you opened files, read email, changed a project, or performed an action. Tool actions are handled separately by deterministic code. If the request needs a capability you do not have, say so clearly.\n\nLocal context:\n{context}\n\nCurrent user request: {message}\n\nSharman:"
    );
    let response = ollama_agent(std::time::Duration::from_secs(90))
        .post("http://127.0.0.1:11434/api/generate")
        .send_json(serde_json::json!({
            "model": model,
            "prompt": prompt,
            "stream": false,
            "options": { "temperature": 0.2 }
        }))
        .ok()?
        .into_json::<OllamaGenerateResponse>()
        .ok()?;
    let answer = response.response.trim().to_owned();
    (!answer.is_empty()).then_some((model, answer))
}

pub fn install_local_runtime() -> Result<DeviceProfile, String> {
    if ollama_path().is_some() {
        ensure_ollama_running()?;
        return Ok(inspect_device());
    }
    let status = hidden_command("winget")
        .args([
            "install",
            "--id",
            "Ollama.Ollama",
            "--exact",
            "--source",
            "winget",
            "--silent",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--disable-interactivity",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| format!("Unable to start the verified Ollama installer: {error}"))?;
    if !status.success() {
        return Err(format!(
            "Ollama installation failed with exit code {}. Use the official Ollama download instead.",
            status.code().unwrap_or(-1)
        ));
    }
    ensure_ollama_running()?;
    Ok(inspect_device())
}

pub fn pull_recommended_model() -> Result<DeviceProfile, String> {
    let profile = inspect_device();
    let model = profile.recommended_model_name.clone();
    let executable =
        ollama_path().ok_or_else(|| "Install the local AI runtime first.".to_owned())?;
    ensure_ollama_running()?;
    let allowed = [
        "qwen3:1.7b",
        "qwen3:4b",
        "qwen3:8b",
        "qwen3:14b",
        "qwen3:30b",
    ];
    if !allowed.contains(&model.as_str()) {
        return Err("The recommended model is not in Sharman’s fixed allowlist.".to_owned());
    }
    let status = hidden_command(executable)
        .args(["pull", &model])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| format!("Unable to download {model}: {error}"))?;
    if !status.success() {
        return Err(format!(
            "The {model} download failed with exit code {}.",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(inspect_device())
}

fn ensure_ollama_running() -> Result<(), String> {
    if ollama_reachable() {
        return Ok(());
    }
    let executable = ollama_path()
        .ok_or_else(|| "The Ollama executable was not found after installation.".to_owned())?;
    hidden_command(executable)
        .arg("serve")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Unable to start the local Ollama service: {error}"))?;
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(500));
        if ollama_reachable() {
            return Ok(());
        }
    }
    Err("Ollama was installed but its local service did not become ready.".to_owned())
}

fn ollama_reachable() -> bool {
    ollama_agent(Duration::from_millis(600))
        .get("http://127.0.0.1:11434/api/tags")
        .call()
        .is_ok()
}

fn ollama_path() -> Option<PathBuf> {
    if let Ok(output) = hidden_command("where.exe").arg("ollama.exe").output() {
        if output.status.success() {
            if let Some(path) = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(PathBuf::from)
                .filter(|path| path.is_file())
            {
                return Some(path);
            }
        }
    }
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|local| local.join("Programs").join("Ollama").join("ollama.exe"))
        .filter(|path| path.is_file())
}

fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

fn same_path(left: &str, right: &str) -> bool {
    fs::canonicalize(left)
        .ok()
        .zip(fs::canonicalize(right).ok())
        .is_some_and(|(left, right)| left == right)
}

#[cfg(windows)]
fn open_path(path: &Path) -> Result<(), String> {
    Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Unable to open the selected file: {error}"))
}

#[cfg(not(windows))]
fn open_path(_path: &Path) -> Result<(), String> {
    Err("Opening files is currently supported on Windows only.".to_owned())
}

#[cfg(windows)]
fn open_url(url: &str) -> Result<(), String> {
    Command::new("rundll32.exe")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Unable to open the website: {error}"))
}

#[cfg(not(windows))]
fn open_url(_url: &str) -> Result<(), String> {
    Err("Opening websites is currently supported on Windows only.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_tier_is_always_actionable() {
        let profile = inspect_device();
        assert!(!profile.recommended_model_size.is_empty());
        assert!(!profile.recommendation.is_empty());
    }

    #[test]
    fn balanced_hardware_receives_the_eight_billion_parameter_model() {
        let recommendation = recommended_model(19);
        assert_eq!(recommendation.name, "qwen3:8b");
        assert_eq!(recommendation.download_gb, 5.2);
    }

    #[test]
    fn unsafe_url_schemes_are_rejected() {
        assert!(is_safe_url("https://example.com"));
        assert!(!is_safe_url("file:///C:/secret.txt"));
        assert!(!is_safe_url("javascript:alert(1)"));
    }

    #[test]
    fn natural_file_query_removes_command_words() {
        assert_eq!(
            search_terms("find me the vidyut report downloaded yesterday"),
            vec!["vidyut", "report"]
        );
    }

    #[test]
    fn youtube_play_request_keeps_the_requested_title() {
        assert!(is_youtube_play_request("please play casa tupka on youtube"));
        assert_eq!(
            youtube_search_query("Please play Casa Tupka on YouTube"),
            "Casa Tupka"
        );
        assert_eq!(encode_query("Casa Tupka"), "Casa+Tupka");
    }

    #[test]
    fn youtube_result_parser_accepts_only_valid_video_ids() {
        let html = r#"{"videoId":"dQw4w9WgXcQ"}"#;
        assert_eq!(first_youtube_video_id(html), Some("dQw4w9WgXcQ"));
        assert_eq!(first_youtube_video_id(r#"{"videoId":"bad id!!!!!"}"#), None);
    }

    #[test]
    fn local_memory_persists_turns_and_explicit_facts() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let store =
            AssistantMemoryStore::open(directory.path().join("memory.db")).expect("memory store");
        let conversation_id = store.create_conversation().expect("conversation");
        store
            .remember("I prefer concise answers")
            .expect("remember fact");
        let mut saved_response = response(
            "assistant",
            "Test",
            "Understood.",
            vec![],
            vec![],
            vec![],
            vec![],
        );
        saved_response.conversation_id = conversation_id.clone();
        store
            .record_turn(&conversation_id, "Hello", &saved_response)
            .expect("record turn");

        let reopened = AssistantMemoryStore::open(directory.path().join("memory.db"))
            .expect("reopen memory store");
        let snapshot = reopened
            .snapshot(Some(&conversation_id))
            .expect("memory snapshot");
        assert_eq!(snapshot.active.turns.len(), 1);
        assert_eq!(snapshot.active.turns[0].request, "Hello");
        assert_eq!(snapshot.memories[0].content, "I prefer concise answers");
    }

    #[test]
    fn supervisor_executes_trusted_browser_and_personal_task_tools() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let core = AgentRelayCore::open(directory.path().join("core.db"), Duration::from_secs(1))
            .expect("core");
        let memory = AssistantMemoryStore::open(directory.path().join("assistant.db"))
            .expect("assistant memory");
        let personal =
            PersonalStore::open(directory.path().join("assistant.db")).expect("personal store");
        let gmail =
            GmailService::open(directory.path().join("assistant.db")).expect("gmail service");
        let audit =
            AssistantAuditStore::open(directory.path().join("assistant.db")).expect("audit store");

        let browser = answer(
            &core,
            &memory,
            &personal,
            &gmail,
            &audit,
            AssistantRequest {
                message: "Open LeetCode".into(),
                conversation_id: None,
            },
        )
        .expect("browser plan");
        assert_eq!(browser.intent, "browser");
        assert_eq!(browser.actions[0].kind, AssistantActionKind::TrustedUrl);
        assert_eq!(browser.plan_steps[0].tool, "browser.open");

        let created = answer(
            &core,
            &memory,
            &personal,
            &gmail,
            &audit,
            AssistantRequest {
                message: "Add task Review internship applications".into(),
                conversation_id: Some(browser.conversation_id.clone()),
            },
        )
        .expect("create task");
        assert_eq!(created.personal_tasks.len(), 1);

        let listed = answer(
            &core,
            &memory,
            &personal,
            &gmail,
            &audit,
            AssistantRequest {
                message: "What tasks do I have today?".into(),
                conversation_id: Some(browser.conversation_id),
            },
        )
        .expect("list tasks");
        assert_eq!(listed.personal_tasks.len(), 1);
        assert_eq!(
            listed.personal_tasks[0].title,
            "Review internship applications"
        );
    }

    #[test]
    fn personal_capabilities_are_grounded_and_mutations_require_a_separate_click() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("assistant.db");
        let personal = PersonalStore::open(path.clone()).unwrap();
        let gmail = GmailService::open(path.clone()).unwrap();
        let audit = AssistantAuditStore::open(path).unwrap();
        let now = chrono::TimeZone::with_ymd_and_hms(&Local, 2026, 10, 21, 12, 0, 0).unwrap();
        let (task, reminder) = personal
            .create_reminder_task("Review proposal", now + chrono::Duration::hours(1))
            .unwrap();
        let execute = |message: &str| {
            execute_supervisor(
                &personal,
                &gmail,
                &audit,
                crate::supervisor::decide(message, now).unwrap(),
            )
            .unwrap()
        };
        let listed = execute("Show all my tasks");
        assert_eq!(listed.personal_tasks[0].id, task.id);
        assert!(listed.local_only);
        assert_eq!(execute("Show my reminders").reminders[0].id, reminder.id);
        let preview = execute("Complete task Review proposal");
        assert_eq!(preview.personal_tasks[0].id, task.id);
        assert_eq!(preview.plan_steps[0].status, "awaiting-confirmation");
        assert!(preview.actions.is_empty());
        let preview = execute("Cancel reminder Review proposal");
        assert_eq!(preview.reminders[0].id, reminder.id);
        assert_eq!(personal.dashboard(now).unwrap().tasks.len(), 1);
        assert_eq!(personal.dashboard(now).unwrap().reminders.len(), 1);
        assert!(execute("Complete task Review").personal_tasks.is_empty());
        personal
            .create_reminder_task("Review proposal", now + chrono::Duration::hours(2))
            .unwrap();
        let ambiguous = execute("Complete task Review proposal");
        assert!(ambiguous.answer.contains("More than one"));
        assert!(ambiguous.personal_tasks.is_empty());
        assert!(execute("Cancel reminder Review proposal")
            .reminders
            .is_empty());
        assert_eq!(personal.dashboard(now).unwrap().tasks.len(), 2);
        assert_eq!(personal.dashboard(now).unwrap().reminders.len(), 2);
    }
}
