use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use thiserror::Error;
use wait_timeout::ChildExt;

use crate::domain::ProviderId;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedWorktree {
    pub branch: String,
    pub path: PathBuf,
    pub base_commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInspection {
    pub head_commit: String,
    pub clean: bool,
    pub changed_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeAssessment {
    pub mergeable: bool,
    pub detail: String,
}

/// Owns the small, fixed set of Git mutations required for isolated task workspaces.
/// It never accepts arbitrary argv or shell text from an agent or the UI.
pub struct GitWorktreeManager {
    command_timeout: Duration,
}

impl GitWorktreeManager {
    pub fn new(command_timeout: Duration) -> Self {
        Self { command_timeout }
    }

    pub fn create(
        &self,
        repository: impl AsRef<Path>,
        workspace_id: &str,
        task_title: &str,
        provider: &ProviderId,
    ) -> Result<CreatedWorktree, WorktreeError> {
        validate_identifier(workspace_id)?;
        let repository = self.repository_root(repository.as_ref())?;
        let base_commit = self.required(&repository, &["rev-parse", "HEAD"])?;
        let base_commit = first_line(&base_commit, "repository has no HEAD commit")?;
        let short_id = workspace_id
            .chars()
            .filter(|value| *value != '-')
            .take(8)
            .collect::<String>();
        let task_slug = slug(task_title);
        let branch = format!("agentrelay/{}/{}-{short_id}", provider.as_str(), task_slug);
        let root = worktree_root(&repository)?;
        fs::create_dir_all(&root)?;
        let path = root.join(format!("{}-{short_id}", task_slug));
        if path.exists() {
            return Err(WorktreeError::UnsafePath(format!(
                "dedicated worktree path already exists: {}",
                path.display()
            )));
        }
        let path_arg = external_path(&path);
        let output = self.run(
            &repository,
            &["worktree", "add", "-b", &branch, &path_arg, &base_commit],
        )?;
        if output.exit_code != 0 {
            return Err(WorktreeError::CommandFailed {
                command: "git worktree add".into(),
                output: output.text,
            });
        }
        let canonical_path = normalized_canonical(&path)?;
        ensure_managed_path(&repository, &canonical_path)?;
        Ok(CreatedWorktree {
            branch,
            path: canonical_path,
            base_commit,
        })
    }

    pub fn inspect(
        &self,
        worktree_path: impl AsRef<Path>,
        base_commit: &str,
    ) -> Result<WorktreeInspection, WorktreeError> {
        validate_revision(base_commit)?;
        let worktree = normalized_canonical(worktree_path.as_ref())?;
        let head = first_line(
            &self.required(&worktree, &["rev-parse", "HEAD"])?,
            "worktree has no HEAD commit",
        )?;
        let committed = self.required(
            &worktree,
            &["diff", "--name-only", &format!("{base_commit}..HEAD")],
        )?;
        let status = self.required(&worktree, &["status", "--porcelain=v1"])?;
        let mut changed = committed
            .lines()
            .map(normalize_path)
            .filter(|value| !value.is_empty())
            .collect::<BTreeSet<_>>();
        for line in status.lines() {
            if line.len() >= 4 {
                let raw = &line[3..];
                let path = raw.rsplit_once(" -> ").map_or(raw, |(_, path)| path);
                let path = normalize_path(path);
                if !path.is_empty() {
                    changed.insert(path);
                }
            }
        }
        Ok(WorktreeInspection {
            head_commit: head,
            clean: status.trim().is_empty(),
            changed_files: changed.into_iter().collect(),
        })
    }

    pub fn base_is_ancestor(
        &self,
        worktree_path: impl AsRef<Path>,
        base_commit: &str,
        head_commit: &str,
    ) -> Result<bool, WorktreeError> {
        validate_revision(base_commit)?;
        validate_revision(head_commit)?;
        Ok(self
            .run(
                worktree_path.as_ref(),
                &["merge-base", "--is-ancestor", base_commit, head_commit],
            )?
            .exit_code
            == 0)
    }

    pub fn assess_merge(
        &self,
        repository: impl AsRef<Path>,
        branch: &str,
    ) -> Result<MergeAssessment, WorktreeError> {
        validate_branch(branch)?;
        let repository = self.repository_root(repository.as_ref())?;
        let current = first_line(
            &self.required(&repository, &["rev-parse", "HEAD"])?,
            "repository has no HEAD commit",
        )?;
        let output = self.run(
            &repository,
            &["merge-tree", "--write-tree", &current, branch],
        )?;
        Ok(MergeAssessment {
            mergeable: output.exit_code == 0,
            detail: if output.exit_code == 0 {
                "Git merge-tree produced a conflict-free candidate tree without changing the checkout."
                    .into()
            } else {
                compact_output(&output.text, "Git reported a merge conflict.")
            },
        })
    }

    pub fn merge(
        &self,
        repository: impl AsRef<Path>,
        branch: &str,
    ) -> Result<String, WorktreeError> {
        validate_branch(branch)?;
        let repository = self.repository_root(repository.as_ref())?;
        let status = self.required(&repository, &["status", "--porcelain=v1"])?;
        if !status.trim().is_empty() {
            return Err(WorktreeError::DirtyMainWorktree);
        }
        let assessment = self.assess_merge(&repository, branch)?;
        if !assessment.mergeable {
            return Err(WorktreeError::MergeConflict(assessment.detail));
        }
        let output = self.run(&repository, &["merge", "--no-ff", "--no-edit", branch])?;
        if output.exit_code != 0 {
            return Err(WorktreeError::CommandFailed {
                command: "git merge --no-ff --no-edit".into(),
                output: output.text,
            });
        }
        first_line(
            &self.required(&repository, &["rev-parse", "HEAD"])?,
            "merge did not produce a HEAD commit",
        )
    }

    pub fn remove(
        &self,
        repository: impl AsRef<Path>,
        worktree_path: impl AsRef<Path>,
    ) -> Result<(), WorktreeError> {
        let repository = self.repository_root(repository.as_ref())?;
        let worktree = normalized_canonical(worktree_path.as_ref())?;
        ensure_managed_path(&repository, &worktree)?;
        let inspection = self.required(&worktree, &["status", "--porcelain=v1"])?;
        if !inspection.trim().is_empty() {
            return Err(WorktreeError::DirtyTaskWorktree);
        }
        let path = external_path(&worktree);
        let output = self.run(&repository, &["worktree", "remove", &path])?;
        if output.exit_code != 0 {
            return Err(WorktreeError::CommandFailed {
                command: "git worktree remove".into(),
                output: output.text,
            });
        }
        let _ = self.run(&repository, &["worktree", "prune"])?;
        Ok(())
    }

    fn repository_root(&self, requested: &Path) -> Result<PathBuf, WorktreeError> {
        let requested = normalized_canonical(requested)?;
        let root = first_line(
            &self.required(&requested, &["rev-parse", "--show-toplevel"])?,
            "path is not inside a Git repository",
        )?;
        normalized_canonical(Path::new(&root))
    }

    fn required(&self, repository: &Path, arguments: &[&str]) -> Result<String, WorktreeError> {
        let output = self.run(repository, arguments)?;
        if output.exit_code != 0 {
            return Err(WorktreeError::CommandFailed {
                command: format!("git {}", arguments.join(" ")),
                output: output.text,
            });
        }
        Ok(output.text)
    }

    fn run(&self, repository: &Path, arguments: &[&str]) -> Result<GitOutput, WorktreeError> {
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
        let mut child = command.spawn().map_err(WorktreeError::Start)?;
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
                return Err(WorktreeError::Timeout(self.command_timeout.as_secs()));
            }
        };
        let stdout = join_output(stdout_reader)?;
        let stderr = join_output(stderr_reader)?;
        let text = match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
            (false, false) => format!("{stdout}\n{stderr}"),
            (false, true) => stdout,
            (true, false) => stderr,
            (true, true) => String::new(),
        };
        Ok(GitOutput {
            exit_code: status.code().unwrap_or(-1),
            text,
        })
    }
}

