use crate::domain::{CiCheck, ExternalIssue, ExternalRepository, PullRequestState};
use thiserror::Error;

/// Read-only provider contract. Implementations return normalized delivery records and never
/// receive a database handle; the application core remains responsible for persistence.
pub trait SourceControlProvider: Send + Sync {
    fn id(&self) -> &str;
    fn resolve_repository(
        &self,
        remote_or_slug: &str,
    ) -> Result<ExternalRepository, SourceControlError>;
    fn list_issues(
        &self,
        repository: &ExternalRepository,
    ) -> Result<Vec<ExternalIssue>, SourceControlError>;
    fn list_pull_requests(
        &self,
        repository: &ExternalRepository,
    ) -> Result<Vec<PullRequestState>, SourceControlError>;
    fn get_pull_request(
        &self,
        repository: &ExternalRepository,
        number: u64,
    ) -> Result<PullRequestState, SourceControlError>;
    fn get_ci_status(
        &self,
        repository: &ExternalRepository,
        revision: &str,
    ) -> Result<Vec<CiCheck>, SourceControlError>;

    fn synchronize(
        &self,
        repository: &ExternalRepository,
    ) -> Result<(Vec<ExternalIssue>, Vec<PullRequestState>), SourceControlError> {
        let mut issues = self.list_issues(repository)?;
        let pull_request_summaries = self.list_pull_requests(repository)?;
        let mut pull_requests = Vec::with_capacity(pull_request_summaries.len());
        for summary in pull_request_summaries {
            let mut pull_request = self.get_pull_request(repository, summary.number)?;
            pull_request.ci_checks = self.get_ci_status(repository, &pull_request.head_revision)?;
            pull_requests.push(pull_request);
        }

        for issue in &mut issues {
            issue.linked_branch = pull_requests
                .iter()
                .find(|pull_request| pull_request.linked_issue_numbers.contains(&issue.number))
                .map(|pull_request| pull_request.branch.clone());
        }
        Ok((issues, pull_requests))
    }
}

#[derive(Debug, Error)]
pub enum SourceControlError {
    #[error("source-control configuration error: {0}")]
    Configuration(String),
    #[error("source-control request failed: {0}")]
    Request(String),
    #[error("source-control response was invalid: {0}")]
    Response(String),
}
