use std::{collections::BTreeSet, env, time::Duration};

use serde::{de::DeserializeOwned, Deserialize};

use crate::{
    domain::{
        CiCategory, CiCheck, CiConclusion, CiStatus, ExternalIssue, ExternalIssueState,
        ExternalRepository, MergeState, PullRequestState, PullRequestStatus, ReviewComment,
    },
    source_control::{SourceControlError, SourceControlProvider},
};

const DEFAULT_API_URL: &str = "https://api.github.com";
const DEFAULT_WEB_URL: &str = "https://github.com";
const DEFAULT_API_VERSION: &str = "2026-03-10";
const MAX_AUTHENTICATED_PULL_REQUESTS: usize = 20;
const MAX_ANONYMOUS_PULL_REQUESTS: usize = 6;

pub struct GitHubSourceControl {
    agent: ureq::Agent,
    api_url: String,
    web_url: String,
    api_version: String,
    token: Option<String>,
}

impl GitHubSourceControl {
    pub fn from_env() -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(10))
                .timeout_read(Duration::from_secs(30))
                .timeout_write(Duration::from_secs(30))
                .build(),
            api_url: env::var("AGENTRELAY_GITHUB_API_URL")
                .unwrap_or_else(|_| DEFAULT_API_URL.into())
                .trim_end_matches('/')
                .into(),
            web_url: env::var("AGENTRELAY_GITHUB_WEB_URL")
                .unwrap_or_else(|_| DEFAULT_WEB_URL.into())
                .trim_end_matches('/')
                .into(),
            api_version: env::var("AGENTRELAY_GITHUB_API_VERSION")
                .unwrap_or_else(|_| DEFAULT_API_VERSION.into()),
            token: env::var("GITHUB_TOKEN")
                .ok()
                .filter(|value| !value.trim().is_empty()),
        }
    }

    pub fn repository(&self, value: &str) -> Result<ExternalRepository, SourceControlError> {
        let (owner, name) = parse_repository_coordinates(value)?;
        Ok(ExternalRepository {
            provider: "github".into(),
            owner: owner.clone(),
            name: name.clone(),
            web_url: format!("{}/{owner}/{name}", self.web_url),
        })
    }

    fn endpoint(&self, repository: &ExternalRepository, suffix: &str) -> String {
        format!(
            "{}/repos/{}/{}/{}",
            self.api_url,
            repository.owner,
            repository.name,
            suffix.trim_start_matches('/')
        )
    }

    fn get<T: DeserializeOwned>(&self, url: &str) -> Result<T, SourceControlError> {
        let mut request = self
            .agent
            .get(url)
            .set("Accept", "application/vnd.github+json")
            .set("X-GitHub-Api-Version", &self.api_version)
            .set("User-Agent", "AgentRelay");
        if let Some(token) = &self.token {
            request = request.set("Authorization", &format!("Bearer {token}"));
        }
        let response = request.call().map_err(|error| match error {
            ureq::Error::Status(code, response) => {
                let message = response
                    .into_string()
                    .unwrap_or_else(|_| "GitHub returned no response body".into());
                SourceControlError::Request(format!(
                    "GitHub returned HTTP {code}: {}",
                    truncate(&message, 500)
                ))
            }
            ureq::Error::Transport(error) => SourceControlError::Request(error.to_string()),
        })?;
        response
            .into_json::<T>()
            .map_err(|error| SourceControlError::Response(error.to_string()))
    }

    fn pull_request_limit(&self) -> usize {
        if self.token.is_some() {
            MAX_AUTHENTICATED_PULL_REQUESTS
        } else {
            MAX_ANONYMOUS_PULL_REQUESTS
        }
    }
}

impl SourceControlProvider for GitHubSourceControl {
    fn id(&self) -> &str {
        "github"
    }

    fn resolve_repository(
        &self,
        remote_or_slug: &str,
    ) -> Result<ExternalRepository, SourceControlError> {
        self.repository(remote_or_slug)
    }