#[cfg(not(windows))]
const _: u32 = CREATE_NO_WINDOW;

fn worktree_root(repository: &Path) -> Result<PathBuf, WorktreeError> {
    let parent = repository.parent().ok_or_else(|| {
        WorktreeError::UnsafePath("repository must have a parent directory".into())
    })?;
    let name = repository
        .file_name()
        .and_then(|name| name.to_str())
        .map(slug)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "project".into());
    Ok(parent.join(".agentrelay-worktrees").join(name))
}

fn ensure_managed_path(repository: &Path, candidate: &Path) -> Result<(), WorktreeError> {
    let root = worktree_root(repository)?;
    let root = if root.exists() {
        normalized_canonical(&root)?
    } else {
        root
    };
    if candidate == repository || !candidate.starts_with(&root) {
        return Err(WorktreeError::UnsafePath(candidate.display().to_string()));
    }
    Ok(())
}

fn validate_identifier(value: &str) -> Result<(), WorktreeError> {
    if value.len() >= 8
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        Ok(())
    } else {
        Err(WorktreeError::InvalidIdentifier(value.into()))
    }
}

fn validate_revision(value: &str) -> Result<(), WorktreeError> {
    if (7..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(WorktreeError::InvalidRevision(value.into()))
    }
}

fn validate_branch(value: &str) -> Result<(), WorktreeError> {
    if value.starts_with("agentrelay/")
        && value.len() <= 180
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.'))
        && !value.contains("..")
    {
        Ok(())
    } else {
        Err(WorktreeError::InvalidBranch(value.into()))
    }
}

fn slug(value: &str) -> String {
    let slug = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    let slug = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .take(5)
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "task".into()
    } else {
        slug.chars().take(48).collect()
    }
}

fn normalize_path(value: &str) -> String {
    value.trim().trim_matches('"').replace('\\', "/")
}

