use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ToolPermissionLevel {
    ReadOnly,
    LowRisk,
    ConfirmationRequired,
    HighImpact,
}

impl ToolPermissionLevel {
    pub fn number(self) -> u8 {
        match self {
            Self::ReadOnly => 0,
            Self::LowRisk => 1,
            Self::ConfirmationRequired => 2,
            Self::HighImpact => 3,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDescriptor {
    pub name: &'static str,
    pub description: &'static str,
    pub permission: ToolPermissionLevel,
    pub arguments: &'static [&'static str],
}

pub fn registry() -> Vec<ToolDescriptor> {
    vec![
        ToolDescriptor {
            name: "browser.open",
            description: "Open a validated web address in the default browser",
            permission: ToolPermissionLevel::LowRisk,
            arguments: &["url"],
        },
        ToolDescriptor {
            name: "leetcode.daily",
            description: "Resolve today's LeetCode challenge through structured data",
            permission: ToolPermissionLevel::ReadOnly,
            arguments: &[],
        },
        ToolDescriptor {
            name: "tasks.today",
            description: "Read incomplete personal tasks due today or earlier",
            permission: ToolPermissionLevel::ReadOnly,
            arguments: &[],
        },
        ToolDescriptor {
            name: "tasks.create",
            description: "Create a personal task in Sharman's local database",
            permission: ToolPermissionLevel::LowRisk,
            arguments: &["title", "due_at"],
        },
        ToolDescriptor {
            name: "tasks.list",
            description: "Read bounded open, completed, overdue or all local personal tasks",
            permission: ToolPermissionLevel::ReadOnly,
            arguments: &["view"],
        },
        ToolDescriptor {
            name: "tasks.find",
            description: "Find an exact open-task title for review without modifying it",
            permission: ToolPermissionLevel::ReadOnly,
            arguments: &["title"],
        },
        ToolDescriptor {
            name: "reminders.list",
            description: "Read the next pending local reminders ordered by time",
            permission: ToolPermissionLevel::ReadOnly,
            arguments: &[],
        },
        ToolDescriptor {
            name: "reminders.find",
            description: "Find an exact pending reminder for review without cancelling it",
            permission: ToolPermissionLevel::ReadOnly,
            arguments: &["title"],
        },
        ToolDescriptor {
            name: "reminders.create",
            description: "Schedule a local desktop reminder",
            permission: ToolPermissionLevel::LowRisk,
            arguments: &["title", "remind_at"],
        },
        ToolDescriptor {
            name: "notifications.show",
            description: "Show a local Windows notification for a due reminder",
            permission: ToolPermissionLevel::LowRisk,
            arguments: &["title", "body"],
        },
        ToolDescriptor {
            name: "gmail.search",
            description: "Search Gmail metadata with the read-only OAuth scope",
            permission: ToolPermissionLevel::ReadOnly,
            arguments: &["query"],
        },
        ToolDescriptor {
            name: "gmail.list_unread",
            description: "List unread Gmail message metadata using the fixed is:unread query",
            permission: ToolPermissionLevel::ReadOnly,
            arguments: &[],
        },
        ToolDescriptor {
            name: "gmail.list_recent",
            description: "List recent inbox message metadata using a bounded read-only query",
            permission: ToolPermissionLevel::ReadOnly,
            arguments: &[],
        },
        ToolDescriptor {
            name: "gmail.connection_status",
            description: "Read the authoritative Gmail credential and last-sync status",
            permission: ToolPermissionLevel::ReadOnly,
            arguments: &[],
        },
        ToolDescriptor {
            name: "gmail.get_message",
            description: "Read bounded Gmail message metadata and snippet text",
            permission: ToolPermissionLevel::ReadOnly,
            arguments: &["message_id"],
        },
        ToolDescriptor {
            name: "gmail.open_in_browser",
            description: "Open a validated Gmail thread in the default browser",
            permission: ToolPermissionLevel::ReadOnly,
            arguments: &["thread_id"],
        },
    ]
}

#[derive(Debug, Clone)]
pub struct LeetcodeDaily {
    pub title: String,
    pub difficulty: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
struct LeetcodeResponse {
    data: Option<LeetcodeData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LeetcodeData {
    active_daily_coding_challenge_question: Option<DailyChallenge>,
}

#[derive(Debug, Deserialize)]
struct DailyChallenge {
    link: String,
    question: DailyQuestion,
}

#[derive(Debug, Deserialize)]
struct DailyQuestion {
    title: String,
    difficulty: String,
}

pub fn resolve_leetcode_daily() -> Result<LeetcodeDaily, String> {
    let response = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(8))
        .timeout_write(Duration::from_secs(3))
        .build()
        .post("https://leetcode.com/graphql")
        .set("Content-Type", "application/json")
        .set("User-Agent", "Sharman/0.21 local assistant")
        .send_json(serde_json::json!({
            "query": "query questionOfToday { activeDailyCodingChallengeQuestion { link question { title difficulty } } }"
        }))
        .map_err(|error| format!("LeetCode did not return today's challenge: {error}"))?
        .into_json::<LeetcodeResponse>()
        .map_err(|error| format!("LeetCode returned an unreadable response: {error}"))?;
    let daily = response
        .data
        .and_then(|data| data.active_daily_coding_challenge_question)
        .ok_or_else(|| "LeetCode did not include an active daily challenge.".to_owned())?;
    if !is_safe_leetcode_path(&daily.link) {
        return Err("LeetCode returned an unexpected challenge address.".to_owned());
    }
    Ok(LeetcodeDaily {
        title: daily.question.title,
        difficulty: daily.question.difficulty,
        url: format!("https://leetcode.com{}", daily.link),
    })
}

fn is_safe_leetcode_path(path: &str) -> bool {
    path.starts_with("/problems/")
        && path.len() <= 240
        && path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.'))
}

pub fn is_trusted_browser_url(url: &str) -> bool {
    url == "https://leetcode.com"
        || url == "https://leetcode.com/problemset/"
        || (url.starts_with("https://leetcode.com/problems/")
            && is_safe_leetcode_path(&url["https://leetcode.com".len()..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_browser_scope_is_narrow() {
        assert!(is_trusted_browser_url("https://leetcode.com"));
        assert!(is_trusted_browser_url(
            "https://leetcode.com/problems/two-sum/"
        ));
        assert!(!is_trusted_browser_url(
            "http://leetcode.com/problems/two-sum/"
        ));
        assert!(!is_trusted_browser_url(
            "https://leetcode.com.evil.test/problems/x/"
        ));
        assert!(!is_trusted_browser_url(
            "https://leetcode.com/account/delete"
        ));
    }

    #[test]
    fn registry_has_explicit_permissions() {
        let tools = registry();
        assert!(tools.iter().any(|tool| {
            tool.name == "tasks.today" && tool.permission == ToolPermissionLevel::ReadOnly
        }));
        assert!(tools.iter().any(|tool| {
            tool.name == "reminders.create" && tool.permission == ToolPermissionLevel::LowRisk
        }));
    }
}