    fn list_issues(
        &self,
        repository: &ExternalRepository,
    ) -> Result<Vec<ExternalIssue>, SourceControlError> {
        let issues: Vec<GitHubIssue> = self.get(&self.endpoint(
            repository,
            "issues?state=all&sort=updated&direction=desc&per_page=100",
        ))?;
        Ok(issues
            .into_iter()
            .filter(|issue| issue.pull_request.is_none())
            .map(|issue| ExternalIssue {
                id: issue.id.to_string(),
                number: issue.number,
                title: issue.title,
                description: issue.body.unwrap_or_default(),
                labels: issue.labels.into_iter().map(|label| label.name).collect(),
                assignees: issue.assignees.into_iter().map(|user| user.login).collect(),
                state: if issue.state == "closed" {
                    ExternalIssueState::Closed
                } else {
                    ExternalIssueState::Open
                },
                linked_branch: None,
                url: issue.html_url,
                updated_at: issue.updated_at,
            })
            .collect())
    }

    fn list_pull_requests(
        &self,
        repository: &ExternalRepository,
    ) -> Result<Vec<PullRequestState>, SourceControlError> {
        let limit = self.pull_request_limit();
        let pulls: Vec<GitHubPullRequest> = self.get(&self.endpoint(
            repository,
            &format!("pulls?state=all&sort=updated&direction=desc&per_page={limit}"),
        ))?;
        Ok(pulls
            .into_iter()
            .take(limit)
            .map(normalize_pull_request)
            .collect())
    }

    fn get_pull_request(
        &self,
        repository: &ExternalRepository,
        number: u64,
    ) -> Result<PullRequestState, SourceControlError> {
        let detail: GitHubPullRequest =
            self.get(&self.endpoint(repository, &format!("pulls/{number}")))?;
        let files: Vec<GitHubFile> =
            self.get(&self.endpoint(repository, &format!("pulls/{number}/files?per_page=100")))?;
        let commits: Vec<GitHubCommit> =
            self.get(&self.endpoint(repository, &format!("pulls/{number}/commits?per_page=100")))?;
        let reviews: Vec<GitHubReview> =
            self.get(&self.endpoint(repository, &format!("pulls/{number}/reviews?per_page=100")))?;
        let comments: Vec<GitHubReviewComment> =
            self.get(&self.endpoint(repository, &format!("pulls/{number}/comments?per_page=100")))?;
        let mut pull_request = normalize_pull_request(detail);
        pull_request.files = files.into_iter().map(|file| file.filename).collect();
        pull_request.commits = commits.into_iter().map(|commit| commit.sha).collect();
        pull_request.reviewers.extend(
            reviews
                .iter()
                .filter(|review| review.state != "DISMISSED")
                .map(|review| review.user.login.clone()),
        );
        pull_request.reviewers.sort();
        pull_request.reviewers.dedup();
        pull_request.review_comments = normalize_review_comments(reviews, comments);
        Ok(pull_request)
    }

    fn get_ci_status(
        &self,
        repository: &ExternalRepository,
        revision: &str,
    ) -> Result<Vec<CiCheck>, SourceControlError> {
        let response: GitHubCheckRuns = self.get(&self.endpoint(
            repository,
            &format!("commits/{revision}/check-runs?per_page=100"),
        ))?;
        Ok(response
            .check_runs
            .into_iter()
            .map(|check| CiCheck {
                id: check.id.to_string(),
                name: check.name.clone(),
                category: ci_category(&check.name),
                status: match check.status.as_str() {
                    "queued" | "waiting" | "requested" | "pending" => CiStatus::Queued,
                    "in_progress" => CiStatus::InProgress,
                    _ => CiStatus::Completed,
                },
                conclusion: match check.conclusion.as_deref() {
                    Some("success") => CiConclusion::Success,
                    Some("failure") | Some("startup_failure") => CiConclusion::Failure,
                    Some("neutral") => CiConclusion::Neutral,
                    Some("cancelled") => CiConclusion::Cancelled,
                    Some("skipped") => CiConclusion::Skipped,
                    Some("timed_out") => CiConclusion::TimedOut,
                    Some("action_required") => CiConclusion::ActionRequired,
                    _ => CiConclusion::Pending,
                },
                revision: revision.into(),
                details_url: check.details_url,
                started_at: check.started_at,
                completed_at: check.completed_at,
            })
            .collect())
    }
}