fn normalized_canonical(path: &Path) -> Result<PathBuf, WorktreeError> {
    let canonical = path.canonicalize()?;
    #[cfg(windows)]
    {
        let value = canonical.to_string_lossy();
        if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
            return Ok(PathBuf::from(format!(r"\\{rest}")));
        }
        if let Some(rest) = value.strip_prefix(r"\\?\") {
            return Ok(PathBuf::from(rest));
        }
    }
    Ok(canonical)
}

fn external_path(path: &Path) -> String {
    #[cfg(windows)]
    {
        let value = path.to_string_lossy();
        if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{rest}");
        }
        if let Some(rest) = value.strip_prefix(r"\\?\") {
            return rest.to_owned();
        }
    }
    path.to_string_lossy().into_owned()
}

fn first_line(value: &str, error: &str) -> Result<String, WorktreeError> {
    value
        .lines()
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| WorktreeError::CommandFailed {
            command: "git output".into(),
            output: error.into(),
        })
}

fn compact_output(value: &str, fallback: &str) -> String {
    let output = value
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(8)
        .collect::<Vec<_>>()
        .join(" · ");
    if output.is_empty() {
        fallback.into()
    } else {
        output
    }
}

fn join_output(
    reader: thread::JoinHandle<Result<Vec<u8>, std::io::Error>>,
) -> Result<String, WorktreeError> {
    let bytes = reader
        .join()
        .map_err(|_| WorktreeError::OutputReaderPanicked)??;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

struct GitOutput {
    exit_code: i32,
    text: String,
}

#[derive(Debug, Error)]
pub enum WorktreeError {
    #[error("unable to start Git: {0}")]
    Start(#[source] std::io::Error),
    #[error("worktree filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Git command timed out after {0} seconds")]
    Timeout(u64),
    #[error("{command} failed: {output}")]
    CommandFailed { command: String, output: String },
    #[error("invalid workspace identifier: {0}")]
    InvalidIdentifier(String),
    #[error("invalid Git revision: {0}")]
    InvalidRevision(String),
    #[error("AgentRelay refused an unmanaged worktree path: {0}")]
    UnsafePath(String),
    #[error("AgentRelay refused a non-owned branch name: {0}")]
    InvalidBranch(String),
    #[error("the main working tree has uncommitted changes; merge it or clean it before approval")]
    DirtyMainWorktree,
    #[error("the task worktree has uncommitted changes; commit them before cleanup")]
    DirtyTaskWorktree,
    #[error("the candidate branch conflicts with the current project branch: {0}")]
    MergeConflict(String),
    #[error("Git output reader stopped unexpectedly")]
    OutputReaderPanicked,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn git(directory: &Path, arguments: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(directory)
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn creates_validates_merges_and_removes_owned_worktree() {
        let directory = TempDir::new().unwrap();
        let repository = directory.path().join("project");
        fs::create_dir(&repository).unwrap();
        git(&repository, &["init"]);
        git(
            &repository,
            &["config", "user.email", "agentrelay@example.test"],
        );
        git(&repository, &["config", "user.name", "AgentRelay Test"]);
        fs::write(repository.join("README.md"), "base\n").unwrap();
        git(&repository, &["add", "README.md"]);
        git(&repository, &["commit", "-m", "base"]);

        let manager = GitWorktreeManager::new(Duration::from_secs(10));
        let created = manager
            .create(
                &repository,
                "workspace-12345678",
                "Provider registry",
                &ProviderId::codex(),
            )
            .unwrap();
        assert!(created.path.is_dir());
        fs::write(created.path.join("provider.rs"), "pub struct Provider;\n").unwrap();
        git(&created.path, &["add", "provider.rs"]);
        git(&created.path, &["commit", "-m", "add provider"]);

        let inspection = manager
            .inspect(&created.path, &created.base_commit)
            .unwrap();
        assert!(inspection.clean);
        assert_eq!(inspection.changed_files, ["provider.rs"]);
        assert!(manager
            .base_is_ancestor(&created.path, &created.base_commit, &inspection.head_commit)
            .unwrap());
        assert!(
            manager
                .assess_merge(&repository, &created.branch)
                .unwrap()
                .mergeable
        );
        manager.merge(&repository, &created.branch).unwrap();
        assert!(repository.join("provider.rs").is_file());
        manager.remove(&repository, &created.path).unwrap();
        assert!(!created.path.exists());
    }

    #[test]
    fn rejects_cleanup_outside_the_managed_worktree_root() {
        let directory = TempDir::new().unwrap();
        let repository = directory.path().join("project");
        fs::create_dir(&repository).unwrap();
        git(&repository, &["init"]);
        git(
            &repository,
            &["config", "user.email", "agentrelay@example.test"],
        );
        git(&repository, &["config", "user.name", "AgentRelay Test"]);
        fs::write(repository.join("README.md"), "base\n").unwrap();
        git(&repository, &["add", "README.md"]);
        git(&repository, &["commit", "-m", "base"]);
        let manager = GitWorktreeManager::new(Duration::from_secs(10));
        let error = manager.remove(&repository, &repository).unwrap_err();
        assert!(matches!(error, WorktreeError::UnsafePath(_)));
    }
}
