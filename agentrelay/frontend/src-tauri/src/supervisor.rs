use chrono::{DateTime, Datelike, Days, Local, NaiveDate, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};

use crate::assistant_tools::ToolPermissionLevel;
use crate::gmail::GmailSearchMode;
use crate::personal::TaskView;

#[derive(Debug, Clone)]
pub enum SupervisorDecision {
    Execute(SupervisorPlan),
    Clarify {
        title: String,
        prompt: String,
        suggestions: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub struct SupervisorPlan {
    pub intent: &'static str,
    pub title: String,
    pub calls: Vec<ToolCall>,
}

#[derive(Debug, Clone)]
pub enum ToolCall {
    OpenTrustedUrl {
        url: String,
        label: String,
    },
    OpenLeetcodeDaily,
    SearchGmail {
        mode: GmailSearchMode,
    },
    ListUnreadGmail,
    ListRecentGmail,
    GmailConnectionStatus,
    ListTodayTasks,
    ListTasks {
        view: TaskView,
    },
    ListReminders,
    PreviewCompleteTask {
        title: String,
    },
    PreviewCancelReminder {
        title: String,
    },
    CreateTask {
        title: String,
    },
    CreateReminder {
        title: String,
        remind_at: DateTime<Local>,
    },
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Created,
    Running,
    Completed,
    Terminated,
    Cancelled,
    TimedOut,
    Denied,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegatedAgentStep {
    pub step_number: u32,
    pub tool: String,
    pub status: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemporaryAgent {
    pub id: String,
    pub parent_agent_id: String,
    pub name: String,
    pub goal: String,
    pub allowed_tools: Vec<String>,
    pub denied_tools: Vec<String>,
    pub permission_ceiling: ToolPermissionLevel,
    pub max_steps: u32,
    pub steps_used: u32,
    pub timeout_ms: u64,
    pub status: AgentStatus,
    pub trace: Vec<DelegatedAgentStep>,
    pub result_summary: Option<String>,
    pub created_at: String,
    pub terminated_at: Option<String>,
}

#[allow(dead_code)]
const PERMANENTLY_FORBIDDEN_TOOLS: &[&str] = &[
    "gmail.send",
    "browser.submit",
    "system.exec",
    "delete",
    "purchase",
    "door.unlock",
];

#[allow(dead_code)]
impl TemporaryAgent {
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_child(
        id: impl Into<String>,
        parent_agent_id: impl Into<String>,
        parent_allowed_tools: &[&str],
        parent_ceiling: ToolPermissionLevel,
        name: impl Into<String>,
        goal: impl Into<String>,
        requested_tools: Vec<String>,
        denied_tools: Vec<String>,
        requested_ceiling: ToolPermissionLevel,
        max_steps: u32,
        timeout_ms: u64,
        now: DateTime<Local>,
    ) -> Result<Self, String> {
        if requested_ceiling.number() > parent_ceiling.number() {
            return Err(format!(
                "Privilege escalation denied: child requested ceiling {:?} (level {}) which exceeds parent ceiling {:?} (level {}).",
                requested_ceiling,
                requested_ceiling.number(),
                parent_ceiling,
                parent_ceiling.number()
            ));
        }
        if max_steps == 0 || max_steps > 16 {
            return Err(format!(
                "Invalid step budget {max_steps}: temporary agents require 1..=16 steps."
            ));
        }
        if timeout_ms == 0 || timeout_ms > 60_000 {
            return Err(format!(
                "Invalid timeout {timeout_ms}ms: temporary agents require 1..=60000ms."
            ));
        }
        for tool in &requested_tools {
            let normalized = tool.trim();
            if PERMANENTLY_FORBIDDEN_TOOLS.contains(&normalized) {
                return Err(format!(
                    "Capability '{normalized}' is permanently forbidden for temporary child agents."
                ));
            }
            if !parent_allowed_tools.contains(&normalized) {
                return Err(format!(
                    "Privilege inheritance violation: parent did not delegate capability '{normalized}'."
                ));
            }
        }
        Ok(Self {
            id: id.into(),
            parent_agent_id: parent_agent_id.into(),
            name: name.into(),
            goal: goal.into(),
            allowed_tools: requested_tools,
            denied_tools,
            permission_ceiling: requested_ceiling,
            max_steps,
            steps_used: 0,
            timeout_ms,
            status: AgentStatus::Running,
            trace: Vec::new(),
            result_summary: None,
            created_at: now.to_rfc3339(),
            terminated_at: None,
        })
    }

    pub fn record_step(
        &mut self,
        tool: &str,
        tool_permission: ToolPermissionLevel,
        elapsed_ms: u64,
    ) -> Result<(), String> {
        if self.status != AgentStatus::Running {
            return Err(format!(
                "Temporary agent '{}' is not running (status: {:?}).",
                self.name, self.status
            ));
        }
        if elapsed_ms > self.timeout_ms {
            self.status = AgentStatus::TimedOut;
            return Err(format!(
                "Temporary agent '{}' exceeded timeout budget ({}ms > {}ms).",
                self.name, elapsed_ms, self.timeout_ms
            ));
        }
        if self.steps_used >= self.max_steps {
            self.status = AgentStatus::Denied;
            return Err(format!(
                "Temporary agent '{}' exhausted step budget ({}/{}).",
                self.name, self.steps_used, self.max_steps
            ));
        }
        if tool_permission.number() > self.permission_ceiling.number() {
            self.status = AgentStatus::Denied;
            return Err(format!(
                "Child agent '{}' cannot execute '{}' (requires {:?}, ceiling is {:?}).",
                self.name, tool, tool_permission, self.permission_ceiling
            ));
        }
        if tool != "prioritize"
            && tool != "return_result"
            && !self.allowed_tools.iter().any(|allowed| allowed == tool)
        {
            self.status = AgentStatus::Denied;
            return Err(format!(
                "Child agent '{}' attempted undelegated tool '{}'.",
                self.name, tool
            ));
        }
        self.steps_used += 1;
        self.trace.push(DelegatedAgentStep {
            step_number: self.steps_used,
            tool: tool.to_owned(),
            status: "completed".to_owned(),
        });
        Ok(())
    }

    pub fn cancel(&mut self, reason: &str, now: DateTime<Local>) {
        self.status = AgentStatus::Cancelled;
        self.result_summary = Some(format!("Cancelled: {reason}"));
        self.terminated_at = Some(now.to_rfc3339());
    }

    pub fn complete_and_terminate(&mut self, summary: impl Into<String>, now: DateTime<Local>) {
        self.result_summary = Some(summary.into());
        self.status = AgentStatus::Terminated;
        self.terminated_at = Some(now.to_rfc3339());
    }
}

#[allow(dead_code)]
pub fn spawn_daily_planner_agent(now: DateTime<Local>) -> Result<TemporaryAgent, String> {
    let supervisor_tools = &[
        "gmail.search",
        "inbox.actions",
        "tasks.list",
        "tasks.create",
        "reminders.list",
        "reminders.create",
        "browser.open",
    ];
    TemporaryAgent::spawn_child(
        "temp-agent-daily-planner",
        "sharman-supervisor",
        supervisor_tools,
        ToolPermissionLevel::LowRisk,
        "DailyPlannerAgent",
        "Produce prioritized actionable plan for today.",
        vec![
            "gmail.search".to_owned(),
            "inbox.actions".to_owned(),
            "tasks.list".to_owned(),
            "reminders.list".to_owned(),
        ],
        vec![
            "gmail.send".to_owned(),
            "browser.submit".to_owned(),
            "system.exec".to_owned(),
        ],
        ToolPermissionLevel::ReadOnly,
        8,
        30_000,
        now,
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub tool: String,
    pub summary: String,
    pub permission_level: u8,
    pub status: String,
}

pub fn decide(message: &str, now: DateTime<Local>) -> Option<SupervisorDecision> {
    // Personal commands take precedence over words inside their titles (e.g.
    // "Add task check my email"). No lowercased byte offsets slice user text.
    if let Some(plan) = personal_command(message) {
        return Some(plan);
    }
    let lower = message.trim().to_lowercase();
    if lower.starts_with("remind me") {
        return Some(match parse_reminder(message.trim(), now) {
            Ok((title, remind_at)) => SupervisorDecision::Execute(SupervisorPlan {
                intent: "reminder",
                title: "Create reminder".to_owned(),
                calls: vec![ToolCall::CreateReminder { title, remind_at }],
            }),
            Err(prompt) => SupervisorDecision::Clarify {
                title: "When should I remind you?".to_owned(),
                prompt,
                suggestions: vec![
                    "Remind me in 20 minutes to take a break".into(),
                    "Remind me tomorrow at 9 AM to review my tasks".into(),
                ],
            },
        });
    }

    if contains_any(
        &lower,
        &[
            "what do i need to do today",
            "what needs my attention today",
            "plan my day",
            "what should i focus on today",
            "prioritize my day",
        ],
    ) {
        return Some(SupervisorDecision::Execute(SupervisorPlan {
            intent: "daily_plan",
            title: "DailyPlannerAgent — Today’s Priorities".to_owned(),
            calls: vec![
                ToolCall::SearchGmail {
                    mode: GmailSearchMode::NeedsAction,
                },
                ToolCall::ListTodayTasks,
            ],
        }));
    }

    if lower.contains("gmail")
        && contains_any(
            &lower,
            &[
                "connection status",
                "connection diagnostic",
                "is gmail connected",
                "gmail connected",
                "authorization status",
            ],
        )
    {
        return Some(SupervisorDecision::Execute(SupervisorPlan {
            intent: "gmail_connection",
            title: "Gmail connection status".to_owned(),
            calls: vec![ToolCall::GmailConnectionStatus],
        }));
    }

    if lower.contains("unread")
        && contains_any(
            &lower,
            &[
                "email", "emails", "mail", "mails", "gmail", "gmails", "inbox",
            ],
        )
    {
        return Some(SupervisorDecision::Execute(SupervisorPlan {
            intent: "gmail_unread",
            title: "Unread Gmail".to_owned(),
            calls: vec![ToolCall::ListUnreadGmail],
        }));
    }

    if contains_any(&lower, &["email", "mail", "inbox", "gmail"])
        && contains_any(&lower, &["recent", "latest", "newest"])
        && !contains_any(
            &lower,
            &[
                "important",
                "action",
                "internship",
                "assessment",
                "interview",
            ],
        )
    {
        return Some(SupervisorDecision::Execute(SupervisorPlan {
            intent: "gmail_recent",
            title: "Recent Gmail".to_owned(),
            calls: vec![ToolCall::ListRecentGmail],
        }));
    }

    if contains_any(&lower, &["email", "emails", "mail", "inbox"])
        && contains_any(&lower, &["show", "check", "find", "list", "what", "recent"])
    {
        let mode = if contains_any(
            &lower,
            &["internship", "online assessment", " oa ", "job application"],
        ) {
            GmailSearchMode::Internship
        } else if contains_any(
            &lower,
            &["need action", "needs action", "require action", "today"],
        ) {
            GmailSearchMode::NeedsAction
        } else {
            GmailSearchMode::Important
        };
        return Some(SupervisorDecision::Execute(SupervisorPlan {
            intent: "email",
            title: "Inbox Intelligence".to_owned(),
            calls: vec![ToolCall::SearchGmail { mode }],
        }));
    }

    if lower.contains("leetcode")
        && contains_any(&lower, &["today", "daily", "problem of the day"])
        && contains_any(&lower, &["open", "show", "go to", "launch"])
    {
        return Some(SupervisorDecision::Execute(SupervisorPlan {
            intent: "leetcode",
            title: "Open today’s LeetCode challenge".to_owned(),
            calls: vec![ToolCall::OpenLeetcodeDaily],
        }));
    }

    if lower.contains("leetcode") && contains_any(&lower, &["open", "go to", "launch"]) {
        return Some(SupervisorDecision::Execute(SupervisorPlan {
            intent: "browser",
            title: "Open LeetCode".to_owned(),
            calls: vec![ToolCall::OpenTrustedUrl {
                url: "https://leetcode.com".to_owned(),
                label: "Open LeetCode".to_owned(),
            }],
        }));
    }

    if contains_any(
        &lower,
        &[
            "what tasks do i have today",
            "what are my tasks today",
            "show today's tasks",
            "show my tasks today",
            "tasks for today",
        ],
    ) {
        return Some(SupervisorDecision::Execute(SupervisorPlan {
            intent: "tasks",
            title: "Today’s tasks".to_owned(),
            calls: vec![ToolCall::ListTodayTasks],
        }));
    }

    None
}

fn argument<'a>(message: &'a str, prefix: &str) -> Option<&'a str> {
    let message = message.trim();
    message
        .get(..prefix.len())?
        .eq_ignore_ascii_case(prefix)
        .then(|| message[prefix.len()..].trim())
}

fn personal_command(message: &str) -> Option<SupervisorDecision> {
    let command = message
        .trim()
        .trim_end_matches(['?', '.', '!'])
        .to_lowercase();
    let call = match command.as_str() {
        "show my tasks" | "list my tasks" | "show open tasks" | "show my open tasks"
        | "list open tasks" | "what are my tasks" | "show pending tasks" => {
            Some(ToolCall::ListTasks {
                view: TaskView::Open,
            })
        }
        "show all tasks" | "show all my tasks" | "list all tasks" | "list all my tasks" => {
            Some(ToolCall::ListTasks {
                view: TaskView::All,
            })
        }
        "show completed tasks" | "show my completed tasks" | "list completed tasks" => {
            Some(ToolCall::ListTasks {
                view: TaskView::Completed,
            })
        }
        "show overdue tasks"
        | "show my overdue tasks"
        | "list overdue tasks"
        | "what tasks are overdue"
        | "what is overdue" => Some(ToolCall::ListTasks {
            view: TaskView::Overdue,
        }),
        "show reminders"
        | "show my reminders"
        | "list reminders"
        | "list my reminders"
        | "show upcoming reminders"
        | "what reminders do i have" => Some(ToolCall::ListReminders),
        _ => None,
    };
    if let Some(call) = call {
        return Some(SupervisorDecision::Execute(SupervisorPlan {
            intent: "personal_list",
            title: "Your local planner".into(),
            calls: vec![call],
        }));
    }
    for (prefix, kind) in [
        ("add task ", 0),
        ("create task ", 0),
        ("new task ", 0),
        ("complete task ", 1),
        ("finish task ", 1),
        ("mark task ", 1),
        ("cancel reminder ", 2),
        ("dismiss reminder ", 2),
    ] {
        if let Some(title) = argument(message, prefix) {
            let title = if prefix == "mark task " {
                title
                    .get(..title.len().saturating_sub(" as done".len()))
                    .filter(|_| title.to_lowercase().ends_with(" as done"))
                    .unwrap_or(title)
            } else {
                title
            };
            let title = title.trim().trim_matches(['"', '“', '”']).trim();
            if title.is_empty()
                || title.len() > 960
                || title.chars().any(char::is_control)
                || (kind != 0
                    && matches!(
                        title.to_lowercase().as_str(),
                        "all" | "everything" | "all tasks" | "all reminders"
                    ))
            {
                return Some(SupervisorDecision::Clarify {
                    title: "Use one specific title".into(),
                    prompt: "Give the exact task or reminder title. Bulk changes are not supported; nothing was changed.".into(),
                    suggestions: vec!["Show my tasks".into(), "Show my reminders".into()],
                });
            }
            let call = match kind {
                0 => ToolCall::CreateTask {
                    title: title.into(),
                },
                1 => ToolCall::PreviewCompleteTask {
                    title: title.into(),
                },
                _ => ToolCall::PreviewCancelReminder {
                    title: title.into(),
                },
            };
            return Some(SupervisorDecision::Execute(SupervisorPlan {
                intent: if kind == 0 {
                    "tasks"
                } else {
                    "personal_review"
                },
                title: if kind == 0 {
                    "Create personal task"
                } else {
                    "Review before changing"
                }
                .into(),
                calls: vec![call],
            }));
        }
    }
    None
}

impl ToolCall {
    pub fn step(&self, status: &str) -> PlanStep {
        let (tool, summary, permission) = match self {
            Self::OpenTrustedUrl { label, .. } => {
                ("browser.open", label.clone(), ToolPermissionLevel::LowRisk)
            }
            Self::OpenLeetcodeDaily => (
                "leetcode.daily → browser.open",
                "Resolve today’s challenge, then prepare the validated page".to_owned(),
                ToolPermissionLevel::LowRisk,
            ),
            Self::SearchGmail { mode } => (
                "gmail.search",
                format!("Read and extract {} metadata", mode.label()),
                ToolPermissionLevel::ReadOnly,
            ),
            Self::ListUnreadGmail => (
                "gmail.list_unread",
                "List unread Gmail message metadata with query is:unread".to_owned(),
                ToolPermissionLevel::ReadOnly,
            ),
            Self::ListRecentGmail => (
                "gmail.list_recent",
                "List recent inbox metadata without action filtering".to_owned(),
                ToolPermissionLevel::ReadOnly,
            ),
            Self::GmailConnectionStatus => (
                "gmail.connection_status",
                "Read the authoritative OAuth credential and last-sync state".to_owned(),
                ToolPermissionLevel::ReadOnly,
            ),
            Self::ListTodayTasks => (
                "tasks.today",
                "Read incomplete local tasks due today or earlier".to_owned(),
                ToolPermissionLevel::ReadOnly,
            ),
            Self::CreateTask { title } => (
                "tasks.create",
                format!("Create “{title}” in the local task list"),
                ToolPermissionLevel::LowRisk,
            ),
            Self::ListTasks { view } => (
                "tasks.list",
                format!("Read up to 50 {} local tasks", view.label()),
                ToolPermissionLevel::ReadOnly,
            ),
            Self::ListReminders => (
                "reminders.list",
                "Read the next 30 pending local reminders".into(),
                ToolPermissionLevel::ReadOnly,
            ),
            Self::PreviewCompleteTask { .. } => (
                "tasks.find",
                "Find an exact open-task title for explicit review; do not change it".into(),
                ToolPermissionLevel::ReadOnly,
            ),
            Self::PreviewCancelReminder { .. } => (
                "reminders.find",
                "Find an exact pending reminder for explicit review; do not cancel it".into(),
                ToolPermissionLevel::ReadOnly,
            ),
            Self::CreateReminder { title, remind_at } => (
                "reminders.create",
                format!(
                    "Schedule “{title}” for {}",
                    remind_at.format("%d %b, %I:%M %p")
                ),
                ToolPermissionLevel::LowRisk,
            ),
        };
        PlanStep {
            tool: tool.to_owned(),
            summary,
            permission_level: permission.number(),
            status: status.to_owned(),
        }
    }
}

fn parse_reminder(
    message: &str,
    now: DateTime<Local>,
) -> Result<(String, DateTime<Local>), String> {
    if let Some(relative) = argument(message, "remind me in ") {
        let mut words = relative.split_whitespace();
        let amount = words
            .next()
            .and_then(|word| word.parse::<i64>().ok())
            .filter(|amount| *amount > 0)
            .ok_or("Use a positive whole duration, like ‘in 20 minutes to stretch’.")?;
        let unit = words.next().unwrap_or("").to_ascii_lowercase();
        let multiplier = match unit.as_str() {
            "minute" | "minutes" | "min" | "mins" => 60,
            "hour" | "hours" | "hr" | "hrs" => 3600,
            "day" | "days" => 86400,
            _ => {
                return Err(
                    "Use minutes, hours, or days, followed by ‘to’ and a reminder title.".into(),
                )
            }
        };
        let seconds = amount
            .checked_mul(multiplier)
            .filter(|seconds| *seconds <= 366 * 86400)
            .ok_or("Reminders must be within the next 366 days.")?;
        if !words
            .next()
            .is_some_and(|word| word.eq_ignore_ascii_case("to"))
        {
            return Err("Include what to do: ‘Remind me in 20 minutes to stretch’.".into());
        }
        let title = words.collect::<Vec<_>>().join(" ");
        if title.is_empty() || title.len() > 960 || title.chars().any(char::is_control) {
            return Err("Include a short reminder title after ‘to’.".into());
        }
        let scheduled = now
            .checked_add_signed(chrono::Duration::seconds(seconds))
            .ok_or("The reminder time is outside the supported range.")?;
        return Ok((title, scheduled));
    }
    let lower = message.to_lowercase();
    let at_index = lower.find(" at ").ok_or_else(|| {
        "Include a time, such as “Remind me at 8 PM to solve today’s LeetCode.”".to_owned()
    })?;
    let after_at = at_index + " at ".len();
    let title_index = lower[after_at..].find(" to ").map(|index| after_at + index);
    let time_end = title_index.unwrap_or(message.len());
    let time_phrase = lower[after_at..time_end]
        .replace("today", "")
        .replace("tomorrow", "")
        .trim()
        .to_owned();
    let time = parse_clock(&time_phrase)
        .ok_or_else(|| "Use a time like 8 PM, 8:30 PM, or 20:00.".to_owned())?;
    let mut date = NaiveDate::from_ymd_opt(now.year(), now.month(), now.day())
        .ok_or_else(|| "The current local date is invalid.".to_owned())?;
    if lower.contains("tomorrow") {
        date = date
            .checked_add_days(Days::new(1))
            .ok_or_else(|| "Tomorrow is outside the supported date range.".to_owned())?;
    }
    let mut scheduled = Local
        .from_local_datetime(&date.and_time(time))
        .earliest()
        .ok_or_else(|| "That local time does not exist on this date.".to_owned())?;
    if !lower.contains("tomorrow") && scheduled <= now {
        let tomorrow = date
            .checked_add_days(Days::new(1))
            .ok_or_else(|| "The next reminder date is outside the supported range.".to_owned())?;
        scheduled = Local
            .from_local_datetime(&tomorrow.and_time(time))
            .earliest()
            .ok_or_else(|| "That local time does not exist tomorrow.".to_owned())?;
    }
    let raw_title = title_index
        .map(|index| message[index + " to ".len()..].trim())
        .filter(|title| !title.is_empty())
        .ok_or_else(|| "Tell me what to remind you about after the time.".to_owned())?;
    let raw_title = raw_title.trim_end_matches(['.', '!', '?']);
    let title = if matches!(raw_title.to_lowercase().as_str(), "solve it" | "do it") {
        "Solve the LeetCode daily problem".to_owned()
    } else {
        sentence_case(raw_title)
    };
    Ok((title, scheduled))
}

fn parse_clock(value: &str) -> Option<NaiveTime> {
    let compact = value.trim().to_lowercase().replace(' ', "");
    let (clock, suffix) = if let Some(clock) = compact.strip_suffix("am") {
        (clock, Some("am"))
    } else if let Some(clock) = compact.strip_suffix("pm") {
        (clock, Some("pm"))
    } else {
        (compact.as_str(), None)
    };
    let mut parts = clock.split(':');
    let mut hour = parts.next()?.parse::<u32>().ok()?;
    let minute = parts.next().map_or(Some(0), |part| part.parse().ok())?;
    if parts.next().is_some() || minute > 59 {
        return None;
    }
    match suffix {
        Some("am") if hour == 12 => hour = 0,
        Some("am") if (1..=11).contains(&hour) => {}
        Some("pm") if hour == 12 => {}
        Some("pm") if (1..=11).contains(&hour) => hour += 12,
        Some(_) => return None,
        None if hour <= 23 => {}
        None => return None,
    }
    NaiveTime::from_hms_opt(hour, minute, 0)
}

fn sentence_case(value: &str) -> String {
    let mut characters = value.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    #[test]
    fn plans_the_four_mvp_commands() {
        let now = Local.with_ymd_and_hms(2026, 10, 11, 10, 0, 0).unwrap();
        assert!(matches!(
            decide("Open LeetCode", now),
            Some(SupervisorDecision::Execute(SupervisorPlan {
                intent: "browser",
                ..
            }))
        ));
        assert!(matches!(
            decide("Find my internship and OA emails", now),
            Some(SupervisorDecision::Execute(SupervisorPlan {
                intent: "email",
                ..
            }))
        ));
        assert!(matches!(
            decide("Open today's LeetCode", now),
            Some(SupervisorDecision::Execute(SupervisorPlan {
                intent: "leetcode",
                ..
            }))
        ));
        assert!(matches!(
            decide("What tasks do I have today?", now),
            Some(SupervisorDecision::Execute(SupervisorPlan {
                intent: "tasks",
                ..
            }))
        ));
        assert!(matches!(
            decide("Remind me at 8 PM to solve it", now),
            Some(SupervisorDecision::Execute(SupervisorPlan {
                intent: "reminder",
                ..
            }))
        ));
    }

    #[test]
    fn routes_recent_mail_without_action_filtering() {
        let now = Local.with_ymd_and_hms(2026, 10, 20, 10, 0, 0).unwrap();
        for phrase in [
            "recent emails",
            "show my recent emails",
            "get the latest gmails",
            "list newest inbox messages",
        ] {
            let Some(SupervisorDecision::Execute(plan)) = decide(phrase, now) else {
                panic!("No plan for {phrase}");
            };
            assert_eq!(plan.intent, "gmail_recent");
            assert!(matches!(plan.calls.as_slice(), [ToolCall::ListRecentGmail]));
            assert_eq!(plan.calls[0].step("ready").permission_level, 0);
        }
    }

    #[test]
    fn personal_views_and_write_previews_are_read_only_and_explicit() {
        let now = Local::now();
        for (phrase, expected) in [
            ("Show my tasks", TaskView::Open),
            ("LIST ALL MY TASKS!", TaskView::All),
            ("Show completed tasks", TaskView::Completed),
            ("What tasks are overdue?", TaskView::Overdue),
        ] {
            let Some(SupervisorDecision::Execute(plan)) = decide(phrase, now) else {
                panic!("{phrase}");
            };
            assert!(
                matches!(plan.calls.as_slice(), [ToolCall::ListTasks { view }] if *view == expected)
            );
            assert_eq!(plan.calls[0].step("completed").permission_level, 0);
        }
        for phrase in ["Show my reminders", "What reminders do I have?"] {
            let Some(SupervisorDecision::Execute(plan)) = decide(phrase, now) else {
                panic!("{phrase}");
            };
            assert!(matches!(plan.calls.as_slice(), [ToolCall::ListReminders]));
        }
        for phrase in [
            "Complete task Review email",
            "Mark task Review email as done",
        ] {
            let Some(SupervisorDecision::Execute(plan)) = decide(phrase, now) else {
                panic!("{phrase}");
            };
            assert!(
                matches!(plan.calls.as_slice(), [ToolCall::PreviewCompleteTask { title }] if title == "Review email")
            );
            assert_eq!(
                plan.calls[0].step("awaiting-confirmation").permission_level,
                0
            );
        }
        let Some(SupervisorDecision::Execute(plan)) = decide("Cancel reminder Review email", now)
        else {
            panic!("cancel");
        };
        assert!(
            matches!(plan.calls.as_slice(), [ToolCall::PreviewCancelReminder { title }] if title == "Review email")
        );
        for phrase in [
            "Complete task all",
            "cancel reminder everything",
            "Complete task \n",
        ] {
            assert!(matches!(
                decide(phrase, now),
                Some(SupervisorDecision::Clarify { .. }) | None
            ));
        }
        assert!(decide("Don't complete task Review email", now).is_none());
    }

    #[test]
    fn task_commands_keep_unicode_and_do_not_route_title_words_to_gmail() {
        let now = Local::now();
        for title in ["Check my important emails", "İstanbul résumé", "整理文件"] {
            let Some(SupervisorDecision::Execute(plan)) =
                decide(&format!("  Add task {title}  "), now)
            else {
                panic!("{title}");
            };
            assert!(
                matches!(plan.calls.as_slice(), [ToolCall::CreateTask { title: actual }] if actual == title)
            );
        }
    }

    #[test]
    fn relative_reminders_support_bounded_durations_and_preserve_titles() {
        let now = Local.with_ymd_and_hms(2026, 10, 21, 23, 59, 0).unwrap();
        for (phrase, seconds, expected_title) in [
            (
                "Remind me in 2 minutes to test SHARMAN",
                120,
                "test SHARMAN",
            ),
            (
                "REMIND ME IN 3 HOURS TO Check my email",
                10800,
                "Check my email",
            ),
            ("Remind me in 1 day to 整理文件", 86400, "整理文件"),
        ] {
            let Some(SupervisorDecision::Execute(plan)) = decide(phrase, now) else {
                panic!("{phrase}");
            };
            let [ToolCall::CreateReminder { title, remind_at }] = plan.calls.as_slice() else {
                panic!("wrong route");
            };
            assert_eq!(title, expected_title);
            assert_eq!(*remind_at - now, chrono::Duration::seconds(seconds));
        }
        for phrase in [
            "Remind me in 0 minutes to test",
            "Remind me in -1 hours to test",
            "Remind me in 1.5 hours to test",
            "Remind me in 999999999999999999999 days to test",
            "Remind me in 367 days to test",
            "Remind me in 3 months to test",
            "Remind me in 2 hours to",
            "Remind me in 2 hours check email",
        ] {
            assert!(
                matches!(
                    decide(phrase, now),
                    Some(SupervisorDecision::Clarify { .. })
                ),
                "{phrase}"
            );
        }
    }

    #[test]
    fn routes_unread_gmail_phrases_to_the_dedicated_read_only_tool() {
        let now = Local.with_ymd_and_hms(2026, 10, 19, 10, 0, 0).unwrap();
        for phrase in [
            "show unread emails",
            "give me all unread emails",
            "all unread gmails",
            "Unread mails",
            "show my unread inbox",
        ] {
            let Some(SupervisorDecision::Execute(plan)) = decide(phrase, now) else {
                panic!("'{phrase}' did not produce a supervisor plan");
            };
            assert!(
                matches!(plan.calls.as_slice(), [ToolCall::ListUnreadGmail]),
                "'{phrase}' did not route to gmail.list_unread"
            );
            assert_eq!(plan.intent, "gmail_unread");
        }
    }

    #[test]
    fn preserves_existing_inbox_intelligence_routes_and_connection_diagnostic() {
        let now = Local.with_ymd_and_hms(2026, 10, 19, 10, 0, 0).unwrap();
        for (phrase, expected_mode) in [
            ("Show my important emails", GmailSearchMode::Important),
            (
                "What emails need action today?",
                GmailSearchMode::NeedsAction,
            ),
            ("Find internship and OA emails", GmailSearchMode::Internship),
        ] {
            let Some(SupervisorDecision::Execute(plan)) = decide(phrase, now) else {
                panic!("'{phrase}' did not produce a supervisor plan");
            };
            assert!(matches!(
                plan.calls.as_slice(),
                [ToolCall::SearchGmail { mode }] if *mode == expected_mode
            ));
        }

        let Some(SupervisorDecision::Execute(plan)) = decide("Gmail connection status", now) else {
            panic!("connection diagnostic did not produce a supervisor plan");
        };
        assert!(matches!(
            plan.calls.as_slice(),
            [ToolCall::GmailConnectionStatus]
        ));
    }

    #[test]
    fn past_clock_rolls_to_tomorrow_and_resolves_it() {
        let now = Local.with_ymd_and_hms(2026, 10, 11, 21, 0, 0).unwrap();
        let (title, scheduled) = parse_reminder("Remind me at 8 PM to solve it", now).unwrap();
        assert_eq!(title, "Solve the LeetCode daily problem");
        assert_eq!(scheduled.day(), 12);
        assert_eq!(scheduled.hour(), 20);
    }

    #[test]
    fn temporary_agent_rejects_child_privilege_escalation_and_undelegated_tools() {
        let now = Local.with_ymd_and_hms(2026, 10, 12, 9, 0, 0).unwrap();
        let parent_tools = &["gmail.search", "tasks.list", "reminders.list"];

        // Child cannot request a higher permission ceiling than its parent
        let ceiling_err = TemporaryAgent::spawn_child(
            "child-1",
            "sharman-supervisor",
            parent_tools,
            ToolPermissionLevel::ReadOnly,
            "EscalatingWorker",
            "Try to gain write permissions",
            vec!["gmail.search".to_owned()],
            vec![],
            ToolPermissionLevel::LowRisk,
            4,
            10_000,
            now,
        )
        .unwrap_err();
        assert!(ceiling_err.contains("Privilege escalation denied"));

        // Child cannot request a permanently forbidden tool even if parent is LowRisk
        let forbidden_err = TemporaryAgent::spawn_child(
            "child-2",
            "sharman-supervisor",
            parent_tools,
            ToolPermissionLevel::LowRisk,
            "UnlockWorker",
            "Try to unlock front door",
            vec!["door.unlock".to_owned()],
            vec![],
            ToolPermissionLevel::LowRisk,
            4,
            10_000,
            now,
        )
        .unwrap_err();
        assert!(forbidden_err.contains("permanently forbidden"));

        // Child cannot request a tool not delegated by parent
        let undelegated_err = TemporaryAgent::spawn_child(
            "child-3",
            "sharman-supervisor",
            parent_tools,
            ToolPermissionLevel::LowRisk,
            "BrowserWorker",
            "Try to open browser without delegation",
            vec!["browser.open".to_owned()],
            vec![],
            ToolPermissionLevel::ReadOnly,
            4,
            10_000,
            now,
        )
        .unwrap_err();
        assert!(undelegated_err.contains("Privilege inheritance violation"));
    }

    #[test]
    fn daily_planner_agent_executes_six_step_trace_and_terminates_cleanly() {
        let now = Local.with_ymd_and_hms(2026, 10, 12, 9, 0, 0).unwrap();
        assert!(matches!(
            decide("What do I need to do today?", now),
            Some(SupervisorDecision::Execute(SupervisorPlan {
                intent: "daily_plan",
                ..
            }))
        ));

        let mut agent = spawn_daily_planner_agent(now).unwrap();
        assert_eq!(agent.name, "DailyPlannerAgent");
        assert_eq!(agent.permission_ceiling, ToolPermissionLevel::ReadOnly);
        assert_eq!(agent.max_steps, 8);
        assert_eq!(agent.timeout_ms, 30_000);

        for tool in [
            "gmail.search",
            "inbox.actions",
            "tasks.list",
            "reminders.list",
            "prioritize",
            "return_result",
        ] {
            agent
                .record_step(tool, ToolPermissionLevel::ReadOnly, 120)
                .unwrap();
        }
        assert_eq!(agent.steps_used, 6);
        assert_eq!(agent.trace.len(), 6);

        // Attempting a write tool mid-run is denied by the READ_ONLY ceiling
        let write_attempt = agent.record_step("tasks.create", ToolPermissionLevel::LowRisk, 150);
        assert!(write_attempt.is_err());
        assert_eq!(agent.status, AgentStatus::Denied);

        // Fresh agent completes and terminates with collected summary
        let mut clean_agent = spawn_daily_planner_agent(now).unwrap();
        clean_agent
            .record_step("gmail.search", ToolPermissionLevel::ReadOnly, 50)
            .unwrap();
        clean_agent.complete_and_terminate(
            "CRITICAL: Amazon Assessment | HIGH: Interview confirmation | MEDIUM: Solve LeetCode | LOW: Read project feedback",
            now,
        );
        assert_eq!(clean_agent.status, AgentStatus::Terminated);
        assert!(clean_agent.terminated_at.is_some());

        // Cancelled agent records reason and rejects further steps
        let mut cancelled_agent = spawn_daily_planner_agent(now).unwrap();
        cancelled_agent.cancel("User cancelled worker", now);
        assert_eq!(cancelled_agent.status, AgentStatus::Cancelled);
        assert!(cancelled_agent
            .record_step("gmail.search", ToolPermissionLevel::ReadOnly, 10)
            .is_err());
    }
}
