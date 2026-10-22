use std::{
    collections::BTreeSet,
    io::Read,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use wait_timeout::ChildExt;

use crate::domain::{FileChange, FileChangeType};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStatus {
    pub repository: String,
    pub branch: String,
    pub modified_files: Vec<String>,
    pub added_files: Vec<String>,
    pub deleted_files: Vec<String>,
    pub last_commit: String,
    #[serde(default)]
    pub head_commit: String,
    pub recent_commits: Vec<String>,
    pub diff_stat: Vec<String>,
    pub scanned_at: String,
}

impl ProjectStatus {
    pub fn change_count(&self) -> usize {
        self.modified_files.len() + self.added_files.len() + self.deleted_files.len()
    }
}

pub struct GitScanner {
    command_timeout: Duration,
}

impl GitScanner {
    pub fn new(command_timeout: Duration) -> Self {
        Self { command_timeout }
    }

    pub fn scan(&self, repository_path: impl AsRef<Path>) -> Result<ProjectStatus, GitError> {
        let requested_path = repository_path.as_ref();
        if !requested_path.is_dir() {
            return Err(GitError::RepositoryMissing(
                requested_path.to_string_lossy().into_owned(),
            ));
        }

        let repository = requested_path.canonicalize()?;
        let validation = self.run_git(&repository, &["rev-parse", "--is-inside-work-tree"])?;
        if validation.exit_code != 0
            || !validation
                .lines
                .iter()
                .any(|line| line.eq_ignore_ascii_case("true"))
        {
            return Err(GitError::NotARepository(repository.display().to_string()));
        }

        let branch_result = self.required(&repository, &["branch", "--show-current"])?;
        let branch = branch_result
            .first()
            .filter(|value| !value.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| "detached HEAD".to_owned());

        let changes = parse_status(self.required(&repository, &["status", "--short"])?);
        let recent_commits = self.optional(&repository, &["log", "-5", "--pretty=%h %s"])?;
        let head_commit = self
            .optional(&repository, &["rev-parse", "HEAD"])?
            .into_iter()
            .next()
            .unwrap_or_default();
        let last_commit = recent_commits
            .first()
            .map(|commit| strip_commit_hash(commit))
            .unwrap_or_else(|| "No commits yet".to_owned());

        Ok(ProjectStatus {
            repository: path_for_display(&repository),
            branch,
            modified_files: changes.modified,
            added_files: changes.added,
            deleted_files: changes.deleted,
            last_commit,
            head_commit,
            recent_commits,
            diff_stat: self.required(&repository, &["diff", "--stat"])?,
            scanned_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        })
    }

    /// Returns Git-tracked and non-ignored untracked files as normalized repository-relative paths.
    /// This remains read-only and uses a fixed argument list owned by the core.
    pub(crate) fn project_files(
        &self,
        repository_path: impl AsRef<Path>,
    ) -> Result<Vec<String>, GitError> {
        let mut files = self.required(
            repository_path.as_ref(),
            &["ls-files", "--cached", "--others", "--exclude-standard"],
        )?;
        files.retain(|path| !path.trim().is_empty());
        files.iter_mut().for_each(|path| {
            *path = path.trim_matches('"').replace('\\', "/");
        });
        files.sort();
        files.dedup();
        Ok(files)
    }

    /// Returns the configured origin URL through a fixed read-only Git command.
    pub(crate) fn origin_url(&self, repository_path: impl AsRef<Path>) -> Result<String, GitError> {
        self.required(repository_path.as_ref(), &["remote", "get-url", "origin"])?
            .into_iter()
            .next()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| GitError::CommandFailed {
                command: "git remote get-url origin".into(),
                output: "repository has no origin remote".into(),
            })
    }

    pub(crate) fn commits_between(
        &self,
        repository_path: impl AsRef<Path>,
        base: &str,
        final_head: &str,
    ) -> Result<Vec<String>, GitError> {
        if base.is_empty() || final_head.is_empty() || base == final_head {
            return Ok(Vec::new());
        }
        validate_revision(base)?;
        validate_revision(final_head)?;
        let range = format!("{base}..{final_head}");
        self.required(
            repository_path.as_ref(),
            &["log", "--reverse", "--pretty=%h %s", &range],
        )
    }

    pub(crate) fn changed_files_between(
        &self,
        repository_path: impl AsRef<Path>,
        base: &str,
        final_head: &str,
    ) -> Result<Vec<FileChange>, GitError> {
        if base.is_empty() || final_head.is_empty() || base == final_head {
            return Ok(Vec::new());
        }
        validate_revision(base)?;
        validate_revision(final_head)?;
        let range = format!("{base}..{final_head}");
        let lines = self.required(repository_path.as_ref(), &["diff", "--name-status", &range])?;
        Ok(parse_name_status(lines))
    }

    fn required(&self, repository: &Path, arguments: &[&str]) -> Result<Vec<String>, GitError> {
        let result = self.run_git(repository, arguments)?;
        if result.exit_code != 0 {
            return Err(GitError::CommandFailed {
                command: format!("git {}", arguments.join(" ")),
                output: result.lines.join(" "),
            });
        }
        Ok(result.lines)
    }

    fn optional(&self, repository: &Path, arguments: &[&str]) -> Result<Vec<String>, GitError> {
        let result = self.run_git(repository, arguments)?;
        Ok(if result.exit_code == 0 {
            result.lines
        } else {
            Vec::new()
        })
    }

    fn run_git(&self, repository: &Path, arguments: &[&str]) -> Result<CommandResult, GitError> {
        let mut command = Command::new("git");
        command
            .arg("-c")
            .arg("core.quotepath=false")
            .arg("-C")
            .arg(repository)
            .args(arguments)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = command.spawn().map_err(GitError::Start)?;
        let mut stdout = child.stdout.take().expect("stdout is piped");
        let mut stderr = child.stderr.take().expect("stderr is piped");

        let stdout_reader = thread::spawn(move || {
            let mut bytes = Vec::new();
            stdout.read_to_end(&mut bytes).map(|_| bytes)
        });
        let stderr_reader = thread::spawn(move || {
            let mut bytes = Vec::new();
            stderr.read_to_end(&mut bytes).map(|_| bytes)
        });

        let status = match child.wait_timeout(self.command_timeout)? {
            Some(status) => status,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(GitError::Timeout {
                    command: format!("git {}", arguments.join(" ")),
                    seconds: self.command_timeout.as_secs(),
                });
            }
        };

        let stdout = join_reader(stdout_reader)?;
        let stderr = join_reader(stderr_reader)?;
        let combined = if stderr.is_empty() {
            stdout
        } else if stdout.is_empty() {
            stderr
        } else {
            format!("{stdout}\n{stderr}")
        };

        Ok(CommandResult {
            exit_code: status.code().unwrap_or(-1),
            lines: combined.lines().map(str::to_owned).collect(),
        })
    }
}

