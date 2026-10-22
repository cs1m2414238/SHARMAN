use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::Read,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use chrono::{SecondsFormat, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;
use wait_timeout::ChildExt;

use crate::domain::{
    EnvironmentDrift, EnvironmentManifest, EnvironmentSnapshot, EnvironmentStatus,
    EnvironmentVariableRequirement, EnvironmentVariableState, ExecutionProfile, FingerprintInput,
    ToolObservation, ToolStatus,
};

const MANIFEST_PATH: &str = ".agentrelay/environment.toml";
const OUTPUT_LIMIT: usize = 256 * 1024;

pub struct EnvironmentService {
    timeout: Duration,
}

pub struct EnvironmentInspection<'a> {
    pub manifest: &'a EnvironmentManifest,
    pub manifest_source: &'a str,
    pub repository: &'a Path,
    pub project_id: &'a str,
    pub commit: &'a str,
    pub branch: &'a str,
    pub last_verified_fingerprint: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentCommandOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration: Duration,
}

impl EnvironmentService {
    pub const fn new(timeout: Duration) -> Self {
        Self { timeout }
    }

    pub fn load_or_infer(
        &self,
        repository: &Path,
    ) -> Result<(EnvironmentManifest, String), EnvironmentError> {
        let path = repository.join(MANIFEST_PATH);
        if path.is_file() {
            let source = fs::read_to_string(&path)?;
            let manifest: EnvironmentManifest = toml::from_str(&source)?;
            validate_manifest(&manifest)?;
            return Ok((manifest, MANIFEST_PATH.into()));
        }
        Ok((self.infer(repository)?, "inferred (not saved)".into()))
    }

    pub fn save(
        &self,
        repository: &Path,
        manifest: &EnvironmentManifest,
    ) -> Result<PathBuf, EnvironmentError> {
        validate_manifest(manifest)?;
        let directory = repository.join(".agentrelay");
        fs::create_dir_all(&directory)?;
        let path = directory.join("environment.toml");
        fs::write(&path, toml::to_string_pretty(manifest)?)?;
        Ok(path)
    }

    pub fn infer(&self, repository: &Path) -> Result<EnvironmentManifest, EnvironmentError> {
        let mut runtimes = BTreeMap::new();
        let has_cargo = repository.join("Cargo.toml").is_file();
        let frontend = repository.join("frontend/package.json");
        let root_package = repository.join("package.json");
        let package_path = if frontend.is_file() {
            Some(frontend.as_path())
        } else if root_package.is_file() {
            Some(root_package.as_path())
        } else {
            None
        };
        if has_cargo {
            let expected = read_rust_requirement(repository)
                .or_else(|| self.detect_version("rustc", &["--version"]).ok().flatten())
                .and_then(|value| extract_version(&value))
                .unwrap_or_else(|| "installed".into());
            runtimes.insert("rust".into(), expected);
        }
        if let Some(package) = package_path {
            let package_json = read_json(package)?;
            let expected = package_json
                .pointer("/engines/node")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| read_first(repository, &[".nvmrc", ".node-version"]))
                .or_else(|| self.detect_version("node", &["--version"]).ok().flatten())
                .and_then(|value| extract_version(&value))
                .unwrap_or_else(|| "installed".into());
            runtimes.insert("node".into(), expected);
        }

