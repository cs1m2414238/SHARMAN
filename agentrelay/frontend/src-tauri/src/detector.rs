use std::sync::Mutex;

use agentrelay_core::{
    detection::{AgentDetector, DetectedAgent, DetectionError},
    sdk::ProviderDetectionRule,
};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System, UpdateKind};

pub struct WindowsAgentDetector {
    system: Mutex<System>,
    rules: Vec<ProviderDetectionRule>,
}

impl WindowsAgentDetector {
    pub fn new(rules: Vec<ProviderDetectionRule>) -> Self {
        Self {
            system: Mutex::new(System::new_with_specifics(process_refresh_kind())),
            rules,
        }
    }
}

impl AgentDetector for WindowsAgentDetector {
    fn detect_agents(&self) -> Result<Vec<DetectedAgent>, DetectionError> {
        let mut system = self
            .system
            .lock()
            .map_err(|_| DetectionError::Inspection("process snapshot lock was poisoned".into()))?;
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cmd(UpdateKind::Always)
                .with_cwd(UpdateKind::Always)
                .with_exe(UpdateKind::OnlyIfNotSet),
        );
        let mut agents = system
            .processes()
            .iter()
            .filter_map(|(pid, process)| {
                let executable = process
                    .exe()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|| process.name().to_string_lossy().into_owned());
                let command_line = process
                    .cmd()
                    .iter()
                    .map(|part| part.to_string_lossy().into_owned())
                    .collect::<Vec<_>>();
                let rule = self
                    .rules
                    .iter()
                    .find(|rule| rule.matches(&executable, &command_line))?;
                Some(DetectedAgent {
                    provider: rule.provider.clone(),
                    pid: pid.as_u32(),
                    executable,
                    working_directory: process.cwd().map(ToOwned::to_owned),
                    command_line,
                })
            })
            .collect::<Vec<_>>();
        agents.sort_by_key(|agent| agent.pid);
        Ok(agents)
    }
}

fn process_refresh_kind() -> RefreshKind {
    RefreshKind::nothing().with_processes(
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_cwd(UpdateKind::Always)
            .with_exe(UpdateKind::OnlyIfNotSet),
    )
}

#[cfg(test)]
mod tests {
    use agentrelay_core::sdk::IntegrationRegistry;

    #[test]
    fn detector_accepts_registry_rules() {
        let detector = super::WindowsAgentDetector::new(
            IntegrationRegistry::built_in().provider_detection_rules(),
        );
        assert!(!detector.rules.is_empty());
    }
}
