use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone, Timelike, Utc, Weekday};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

const CREDENTIAL_TARGET: &str = "Sharman.GmailOAuth.v1";
const GMAIL_SCOPE: &str = "https://www.googleapis.com/auth/gmail.readonly";
const GMAIL_UNREAD_QUERY: &str = "is:unread";
const GMAIL_RECENT_QUERY: &str = "in:inbox newer_than:14d";
const MAX_CREDENTIAL_FILE_BYTES: u64 = 128 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GmailSearchMode {
    Important,
    NeedsAction,
    Internship,
}

impl GmailSearchMode {
    fn query(self) -> &'static str {
        match self {
            Self::Important => "newer_than:14d (is:important OR is:starred) -category:promotions",
            Self::NeedsAction => "newer_than:21d (is:important OR is:starred OR label:inbox) -category:promotions",
            Self::Internship => "newer_than:120d (internship OR assessment OR \"online assessment\" OR interview OR application OR opportunity)",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Important => "important recent email",
            Self::NeedsAction => "email that may need action",
            Self::Internship => "internship and assessment email",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EmailActionType {
    Assessment,
    Interview,
    Application,
    Deadline,
    FollowUp,
    Review,
}

impl EmailActionType {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Assessment => "assessment",
            Self::Interview => "interview",
            Self::Application => "application",
            Self::Deadline => "deadline",
            Self::FollowUp => "followUp",
            Self::Review => "review",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "assessment" => Self::Assessment,
            "interview" => Self::Interview,
            "application" => Self::Application,
            "deadline" => Self::Deadline,
            "followUp" => Self::FollowUp,
            _ => Self::Review,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailAction {
    pub id: String,
    pub message_id: String,
    pub thread_id: String,
    pub action_type: EmailActionType,
    pub title: String,
    pub deadline: Option<String>,
    pub company: Option<String>,
    pub confidence: f32,
    pub evidence: String,
    pub sender: String,
    pub received_at: String,
    pub state: String,
    pub reminder_created: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailMessageSummary {
    pub id: String,
    pub thread_id: String,
    pub subject: String,
    pub sender: String,
    pub snippet: String,
    pub received_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailMessageSearchResult {
    pub query: String,
    pub scanned: usize,
    pub messages: Vec<GmailMessageSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailScanSummary {
    pub mode: GmailSearchMode,
    pub scanned: usize,
    pub matched: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailConnectionStatus {
    pub connected: bool,
    pub account_email: Option<String>,
    pub last_sync_at: Option<String>,
    pub sync: GmailSyncState,
    pub credential_storage: String,
    pub setup_hint: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GmailSyncState {
    pub account_id: Option<String>,
    pub last_history_id: Option<String>,
    pub last_successful_sync: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxDashboard {
    pub status: GmailConnectionStatus,
    pub needs_action: Vec<EmailAction>,
    pub upcoming_deadlines: Vec<EmailAction>,
    pub dismissed: Vec<EmailAction>,
    pub generated_at: String,
    pub scan: Option<GmailScanSummary>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct StoredOAuthCredential {
    client_id: String,
    client_secret: String,
    refresh_token: String,
}

trait SecretVault: Send + Sync {
    fn read(&self, target: &str) -> Result<Option<String>, String>;
    fn write(&self, target: &str, value: &str) -> Result<(), String>;
    fn delete(&self, target: &str) -> Result<(), String>;
}

pub struct GmailService {
    path: PathBuf,
    vault: Arc<dyn SecretVault>,
}

impl GmailService {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        Self::open_with_vault(path, default_vault())
    }

    fn open_with_vault(path: PathBuf, vault: Arc<dyn SecretVault>) -> Result<Self, String> {
        let service = Self { path, vault };
        service
            .connection()?
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 CREATE TABLE IF NOT EXISTS gmail_connection_state (
                    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                    account_email TEXT,
                    connected_at TEXT,
                    last_sync_at TEXT,
                    last_history_id TEXT,
                    last_successful_sync TEXT,
                    last_error TEXT
                 );
                 CREATE TABLE IF NOT EXISTS gmail_action_items (
                    id TEXT PRIMARY KEY,
                    message_id TEXT NOT NULL UNIQUE,
                    identity_key TEXT,
                    thread_id TEXT NOT NULL,
                    action_type TEXT NOT NULL,
                    title TEXT NOT NULL,
                    deadline TEXT,
                    company TEXT,
                    confidence REAL NOT NULL,
                    evidence TEXT NOT NULL,
                    sender TEXT NOT NULL,
                    received_at TEXT NOT NULL,
                    state TEXT NOT NULL,
                    reminder_created INTEGER NOT NULL DEFAULT 0,
                    updated_at TEXT NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS idx_gmail_actions_state_deadline
                    ON gmail_action_items(state, deadline);",
            )
            .map_err(|error| format!("Unable to initialize Inbox Intelligence: {error}"))?;
        let connection = service.connection()?;
        ensure_column(
            &connection,
            "gmail_connection_state",
            "last_history_id",
            "TEXT",
        )?;
        ensure_column(
            &connection,
            "gmail_connection_state",
            "last_successful_sync",
            "TEXT",
        )?;
        ensure_column(&connection, "gmail_connection_state", "last_error", "TEXT")?;
        ensure_column(&connection, "gmail_action_items", "identity_key", "TEXT")?;
        connection
            .execute(
                "UPDATE gmail_action_items SET identity_key = id WHERE identity_key IS NULL OR identity_key = ''",
                [],
            )
            .map_err(|error| format!("Unable to migrate Gmail action identities: {error}"))?;
        connection
            .execute_batch(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_gmail_actions_identity
                    ON gmail_action_items(identity_key);",
            )
            .map_err(|error| format!("Unable to index Gmail action identities: {error}"))?;
        Ok(service)
    }

    fn connection(&self) -> Result<Connection, String> {
        let connection = Connection::open(&self.path)
            .map_err(|error| format!("Unable to open Inbox Intelligence storage: {error}"))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| format!("Unable to configure Inbox Intelligence storage: {error}"))?;
        Ok(connection)
    }

    pub fn status(&self) -> Result<GmailConnectionStatus, String> {
        let has_credential = self.vault.read(CREDENTIAL_TARGET)?.is_some();
        let connection = self.connection()?;
        let state = connection
            .query_row(
                "SELECT account_email, last_sync_at, last_history_id,
                        last_successful_sync, last_error
                 FROM gmail_connection_state WHERE singleton = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("Unable to read Gmail connection state: {error}"))?;
        Ok(GmailConnectionStatus {
            connected: has_credential,
            account_email: state.as_ref().and_then(|value| value.0.clone()),
            last_sync_at: state.as_ref().and_then(|value| value.1.clone()),
            sync: GmailSyncState {
                account_id: state.as_ref().and_then(|value| value.0.clone()),
                last_history_id: state.as_ref().and_then(|value| value.2.clone()),
                last_successful_sync: state.as_ref().and_then(|value| value.3.clone()),
                last_error: state.and_then(|value| value.4),
            },
            credential_storage: "Windows Credential Manager".to_owned(),
            setup_hint: "Choose the Desktop OAuth client JSON downloaded from Google Cloud. Sharman requests Gmail read-only access.".to_owned(),
        })
    }

    pub fn dashboard(&self) -> Result<InboxDashboard, String> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, message_id, thread_id, action_type, title, deadline, company,
                        confidence, evidence, sender, received_at, state, reminder_created
                 FROM gmail_action_items
                 ORDER BY CASE WHEN deadline IS NULL THEN 1 ELSE 0 END, deadline, received_at DESC
                 LIMIT 100",
            )
            .map_err(|error| format!("Unable to read the Action Inbox: {error}"))?;
        let actions = statement
            .query_map([], email_action_from_row)
            .map_err(|error| format!("Unable to read the Action Inbox: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Unable to decode the Action Inbox: {error}"))?;
        let needs_action = actions
            .iter()
            .filter(|item| item.state == "active")
            .cloned()
            .collect::<Vec<_>>();
        let upcoming_deadlines = needs_action
            .iter()
            .filter(|item| item.deadline.is_some())
            .cloned()
            .collect::<Vec<_>>();
        let dismissed = actions
            .into_iter()
            .filter(|item| item.state == "dismissed")
            .collect::<Vec<_>>();
        Ok(InboxDashboard {
            status: self.status()?,
            needs_action,
            upcoming_deadlines,
            dismissed,
            generated_at: Local::now().to_rfc3339(),
            scan: None,
        })
    }

    pub fn connect_interactive(&self) -> Result<GmailConnectionStatus, String> {
        let path = rfd::FileDialog::new()
            .set_title("Choose Google Desktop OAuth credentials")
            .add_filter("Google OAuth JSON", &["json"])
            .pick_file()
            .ok_or_else(|| {
                "Gmail connection was cancelled before a credentials file was chosen.".to_owned()
            })?;
        let metadata = fs::metadata(&path)
            .map_err(|error| format!("Unable to inspect the OAuth credentials file: {error}"))?;
        if metadata.len() > MAX_CREDENTIAL_FILE_BYTES {
            return Err("The OAuth credentials file is unexpectedly large.".to_owned());
        }
        let config = parse_oauth_config(&path)?;
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|error| format!("Unable to start the private OAuth callback: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("Unable to configure the private OAuth callback: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| format!("Unable to inspect the private OAuth callback: {error}"))?
            .port();
        let redirect_uri = format!("http://127.0.0.1:{port}/oauth/callback");
        let state = random_url_token(24)?;
        let verifier = random_url_token(48)?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut authorization = Url::parse("https://accounts.google.com/o/oauth2/v2/auth")
            .map_err(|error| format!("Unable to prepare Google authorization: {error}"))?;
        authorization.query_pairs_mut().extend_pairs([
            ("client_id", config.client_id.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("response_type", "code"),
            ("scope", GMAIL_SCOPE),
            ("access_type", "offline"),
            ("prompt", "consent"),
            ("state", state.as_str()),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
        ]);
        open_web_url(authorization.as_str())?;
        let code = wait_for_oauth_code(&listener, &state, Duration::from_secs(180))?;
        let token = exchange_authorization_code(&config, &code, &redirect_uri, &verifier)?;
        let refresh_token = token.refresh_token.ok_or_else(|| {
            "Google did not return an offline refresh token. Revoke the previous grant and connect again."
                .to_owned()
        })?;
        let profile = fetch_profile(&token.access_token)?;
        let account_email = profile.email_address;
        let stored = StoredOAuthCredential {
            client_id: config.client_id,
            client_secret: config.client_secret,
            refresh_token,
        };
        self.vault.write(
            CREDENTIAL_TARGET,
            &serde_json::to_string(&stored)
                .map_err(|error| format!("Unable to encode the OAuth credential: {error}"))?,
        )?;
        let now = Utc::now().to_rfc3339();
        self.connection()?
            .execute(
                "INSERT INTO gmail_connection_state
                    (singleton, account_email, connected_at, last_sync_at, last_history_id,
                     last_successful_sync, last_error)
                 VALUES (1, ?1, ?2, NULL, ?3, NULL, NULL)
                 ON CONFLICT(singleton) DO UPDATE SET
                    account_email = excluded.account_email,
                    connected_at = excluded.connected_at,
                    last_history_id = excluded.last_history_id,
                    last_error = NULL",
                params![account_email, now, profile.history_id],
            )
            .map_err(|error| format!("Unable to save Gmail connection state: {error}"))?;
        self.status()
    }

    pub fn disconnect(&self) -> Result<GmailConnectionStatus, String> {
        self.vault.delete(CREDENTIAL_TARGET)?;
        self.status()
    }

    pub fn refresh(&self, mode: GmailSearchMode) -> Result<InboxDashboard, String> {
        match self.refresh_inner(mode) {
            Ok(dashboard) => Ok(dashboard),
            Err(error) => {
                let sanitized = bounded(&error, 300);
                let _ = self.connection().and_then(|connection| {
                    connection
                        .execute(
                            "INSERT INTO gmail_connection_state(singleton, connected_at, last_error)
                             VALUES (1, ?1, ?2)
                             ON CONFLICT(singleton) DO UPDATE SET last_error = excluded.last_error",
                            params![Utc::now().to_rfc3339(), sanitized],
                        )
                        .map(|_| ())
                        .map_err(|failure| format!("Unable to record the Gmail sync failure: {failure}"))
                });
                Err(error)
            }
        }
    }

    fn refresh_inner(&self, mode: GmailSearchMode) -> Result<InboxDashboard, String> {
        let access_token = self.access_token()?;
        let last_history_id = self.status()?.sync.last_history_id;
        let message_ids = match last_history_id.as_deref() {
            Some(history_id) => list_history_message_ids(&access_token, history_id)
                .or_else(|_| list_message_ids(&access_token, mode.query()))?,
            None => list_message_ids(&access_token, mode.query())?,
        };
        let mut scanned = 0;
        let mut matched = 0;
        for message_id in message_ids.into_iter().take(30) {
            let message = fetch_message_metadata(&access_token, &message_id)?;
            scanned += 1;
            if let Some(action) = extract_email_action(&message, mode, Local::now()) {
                self.upsert_action(&action)?;
                matched += 1;
            }
        }
        let profile = fetch_profile(&access_token)?;
        let now = Utc::now().to_rfc3339();
        self.connection()?
            .execute(
                "INSERT INTO gmail_connection_state
                    (singleton, account_email, connected_at, last_sync_at, last_history_id,
                     last_successful_sync, last_error)
                 VALUES (1, ?1, ?2, ?2, ?3, ?2, NULL)
                 ON CONFLICT(singleton) DO UPDATE SET
                    account_email = excluded.account_email,
                    last_sync_at = excluded.last_sync_at,
                    last_history_id = excluded.last_history_id,
                    last_successful_sync = excluded.last_successful_sync,
                    last_error = NULL",
                params![profile.email_address, now, profile.history_id],
            )
            .map_err(|error| format!("Unable to update the Gmail checkpoint: {error}"))?;
        let mut dashboard = self.dashboard()?;
        dashboard.scan = Some(GmailScanSummary {
            mode,
            scanned,
            matched,
        });
        Ok(dashboard)
    }

    pub fn list_unread(&self) -> Result<GmailMessageSearchResult, String> {
        self.list_messages(GMAIL_UNREAD_QUERY)
    }

    pub fn list_recent(&self) -> Result<GmailMessageSearchResult, String> {
        self.list_messages(GMAIL_RECENT_QUERY)
    }

    fn list_messages(&self, query: &str) -> Result<GmailMessageSearchResult, String> {
        let access_token = self.access_token()?;
        let message_ids = list_message_ids(&access_token, query)?;
        let mut messages = Vec::new();
        for message_id in message_ids.into_iter().take(30) {
            let message = fetch_message_metadata(&access_token, &message_id)?;
            messages.push(message_summary(&message));
        }
        messages.sort_by(|left, right| right.received_at.cmp(&left.received_at));
        let now = Utc::now().to_rfc3339();
        self.connection()?
            .execute(
                "INSERT INTO gmail_connection_state
                    (singleton, connected_at, last_sync_at, last_successful_sync, last_error)
                 VALUES (1, ?1, ?1, ?1, NULL)
                 ON CONFLICT(singleton) DO UPDATE SET
                    last_sync_at = excluded.last_sync_at,
                    last_successful_sync = excluded.last_successful_sync,
                    last_error = NULL",
                [&now],
            )
            .map_err(|error| format!("Unable to update the Gmail read checkpoint: {error}"))?;
        Ok(GmailMessageSearchResult {
            query: query.to_owned(),
            scanned: messages.len(),
            messages,
        })
    }

    pub fn dismiss_action(&self, action_id: &str) -> Result<InboxDashboard, String> {
        let changed = self
            .connection()?
            .execute(
                "UPDATE gmail_action_items SET state = 'dismissed', updated_at = ?2 WHERE id = ?1",
                params![action_id, Utc::now().to_rfc3339()],
            )
            .map_err(|error| format!("Unable to dismiss the email action: {error}"))?;
        if changed == 0 {
            return Err("The email action no longer exists.".to_owned());
        }
        self.dashboard()
    }

    pub fn action(&self, action_id: &str) -> Result<EmailAction, String> {
        self.connection()?
            .query_row(
                "SELECT id, message_id, thread_id, action_type, title, deadline, company,
                        confidence, evidence, sender, received_at, state, reminder_created
                 FROM gmail_action_items WHERE id = ?1",
                [action_id],
                email_action_from_row,
            )
            .optional()
            .map_err(|error| format!("Unable to read the email action: {error}"))?
            .ok_or_else(|| "The email action no longer exists.".to_owned())
    }

    pub fn mark_reminder_created(&self, action_id: &str) -> Result<(), String> {
        self.connection()?
            .execute(
                "UPDATE gmail_action_items SET reminder_created = 1, updated_at = ?2 WHERE id = ?1",
                params![action_id, Utc::now().to_rfc3339()],
            )
            .map_err(|error| format!("Unable to link the reminder to the email action: {error}"))?;
        Ok(())
    }

    fn access_token(&self) -> Result<String, String> {
        let stored = self.vault.read(CREDENTIAL_TARGET)?.ok_or_else(|| {
            "Gmail is not connected. Use Connect Gmail in the Action Inbox first.".to_owned()
        })?;
        let credential = serde_json::from_str::<StoredOAuthCredential>(&stored).map_err(|_| {
            "The Gmail credential in Windows Credential Manager is unreadable.".to_owned()
        })?;
        refresh_access_token(&credential)
    }

    fn upsert_action(&self, action: &EmailAction) -> Result<(), String> {
        self.connection()?
            .execute(
                "INSERT INTO gmail_action_items
                 (id, message_id, identity_key, thread_id, action_type, title, deadline, company, confidence,
                  evidence, sender, received_at, state, reminder_created, updated_at)
                 VALUES (?1, ?2, ?1, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'active', 0, ?12)
                 ON CONFLICT(message_id) DO UPDATE SET
                    id = excluded.id,
                    identity_key = excluded.identity_key,
                    thread_id = excluded.thread_id,
                    action_type = excluded.action_type,
                    title = excluded.title,
                    deadline = excluded.deadline,
                    company = excluded.company,
                    confidence = excluded.confidence,
                    evidence = excluded.evidence,
                    sender = excluded.sender,
                    received_at = excluded.received_at,
                    updated_at = excluded.updated_at",
                params![
                    action.id,
                    action.message_id,
                    action.thread_id,
                    action.action_type.as_str(),
                    action.title,
                    action.deadline,
                    action.company,
                    action.confidence,
                    action.evidence,
                    action.sender,
                    action.received_at,
                    Utc::now().to_rfc3339()
                ],
            )
            .map_err(|error| format!("Unable to update the Action Inbox: {error}"))?;
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct OAuthFile {
    installed: Option<OAuthClient>,
}

#[derive(Debug, Deserialize)]
struct OAuthClient {
    client_id: String,
    #[serde(default)]
    client_secret: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GmailProfile {
    #[serde(rename = "emailAddress")]
    email_address: String,
    #[serde(rename = "historyId")]
    history_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GmailMessageList {
    #[serde(default)]
    messages: Vec<GmailMessageId>,
}

#[derive(Debug, Deserialize)]
struct GmailMessageId {
    id: String,
}

#[derive(Debug, Default, Deserialize)]
struct GmailHistoryList {
    #[serde(default)]
    history: Vec<GmailHistory>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct GmailHistory {
    #[serde(rename = "messagesAdded", default)]
    messages_added: Vec<GmailMessageAdded>,
}

#[derive(Debug, Deserialize)]
struct GmailMessageAdded {
    message: GmailMessageId,
}

#[derive(Debug, Deserialize)]
struct GmailMessage {
    id: String,
    #[serde(rename = "threadId")]
    thread_id: String,
    #[serde(default)]
    snippet: String,
    #[serde(rename = "internalDate")]
    internal_date: Option<String>,
    #[serde(default)]
    payload: GmailPayload,
}

#[derive(Debug, Default, Deserialize)]
struct GmailPayload {
    #[serde(default)]
    headers: Vec<GmailHeader>,
}

#[derive(Debug, Deserialize)]
struct GmailHeader {
    name: String,
    value: String,
}

fn parse_oauth_config(path: &Path) -> Result<OAuthClient, String> {
    let content = fs::read_to_string(path)
        .map_err(|error| format!("Unable to read the OAuth credentials file: {error}"))?;
    let file = serde_json::from_str::<OAuthFile>(&content)
        .map_err(|_| "Choose a Google OAuth Desktop client JSON file.".to_owned())?;
    let client = file
        .installed
        .ok_or_else(|| "The OAuth JSON is not a Google Desktop client configuration.".to_owned())?;
    if !client.client_id.ends_with(".apps.googleusercontent.com") {
        return Err("The OAuth client ID is not a Google installed-app client.".to_owned());
    }
    Ok(client)
}

fn random_url_token(size: usize) -> Result<String, String> {
    let mut bytes = vec![0_u8; size];
    getrandom::fill(&mut bytes)
        .map_err(|error| format!("Unable to create secure OAuth state: {error}"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn wait_for_oauth_code(
    listener: &TcpListener,
    expected_state: &str,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match listener.accept() {
            Ok((mut stream, address)) => {
                if !address.ip().is_loopback() {
                    continue;
                }
                stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
                let mut request = [0_u8; 8192];
                let size = stream
                    .read(&mut request)
                    .map_err(|error| format!("Unable to read the OAuth callback: {error}"))?;
                let first_line = String::from_utf8_lossy(&request[..size])
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_owned();
                let target = first_line
                    .strip_prefix("GET ")
                    .and_then(|value| value.split_whitespace().next())
                    .ok_or_else(|| "Google returned an invalid OAuth callback.".to_owned())?;
                let callback = Url::parse(&format!("http://127.0.0.1{target}"))
                    .map_err(|_| "Google returned an invalid OAuth callback address.".to_owned())?;
                let pairs = callback
                    .query_pairs()
                    .collect::<std::collections::HashMap<_, _>>();
                let state = pairs
                    .get("state")
                    .map(|value| value.as_ref())
                    .unwrap_or_default();
                let code = pairs.get("code").map(|value| value.to_string());
                let error = pairs.get("error").map(|value| value.to_string());
                let accepted = state == expected_state && code.is_some();
                let body = if accepted {
                    "<h1>Gmail connected</h1><p>You can return to Sharman.</p>"
                } else {
                    "<h1>Gmail was not connected</h1><p>Return to Sharman and try again.</p>"
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                if state != expected_state {
                    return Err(
                        "OAuth state validation failed; no credential was stored.".to_owned()
                    );
                }
                if let Some(error) = error {
                    return Err(format!("Google authorization was not completed: {error}"));
                }
                return code
                    .ok_or_else(|| "Google did not return an authorization code.".to_owned());
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(format!("The OAuth callback failed: {error}")),
        }
    }
    Err("Gmail connection timed out after three minutes.".to_owned())
}

fn exchange_authorization_code(
    config: &OAuthClient,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Result<TokenResponse, String> {
    let mut form = url::form_urlencoded::Serializer::new(String::new());
    form.append_pair("client_id", &config.client_id)
        .append_pair("client_secret", &config.client_secret)
        .append_pair("code", code)
        .append_pair("code_verifier", verifier)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("grant_type", "authorization_code");
    post_token_form(&form.finish(), false)
}

fn refresh_access_token(credential: &StoredOAuthCredential) -> Result<String, String> {
    let mut form = url::form_urlencoded::Serializer::new(String::new());
    form.append_pair("client_id", &credential.client_id)
        .append_pair("client_secret", &credential.client_secret)
        .append_pair("refresh_token", &credential.refresh_token)
        .append_pair("grant_type", "refresh_token");
    Ok(post_token_form(&form.finish(), true)?.access_token)
}

fn post_token_form(form: &str, refreshing: bool) -> Result<TokenResponse, String> {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(20))
        .timeout_write(Duration::from_secs(10))
        .build()
        .post("https://oauth2.googleapis.com/token")
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string(form)
        .map_err(|error| match error {
            ureq::Error::Status(status, _) if refreshing && matches!(status, 400 | 401) => {
                "Gmail authorization needs reconnecting. Open the Action Inbox, disconnect Gmail, and connect it again.".to_owned()
            }
            ureq::Error::Status(status, _) => {
                format!("Google OAuth rejected the token request (HTTP {status}).")
            }
            ureq::Error::Transport(error) => format!(
                "Google OAuth could not be reached. Check the network and try again; existing inbox data is preserved. ({})",
                bounded(&error.to_string(), 180)
            ),
        })?
        .into_json::<TokenResponse>()
        .map_err(|error| format!("Google OAuth returned an unreadable token response: {error}"))
}

fn fetch_profile(access_token: &str) -> Result<GmailProfile, String> {
    gmail_get(
        "https://gmail.googleapis.com/gmail/v1/users/me/profile",
        access_token,
    )?
    .into_json::<GmailProfile>()
    .map_err(|error| format!("Unable to read the connected Gmail profile: {error}"))
}

fn list_message_ids(access_token: &str, query: &str) -> Result<Vec<String>, String> {
    let mut url = Url::parse("https://gmail.googleapis.com/gmail/v1/users/me/messages")
        .map_err(|error| format!("Unable to prepare the Gmail search: {error}"))?;
    url.query_pairs_mut()
        .append_pair("q", query)
        .append_pair("maxResults", "30");
    gmail_get(url.as_str(), access_token)?
        .into_json::<GmailMessageList>()
        .map(|response| response.messages.into_iter().map(|item| item.id).collect())
        .map_err(|error| format!("Gmail returned an unreadable search result: {error}"))
}

fn list_history_message_ids(
    access_token: &str,
    start_history_id: &str,
) -> Result<Vec<String>, String> {
    if !safe_gmail_id(start_history_id) {
        return Err("The saved Gmail history checkpoint is invalid.".to_owned());
    }
    let mut page_token = None::<String>;
    let mut ids = HashSet::new();
    for _ in 0..5 {
        let mut url = Url::parse("https://gmail.googleapis.com/gmail/v1/users/me/history")
            .map_err(|error| format!("Unable to prepare incremental Gmail sync: {error}"))?;
        url.query_pairs_mut()
            .append_pair("startHistoryId", start_history_id)
            .append_pair("historyTypes", "messageAdded")
            .append_pair("maxResults", "100");
        if let Some(token) = page_token.as_deref() {
            url.query_pairs_mut().append_pair("pageToken", token);
        }
        let response = gmail_get(url.as_str(), access_token)?
            .into_json::<GmailHistoryList>()
            .map_err(|error| format!("Gmail returned unreadable history data: {error}"))?;
        for entry in response.history {
            ids.extend(entry.messages_added.into_iter().map(|item| item.message.id));
        }
        page_token = response.next_page_token;
        if page_token.is_none() {
            break;
        }
    }
    Ok(ids.into_iter().collect())
}

fn fetch_message_metadata(access_token: &str, message_id: &str) -> Result<GmailMessage, String> {
    if !safe_gmail_id(message_id) {
        return Err("Gmail returned an invalid message identifier.".to_owned());
    }
    let mut url = Url::parse(&format!(
        "https://gmail.googleapis.com/gmail/v1/users/me/messages/{message_id}"
    ))
    .map_err(|error| format!("Unable to prepare the Gmail message request: {error}"))?;
    url.query_pairs_mut()
        .append_pair("format", "metadata")
        .append_pair("metadataHeaders", "Subject")
        .append_pair("metadataHeaders", "From")
        .append_pair("metadataHeaders", "Date");
    gmail_get(url.as_str(), access_token)?
        .into_json::<GmailMessage>()
        .map_err(|error| format!("Gmail returned unreadable message metadata: {error}"))
}

fn gmail_get(url: &str, access_token: &str) -> Result<ureq::Response, String> {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(20))
        .build()
        .get(url)
        .set("Authorization", &format!("Bearer {access_token}"))
        .set("Accept", "application/json")
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(401 | 403, _) => {
                "Gmail authorization needs reconnecting. Open the Action Inbox, disconnect Gmail, and connect it again.".to_owned()
            }
            ureq::Error::Status(status, _) => format!(
                "Gmail API returned HTTP {status}. This is recoverable; existing inbox data is preserved."
            ),
            ureq::Error::Transport(error) => format!(
                "Gmail could not be reached. Check the network and try again; existing inbox data is preserved. ({})",
                bounded(&error.to_string(), 180)
            ),
        })
}

fn extract_email_action(
    message: &GmailMessage,
    mode: GmailSearchMode,
    now: DateTime<Local>,
) -> Option<EmailAction> {
    let subject = header(&message.payload, "Subject").unwrap_or("Untitled email");
    let sender = header(&message.payload, "From").unwrap_or("Unknown sender");
    if is_non_actionable_noise(subject, sender, &message.snippet) {
        return None;
    }
    let combined = format!("{} {}", subject, message.snippet);
    let lower = combined.to_lowercase();
    let action_type = if contains_any(&lower, &["online assessment", "assessment", " oa "]) {
        EmailActionType::Assessment
    } else if lower.contains("interview") {
        EmailActionType::Interview
    } else if contains_any(&lower, &["application", "internship", "opportunity"])
        && mode == GmailSearchMode::Internship
    {
        EmailActionType::Application
    } else if contains_any(&lower, &["deadline", "due by", "due on", "expires"]) {
        EmailActionType::Deadline
    } else if contains_any(
        &lower,
        &[
            "action required",
            "please complete",
            "respond",
            "reply",
            "confirm",
            "meeting moved",
            "meeting rescheduled",
            "interview moved",
            "interview rescheduled",
        ],
    ) {
        EmailActionType::FollowUp
    } else if mode == GmailSearchMode::Internship {
        EmailActionType::Review
    } else {
        return None;
    };
    let (deadline, date_confidence) = extract_deadline(&combined, now);
    if deadline
        .as_ref()
        .is_some_and(|value| *value < now.with_timezone(&Utc))
    {
        return None;
    }
    let mut confidence = 0.58_f32 + date_confidence;
    if action_type != EmailActionType::Review {
        confidence += 0.12;
    }
    let company = company_from_sender(sender);
    if company.is_some() {
        confidence += 0.05;
    }
    let received_at = message
        .internal_date
        .as_deref()
        .and_then(|value| value.parse::<i64>().ok())
        .and_then(DateTime::<Utc>::from_timestamp_millis)
        .unwrap_or_else(Utc::now)
        .to_rfc3339();
    let deadline = deadline.map(|value| value.to_rfc3339());
    let id = stable_action_id(&message.id, action_type.as_str(), deadline.as_deref());
    Some(EmailAction {
        id,
        message_id: message.id.clone(),
        thread_id: message.thread_id.clone(),
        action_type,
        title: bounded(subject.trim(), 180),
        deadline,
        company,
        confidence: confidence.min(0.98),
        evidence: bounded(message.snippet.trim(), 300),
        sender: bounded(sender.trim(), 240),
        received_at,
        state: "active".to_owned(),
        reminder_created: false,
    })
}

fn extract_deadline(text: &str, now: DateTime<Local>) -> (Option<DateTime<Utc>>, f32) {
    let lower = text.to_lowercase();
    if let Some(hours) = relative_hours(&lower) {
        return (
            Some((now + chrono::Duration::hours(hours)).with_timezone(&Utc)),
            0.24,
        );
    }
    let clock = extract_clock_time(&lower);
    let normalized = text
        .replace([',', '.', '(', ')', '[', ']'], " ")
        .replace('/', " ");
    let tokens = normalized.split_whitespace().collect::<Vec<_>>();
    let mut candidates = Vec::<(usize, NaiveDate)>::new();
    for (index, token) in tokens.iter().enumerate() {
        if let Ok(date) = NaiveDate::parse_from_str(token, "%Y-%m-%d") {
            candidates.push((index, date));
        }
    }
    for (index, token) in tokens.iter().enumerate() {
        let Some(month) = month_number(token) else {
            continue;
        };
        let before = index
            .checked_sub(1)
            .and_then(|position| parse_day(tokens[position]));
        let after = tokens.get(index + 1).and_then(|value| parse_day(value));
        let day = before.or(after);
        let Some(day) = day else { continue };
        let year_token = if before.is_some() {
            tokens.get(index + 1)
        } else {
            tokens.get(index + 2)
        };
        let year_candidate = year_token
            .and_then(|value| value.parse::<i32>().ok())
            .filter(|year| (1900..=now.year() + 2).contains(year));
        let mut year = year_candidate.unwrap_or(now.year());
        let Some(mut date) = NaiveDate::from_ymd_opt(year, month, day) else {
            continue;
        };
        if date < now.date_naive() && year_candidate.is_none() {
            year += 1;
            if let Some(next) = NaiveDate::from_ymd_opt(year, month, day) {
                date = next;
            }
        }
        candidates.push((index, date));
    }
    if let Some((_, date)) = candidates
        .into_iter()
        .min_by_key(|(index, _)| deadline_keyword_distance(&tokens, *index))
    {
        return (local_date_time(date, clock), 0.25);
    }
    if lower.contains("tomorrow") {
        let date = now.date_naive() + chrono::Duration::days(1);
        return (local_date_time(date, clock), 0.12);
    }
    if let Some(weekday) = weekday_from_text(&lower) {
        let current = now.weekday().num_days_from_monday() as i64;
        let target = weekday.num_days_from_monday() as i64;
        let mut days = (target - current).rem_euclid(7);
        if days == 0
            && clock.is_some_and(|(hour, minute)| (hour, minute) <= (now.hour(), now.minute()))
        {
            days = 7;
        }
        let date = now.date_naive() + chrono::Duration::days(days);
        return (local_date_time(date, clock), 0.16);
    }
    if lower.contains("today") && contains_any(&lower, &["deadline", "due", "expires"]) {
        return (local_date_time(now.date_naive(), clock), 0.12);
    }
    (None, 0.0)
}

fn weekday_from_text(text: &str) -> Option<Weekday> {
    text.split(|character: char| !character.is_ascii_alphabetic())
        .find_map(|token| match token {
            "monday" | "mon" => Some(Weekday::Mon),
            "tuesday" | "tue" | "tues" => Some(Weekday::Tue),
            "wednesday" | "wed" => Some(Weekday::Wed),
            "thursday" | "thu" | "thur" | "thurs" => Some(Weekday::Thu),
            "friday" | "fri" => Some(Weekday::Fri),
            "saturday" | "sat" => Some(Weekday::Sat),
            "sunday" | "sun" => Some(Weekday::Sun),
            _ => None,
        })
}

fn local_date_time(date: NaiveDate, clock: Option<(u32, u32)>) -> Option<DateTime<Utc>> {
    let (hour, minute) = clock.unwrap_or((23, 59));
    Local
        .from_local_datetime(&date.and_hms_opt(hour, minute, 0)?)
        .earliest()
        .map(|value| value.with_timezone(&Utc))
}

fn relative_hours(text: &str) -> Option<i64> {
    let tokens = text
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    tokens.windows(3).find_map(|window| {
        (window[0] == "within" && matches!(window[2], "hour" | "hours"))
            .then(|| window[1].parse::<i64>().ok())
            .flatten()
            .filter(|hours| (1..=24 * 30).contains(hours))
    })
}

fn extract_clock_time(text: &str) -> Option<(u32, u32)> {
    let normalized = text.replace([',', '.', '(', ')', '[', ']'], " ");
    let tokens = normalized.split_whitespace().collect::<Vec<_>>();
    let mut clocks = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let compact = token
            .trim_matches(|character: char| !character.is_ascii_alphanumeric() && character != ':');
        let (time, suffix) = if let Some(value) = compact.strip_suffix("am") {
            (value, Some("am"))
        } else if let Some(value) = compact.strip_suffix("pm") {
            (value, Some("pm"))
        } else {
            (
                compact,
                tokens
                    .get(index + 1)
                    .copied()
                    .filter(|value| matches!(*value, "am" | "pm")),
            )
        };
        let Some(suffix) = suffix else { continue };
        let mut parts = time.split(':');
        let Some(hour) = parts.next().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        let minute = match parts.next() {
            Some(value) => {
                let Some(value) = value.parse::<u32>().ok() else {
                    continue;
                };
                value
            }
            None => 0,
        };
        if !(1..=12).contains(&hour) || minute > 59 {
            continue;
        }
        let hour = match (suffix, hour) {
            ("am", 12) => 0,
            ("pm", 12) => 12,
            ("pm", value) => value + 12,
            (_, value) => value,
        };
        clocks.push((hour, minute));
    }
    if contains_any(text, &["moved from", "rescheduled from", "changed from"]) {
        clocks.last().copied()
    } else {
        clocks.first().copied()
    }
}

fn deadline_keyword_distance(tokens: &[&str], candidate_index: usize) -> usize {
    const KEYWORDS: &[&str] = &[
        "by", "before", "deadline", "due", "close", "closes", "expires", "complete", "reply",
        "submit",
    ];
    tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| {
            let normalized = token
                .trim_matches(|character: char| !character.is_ascii_alphabetic())
                .to_ascii_lowercase();
            KEYWORDS.contains(&normalized.as_str())
        })
        .map(|(index, _)| {
            let direction_penalty = usize::from(candidate_index < index) * 4;
            index.abs_diff(candidate_index) + direction_penalty
        })
        .min()
        .unwrap_or(usize::MAX / 2)
}

fn is_non_actionable_noise(subject: &str, sender: &str, snippet: &str) -> bool {
    let subject = subject.to_ascii_lowercase();
    let sender = sender.to_ascii_lowercase();
    let combined = format!("{subject} {}", snippet.to_ascii_lowercase());
    contains_any(
        &combined,
        &[
            "one-time password",
            "one time password",
            "verification code",
            "security code",
            "your otp",
            "login code",
            "two-factor code",
            "payment receipt",
            "order confirmation",
            "shipping confirmation",
            "your receipt from",
            "invoice #",
        ],
    ) || contains_any(
        &subject,
        &[
            "newsletter",
            "weekly digest",
            "daily digest",
            "flash sale",
            "limited-time offer",
            "promotional offer",
            "special offer",
            "receipt",
            "invoice",
            "order confirmed",
            "payment received",
            "subscription renewal",
        ],
    ) || sender.contains("newsletter@")
        || sender.contains("receipts@")
        || sender.contains("billing@")
        || sender.contains("notifications@github.com")
        || sender.contains("noreply@github.com")
}

fn month_number(value: &str) -> Option<u32> {
    match value.to_ascii_lowercase().as_str() {
        "jan" | "january" => Some(1),
        "feb" | "february" => Some(2),
        "mar" | "march" => Some(3),
        "apr" | "april" => Some(4),
        "may" => Some(5),
        "jun" | "june" => Some(6),
        "jul" | "july" => Some(7),
        "aug" | "august" => Some(8),
        "sep" | "sept" | "september" => Some(9),
        "oct" | "october" => Some(10),
        "nov" | "november" => Some(11),
        "dec" | "december" => Some(12),
        _ => None,
    }
}

fn parse_day(value: &str) -> Option<u32> {
    value
        .trim_end_matches(|character: char| character.is_ascii_alphabetic())
        .parse::<u32>()
        .ok()
        .filter(|day| (1..=31).contains(day))
}

fn header<'a>(payload: &'a GmailPayload, name: &str) -> Option<&'a str> {
    payload
        .headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str())
}

fn message_summary(message: &GmailMessage) -> GmailMessageSummary {
    let received_at = message
        .internal_date
        .as_deref()
        .and_then(|value| value.parse::<i64>().ok())
        .and_then(DateTime::<Utc>::from_timestamp_millis)
        .unwrap_or_else(Utc::now)
        .to_rfc3339();
    GmailMessageSummary {
        id: message.id.clone(),
        thread_id: message.thread_id.clone(),
        subject: bounded(
            header(&message.payload, "Subject")
                .unwrap_or("Untitled email")
                .trim(),
            180,
        ),
        sender: bounded(
            header(&message.payload, "From")
                .unwrap_or("Unknown sender")
                .trim(),
            240,
        ),
        snippet: bounded(message.snippet.trim(), 300),
        received_at,
    }
}

fn company_from_sender(sender: &str) -> Option<String> {
    let display = sender
        .split('<')
        .next()
        .unwrap_or_default()
        .trim()
        .trim_matches('"');
    (!display.is_empty() && !display.contains('@')).then(|| bounded(display, 100))
}

fn email_action_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EmailAction> {
    Ok(EmailAction {
        id: row.get(0)?,
        message_id: row.get(1)?,
        thread_id: row.get(2)?,
        action_type: EmailActionType::parse(&row.get::<_, String>(3)?),
        title: row.get(4)?,
        deadline: row.get(5)?,
        company: row.get(6)?,
        confidence: row.get(7)?,
        evidence: row.get(8)?,
        sender: row.get(9)?,
        received_at: row.get(10)?,
        state: row.get(11)?,
        reminder_created: row.get::<_, i64>(12)? != 0,
    })
}

pub fn gmail_message_url(thread_id: &str) -> Result<String, String> {
    if !safe_gmail_id(thread_id) {
        return Err("The Gmail thread identifier is invalid.".to_owned());
    }
    Ok(format!("https://mail.google.com/mail/u/0/#all/{thread_id}"))
}

pub fn open_gmail_message(thread_id: &str) -> Result<(), String> {
    open_web_url(&gmail_message_url(thread_id)?)
}

fn safe_gmail_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn bounded(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn stable_action_id(message_id: &str, action_type: &str, deadline: Option<&str>) -> String {
    let mut hash = Sha256::new();
    hash.update(message_id.as_bytes());
    hash.update([0]);
    hash.update(action_type.as_bytes());
    hash.update([0]);
    hash.update(deadline.unwrap_or("none").as_bytes());
    format!("gmail-action-{:x}", hash.finalize())
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), String> {
    if !table
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || !column
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || !definition
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte.is_ascii_whitespace())
    {
        return Err("Unsafe SQLite migration identifier.".to_owned());
    }
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| format!("Unable to inspect Gmail storage migration: {error}"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("Unable to inspect Gmail storage migration: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Unable to decode Gmail storage migration: {error}"))?;
    if columns.iter().any(|existing| existing == column) {
        return Ok(());
    }
    connection
        .execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )
        .map(|_| ())
        .map_err(|error| format!("Unable to upgrade Gmail storage: {error}"))
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundSyncPolicy {
    pub agent_id: String,
    pub interval_seconds: u64,
    pub permission_ceiling: String,
    pub allowed_capabilities: Vec<String>,
    pub forbidden_capabilities: Vec<String>,
}

impl Default for BackgroundSyncPolicy {
    fn default() -> Self {
        Self {
            agent_id: "sharman-background-inbox-agent".to_owned(),
            interval_seconds: 3600,
            permission_ceiling: "READ_AND_SUGGEST".to_owned(),
            allowed_capabilities: vec![
                "gmail.search".to_owned(),
                "gmail.read".to_owned(),
                "inbox.classify".to_owned(),
                "task.suggest".to_owned(),
                "reminder.suggest".to_owned(),
                "notify".to_owned(),
            ],
            forbidden_capabilities: vec![
                "gmail.send".to_owned(),
                "browser.submit".to_owned(),
                "system.exec".to_owned(),
                "delete".to_owned(),
                "purchase".to_owned(),
            ],
        }
    }
}

#[allow(dead_code)]
impl BackgroundSyncPolicy {
    pub fn authorize_capability(&self, capability: &str) -> Result<(), String> {
        let normalized = capability.trim();
        if self
            .forbidden_capabilities
            .iter()
            .any(|item| item == normalized)
        {
            return Err(format!(
                "Background agent '{}' is permanently denied '{}' (ceiling: {}).",
                self.agent_id, normalized, self.permission_ceiling
            ));
        }
        if self
            .allowed_capabilities
            .iter()
            .any(|item| item == normalized)
        {
            return Ok(());
        }
        Err(format!(
            "Background agent '{}' cannot execute undelegated capability '{}' (ceiling: {}).",
            self.agent_id, normalized, self.permission_ceiling
        ))
    }
}

#[cfg(windows)]
fn open_web_url(url: &str) -> Result<(), String> {
    std::process::Command::new("rundll32.exe")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Unable to open Google authorization: {error}"))
}

#[cfg(not(windows))]
fn open_web_url(_url: &str) -> Result<(), String> {
    Err("Gmail OAuth is currently supported on Windows only.".to_owned())
}

#[cfg(windows)]
struct WindowsCredentialVault;

#[cfg(windows)]
impl SecretVault for WindowsCredentialVault {
    fn read(&self, target: &str) -> Result<Option<String>, String> {
        use std::{ffi::c_void, os::windows::ffi::OsStrExt, ptr};
        use windows_sys::Win32::Security::Credentials::{
            CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC,
        };

        let target = std::ffi::OsStr::new(target)
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let mut credential: *mut CREDENTIALW = ptr::null_mut();
        // SAFETY: target is NUL-terminated and credential is an out pointer owned by CredReadW.
        let success = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) };
        if success == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(1168) {
                return Ok(None);
            }
            return Err(format!(
                "Unable to read Windows Credential Manager: {error}"
            ));
        }
        // SAFETY: successful CredReadW returns a valid credential until CredFree is called.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                (*credential).CredentialBlob,
                (*credential).CredentialBlobSize as usize,
            )
            .to_vec()
        };
        // SAFETY: credential was allocated by CredReadW.
        unsafe { CredFree(credential.cast::<c_void>()) };
        String::from_utf8(bytes).map(Some).map_err(|_| {
            "The Gmail credential in Windows Credential Manager is not UTF-8.".to_owned()
        })
    }

    fn write(&self, target: &str, value: &str) -> Result<(), String> {
        use std::{os::windows::ffi::OsStrExt, ptr};
        use windows_sys::Win32::Security::Credentials::{
            CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
        };

        let mut target = std::ffi::OsStr::new(target)
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let mut username = std::ffi::OsStr::new("Sharman Gmail OAuth")
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let mut blob = value.as_bytes().to_vec();
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: username.as_mut_ptr(),
            Attributes: ptr::null_mut(),
            TargetAlias: ptr::null_mut(),
            Comment: ptr::null_mut(),
            ..Default::default()
        };
        // SAFETY: all pointers remain valid for the duration of CredWriteW, which copies the data.
        let success = unsafe { CredWriteW(&credential, 0) };
        blob.fill(0);
        if success == 0 {
            return Err(format!(
                "Unable to store Gmail OAuth in Windows Credential Manager: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }

    fn delete(&self, target: &str) -> Result<(), String> {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};

        let target = std::ffi::OsStr::new(target)
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        // SAFETY: target is a valid NUL-terminated UTF-16 string.
        let success = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
        if success == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(1168) {
                return Err(format!("Unable to remove the Gmail credential: {error}"));
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
fn default_vault() -> Arc<dyn SecretVault> {
    Arc::new(WindowsCredentialVault)
}

#[cfg(not(windows))]
struct UnsupportedVault;

#[cfg(not(windows))]
impl SecretVault for UnsupportedVault {
    fn read(&self, _target: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    fn write(&self, _target: &str, _value: &str) -> Result<(), String> {
        Err("Secure Gmail credential storage is currently supported on Windows only.".to_owned())
    }
    fn delete(&self, _target: &str) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(not(windows))]
fn default_vault() -> Arc<dyn SecretVault> {
    Arc::new(UnsupportedVault)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Timelike};

    #[derive(Default)]
    struct TestVault;

    impl SecretVault for TestVault {
        fn read(&self, _target: &str) -> Result<Option<String>, String> {
            Ok(None)
        }

        fn write(&self, _target: &str, _value: &str) -> Result<(), String> {
            Ok(())
        }

        fn delete(&self, _target: &str) -> Result<(), String> {
            Ok(())
        }
    }

    fn message(subject: &str, snippet: &str) -> GmailMessage {
        message_from(subject, snippet, "Amazon Jobs <jobs@example.com>")
    }

    fn message_from(subject: &str, snippet: &str, sender: &str) -> GmailMessage {
        GmailMessage {
            id: "18abc123".to_owned(),
            thread_id: "18def456".to_owned(),
            snippet: snippet.to_owned(),
            internal_date: Some("1791676800000".to_owned()),
            payload: GmailPayload {
                headers: vec![
                    GmailHeader {
                        name: "Subject".to_owned(),
                        value: subject.to_owned(),
                    },
                    GmailHeader {
                        name: "From".to_owned(),
                        value: sender.to_owned(),
                    },
                ],
            },
        }
    }

    #[test]
    fn extracts_explicit_deadline_with_grounded_evidence() {
        let now = Local.with_ymd_and_hms(2026, 10, 11, 10, 0, 0).unwrap();
        let action = extract_email_action(
            &message(
                "Amazon SDE Online Assessment",
                "Please complete your assessment by 13 Oct 2026.",
            ),
            GmailSearchMode::Internship,
            now,
        )
        .unwrap();
        assert_eq!(action.action_type, EmailActionType::Assessment);
        assert_eq!(action.company.as_deref(), Some("Amazon Jobs"));
        assert!(action
            .deadline
            .as_deref()
            .unwrap()
            .starts_with("2026-10-13"));
        assert!(action.confidence >= 0.9);
        assert!(action.evidence.contains("13 Oct 2026"));
    }

    #[test]
    fn ignores_non_actionable_important_mail() {
        let now = Local.with_ymd_and_hms(2026, 10, 11, 10, 0, 0).unwrap();
        assert!(extract_email_action(
            &message("Weekly newsletter", "Here are this week's stories."),
            GmailSearchMode::Important,
            now,
        )
        .is_none());
    }

    #[test]
    fn gmail_urls_accept_only_bounded_identifiers() {
        assert!(gmail_message_url("18abc_DEF-123").is_ok());
        assert!(gmail_message_url("../../settings").is_err());
        assert!(gmail_message_url("").is_err());
    }

    #[test]
    fn extracts_tomorrow_with_explicit_clock_time() {
        let now = Local.with_ymd_and_hms(2026, 10, 11, 10, 0, 0).unwrap();
        let (deadline, confidence) = extract_deadline("Please finish by 5 PM tomorrow", now);
        let local = deadline.unwrap().with_timezone(&Local);
        assert_eq!(
            local.date_naive(),
            NaiveDate::from_ymd_opt(2026, 10, 12).unwrap()
        );
        assert_eq!((local.hour(), local.minute()), (17, 0));
        assert!(confidence >= 0.1);
    }

    #[test]
    fn extracts_explicit_date_and_minute_precision() {
        let now = Local.with_ymd_and_hms(2026, 10, 11, 10, 0, 0).unwrap();
        let (deadline, _) = extract_deadline("Submit before 13 Oct 11:59 PM", now);
        let local = deadline.unwrap().with_timezone(&Local);
        assert_eq!(
            local.date_naive(),
            NaiveDate::from_ymd_opt(2026, 10, 13).unwrap()
        );
        assert_eq!((local.hour(), local.minute()), (23, 59));
    }

    #[test]
    fn extracts_relative_hour_window() {
        let now = Local.with_ymd_and_hms(2026, 10, 11, 10, 0, 0).unwrap();
        let (deadline, _) = extract_deadline("Choose a slot within 48 hours", now);
        assert_eq!(
            deadline.unwrap(),
            (now + chrono::Duration::hours(48)).with_timezone(&Utc)
        );
    }

    #[test]
    fn extracts_interview_on_next_named_weekday() {
        let now = Local.with_ymd_and_hms(2026, 10, 11, 10, 0, 0).unwrap();
        let action = extract_email_action(
            &message("Interview scheduled", "Your interview is Monday at 3 PM."),
            GmailSearchMode::Internship,
            now,
        )
        .unwrap();
        let local = DateTime::parse_from_rfc3339(action.deadline.as_deref().unwrap())
            .unwrap()
            .with_timezone(&Local);
        assert_eq!(
            local.date_naive(),
            NaiveDate::from_ymd_opt(2026, 10, 12).unwrap()
        );
        assert_eq!((local.hour(), local.minute()), (15, 0));
        assert_eq!(action.action_type, EmailActionType::Interview);
    }

    #[test]
    fn extracts_application_closing_tomorrow() {
        let now = Local.with_ymd_and_hms(2026, 10, 11, 10, 0, 0).unwrap();
        let action = extract_email_action(
            &message("Internship applications", "Applications close tomorrow."),
            GmailSearchMode::Internship,
            now,
        )
        .unwrap();
        let local = DateTime::parse_from_rfc3339(action.deadline.as_deref().unwrap())
            .unwrap()
            .with_timezone(&Local);
        assert_eq!(
            local.date_naive(),
            NaiveDate::from_ymd_opt(2026, 10, 12).unwrap()
        );
        assert_eq!(action.action_type, EmailActionType::Application);
    }

    #[test]
    fn meeting_move_is_actionable_without_inventing_a_date() {
        let now = Local.with_ymd_and_hms(2026, 10, 11, 10, 0, 0).unwrap();
        let action = extract_email_action(
            &message("Meeting moved", "The meeting moved from 2 PM to 4 PM."),
            GmailSearchMode::NeedsAction,
            now,
        )
        .unwrap();
        assert_eq!(action.action_type, EmailActionType::FollowUp);
        assert_eq!(action.deadline, None);
    }

    #[test]
    fn ignores_noise_even_when_it_contains_action_keywords() {
        let now = Local.with_ymd_and_hms(2026, 10, 11, 10, 0, 0).unwrap();
        let cases = [
            message(
                "Weekly newsletter deadline roundup",
                "Five deadlines to watch tomorrow.",
            ),
            message(
                "Limited-time offer: internship deadline",
                "Apply today and save 20 percent.",
            ),
            message(
                "Your OTP",
                "Your one-time password is 123456. It expires in ten minutes.",
            ),
            message_from(
                "[repo] Review requested",
                "Please review this pull request.",
                "GitHub <notifications@github.com>",
            ),
        ];
        for candidate in cases {
            assert!(extract_email_action(&candidate, GmailSearchMode::Internship, now).is_none());
        }
    }

    #[test]
    fn ignores_an_explicitly_expired_assessment() {
        let now = Local.with_ymd_and_hms(2026, 10, 11, 10, 0, 0).unwrap();
        assert!(extract_email_action(
            &message("Online assessment", "Complete the OA by 13 Oct 2024."),
            GmailSearchMode::Internship,
            now,
        )
        .is_none());
    }

    #[test]
    fn selects_the_date_nearest_the_deadline_keyword() {
        let now = Local.with_ymd_and_hms(2026, 10, 11, 10, 0, 0).unwrap();
        let (deadline, _) = extract_deadline(
            "Orientation is on 20 Oct, but complete the assessment by 13 Oct.",
            now,
        );
        assert_eq!(
            deadline.unwrap().with_timezone(&Local).date_naive(),
            NaiveDate::from_ymd_opt(2026, 10, 13).unwrap()
        );
    }

    #[test]
    fn repeated_message_upsert_does_not_duplicate_the_action() {
        let directory = tempfile::tempdir().unwrap();
        let service =
            GmailService::open_with_vault(directory.path().join("gmail.db"), Arc::new(TestVault))
                .unwrap();
        let now = Local.with_ymd_and_hms(2026, 10, 11, 10, 0, 0).unwrap();
        let action = extract_email_action(
            &message(
                "Amazon SDE Online Assessment",
                "Complete the assessment by 13 Oct 2026.",
            ),
            GmailSearchMode::Internship,
            now,
        )
        .unwrap();
        service.upsert_action(&action).unwrap();
        service.upsert_action(&action).unwrap();
        assert_eq!(service.dashboard().unwrap().needs_action.len(), 1);
    }

    #[test]
    fn missing_or_revoked_credential_fails_safely_and_records_a_bounded_error() {
        let directory = tempfile::tempdir().unwrap();
        let service =
            GmailService::open_with_vault(directory.path().join("gmail.db"), Arc::new(TestVault))
                .unwrap();
        let error = service.refresh(GmailSearchMode::NeedsAction).unwrap_err();
        assert!(error.contains("Gmail is not connected"));
        let status = service.status().unwrap();
        assert!(!status.connected);
        assert!(status.sync.last_error.as_deref().unwrap().len() <= 300);
        assert_eq!(status.sync.last_successful_sync, None);
    }

    #[test]
    fn action_identity_is_stable_and_deadline_sensitive() {
        let first = stable_action_id("message-1", "assessment", Some("2026-10-13T18:29:00Z"));
        let duplicate = stable_action_id("message-1", "assessment", Some("2026-10-13T18:29:00Z"));
        let changed = stable_action_id("message-1", "assessment", Some("2026-10-14T18:29:00Z"));
        assert_eq!(first, duplicate);
        assert_ne!(first, changed);
    }

    #[test]
    fn schema_upgrade_persists_sync_checkpoint_fields() {
        let directory = tempfile::tempdir().unwrap();
        let service =
            GmailService::open_with_vault(directory.path().join("gmail.db"), Arc::new(TestVault))
                .unwrap();
        let connection = service.connection().unwrap();
        connection
            .execute(
                "INSERT INTO gmail_connection_state
                 (singleton, account_email, connected_at, last_history_id, last_successful_sync, last_error)
                 VALUES (1, 'user@example.com', 'now', '12345', 'later', 'recoverable')",
                [],
            )
            .unwrap();
        let status = service.status().unwrap();
        assert_eq!(status.sync.account_id.as_deref(), Some("user@example.com"));
        assert_eq!(status.sync.last_history_id.as_deref(), Some("12345"));
        assert_eq!(status.sync.last_successful_sync.as_deref(), Some("later"));
        assert_eq!(status.sync.last_error.as_deref(), Some("recoverable"));
    }

    #[derive(Default)]
    struct StatefulVault {
        secret: std::sync::Mutex<Option<String>>,
    }

    impl SecretVault for StatefulVault {
        fn read(&self, _target: &str) -> Result<Option<String>, String> {
            Ok(self.secret.lock().unwrap().clone())
        }

        fn write(&self, _target: &str, value: &str) -> Result<(), String> {
            *self.secret.lock().unwrap() = Some(value.to_owned());
            Ok(())
        }

        fn delete(&self, _target: &str) -> Result<(), String> {
            *self.secret.lock().unwrap() = None;
            Ok(())
        }
    }

    fn custom_message(id: &str, sender: &str, subject: &str, snippet: &str) -> GmailMessage {
        GmailMessage {
            id: id.to_owned(),
            thread_id: format!("thread-{id}"),
            snippet: snippet.to_owned(),
            internal_date: Some("1791676800000".to_owned()),
            payload: GmailPayload {
                headers: vec![
                    GmailHeader {
                        name: "Subject".to_owned(),
                        value: subject.to_owned(),
                    },
                    GmailHeader {
                        name: "From".to_owned(),
                        value: sender.to_owned(),
                    },
                ],
            },
        }
    }

    #[test]
    fn unread_listing_uses_exact_gmail_query_and_returns_metadata_without_action_filtering() {
        assert_eq!(GMAIL_UNREAD_QUERY, "is:unread");
        let source = custom_message(
            "unread-1",
            "Friend <friend@example.com>",
            "Ordinary unread conversation",
            "This has no deadline or deterministic action signal.",
        );
        let summary = message_summary(&source);
        assert_eq!(summary.id, "unread-1");
        assert_eq!(summary.thread_id, "thread-unread-1");
        assert_eq!(summary.subject, "Ordinary unread conversation");
        assert_eq!(summary.sender, "Friend <friend@example.com>");
        assert_eq!(
            summary.snippet,
            "This has no deadline or deterministic action signal."
        );
    }

    #[test]
    fn credential_vault_is_the_authoritative_connection_state() {
        let directory = tempfile::tempdir().unwrap();
        let vault = Arc::new(StatefulVault::default());
        vault
            .write(
                CREDENTIAL_TARGET,
                r#"{"client_id":"test.apps.googleusercontent.com","client_secret":"","refresh_token":"saved-refresh-token"}"#,
            )
            .unwrap();
        let service =
            GmailService::open_with_vault(directory.path().join("gmail.db"), vault).unwrap();
        let status = service.status().unwrap();
        assert!(status.connected);
        assert_eq!(status.account_email, None);
    }

    #[test]
    fn validates_thirty_six_message_corpus_across_all_eight_categories() {
        let now = Local.with_ymd_and_hms(2026, 10, 11, 10, 0, 0).unwrap();
        let non_actionable = vec![
            // 1. Newsletters and promotions (5)
            (
                "n1",
                "Tech Digest <newsletter@example.com>",
                "Weekly digest: AI tools",
                "Please reply to our survey or read top stories.",
            ),
            (
                "n2",
                "Cloud Store <deals@example.com>",
                "Flash sale ends tomorrow at 5 PM",
                "Confirm your cart before 13 Oct.",
            ),
            (
                "n3",
                "Dev Weekly <hello@example.com>",
                "Engineering newsletter #42",
                "Top articles for this week.",
            ),
            (
                "n4",
                "SaaS Updates <marketing@example.com>",
                "Limited-time offer on Pro plan",
                "Upgrade within 48 hours.",
            ),
            (
                "n5",
                "Daily Brief <brief@example.com>",
                "Daily digest: market wrap",
                "No action required.",
            ),
            // 2. OTP / security codes (5)
            (
                "o1",
                "Security <security@example.com>",
                "Your verification code is 482910",
                "Enter this verification code within 10 minutes.",
            ),
            (
                "o2",
                "Auth <no-reply@example.com>",
                "One-time password for login",
                "Your one-time password expires soon.",
            ),
            (
                "o3",
                "Account <auth@example.com>",
                "Security code required",
                "Use security code 991204 to confirm sign-in.",
            ),
            (
                "o4",
                "SSO <sso@example.com>",
                "Login code for SHARMAN",
                "Use login code 118231 to complete sign-in.",
            ),
            (
                "o5",
                "Bank Alert <alerts@example.com>",
                "Your OTP for transaction",
                "Your OTP is 837192. Do not share it.",
            ),
            // 3. GitHub alerts (5)
            (
                "g1",
                "GitHub <notifications@github.com>",
                "[sharman] PR #12 review requested",
                "Please review before 13 Oct 2026.",
            ),
            (
                "g2",
                "GitHub <noreply@github.com>",
                "[sharman] CI build failed on main",
                "Action required: fix workflow.",
            ),
            (
                "g3",
                "dependabot[bot] <notifications@github.com>",
                "Bump tokio from 1.38 to 1.40",
                "Merge or close this pull request.",
            ),
            (
                "g4",
                "GitHub Actions <notifications@github.com>",
                "Run failed: Verification suite",
                "Please complete check run.",
            ),
            (
                "g5",
                "GitHub Security <noreply@github.com>",
                "Dependabot alert confirm",
                "Review dependency alert.",
            ),
            // 4. Receipts & invoices (5)
            (
                "r1",
                "Stripe <receipts@example.com>",
                "Receipt for your payment",
                "We confirm your payment of $19.00.",
            ),
            (
                "r2",
                "Store <orders@example.com>",
                "Order confirmation #4821",
                "Your order is confirmed and will ship tomorrow.",
            ),
            (
                "r3",
                "Cloud Billing <billing@example.com>",
                "Invoice #9921 is paid",
                "Payment receipt for October usage.",
            ),
            (
                "r4",
                "Bookshop <shop@example.com>",
                "Shipping confirmation for order",
                "Your package arrives by 5 PM tomorrow.",
            ),
            (
                "r5",
                "Media Plus <no-reply@example.com>",
                "Subscription renewal receipt",
                "Your monthly subscription was confirmed.",
            ),
            // 5. Ordinary conversations (4)
            (
                "c1",
                "Rohan <rohan@example.com>",
                "Lunch plans",
                "Are we meeting at the cafe later?",
            ),
            (
                "c2",
                "Priya <priya@example.com>",
                "Project notes shared",
                "Here are the lecture notes from yesterday.",
            ),
            (
                "c3",
                "Mom <family@example.com>",
                "Photos from weekend",
                "Attached the family photos.",
            ),
            (
                "c4",
                "Teammate <team@example.com>",
                "Great job on the demo",
                "Everything looked awesome in class today.",
            ),
        ];

        let actionable = vec![
            // 6. Online assessments (4)
            (
                "a1",
                "Amazon University Recruiting <jobs@amazon.com>",
                "Complete your SDE online assessment",
                "Please complete your online assessment before 13 Oct 11:59 PM.",
                GmailSearchMode::Internship,
                EmailActionType::Assessment,
            ),
            (
                "a2",
                "CodeSignal <assessments@codesignal.com>",
                "Coding assessment invitation",
                "Submit your assessment within 48 hours.",
                GmailSearchMode::NeedsAction,
                EmailActionType::Assessment,
            ),
            (
                "a3",
                "HackerRank <no-reply@hackerrank.com>",
                "Backend developer OA invitation",
                "Complete the assessment by 15 Oct 2026.",
                GmailSearchMode::Internship,
                EmailActionType::Assessment,
            ),
            (
                "a4",
                "Systems Corp <recruiting@systemscorp.com>",
                "Technical assessment link",
                "Your assessment expires by 5 PM tomorrow.",
                GmailSearchMode::NeedsAction,
                EmailActionType::Assessment,
            ),
            // 7. Interview scheduling (4, including rescheduled time)
            (
                "i1",
                "Talent Team <talent@example.com>",
                "Interview slot selection",
                "Please choose your interview slot within 48 hours.",
                GmailSearchMode::NeedsAction,
                EmailActionType::Interview,
            ),
            (
                "i2",
                "Recruiting Coordinator <coord@example.com>",
                "Interview moved",
                "Your interview moved from 2 PM to 4:30 PM tomorrow.",
                GmailSearchMode::NeedsAction,
                EmailActionType::Interview,
            ),
            (
                "i3",
                "Engineering Hiring <hiring@example.com>",
                "Final round interview confirmation",
                "Please confirm your interview availability by 14 Oct 2026.",
                GmailSearchMode::Internship,
                EmailActionType::Interview,
            ),
            (
                "i4",
                "Campus Team <campus@example.com>",
                "Technical interview invite",
                "Reply with your preferred interview time by Friday 6 PM.",
                GmailSearchMode::NeedsAction,
                EmailActionType::Interview,
            ),
            // 8. Internship / application / deadline messages (4)
            (
                "p1",
                "University Careers <careers@example.com>",
                "Summer internship application update",
                "Complete your candidate profile before 16 Oct 2026.",
                GmailSearchMode::Internship,
                EmailActionType::Application,
            ),
            (
                "p2",
                "Fellowship Program <fellows@example.com>",
                "Fellowship submission deadline",
                "Application deadline is 18 Oct 11:59 PM.",
                GmailSearchMode::NeedsAction,
                EmailActionType::Deadline,
            ),
            (
                "p3",
                "Hiring Ops <ops@example.com>",
                "Action required: background check form",
                "Please complete the verification form by 5 PM tomorrow.",
                GmailSearchMode::NeedsAction,
                EmailActionType::FollowUp,
            ),
            (
                "p4",
                "Graduate Recruiting <grad@example.com>",
                "New software internship opportunity",
                "Review role details for the 2027 cohort.",
                GmailSearchMode::Internship,
                EmailActionType::Application,
            ),
        ];

        assert_eq!(non_actionable.len() + actionable.len(), 36);

        for (id, sender, subject, snippet) in &non_actionable {
            let msg = custom_message(id, sender, subject, snippet);
            assert!(
                extract_email_action(&msg, GmailSearchMode::NeedsAction, now).is_none(),
                "Expected noise message {id} ({subject}) to produce no action"
            );
        }

        let mut extracted_count = 0;
        for (id, sender, subject, snippet, mode, expected_type) in &actionable {
            let msg = custom_message(id, sender, subject, snippet);
            let action = extract_email_action(&msg, *mode, now).unwrap_or_else(|| {
                panic!("Expected actionable message {id} ({subject}) to extract")
            });
            assert_eq!(
                &action.action_type, expected_type,
                "Mismatch on message {id}"
            );
            if *id == "i2" {
                let local = DateTime::parse_from_rfc3339(action.deadline.as_deref().unwrap())
                    .unwrap()
                    .with_timezone(&Local);
                assert_eq!((local.hour(), local.minute()), (16, 30));
            }
            extracted_count += 1;
        }
        assert_eq!(extracted_count, 12);
    }

    #[test]
    fn repeat_refresh_and_restart_preserve_dismissed_and_reminder_state_without_duplicates() {
        let directory = tempfile::tempdir().unwrap();
        let db_path = directory.path().join("gmail.db");
        let now = Local.with_ymd_and_hms(2026, 10, 11, 10, 0, 0).unwrap();
        let vault = Arc::new(StatefulVault::default());

        let msg_a = custom_message(
            "msg-oa-1",
            "Amazon Jobs <jobs@amazon.com>",
            "Amazon SDE Online Assessment",
            "Please complete your assessment before 13 Oct 11:59 PM.",
        );
        let msg_b = custom_message(
            "msg-int-2",
            "Talent Team <talent@example.com>",
            "Choose an interview slot",
            "Select an interview time within 48 hours.",
        );
        let action_a = extract_email_action(&msg_a, GmailSearchMode::Internship, now).unwrap();
        let action_b = extract_email_action(&msg_b, GmailSearchMode::NeedsAction, now).unwrap();

        {
            let service = GmailService::open_with_vault(db_path.clone(), vault.clone()).unwrap();
            service.upsert_action(&action_a).unwrap();
            service.upsert_action(&action_b).unwrap();
            service.mark_reminder_created(&action_a.id).unwrap();
            service.dismiss_action(&action_b.id).unwrap();
        }

        // Simulate app restart and repeated sync of the same messages
        let reopened = GmailService::open_with_vault(db_path, vault).unwrap();
        reopened.upsert_action(&action_a).unwrap();
        reopened.upsert_action(&action_b).unwrap();
        let dashboard = reopened.dashboard().unwrap();

        assert_eq!(dashboard.needs_action.len(), 1);
        assert_eq!(dashboard.needs_action[0].id, action_a.id);
        assert!(dashboard.needs_action[0].reminder_created);
        assert_eq!(dashboard.dismissed.len(), 1);
        assert_eq!(dashboard.dismissed[0].id, action_b.id);
    }

    #[test]
    fn disconnect_and_missing_credential_recover_cleanly_without_corrupting_checkpoint() {
        let directory = tempfile::tempdir().unwrap();
        let vault = Arc::new(StatefulVault::default());
        vault
            .write(
                CREDENTIAL_TARGET,
                r#"{"client_id":"test.apps.googleusercontent.com","client_secret":"top-secret-value","refresh_token":"refresh-secret-token"}"#,
            )
            .unwrap();
        let service =
            GmailService::open_with_vault(directory.path().join("gmail.db"), vault.clone())
                .unwrap();
        service
            .connection()
            .unwrap()
            .execute(
                "INSERT INTO gmail_connection_state
                 (singleton, account_email, connected_at, last_history_id, last_successful_sync)
                 VALUES (1, 'owner@example.com', '2026-10-12T00:00:00Z', '98765', '2026-10-12T00:05:00Z')",
                [],
            )
            .unwrap();

        assert!(service.status().unwrap().connected);

        // Simulate deleted Credential Manager entry / disconnect
        let disconnected = service.disconnect().unwrap();
        assert!(!disconnected.connected);
        assert_eq!(disconnected.sync.last_history_id.as_deref(), Some("98765"));

        // Refresh when disconnected records a bounded error without leaking tokens or wiping history checkpoint
        let error = service.refresh(GmailSearchMode::NeedsAction).unwrap_err();
        assert!(error.contains("not connected"));
        let after_error = service.status().unwrap();
        assert_eq!(after_error.sync.last_history_id.as_deref(), Some("98765"));
        assert!(after_error
            .sync
            .last_error
            .as_deref()
            .unwrap()
            .contains("not connected"));
        assert!(!after_error
            .sync
            .last_error
            .as_deref()
            .unwrap()
            .contains("refresh-secret-token"));
    }

    #[test]
    fn background_sync_policy_enforces_read_and_suggest_ceiling() {
        let policy = BackgroundSyncPolicy::default();
        assert_eq!(policy.interval_seconds, 3600);
        assert_eq!(policy.permission_ceiling, "READ_AND_SUGGEST");

        for allowed in [
            "gmail.search",
            "gmail.read",
            "inbox.classify",
            "task.suggest",
            "reminder.suggest",
            "notify",
        ] {
            assert!(
                policy.authorize_capability(allowed).is_ok(),
                "expected '{allowed}' to be allowed for background sync"
            );
        }

        for forbidden in [
            "gmail.send",
            "browser.submit",
            "system.exec",
            "delete",
            "purchase",
            "door.unlock",
        ] {
            let err = policy.authorize_capability(forbidden).unwrap_err();
            assert!(err.contains("READ_AND_SUGGEST"));
        }
    }
}