        let package_manager = package_path
            .and_then(|path| {
                read_json(path).ok().and_then(|value| {
                    value
                        .get("packageManager")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
            })
            .or_else(|| {
                repository
                    .join("frontend/package-lock.json")
                    .is_file()
                    .then_some("npm".into())
                    .or_else(|| {
                        repository
                            .join("package-lock.json")
                            .is_file()
                            .then_some("npm".into())
                    })
            });

        let mut commands = BTreeMap::new();
        if has_cargo {
            commands.insert(
                "format".into(),
                strings(["cargo", "fmt", "--all", "--", "--check"]),
            );
            commands.insert(
                "lint".into(),
                strings([
                    "cargo",
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--",
                    "-D",
                    "warnings",
                ]),
            );
            commands.insert("test".into(), strings(["cargo", "test", "--workspace"]));
            commands.insert("build".into(), strings(["cargo", "build", "--workspace"]));
        }
        if frontend.is_file() && has_script(&frontend, "build")? {
            commands.insert(
                "frontendBuild".into(),
                strings(["npm", "--prefix", "frontend", "run", "build"]),
            );
        } else if root_package.is_file() && has_script(&root_package, "build")? {
            commands.insert("frontendBuild".into(), strings(["npm", "run", "build"]));
        }
        if repository
            .join("frontend/src-tauri/tauri.conf.json")
            .is_file()
            && has_script(&frontend, "tauri")?
        {
            commands.insert(
                "desktopBuild".into(),
                strings(["npm", "--prefix", "frontend", "run", "tauri", "--", "build"]),
            );
        }

        let mut system_tools = vec!["git".into()];
        let is_tauri = repository
            .join("frontend/src-tauri/tauri.conf.json")
            .is_file()
            || repository.join("src-tauri/tauri.conf.json").is_file();
        if is_tauri {
            system_tools.extend(["tauri-cli".into(), "msvc".into()]);
        }
        let mut environment = environment_names(repository)?
            .into_iter()
            .map(|name| EnvironmentVariableRequirement {
                name,
                required: false,
                secret: true,
                value: None,
            })
            .collect::<Vec<_>>();
        if repository
            .join("crates/agentrelay-core/src/source_control.rs")
            .is_file()
        {
            for name in ["GITHUB_TOKEN", "NEBIUS_API_KEY", "AGENTRELAY_SANDBOX_IMAGE"] {
                if environment.iter().all(|item| item.name != name) {
                    environment.push(EnvironmentVariableRequirement {
                        name: name.into(),
                        required: false,
                        secret: true,
                        value: None,
                    });
                }
            }
        }
        environment.sort_by(|left, right| left.name.cmp(&right.name));
        let architecture_rules = if repository
            .join("crates/agentrelay-core/Cargo.toml")
            .is_file()
        {
            vec![
                "core-must-not-depend-on-tauri".into(),
                "frontend-must-not-spawn-processes".into(),
            ]
        } else {
            Vec::new()
        };
        let mut cache_kinds = Vec::new();
        if has_cargo {
            cache_kinds.extend(["cargoRegistry".into(), "cargoTarget".into()]);
        }
        if package_path.is_some() {
            cache_kinds.push("npm".into());
        }
        Ok(EnvironmentManifest {
            schema_version: 1,
            profile: ExecutionProfile::NativeWindows,
            working_directory: ".".into(),
            container_image: None,
            disposable_verification: true,
            runtimes,
            package_manager,
            system_tools,
            commands,
            environment,
            cache_kinds,
            architecture_rules,
        })
    }

    pub fn inspect(
        &self,
        input: EnvironmentInspection<'_>,
    ) -> Result<EnvironmentSnapshot, EnvironmentError> {
        let EnvironmentInspection {
            manifest,
            manifest_source,
            repository,
            project_id,
            commit,
            branch,
            last_verified_fingerprint,
        } = input;
        validate_manifest(manifest)?;
        let mut tools = Vec::new();
        if let Some(expected) = manifest.runtimes.get("rust") {
            tools.push(self.observe("Rust", Some(expected), "rustc", &["--version"]));
        }
        if let Some(expected) = manifest.runtimes.get("node") {
            tools.push(self.observe("Node", Some(expected), "node", &["--version"]));
        }
        if let Some(manager) = &manifest.package_manager {
            let (program, expected) = manager
                .split_once('@')
                .map(|(program, version)| (program, Some(version.to_owned())))
                .unwrap_or((manager.as_str(), None));
            tools.push(self.observe(
                "Package manager",
                expected.as_ref(),
                program,
                &["--version"],
            ));
        }
        for tool in &manifest.system_tools {
            let observation = match tool.as_str() {
                "git" => self.observe("Git", None, "git", &["--version"]),
                "tauri-cli" => {
                    if repository.join("frontend/package.json").is_file() {
                        self.observe_in(
                            "Tauri CLI",
                            None,
                            "npm",
                            &["--prefix", "frontend", "run", "tauri", "--", "--version"],
                            repository,
                        )
                    } else {
                        self.observe("Tauri CLI", None, "cargo", &["tauri", "--version"])
                    }
                }
                "msvc" => self.observe_msvc(),
                "docker" => self.observe("Docker", None, "docker", &["--version"]),
                other => self.observe(other, None, other, &["--version"]),
            };
            tools.push(observation);
        }
        if manifest.profile == ExecutionProfile::Container
            && tools.iter().all(|tool| tool.name != "Docker")
        {
            tools.push(self.observe("Docker", None, "docker", &["--version"]));
        }
        let environment = manifest
            .environment
            .iter()
            .map(|requirement| EnvironmentVariableState {
                name: requirement.name.clone(),
                required: requirement.required,
                secret: requirement.secret,
                configured: requirement
                    .value
                    .as_ref()
                    .is_some_and(|value| !value.is_empty())
                    || env::var_os(&requirement.name).is_some(),
            })
            .collect::<Vec<_>>();
        let mut fingerprint_inputs = fingerprint_files(repository)?;
        let manifest_digest = digest(&serde_json::to_vec(manifest)?);
        fingerprint_inputs.push(FingerprintInput {
            kind: "environmentManifest".into(),
            path: manifest_source.into(),
            digest: manifest_digest,
        });
        let os = os_description();
        let architecture = env::consts::ARCH.to_owned();
        let fingerprint = fingerprint(
            manifest,
            &tools,
            &environment,
            &fingerprint_inputs,
            &os,
            &architecture,
        )?;
        let mut drift = Vec::new();
        for tool in &tools {
            if tool.status == ToolStatus::Drift {
                drift.push(EnvironmentDrift {
                    subject: tool.name.clone(),
                    expected: tool.expected.clone().unwrap_or_default(),
                    detected: tool.detected.clone().unwrap_or_else(|| "missing".into()),
                    detail: "Installed toolchain does not match the environment manifest.".into(),
                });
            }
        }
        if manifest.profile == ExecutionProfile::SandboxedLocal {
            drift.push(EnvironmentDrift {
                subject: "Sandbox adapter".into(),
                expected: "configured".into(),
                detected: "unavailable".into(),
                detail: "The sandboxed-local profile is reserved until an execution adapter is configured; no native fallback is allowed.".into(),
            });
        }
        if let Some(previous) = last_verified_fingerprint.filter(|value| *value != fingerprint) {
            drift.push(EnvironmentDrift {
                subject: "Verification fingerprint".into(),
                expected: previous.into(),
                detected: fingerprint.clone(),
                detail: "The current environment differs from the last verified environment."
                    .into(),
            });
        }
        let required_missing = tools.iter().any(|tool| tool.status == ToolStatus::Missing)
            || environment
                .iter()
                .any(|item| item.required && !item.configured)
            || manifest.profile == ExecutionProfile::SandboxedLocal
            || (manifest.profile == ExecutionProfile::Container
                && manifest.container_image.as_deref().unwrap_or("").is_empty());
        let status = if required_missing {
            EnvironmentStatus::Incomplete
        } else if !drift.is_empty() {
            EnvironmentStatus::EnvironmentDrift
        } else if manifest_source.starts_with("inferred") {
            EnvironmentStatus::Unmanaged
        } else {
            EnvironmentStatus::Reproducible
        };
        Ok(EnvironmentSnapshot {
            id: Uuid::new_v4().to_string(),
            project_id: project_id.into(),
            profile: manifest.profile,
            manifest_source: manifest_source.into(),
            fingerprint,
            status,
            tools,
            environment,
            fingerprint_inputs,
            drift,
            os,
            architecture,
            commit: commit.into(),
            branch: branch.into(),
            worktree: repository.to_string_lossy().replace('\\', "/"),
            captured_at: now(),
        })
    }

    pub fn execute(
        &self,
        manifest: &EnvironmentManifest,
        snapshot: &EnvironmentSnapshot,
        repository: &Path,
        argv: &[String],
        cache_root: &Path,
        run_id: &str,
    ) -> Result<EnvironmentCommandOutput, EnvironmentError> {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| EnvironmentError::Validation("verification argv is empty".into()))?;
        let working = resolve_working_directory(repository, &manifest.working_directory)?;
        let run_cache = if manifest.disposable_verification {
            cache_root.join("runs").join(run_id)
        } else {
            cache_root.join(&snapshot.fingerprint)
        };
        fs::create_dir_all(&run_cache)?;
        match manifest.profile {
            ExecutionProfile::NativeWindows => {
                self.execute_native(manifest, &working, program, args, &run_cache)
            }
            ExecutionProfile::Container => self.execute_container(
                manifest,
                repository,
                program,
                args,
                cache_root,
                &snapshot.fingerprint,
            ),
            ExecutionProfile::SandboxedLocal => Err(EnvironmentError::Unavailable(
                "the sandboxed-local profile has no configured execution adapter".into(),
            )),
        }
    }

