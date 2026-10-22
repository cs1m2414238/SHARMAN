use std::{fs, path::Path};

use thiserror::Error;

use crate::{domain::ActionKind, ports::CommandSpec};

/// Resolve a named action to a trusted argv. Agent input never becomes command text.
pub fn resolve_action(repository: &Path, action: ActionKind) -> Result<CommandSpec, ActionError> {
    match action {
        ActionKind::RunTests => resolve_tests(repository),
        ActionKind::RunBuild => resolve_build(repository),
        _ => Err(ActionError::Unsupported(
            "source changes and Git mutations are not implemented in v0.11".into(),
        )),
    }
}

fn resolve_tests(repository: &Path) -> Result<CommandSpec, ActionError> {
    if repository.join("Cargo.toml").is_file() {
        return Ok(command("cargo", ["test", "--workspace"]));
    }
    if has_npm_script(repository, "test")? {
        return Ok(command("npm", ["test"]));
    }
    if repository.join("pyproject.toml").is_file()
        || repository.join("pytest.ini").is_file()
        || repository.join("setup.cfg").is_file()
    {
        return Ok(command("python", ["-m", "pytest"]));
    }
    if repository.join("go.mod").is_file() {
        return Ok(command("go", ["test", "./..."]));
    }
    if repository.join("pom.xml").is_file() {
        return Ok(command("mvn", ["test"]));
    }
    if repository.join("gradlew").is_file() {
        return Ok(command("./gradlew", ["test"]));
    }
    Err(ActionError::Unsupported(
        "no supported test manifest or test script was found".into(),
    ))
}

fn resolve_build(repository: &Path) -> Result<CommandSpec, ActionError> {
    if repository.join("Cargo.toml").is_file() {
        return Ok(command("cargo", ["build", "--workspace"]));
    }
    if has_npm_script(repository, "build")? {
        return Ok(command("npm", ["run", "build"]));
    }
    if repository.join("pyproject.toml").is_file() {
        return Ok(command("python", ["-m", "build"]));
    }
    if repository.join("go.mod").is_file() {
        return Ok(command("go", ["build", "./..."]));
    }
    if repository.join("pom.xml").is_file() {
        return Ok(command("mvn", ["package", "-DskipTests"]));
    }
    if repository.join("gradlew").is_file() {
        return Ok(command("./gradlew", ["build"]));
    }
    Err(ActionError::Unsupported(
        "no supported build manifest or build script was found".into(),
    ))
}

fn has_npm_script(repository: &Path, script: &str) -> Result<bool, ActionError> {
    let path = repository.join("package.json");
    if !path.is_file() {
        return Ok(false);
    }
    let source = fs::read_to_string(&path)
        .map_err(|error| ActionError::Manifest(format!("{}: {error}", path.display())))?;
    let value: serde_json::Value = serde_json::from_str(&source)
        .map_err(|error| ActionError::Manifest(format!("{}: {error}", path.display())))?;
    Ok(value
        .get("scripts")
        .and_then(|scripts| scripts.get(script))
        .and_then(|value| value.as_str())
        .is_some_and(|value| !value.trim().is_empty()))
}

fn command<const N: usize>(program: &str, args: [&str; N]) -> CommandSpec {
    CommandSpec {
        program: program.into(),
        args: args.into_iter().map(str::to_owned).collect(),
    }
}

#[derive(Debug, Error)]
pub enum ActionError {
    #[error("action is unavailable: {0}")]
    Unsupported(String),
    #[error("unable to inspect project manifest: {0}")]
    Manifest(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn resolves_cargo_actions_without_accepting_command_text() {
        let directory = TempDir::new().unwrap();
        fs::write(directory.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        assert_eq!(
            resolve_action(directory.path(), ActionKind::RunTests)
                .unwrap()
                .argv(),
            ["cargo", "test", "--workspace"]
        );
        assert_eq!(
            resolve_action(directory.path(), ActionKind::RunBuild)
                .unwrap()
                .argv(),
            ["cargo", "build", "--workspace"]
        );
    }

    #[test]
    fn only_uses_declared_npm_scripts() {
        let directory = TempDir::new().unwrap();
        fs::write(
            directory.path().join("package.json"),
            r#"{"scripts":{"test":"vitest"}}"#,
        )
        .unwrap();
        assert_eq!(
            resolve_action(directory.path(), ActionKind::RunTests)
                .unwrap()
                .program,
            "npm"
        );
        assert!(resolve_action(directory.path(), ActionKind::RunBuild).is_err());
    }
}