#[cfg(not(windows))]
const _: u32 = CREATE_NO_WINDOW;

fn join_reader(
    reader: thread::JoinHandle<Result<Vec<u8>, std::io::Error>>,
) -> Result<String, GitError> {
    let bytes = reader
        .join()
        .map_err(|_| GitError::OutputReaderPanicked)??;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn parse_status(lines: Vec<String>) -> FileChanges {
    let mut modified = BTreeSet::new();
    let mut added = BTreeSet::new();
    let mut deleted = BTreeSet::new();

    for line in lines {
        if line.len() < 4 {
            continue;
        }
        let state = &line[..2];
        let path = normalize_status_path(&line[3..]);
        if state == "??" || state.contains('A') {
            added.insert(path);
        } else if state.contains('D') {
            deleted.insert(path);
        } else {
            modified.insert(path);
        }
    }

    FileChanges {
        modified: modified.into_iter().collect(),
        added: added.into_iter().collect(),
        deleted: deleted.into_iter().collect(),
    }
}

fn normalize_status_path(path: &str) -> String {
    let renamed = path
        .rsplit_once(" -> ")
        .map_or(path, |(_, new_path)| new_path);
    renamed.trim_matches('"').replace('\\', "/")
}

fn strip_commit_hash(commit: &str) -> String {
    commit
        .split_once(' ')
        .map_or(commit, |(_, subject)| subject)
        .to_owned()
}

fn validate_revision(revision: &str) -> Result<(), GitError> {
    if (7..=64).contains(&revision.len()) && revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(GitError::InvalidRevision(revision.to_owned()))
    }
}