    pub fn cleanup_disposable(
        &self,
        manifest: &EnvironmentManifest,
        cache_root: &Path,
        run_id: &str,
    ) -> Result<(), EnvironmentError> {
        if !manifest.disposable_verification {
            return Ok(());
        }
        let runs = cache_root.join("runs");
        let target = runs.join(run_id);
        if !target.exists() {
            return Ok(());
        }
        let root = cache_root.canonicalize()?;
        let target = target.canonicalize()?;
        if target == root || !target.starts_with(&root) {
            return Err(EnvironmentError::Validation(
                "refused to clean a verifier cache outside the managed cache root".into(),
            ));
        }
        fs::remove_dir_all(target)?;
        Ok(())
    }

    fn execute_native(
        &self,
        manifest: &EnvironmentManifest,
        working: &Path,
        program: &str,
        args: &[String],
        cache: &Path,
    ) -> Result<EnvironmentCommandOutput, EnvironmentError> {
        let mut command = Command::new(program);
        command.args(args).current_dir(working);
        apply_sanitized_environment(&mut command, manifest, cache)?;
        self.run(command, manifest)
    }

    fn execute_container(
        &self,
        manifest: &EnvironmentManifest,
        repository: &Path,
        program: &str,
        args: &[String],
        cache_root: &Path,
        fingerprint: &str,
    ) -> Result<EnvironmentCommandOutput, EnvironmentError> {
        let image = manifest
            .container_image
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                EnvironmentError::Unavailable("container image is not configured".into())
            })?;
        if manifest
            .environment
            .iter()
            .any(|item| item.secret && item.required && env::var_os(&item.name).is_some())
        {
            return Err(EnvironmentError::Unavailable(
                "container secret injection is disabled until a credential broker is configured"
                    .into(),
            ));
        }
        let cache = cache_root.join(fingerprint);
        fs::create_dir_all(&cache)?;
        let repository_mapping = format!("{}:/workspace", repository.canonicalize()?.display());
        let cache_mapping = format!("{}:/agentrelay-cache", cache.canonicalize()?.display());
        let container_working = if manifest.working_directory == "." {
            "/workspace".into()
        } else {
            format!(
                "/workspace/{}",
                manifest.working_directory.replace('\\', "/")
            )
        };
        let mut command = Command::new("docker");
        command.args([
            "run",
            "--rm",
            "--workdir",
            &container_working,
            "--volume",
            &repository_mapping,
            "--volume",
            &cache_mapping,
            "--env",
            "CARGO_TARGET_DIR=/agentrelay-cache/cargo-target",
            "--env",
            "npm_config_cache=/agentrelay-cache/npm",
        ]);
        for requirement in manifest.environment.iter().filter(|item| !item.secret) {
            if let Some(value) = requirement
                .value
                .clone()
                .or_else(|| env::var(&requirement.name).ok())
            {
                command.args(["--env", &format!("{}={value}", requirement.name)]);
            } else if requirement.required {
                return Err(EnvironmentError::Unavailable(format!(
                    "required environment variable `{}` is not configured",
                    requirement.name
                )));
            }
        }
        command.arg(image).arg(program).args(args);
        self.run(command, manifest)
    }

    fn run(
        &self,
        mut command: Command,
        manifest: &EnvironmentManifest,
    ) -> Result<EnvironmentCommandOutput, EnvironmentError> {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let started = Instant::now();
        let mut child = command
            .spawn()
            .map_err(|error| EnvironmentError::Unavailable(error.to_string()))?;
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");
        let stdout_reader = thread::spawn(move || read_bounded(stdout));
        let stderr_reader = thread::spawn(move || read_bounded(stderr));
        let status = match child.wait_timeout(self.timeout)? {
            Some(status) => status,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(EnvironmentError::Timeout(self.timeout.as_secs()));
            }
        };
        let secrets = manifest
            .environment
            .iter()
            .filter(|item| item.secret)
            .filter_map(|item| env::var(&item.name).ok())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        Ok(EnvironmentCommandOutput {
            exit_code: status.code().unwrap_or(-1),
            stdout: redact(
                &stdout_reader
                    .join()
                    .map_err(|_| EnvironmentError::Execution("stdout reader stopped".into()))?,
                &secrets,
            ),
            stderr: redact(
                &stderr_reader
                    .join()
                    .map_err(|_| EnvironmentError::Execution("stderr reader stopped".into()))?,
                &secrets,
            ),
            duration: started.elapsed(),
        })
    }

    fn observe(
        &self,
        name: &str,
        expected: Option<&String>,
        program: &str,
        args: &[&str],
    ) -> ToolObservation {
        self.observe_in(name, expected, program, args, Path::new("."))
    }

    fn observe_msvc(&self) -> ToolObservation {
        let direct = self.observe("MSVC", None, "where.exe", &["cl.exe"]);
        if direct.status != ToolStatus::Missing {
            return direct;
        }
        let rust_host = self
            .detect_version("rustc", &["-vV"])
            .ok()
            .flatten()
            .filter(|value| value.to_ascii_lowercase().contains("msvc"));
        match rust_host {
            Some(value) => ToolObservation {
                name: "MSVC".into(),
                expected: None,
                detected: value
                    .lines()
                    .find(|line| line.starts_with("host:"))
                    .map(str::to_owned),
                status: ToolStatus::Detected,
            },
            None => direct,
        }
    }

    fn observe_in(
        &self,
        name: &str,
        expected: Option<&String>,
        program: &str,
        args: &[&str],
        working: &Path,
    ) -> ToolObservation {
        let detected = self
            .detect_version_in(program, args, working)
            .ok()
            .flatten();
        let status = match (&detected, expected) {
            (None, _) => ToolStatus::Missing,
            (Some(value), Some(expected)) if !version_matches(expected, value) => ToolStatus::Drift,
            (Some(_), Some(_)) => ToolStatus::Matched,
            (Some(_), None) => ToolStatus::Detected,
        };
        ToolObservation {
            name: name.into(),
            expected: expected.cloned(),
            detected,
            status,
        }
    }

    fn detect_version(
        &self,
        program: &str,
        args: &[&str],
    ) -> Result<Option<String>, EnvironmentError> {
        self.detect_version_in(program, args, Path::new("."))
    }

    fn detect_version_in(
        &self,
        program: &str,
        args: &[&str],
        working: &Path,
    ) -> Result<Option<String>, EnvironmentError> {
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(working)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => return Ok(None),
        };
        let status = match child.wait_timeout(Duration::from_secs(8))? {
            Some(status) => status,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(None);
            }
        };
        let output = child.wait_with_output()?;
        if !status.success() {
            return Ok(None);
        }
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let value = if stdout.is_empty() { stderr } else { stdout };
        Ok((!value.is_empty()).then_some(value))
    }
}

