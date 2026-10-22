use std::{
    env,
    io::Read,
    process::{Command, Stdio},
    thread,
    time::Instant,
};

use wait_timeout::ChildExt;

use crate::ports::{
    ExecutionSandbox, SandboxError, SandboxExecutionOutput, SandboxExecutionRequest,
};

const OUTPUT_LIMIT: usize = 256 * 1024;

/// Token Factory Sandbox adapter using the official ConTree CLI. Repository files are uploaded to
/// an isolated, disposable instance; project commands never execute on the AgentRelay host.
pub struct ContreeCliSandbox {
    cli: String,
    image: String,
}

impl ContreeCliSandbox {
    pub fn from_env() -> Result<Option<Self>, String> {
        let image = match env::var("AGENTRELAY_SANDBOX_IMAGE") {
            Ok(value) if !value.trim().is_empty() => value,
            _ => return Ok(None),
        };
        let cli = env::var("AGENTRELAY_CONTREE_CLI").unwrap_or_else(|_| "contree".into());
        if cli.trim().is_empty() {
            return Err("AGENTRELAY_CONTREE_CLI cannot be empty".into());
        }
        Ok(Some(Self { cli, image }))
    }
}

impl ExecutionSandbox for ContreeCliSandbox {
    fn name(&self) -> &str {
        "Nebius Token Factory Sandbox"
    }

    fn execute(
        &self,
        request: &SandboxExecutionRequest,
    ) -> Result<SandboxExecutionOutput, SandboxError> {
        let repository = request
            .repository_path
            .canonicalize()
            .map_err(|error| SandboxError::Unavailable(error.to_string()))?;
        let session = format!(
            "agentrelay-{}",
            request
                .project_id
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .take(12)
                .collect::<String>()
        );
        let file_mapping = format!("{}:/workspace", repository.display());
        let timeout_seconds = request.timeout.as_secs().max(1);
        let mut command = Command::new(&self.cli);
        command.args([
            "-S",
            &session,
            "run",
            "--use",
            &self.image,
            "--disposable",
            "--timeout",
            &timeout_seconds.to_string(),
            "--truncate",
            &OUTPUT_LIMIT.to_string(),
            "--cwd",
            "/workspace",
            "--file",
            &file_mapping,
            "--",
            &request.command.program,
        ]);
        command.args(&request.command.args);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }

        let started = Instant::now();
        let mut child = command.spawn().map_err(|error| {
            SandboxError::Unavailable(format!("could not start ConTree CLI: {error}"))
        })?;
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");
        let stdout_reader = thread::spawn(move || read_bounded(stdout));
        let stderr_reader = thread::spawn(move || read_bounded(stderr));

        let status = match child
            .wait_timeout(request.timeout)
            .map_err(|error| SandboxError::Execution(error.to_string()))?
        {
            Some(status) => status,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(SandboxError::Timeout(timeout_seconds));
            }
        };
        let stdout = stdout_reader
            .join()
            .map_err(|_| SandboxError::Execution("stdout reader stopped unexpectedly".into()))?;
        let stderr = stderr_reader
            .join()
            .map_err(|_| SandboxError::Execution("stderr reader stopped unexpectedly".into()))?;
        Ok(SandboxExecutionOutput {
            exit_code: status.code().unwrap_or(-1),
            stdout,
            stderr,
            duration: started.elapsed(),
        })
    }
}

fn read_bounded(stream: impl Read) -> String {
    let mut bytes = Vec::new();
    let _ = stream.take(OUTPUT_LIMIT as u64).read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}