fn normalize_pull_request(pull_request: GitHubPullRequest) -> PullRequestState {
    let body = pull_request.body.unwrap_or_default();
    let mut linked_issue_numbers = issue_references(&pull_request.title);
    linked_issue_numbers.extend(issue_references(&body));
    linked_issue_numbers.sort_unstable();
    linked_issue_numbers.dedup();
    PullRequestState {
        id: pull_request.id.to_string(),
        number: pull_request.number,
        title: pull_request.title,
        description: body,
        status: if pull_request.merged_at.is_some() {
            PullRequestStatus::Merged
        } else if pull_request.state == "closed" {
            PullRequestStatus::Closed
        } else if pull_request.draft {
            PullRequestStatus::Draft
        } else {
            PullRequestStatus::Open
        },
        merge_state: match pull_request.mergeable_state.as_deref() {
            Some("clean" | "has_hooks") => MergeState::Ready,
            Some("blocked") => MergeState::Blocked,
            Some("behind") => MergeState::Behind,
            Some("unstable") => MergeState::Unstable,
            Some("dirty") => MergeState::Dirty,
            _ => MergeState::Unknown,
        },
        branch: pull_request.head.reference,
        base_branch: pull_request.base.reference,
        head_revision: pull_request.head.sha,
        author: pull_request.user.login,
        reviewers: pull_request
            .requested_reviewers
            .into_iter()
            .map(|user| user.login)
            .collect(),
        files: Vec::new(),
        commits: Vec::new(),
        linked_issue_numbers,
        ci_checks: Vec::new(),
        review_comments: Vec::new(),
        url: pull_request.html_url,
        updated_at: pull_request.updated_at,
    }
}

fn normalize_review_comments(
    reviews: Vec<GitHubReview>,
    comments: Vec<GitHubReviewComment>,
) -> Vec<ReviewComment> {
    let mut normalized = reviews
        .into_iter()
        .filter(|review| {
            review
                .body
                .as_deref()
                .is_some_and(|body| !body.trim().is_empty())
                && matches!(review.state.as_str(), "CHANGES_REQUESTED" | "COMMENTED")
        })
        .map(|review| ReviewComment {
            id: format!("review:{}", review.id),
            author: review.user.login,
            body: review.body.unwrap_or_default(),
            state: review.state.to_ascii_lowercase(),
            path: None,
            line: None,
            url: review.html_url,
            created_at: review.submitted_at.unwrap_or_default(),
        })
        .collect::<Vec<_>>();
    normalized.extend(comments.into_iter().filter_map(|comment| {
        (!comment.body.trim().is_empty()).then(|| ReviewComment {
            id: format!("comment:{}", comment.id),
            author: comment.user.login,
            body: comment.body,
            state: "commented".into(),
            path: Some(comment.path),
            line: comment.line.or(comment.original_line),
            url: comment.html_url,
            created_at: comment.created_at,
        })
    }));
    normalized.sort_by(|left, right| left.created_at.cmp(&right.created_at));
    normalized.dedup_by(|left, right| left.id == right.id);
    normalized
}

fn issue_references(value: &str) -> Vec<u64> {
    let bytes = value.as_bytes();
    let mut references = BTreeSet::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'#' {
            let start = index + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end > start {
                if let Ok(number) = value[start..end].parse::<u64>() {
                    references.insert(number);
                }
                index = end;
                continue;
            }
        }
        index += 1;
    }
    references.into_iter().collect()
}

fn ci_category(name: &str) -> CiCategory {
    let name = name.to_ascii_lowercase();
    if name.contains("security") || name.contains("codeql") || name.contains("audit") {
        CiCategory::Security
    } else if name.contains("lint") || name.contains("clippy") || name.contains("format") {
        CiCategory::Lint
    } else if name.contains("test") || name.contains("coverage") {
        CiCategory::Test
    } else if name.contains("build") || name.contains("compile") || name.contains("package") {
        CiCategory::Build
    } else {
        CiCategory::Other
    }
}

fn parse_repository_coordinates(value: &str) -> Result<(String, String), SourceControlError> {
    let value = value.trim().trim_end_matches('/').trim_end_matches(".git");
    let path = if let Some((_, path)) = value.split_once(':').filter(|_| value.starts_with("git@"))
    {
        path
    } else if let Some((_, path)) = value.split_once("://") {
        path.split_once('/').map_or(path, |(_, path)| path)
    } else {
        value
    };
    let parts = path
        .split('/')
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>();
    let (owner, name) = match parts.as_slice() {
        [owner, name] => (*owner, *name),
        [.., owner, name] => (*owner, *name),
        _ => {
            return Err(SourceControlError::Configuration(format!(
                "cannot determine GitHub owner/repository from '{value}'"
            )))
        }
    };
    let valid = |part: &str| {
        !part.is_empty()
            && part.len() <= 100
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    };
    if !valid(owner) || !valid(name) {
        return Err(SourceControlError::Configuration(
            "GitHub owner/repository contains unsupported characters".into(),
        ));
    }
    Ok((owner.into(), name.into()))
}