fn validate_manifest(manifest: &EnvironmentManifest) -> Result<(), EnvironmentError> {
    if manifest.schema_version != 1 {
        return Err(EnvironmentError::Validation(format!(
            "unsupported environment schema version {}",
            manifest.schema_version
        )));
    }
    validate_relative_path(&manifest.working_directory)?;
    if manifest.profile == ExecutionProfile::Container
        && manifest
            .container_image
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
    {
        return Err(EnvironmentError::Validation(
            "container profile requires a pinned container image".into(),
        ));
    }
    for (name, argv) in &manifest.commands {
        if name.trim().is_empty() || argv.first().is_none_or(|program| program.trim().is_empty()) {
            return Err(EnvironmentError::Validation(
                "every environment command needs a name and executable".into(),
            ));
        }
    }
    let mut names = BTreeSet::new();
    for requirement in &manifest.environment {
        if !valid_environment_name(&requirement.name) || !names.insert(&requirement.name) {
            return Err(EnvironmentError::Validation(format!(
                "invalid or duplicate environment variable name `{}`",
                requirement.name
            )));
        }
        if requirement.secret && requirement.value.is_some() {
            return Err(EnvironmentError::Validation(format!(
                "secret `{}` must be referenced by name and cannot store a value",
                requirement.name
            )));
        }
    }
    Ok(())
}