fn parse_name_status(lines: Vec<String>) -> Vec<FileChange> {
    lines
        .into_iter()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let state = fields.next()?;
            let first_path = fields.next()?;
            let (path, change_type) = match state.chars().next()? {
                'A' => (first_path, FileChangeType::Created),
                'D' => (first_path, FileChangeType::Deleted),
                'R' => (fields.next().unwrap_or(first_path), FileChangeType::Renamed),
                _ => (first_path, FileChangeType::Modified),
            };
            Some(FileChange {
                path: path.replace('\\', "/"),
                change_type,
            })
        })
        .collect()
}

fn path_for_display(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

struct CommandResult {
    exit_code: i32,
    lines: Vec<String>,
}

struct FileChanges {
    modified: Vec<String>,
    added: Vec<String>,
    deleted: Vec<String>,
}

#[derive(Debug, Error)]
pub enum GitError {
    #[error("repository does not exist: {0}")]
    RepositoryMissing(String),
    #[error("path is not inside a Git work tree: {0}")]
    NotARepository(String),
    #[error("unable to start Git; confirm git is available on PATH: {0}")]
    Start(std::io::Error),
    #[error("Git command failed ({command}): {output}")]
    CommandFailed { command: String, output: String },
    #[error("Git command timed out after {seconds} seconds: {command}")]
    Timeout { command: String, seconds: u64 },
    #[error("Git output reader stopped unexpectedly")]
    OutputReaderPanicked,
    #[error("invalid Git revision stored for a session: {0}")]
    InvalidRevision(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, process::Command};
    use tempfile::TempDir;

    #[test]
    fn scans_a_real_repository() {
        let repository = TempDir::new().unwrap();
        git(repository.path(), &["init", "-b", "main"]);
        git(
            repository.path(),
            &["config", "user.name", "AgentRelay Test"],
        );
        git(
            repository.path(),
            &["config", "user.email", "agentrelay@example.test"],
        );
        fs::write(repository.path().join("tracked.txt"), "before\n").unwrap();
        git(repository.path(), &["add", "tracked.txt"]);
        git(repository.path(), &["commit", "-m", "Initial project"]);
        fs::write(repository.path().join("tracked.txt"), "before\nafter\n").unwrap();
        fs::write(repository.path().join("new file.txt"), "new\n").unwrap();

        let status = GitScanner::new(Duration::from_secs(5))
            .scan(repository.path())
            .unwrap();

        assert_eq!(status.branch, "main");
        assert_eq!(status.modified_files, ["tracked.txt"]);
        assert_eq!(status.added_files, ["new file.txt"]);
        assert_eq!(status.last_commit, "Initial project");
        assert!(status
            .diff_stat
            .iter()
            .any(|line| line.contains("tracked.txt")));
    }

    #[test]
    fn rejects_a_non_git_directory() {
        let directory = TempDir::new().unwrap();
        let error = GitScanner::new(Duration::from_secs(5))
            .scan(directory.path())
            .unwrap_err();
        assert!(matches!(error, GitError::NotARepository(_)));
    }

    fn git(repository: &Path, arguments: &[&str]) {
        let result = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