fn truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

#[derive(Deserialize)]
struct GitHubUser {
    login: String,
}

#[derive(Deserialize)]
struct GitHubLabel {
    name: String,
}

#[derive(Deserialize)]
struct GitHubIssue {
    id: u64,
    number: u64,
    title: String,
    body: Option<String>,
    state: String,
    #[serde(default)]
    labels: Vec<GitHubLabel>,
    #[serde(default)]
    assignees: Vec<GitHubUser>,
    html_url: String,
    updated_at: String,
    pull_request: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct GitHubBranch {
    #[serde(rename = "ref")]
    reference: String,
    sha: String,
}

#[derive(Deserialize)]
struct GitHubPullRequest {
    id: u64,
    number: u64,
    title: String,
    body: Option<String>,
    state: String,
    #[serde(default)]
    draft: bool,
    merged_at: Option<String>,
    mergeable_state: Option<String>,
    head: GitHubBranch,
    base: GitHubBranch,
    user: GitHubUser,
    #[serde(default)]
    requested_reviewers: Vec<GitHubUser>,
    html_url: String,
    updated_at: String,
}

#[derive(Deserialize)]
struct GitHubFile {
    filename: String,
}

#[derive(Deserialize)]
struct GitHubCommit {
    sha: String,
}

#[derive(Deserialize)]
struct GitHubReview {
    id: u64,
    user: GitHubUser,
    body: Option<String>,
    state: String,
    html_url: String,
    submitted_at: Option<String>,
}

#[derive(Deserialize)]
struct GitHubReviewComment {
    id: u64,
    user: GitHubUser,
    body: String,
    path: String,
    line: Option<u64>,
    original_line: Option<u64>,
    html_url: String,
    created_at: String,
}

#[derive(Deserialize)]
struct GitHubCheckRuns {
    check_runs: Vec<GitHubCheckRun>,
}

#[derive(Deserialize)]
struct GitHubCheckRun {
    id: u64,
    name: String,
    status: String,
    conclusion: Option<String>,
    details_url: Option<String>,
    started_at: Option<String>,
    completed_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn parses_https_ssh_and_slug_repository_coordinates() {
        assert_eq!(
            parse_repository_coordinates("https://github.com/openai/codex.git").unwrap(),
            ("openai".into(), "codex".into())
        );
        assert_eq!(
            parse_repository_coordinates("git@github.com:openai/codex.git").unwrap(),
            ("openai".into(), "codex".into())
        );
        assert_eq!(
            parse_repository_coordinates("openai/codex").unwrap(),
            ("openai".into(), "codex".into())
        );
    }

    #[test]
    fn extracts_unique_issue_references_and_classifies_checks() {
        assert_eq!(
            issue_references("Fixes #42 and relates to #7, #42"),
            [7, 42]
        );
        assert_eq!(ci_category("cargo clippy"), CiCategory::Lint);
        assert_eq!(ci_category("CodeQL Security"), CiCategory::Security);
        assert_eq!(ci_category("Integration tests"), CiCategory::Test);
    }

    #[test]
    fn network_failure_is_bounded_and_never_exposes_the_token() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let secret = "github-secret-that-must-not-leak";
        let provider = GitHubSourceControl {
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_millis(100))
                .timeout_read(Duration::from_millis(100))
                .timeout_write(Duration::from_millis(100))
                .build(),
            api_url: format!("http://{address}"),
            web_url: "https://github.invalid".into(),
            api_version: DEFAULT_API_VERSION.into(),
            token: Some(secret.into()),
        };
        let repository = provider.repository("agentrelay/offline-test").unwrap();
        let error = provider.list_issues(&repository).unwrap_err().to_string();
        assert!(error.contains("source-control request failed"));
        assert!(!error.contains(secret));
    }
}