fn apply_sanitized_environment(
    command: &mut Command,
    manifest: &EnvironmentManifest,
    cache: &Path,
) -> Result<(), EnvironmentError> {
    const SAFE: &[&str] = &[
        "PATH",
        "PATHEXT",
        "SYSTEMROOT",
        "WINDIR",
        "COMSPEC",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "LOCALAPPDATA",
        "APPDATA",
        "PROGRAMDATA",
        "PROGRAMFILES",
        "PROGRAMFILES(X86)",
        "CARGO_HOME",
        "RUSTUP_HOME",
    ];
    command.env_clear();
    for name in SAFE {
        if let Some(value) = env::var_os(name) {
            command.env(name, value);
        }
    }
    let cargo = cache.join("cargo-target");
    let npm = cache.join("npm");
    fs::create_dir_all(&cargo)?;
    fs::create_dir_all(&npm)?;
    command.env("CARGO_TARGET_DIR", cargo);
    command.env("npm_config_cache", npm);
    command.env("CI", "true");
    for requirement in &manifest.environment {
        if let Some(value) = requirement
            .value
            .as_ref()
            .map(Into::into)
            .or_else(|| env::var_os(&requirement.name))
        {
            command.env(&requirement.name, value);
        } else if requirement.required {
            return Err(EnvironmentError::Unavailable(format!(
                "required environment variable `{}` is not configured",
                requirement.name
            )));
        }
    }
    Ok(())
}

fn fingerprint(
    manifest: &EnvironmentManifest,
    tools: &[ToolObservation],
    environment: &[EnvironmentVariableState],
    inputs: &[FingerprintInput],
    os: &str,
    architecture: &str,
) -> Result<String, EnvironmentError> {
    let value = serde_json::json!({
        "manifest": manifest,
        "tools": tools,
        "environment": environment,
        "inputs": inputs,
        "os": os,
        "architecture": architecture,
    });
    Ok(digest(&serde_json::to_vec(&value)?))
}

fn fingerprint_files(repository: &Path) -> Result<Vec<FingerprintInput>, EnvironmentError> {
    let candidates = [
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain",
        "rust-toolchain.toml",
        "package.json",
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "frontend/package.json",
        "frontend/package-lock.json",
        ".nvmrc",
        ".node-version",
    ];
    let mut inputs = Vec::new();
    for relative in candidates {
        let path = repository.join(relative);
        if path.is_file() {
            inputs.push(FingerprintInput {
                kind: if relative.contains("lock") {
                    "lockfile".into()
                } else {
                    "dependencyManifest".into()
                },
                path: relative.into(),
                digest: digest(&fs::read(path)?),
            });
        }
    }
    Ok(inputs)
}

fn environment_names(repository: &Path) -> Result<BTreeSet<String>, EnvironmentError> {
    let mut names = BTreeSet::new();
    for relative in [".env.example", ".env.sample", "frontend/.env.example"] {
        let path = repository.join(relative);
        if !path.is_file() {
            continue;
        }
        for line in fs::read_to_string(path)?.lines() {
            let name = line
                .trim()
                .strip_prefix("export ")
                .unwrap_or(line.trim())
                .split_once('=')
                .map(|(name, _)| name.trim())
                .unwrap_or("");
            if valid_environment_name(name) {
                names.insert(name.into());
            }
        }
    }
    Ok(names)
}

fn read_rust_requirement(repository: &Path) -> Option<String> {
    let toolchain = repository.join("rust-toolchain.toml");
    if toolchain.is_file() {
        let source = fs::read_to_string(toolchain).ok()?;
        let value: toml::Value = toml::from_str(&source).ok()?;
        if let Some(channel) = value
            .get("toolchain")
            .and_then(|value| value.get("channel"))
            .and_then(toml::Value::as_str)
        {
            return Some(channel.into());
        }
    }
    read_first(repository, &["rust-toolchain"])
}

fn read_first(repository: &Path, candidates: &[&str]) -> Option<String> {
    candidates.iter().find_map(|relative| {
        fs::read_to_string(repository.join(relative))
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    })
}

fn has_script(path: &Path, script: &str) -> Result<bool, EnvironmentError> {
    if !path.is_file() {
        return Ok(false);
    }
    Ok(read_json(path)?
        .pointer(&format!("/scripts/{script}"))
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty()))
}

fn read_json(path: &Path) -> Result<Value, EnvironmentError> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn resolve_working_directory(
    repository: &Path,
    relative: &str,
) -> Result<PathBuf, EnvironmentError> {
    validate_relative_path(relative)?;
    let path = if relative == "." {
        repository.to_path_buf()
    } else {
        repository.join(relative)
    };
    if !path.is_dir() {
        return Err(EnvironmentError::Validation(format!(
            "environment working directory does not exist: {}",
            path.display()
        )));
    }
    Ok(path)
}

fn validate_relative_path(value: &str) -> Result<(), EnvironmentError> {
    let path = Path::new(value);
    if value.trim().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(EnvironmentError::Validation(
            "environment working directory must stay inside the repository".into(),
        ));
    }
    Ok(())
}

fn valid_environment_name(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn version_matches(expected: &str, detected: &str) -> bool {
    if expected.eq_ignore_ascii_case("installed") {
        return true;
    }
    let expected = expected
        .trim()
        .trim_start_matches(['v', '^', '~'])
        .trim_start_matches(['>', '=', '<'])
        .trim();
    let detected = extract_version(detected).unwrap_or_else(|| detected.to_owned());
    detected == expected
        || detected
            .strip_prefix(expected)
            .is_some_and(|suffix| suffix.starts_with('.') || suffix.starts_with('-'))
}

fn extract_version(value: &str) -> Option<String> {
    value
        .split_whitespace()
        .map(|part| {
            part.trim_start_matches('v')
                .trim_matches(|ch: char| ch == ',' || ch == ';')
        })
        .find(|part| {
            part.chars().next().is_some_and(|ch| ch.is_ascii_digit()) && part.contains('.')
        })
        .map(str::to_owned)
}

fn os_description() -> String {
    let base = format!("{} {}", env::consts::OS, env::consts::FAMILY);
    #[cfg(windows)]
    {
        if let Ok(value) = env::var("OS") {
            return format!("{base} {value}");
        }
    }
    base
}

fn strings<const N: usize>(items: [&str; N]) -> Vec<String> {
    items.into_iter().map(str::to_owned).collect()
}

fn digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn read_bounded(mut stream: impl Read) -> String {
    let mut bytes = Vec::with_capacity(OUTPUT_LIMIT);
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let remaining = OUTPUT_LIMIT.saturating_sub(bytes.len());
        bytes.extend_from_slice(&buffer[..read.min(remaining)]);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn redact(value: &str, secrets: &[String]) -> String {
    secrets.iter().fold(value.to_owned(), |text, secret| {
        text.replace(secret, "[REDACTED]")
    })
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[derive(Debug, Error)]
pub enum EnvironmentError {
    #[error("environment I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("environment manifest is invalid: {0}")]
    TomlDecode(#[from] toml::de::Error),
    #[error("environment manifest could not be written: {0}")]
    TomlEncode(#[from] toml::ser::Error),
    #[error("environment data could not be serialized: {0}")]
    Json(#[from] serde_json::Error),
    #[error("environment is unavailable: {0}")]
    Unavailable(String),
    #[error("environment execution failed: {0}")]
    Execution(String),
    #[error("environment execution exceeded {0} seconds")]
    Timeout(u64),
    #[error("{0}")]
    Validation(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn infers_fixed_commands_and_never_stores_secret_values() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(directory.path().join("Cargo.lock"), "version = 4\n").unwrap();
        fs::write(
            directory.path().join(".env.example"),
            "API_TOKEN=replace-me\n",
        )
        .unwrap();
        let service = EnvironmentService::new(Duration::from_secs(1));
        let manifest = service.infer(directory.path()).unwrap();
        assert_eq!(manifest.profile, ExecutionProfile::NativeWindows);
        assert_eq!(
            manifest.commands["test"],
            strings(["cargo", "test", "--workspace"])
        );
        let secret = manifest
            .environment
            .iter()
            .find(|item| item.name == "API_TOKEN")
            .unwrap();
        assert!(secret.secret);
        assert!(secret.value.is_none());
    }

    #[test]
    fn rejects_secret_values_and_parent_working_directories() {
        let mut manifest = EnvironmentManifest {
            schema_version: 1,
            profile: ExecutionProfile::NativeWindows,
            working_directory: "..".into(),
            container_image: None,
            disposable_verification: true,
            runtimes: BTreeMap::new(),
            package_manager: None,
            system_tools: vec![],
            commands: BTreeMap::new(),
            environment: vec![],
            cache_kinds: vec![],
            architecture_rules: vec![],
        };
        assert!(validate_manifest(&manifest).is_err());
        manifest.working_directory = ".".into();
        manifest.environment.push(EnvironmentVariableRequirement {
            name: "TOKEN".into(),
            required: true,
            secret: true,
            value: Some("do-not-store".into()),
        });
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn fingerprint_changes_with_lockfile_content() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(directory.path().join("Cargo.lock"), "one\n").unwrap();
        let first = fingerprint_files(directory.path()).unwrap();
        fs::write(directory.path().join("Cargo.lock"), "two\n").unwrap();
        let second = fingerprint_files(directory.path()).unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn checked_in_agentrelay_manifest_is_valid_and_pinned() {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let service = EnvironmentService::new(Duration::from_secs(1));
        let (manifest, source) = service.load_or_infer(&repository).unwrap();
        assert_eq!(source, ".agentrelay/environment.toml");
        assert_eq!(
            manifest.runtimes.get("rust").map(String::as_str),
            Some("1.98.1")
        );
        assert_eq!(manifest.package_manager.as_deref(), Some("npm@11.6.0"));
        assert!(manifest.commands.contains_key("desktopBuild"));
        assert!(manifest.environment.iter().all(|item| item.value.is_none()));
    }

    #[test]
    fn version_matching_detects_patch_drift_without_rejecting_major_pins() {
        assert!(version_matches("24", "v24.19.0"));
        assert!(version_matches("1.98", "rustc 1.98.1"));
        assert!(!version_matches("1.98.1", "rustc 1.98.2"));
        assert!(!version_matches("1.98.1", "rustc 1.99.0"));
    }
}
