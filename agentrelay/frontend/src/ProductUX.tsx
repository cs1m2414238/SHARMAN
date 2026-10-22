import { useEffect, useMemo, useState } from 'react'
import type { ReactNode } from 'react'
import {
  chooseRepository,
  completeProjectOnboarding,
  createBackup,
  exportDiagnostics,
  getProjectHealth,
  getTrackedProjects,
  inspectOnboarding,
  listBackups,
  updateApplicationSettings,
  type ApplicationSettings,
  type BackgroundStatus,
  type BackupRecord,
  type DeliverySnapshot,
  type EnvironmentDashboard,
  type OnboardingInspection,
  type PermissionMode,
  type ProjectHealthSnapshot,
  type ProjectMemory,
  type ProjectRecord,
  type RegistrySnapshot,
  type SandboxStatus,
  type VerificationSnapshot,
} from './api'

function message(error: unknown) {
  return error instanceof Error ? error.message : typeof error === 'string' ? error : 'AgentRelay could not complete that action.'
}

const PROVIDERS = [
  ['claude-code', 'Claude Code'],
  ['codex', 'Codex'],
  ['gemini', 'Gemini CLI'],
  ['cursor', 'Cursor'],
  ['github-copilot', 'GitHub Copilot'],
] as const

export function PermissionModeControl({ value, disabled, onChange }: {
  value: PermissionMode
  disabled?: boolean
  onChange: (value: PermissionMode) => void
}) {
  return <div className={`permission-mode-control ${disabled ? 'disabled' : ''}`}>
    {(['allow', 'ask', 'deny'] as PermissionMode[]).map((mode) => <button type="button" disabled={disabled} className={value === mode ? `active ${mode}` : ''} onClick={() => onChange(mode)} key={mode}>{mode[0].toUpperCase() + mode.slice(1)}</button>)}
  </div>
}

export function OnboardingWizard({ settings, initialRepository, canCancel, onCancel, onFinished }: {
  settings: ApplicationSettings
  initialRepository: string
  canCancel: boolean
  onCancel: () => void
  onFinished: (repository: string, settings: ApplicationSettings) => Promise<void>
}) {
  const [step, setStep] = useState(0)
  const [repository, setRepository] = useState(initialRepository)
  const [inspection, setInspection] = useState<OnboardingInspection | null>(null)
  const [selectedAgents, setSelectedAgents] = useState<string[]>(settings.selectedAgents)
  const [runTests, setRunTests] = useState<PermissionMode>('ask')
  const [runBuild, setRunBuild] = useState<PermissionMode>('ask')
  const [preserveContext, setPreserveContext] = useState(true)
  const [health, setHealth] = useState<ProjectHealthSnapshot | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let active = true
    setBusy(true)
    void inspectOnboarding(initialRepository || null).then((result) => {
      if (!active) return
      setInspection(result)
      if (result.repository) setRepository(result.repository.repositoryPath)
    }).catch((reason) => active && setError(message(reason))).finally(() => active && setBusy(false))
    return () => { active = false }
  }, [initialRepository])

  useEffect(() => {
    if (!canCancel) return
    const dismiss = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !busy) onCancel()
    }
    window.addEventListener('keydown', dismiss)
    return () => window.removeEventListener('keydown', dismiss)
  }, [busy, canCancel, onCancel])

  const detected = useMemo(() => new Set(inspection?.detectedAgents.map((agent) => agent.provider) || []), [inspection])

  async function browse() {
    setError(null)
    const selected = await chooseRepository()
    if (!selected) return
    setRepository(selected)
    await inspect(selected)
  }

  async function inspect(path = repository) {
    if (!path.trim()) return
    setBusy(true)
    setError(null)
    try {
      const result = await inspectOnboarding(path.trim())
      setInspection(result)
      if (result.repository) setRepository(result.repository.repositoryPath)
    } catch (reason) {
      setError(message(reason))
    } finally {
      setBusy(false)
    }
  }

  async function finish() {
    if (!inspection?.repository?.gitRepository) return
    setBusy(true)
    setError(null)
    try {
      const result = await completeProjectOnboarding({
        repositoryPath: repository,
        preserveExistingContext: preserveContext,
        runTests,
        runBuild,
      })
      const nextSettings = await updateApplicationSettings({
        ...settings,
        onboardingCompleted: true,
        selectedAgents,
      })
      setHealth(await getProjectHealth(result.repositoryPath))
      setStep(5)
      await onFinished(result.repositoryPath, nextSettings)
    } catch (reason) {
      setError(message(reason))
    } finally {
      setBusy(false)
    }
  }

  const steps = ['Repository', 'System', 'Agents', 'Permissions', 'Project memory', 'Ready']
  return <div className="onboarding-shell">
    <section className="onboarding-card">
      <header className="onboarding-header"><div><span className="onboarding-mark">S</span><div><p>SHARMAN · OPTIONAL PROJECT SETUP</p><h1>{settings.onboardingCompleted ? 'Add another project' : 'Add a coding project when you are ready'}</h1></div></div>{canCancel && <div className="onboarding-dismiss"><button type="button" className="ghost-button" disabled={busy} onClick={onCancel}>{settings.onboardingCompleted ? 'Close' : 'Skip for now'}</button><button type="button" className="onboarding-close" aria-label="Close project setup" title="Close (Esc)" disabled={busy} onClick={onCancel}>×</button></div>}</header>
      <div className="onboarding-progress">{steps.map((label, index) => <div className={index === step ? 'active' : index < step ? 'complete' : ''} key={label}><span>{index < step ? '✓' : index + 1}</span><small>{label}</small></div>)}</div>
      {error && <div className="onboarding-error"><strong>Setup needs attention</strong><span>{error}</span></div>}

      {step === 0 && <div className="onboarding-step"><div className="step-copy"><p className="section-label">ADD PROJECT · OPTIONAL</p><h2>Connect a repository for coding-agent coordination.</h2><p>SHARMAN works without a project. Add one only when you want AgentRelay to track Git changes, handoffs, tasks, and verification locally.</p></div><div className="repository-browse"><input value={repository} onChange={(event) => setRepository(event.target.value)} placeholder="C:\Projects\my-project" /><button type="button" className="ghost-button" disabled={busy} onClick={() => void browse()}>Browse…</button><button type="button" className="primary-button" disabled={busy || !repository.trim()} onClick={() => void inspect()}>Inspect repository</button></div>{inspection?.repository && <article className={`repository-inspection ${inspection.repository.gitRepository ? 'valid' : 'invalid'}`}><div><strong>{inspection.repository.name}</strong><span>{inspection.repository.repositoryPath}</span></div><b>{inspection.repository.gitRepository ? 'Git repository ✓' : 'Git required'}</b><div className="setup-tags">{inspection.repository.detectedStack.map((stack) => <span key={stack}>{stack}</span>)}</div></article>}<div className="wizard-actions"><button type="button" className="ghost-button" disabled={busy} onClick={onCancel}>Use SHARMAN without a project</button><button type="button" className="primary-button" disabled={!inspection?.repository?.gitRepository} onClick={() => setStep(1)}>Continue →</button></div></div>}

      {step === 1 && <div className="onboarding-step"><div className="step-copy"><p className="section-label">SYSTEM CHECK</p><h2>Required tools are visible.</h2><p>Git is required. Other toolchains are matched to the detected project stack and quality gates.</p></div><div className="tool-check-grid">{inspection?.tools.map((tool) => <article className={tool.detected ? 'detected' : tool.required ? 'missing required' : 'missing'} key={tool.id}><span>{tool.detected ? '✓' : '!'}</span><div><strong>{tool.name}{tool.required ? ' · required' : ''}</strong><small>{tool.version || tool.detail || 'Not detected on PATH'}</small></div></article>)}</div><div className="wizard-actions"><button className="ghost-button" onClick={() => setStep(0)}>← Back</button><button className="primary-button" disabled={inspection?.tools.some((tool) => tool.required && !tool.detected)} onClick={() => setStep(2)}>Continue →</button></div></div>}

      {step === 2 && <div className="onboarding-step"><div className="step-copy"><p className="section-label">AGENT DETECTION</p><h2>Choose which agents AgentRelay may track.</h2><p>Process detection is local and provider-manifest driven. Selecting an agent enables automatic sessions only inside tracked repositories.</p></div><div className="agent-choice-grid">{PROVIDERS.map(([id, name]) => { const checked = selectedAgents.includes(id); return <button type="button" className={checked ? 'selected' : ''} onClick={() => setSelectedAgents((current) => checked ? current.filter((item) => item !== id) : [...current, id])} key={id}><span>{checked ? '✓' : '+'}</span><div><strong>{name}</strong><small>{detected.has(id) ? 'Running now · detected' : 'Available provider profile'}</small></div></button> })}</div><div className="wizard-actions"><button className="ghost-button" onClick={() => setStep(1)}>← Back</button><button className="primary-button" disabled={selectedAgents.length === 0} onClick={() => setStep(3)}>Continue →</button></div></div>}

      {step === 3 && <div className="onboarding-step"><div className="step-copy"><p className="section-label">PERMISSION POLICY</p><h2>Safe defaults, understandable choices.</h2><p><b>Allow</b> runs without another prompt. <b>Ask</b> requires approval every time. <b>Deny</b> blocks the capability in the Rust core.</p></div><div className="onboarding-permissions"><PermissionRow label="Read repository" detail="Needed to understand project structure" mode="allow" locked onChange={() => undefined} /><PermissionRow label="Read Git history" detail="Needed for change tracking and handoffs" mode="allow" locked onChange={() => undefined} /><PermissionRow label="Watch file changes" detail="Only while project tracking is enabled" mode="allow" locked onChange={() => undefined} /><PermissionRow label="Write AGENT_CONTEXT.md" detail="Generated projection of local memory" mode="allow" locked onChange={() => undefined} /><PermissionRow label="Run tests" detail="Runs only through the controlled execution layer" mode={runTests} onChange={setRunTests} /><PermissionRow label="Run builds" detail="Runs only through the controlled execution layer" mode={runBuild} onChange={setRunBuild} /><PermissionRow label="Modify source files" detail="Unavailable to agents in this release" mode="deny" locked onChange={() => undefined} /><PermissionRow label="Create commits / push" detail="Unavailable to agents in this release" mode="deny" locked onChange={() => undefined} /></div><div className="wizard-actions"><button className="ghost-button" onClick={() => setStep(2)}>← Back</button><button className="primary-button" onClick={() => setStep(4)}>Continue →</button></div></div>}

      {step === 4 && <div className="onboarding-step"><div className="step-copy"><p className="section-label">PROJECT MEMORY</p><h2>Initialize context and quality gates.</h2><p>SQLite becomes the source of truth. Existing context can be archived as a reference before AgentRelay generates its deterministic projection.</p></div>{inspection?.repository?.existingContext && <label className="context-import-choice"><input type="checkbox" checked={preserveContext} onChange={(event) => setPreserveContext(event.target.checked)} /><span><strong>Archive the existing AGENT_CONTEXT.md</strong><small>The original is copied to <code>.agentrelay/imports</code>; it is never parsed as authoritative state.</small></span></label>}<div className="setup-summary"><article><span>Stack</span><strong>{inspection?.repository?.detectedStack.join(' · ')}</strong></article><article><span>Quality gates</span><div>{inspection?.repository?.suggestedQualityGates.map((gate) => <code key={gate}>{gate}</code>)}</div></article><article><span>Initial backup</span><strong>Created automatically after setup</strong></article></div><div className="wizard-actions"><button className="ghost-button" onClick={() => setStep(3)}>← Back</button><button className="primary-button" disabled={busy} onClick={() => void finish()}>{busy ? 'Initializing…' : 'Initialize AgentRelay'}</button></div></div>}

      {step === 5 && <div className="onboarding-step ready-step"><div className="ready-check">✓</div><div className="step-copy"><p className="section-label">READY</p><h2>This project is connected.</h2><p>Memory, context, tracking, permissions, and the initial backup are configured. Start a selected coding agent in the repository and AgentRelay will create the first session automatically.</p></div>{health && <div className="ready-health"><strong>Health score {health.score}%</strong><span>{health.items.filter((item) => item.status === 'healthy').length}/{health.items.length} checks healthy</span></div>}<div className="wizard-actions"><span /><button className="primary-button" onClick={onCancel}>Open AgentRelay →</button></div></div>}
    </section>
  </div>
}

function PermissionRow({ label, detail, mode, locked, onChange }: { label: string; detail: string; mode: PermissionMode; locked?: boolean; onChange: (mode: PermissionMode) => void }) {
  return <article><div><strong>{label}</strong><small>{detail}</small></div><PermissionModeControl value={mode} disabled={locked} onChange={onChange} /></article>
}

export function ProjectHealthDashboard({ repositoryPath, refreshKey = 0 }: { repositoryPath: string; refreshKey?: number }) {
  const [health, setHealth] = useState<ProjectHealthSnapshot | null>(null)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    let active = true
    void getProjectHealth(repositoryPath).then((next) => active && setHealth(next)).catch((reason) => active && setError(message(reason)))
    return () => { active = false }
  }, [repositoryPath, refreshKey])
  return <section className="health-dashboard"><div className="health-heading"><div><p className="section-label">PROJECT HEALTH</p><h2>{health ? `${health.score}% · ${health.status === 'healthy' ? 'Ready' : health.status === 'warning' ? 'Review suggested' : 'Action required'}` : 'Checking project…'}</h2></div>{health && <span className={`health-score ${health.status}`}>{health.score}</span>}</div>{error && <div className="health-error">{error}</div>}<div className="health-grid">{health?.items.map((item) => <article className={item.status} title={item.detail} key={item.id}><span className="health-dot" /><div><small>{item.label}</small><strong>{item.value}</strong></div></article>) || Array.from({ length: 8 }, (_, index) => <article className="loading" key={index}><span className="health-dot" /><div><small>Checking</small><strong>…</strong></div></article>)}</div></section>
}

type SettingsSection = 'general' | 'agents' | 'projects' | 'permissions' | 'context' | 'verification' | 'integrations' | 'environment' | 'notifications' | 'backups' | 'advanced'

export function SettingsView({ settings, repositoryPath, memory, background, registry, environment, delivery, verification, sandbox, onSaveSettings, onSavePermissions, onOpenOnboarding, onNotice, onError }: {
  settings: ApplicationSettings
  repositoryPath: string
  memory: ProjectMemory
  background: BackgroundStatus | null
  registry: RegistrySnapshot | null
  environment: EnvironmentDashboard | null
  delivery: DeliverySnapshot | null
  verification: VerificationSnapshot | null
  sandbox: SandboxStatus | null
  onSaveSettings: (settings: ApplicationSettings) => Promise<void>
  onSavePermissions: (runTests: PermissionMode, runBuild: PermissionMode) => Promise<void>
  onOpenOnboarding: () => void
  onNotice: (notice: string) => void
  onError: (error: string) => void
}) {
  const [section, setSection] = useState<SettingsSection>('general')
  const [draft, setDraft] = useState(settings)
  const [projects, setProjects] = useState<ProjectRecord[]>([])
  const [backups, setBackups] = useState<BackupRecord[]>([])
  const [busy, setBusy] = useState(false)
  useEffect(() => setDraft(settings), [settings])
  useEffect(() => { void getTrackedProjects().then(setProjects); void listBackups().then(setBackups) }, [])

  const sections: Array<[SettingsSection, string]> = [['general', 'General'], ['agents', 'Agents'], ['projects', 'Projects'], ['permissions', 'Permissions'], ['context', 'Context'], ['verification', 'Verification'], ['integrations', 'Integrations'], ['environment', 'Environment'], ['notifications', 'Notifications'], ['backups', 'Backups'], ['advanced', 'Advanced']]

  async function save() {
    setBusy(true)
    try { await onSaveSettings(draft); onNotice('Settings saved.') } catch (reason) { onError(message(reason)) } finally { setBusy(false) }
  }

  async function backup() {
    setBusy(true)
    try { const next = await createBackup(); setBackups((current) => [next, ...current]); onNotice('A consistent SQLite memory backup was created.') } catch (reason) { onError(message(reason)) } finally { setBusy(false) }
  }

  async function diagnostics() {
    setBusy(true)
    try { const result = await exportDiagnostics(repositoryPath); onNotice(`Diagnostics exported to ${result.path}. Source code and secrets were excluded.`) } catch (reason) { if (!String(reason).includes('cancelled')) onError(message(reason)) } finally { setBusy(false) }
  }

  return <section className="settings-view"><div className="view-heading context-heading"><div><p className="section-label">APPLICATION SETTINGS</p><h2>Clear controls for every trust boundary.</h2><p>General preferences are global. Permissions and environment configuration remain project-specific.</p></div><button className="primary-button" disabled={busy} onClick={() => void save()}>Save settings</button></div><div className="settings-layout"><aside className="settings-nav">{sections.map(([id, label]) => <button className={section === id ? 'active' : ''} onClick={() => setSection(id)} key={id}>{label}</button>)}</aside><div className="settings-panel">
    {section === 'general' && <SettingsGroup title="General" description="Windows lifecycle and background behavior."><Toggle label="Launch AgentRelay when I sign in" checked={draft.launchAtStartup} onChange={(value) => setDraft({ ...draft, launchAtStartup: value })} /><Toggle label="Keep AgentRelay running when the window closes" checked={draft.closeToTray} onChange={(value) => setDraft({ ...draft, closeToTray: value })} /><Toggle label="Pause all background tracking" checked={draft.trackingPaused} onChange={(value) => setDraft({ ...draft, trackingPaused: value })} /><label className="settings-field"><span>Idle timeout</span><div><input type="number" value={background?.idleTimeoutMinutes || 15} disabled /><small>Configured from the Repository dashboard · minutes</small></div></label></SettingsGroup>}
    {section === 'agents' && <SettingsGroup title="Agents" description="Only selected providers may create automatic background sessions."><div className="settings-agent-list">{(registry?.entries.filter((entry) => entry.manifest.kind === 'provider').map((entry) => [entry.manifest.id, entry.manifest.name] as const) || PROVIDERS).map(([id, name]) => <Toggle key={id} label={name} detail={background?.detectedAgents.some((agent) => agent.provider === id) ? 'Process detected now' : 'No running process detected'} checked={draft.selectedAgents.includes(id)} onChange={(checked) => setDraft({ ...draft, selectedAgents: checked ? [...draft.selectedAgents, id] : draft.selectedAgents.filter((item) => item !== id) })} />)}</div></SettingsGroup>}
    {section === 'projects' && <SettingsGroup title="Projects" description="Tracking is always explicit per repository."><button className="primary-button" onClick={onOpenOnboarding}>Add repository</button><div className="settings-project-list">{projects.map((project) => <article key={project.id}><div><strong>{project.name}</strong><small>{project.path}</small></div><span className={project.trackingEnabled ? 'enabled' : ''}>{project.trackingEnabled ? 'Tracked' : 'Paused'}</span></article>)}</div></SettingsGroup>}
    {section === 'permissions' && <SettingsGroup title="Permissions" description="Ask requires a fresh confirmation at execution time."><div className="settings-permissions"><PermissionRow label="Read repository" detail="Core project understanding" mode="allow" locked onChange={() => undefined} /><PermissionRow label="Read Git history" detail="Changes, commits, branches, and handoffs" mode="allow" locked onChange={() => undefined} /><PermissionRow label="Watch file changes" detail="Only for explicitly tracked projects" mode="allow" locked onChange={() => undefined} /><PermissionRow label="Write AGENT_CONTEXT.md" detail="Generated context projection" mode="allow" locked onChange={() => undefined} /><PermissionRow label="Run tests" detail="Sandbox or controlled verifier" mode={memory.permissions.runTests} onChange={(value) => void onSavePermissions(value, memory.permissions.runBuild)} /><PermissionRow label="Run builds" detail="Sandbox or controlled verifier" mode={memory.permissions.runBuild} onChange={(value) => void onSavePermissions(memory.permissions.runTests, value)} /><PermissionRow label="Modify source files" detail="Unavailable to autonomous actions" mode="deny" locked onChange={() => undefined} /><PermissionRow label="Create Git commits" detail="User-owned operation" mode="deny" locked onChange={() => undefined} /><PermissionRow label="Push to remote" detail="User-owned operation" mode="deny" locked onChange={() => undefined} /></div></SettingsGroup>}
    {section === 'context' && <SettingsGroup title="Context" description="Defaults used when opening a new project or generating a handoff."><label className="settings-field"><span>Default depth</span><select value={draft.defaultContextDepth} onChange={(event) => setDraft({ ...draft, defaultContextDepth: event.target.value as ApplicationSettings['defaultContextDepth'] })}><option value="compact">Compact · 2k</option><option value="normal">Normal · 6k</option><option value="deep">Deep · 12k</option></select></label><label className="settings-field"><span>Default profile</span><select value={draft.defaultContextProfile} onChange={(event) => setDraft({ ...draft, defaultContextProfile: event.target.value as ApplicationSettings['defaultContextProfile'] })}><option value="auto">Automatic</option><option value="balanced">Balanced</option><option value="architecture">Architecture</option><option value="implementation">Implementation</option><option value="product">Product</option></select></label></SettingsGroup>}
    {section === 'verification' && <SettingsGroup title="Verification" description="Independent gates remain bound to an exact commit and environment."><StatusRow label="Latest suite" value={verification?.latest ? `${verification.latest.status} · ${verification.latest.commit.slice(0, 12)}` : 'Not run'} /><StatusRow label="Test policy" value={memory.permissions.runTests} /><StatusRow label="Build policy" value={memory.permissions.runBuild} /><StatusRow label="Execution sandbox" value={sandbox?.configured ? sandbox.name || 'Configured' : 'Not configured'} /></SettingsGroup>}
    {section === 'integrations' && <SettingsGroup title="Integrations" description="External adapters remain provider-neutral and never write directly to SQLite."><StatusRow label="GitHub" value={delivery ? `${delivery.repository.owner}/${delivery.repository.name}` : 'Not connected'} /><StatusRow label="Provider SDK" value={`${registry?.entries.length || 0} registered integrations`} /></SettingsGroup>}
    {section === 'environment' && <SettingsGroup title="Environment" description="Current reproducibility and toolchain fingerprint."><StatusRow label="Status" value={environment?.snapshot.status || 'Open Environment to inspect'} /><StatusRow label="Fingerprint" value={environment?.snapshot.fingerprint.slice(0, 16) || 'Not captured'} /><StatusRow label="Profile" value={environment?.snapshot.profile || 'Not captured'} /></SettingsGroup>}
    {section === 'notifications' && <SettingsGroup title="Notifications" description="Choose which background events may interrupt you."><Toggle label="Desktop notifications" checked={draft.notificationsEnabled} onChange={(value) => setDraft({ ...draft, notificationsEnabled: value })} /><Toggle label="Agent sessions" checked={draft.notifyAgentSessions} onChange={(value) => setDraft({ ...draft, notifyAgentSessions: value })} /><Toggle label="Conflicts and stale context" checked={draft.notifyConflicts} onChange={(value) => setDraft({ ...draft, notifyConflicts: value })} /><Toggle label="Handoffs ready" checked={draft.notifyHandoffs} onChange={(value) => setDraft({ ...draft, notifyHandoffs: value })} /><Toggle label="Blocked tasks" checked={draft.notifyBlockers} onChange={(value) => setDraft({ ...draft, notifyBlockers: value })} /></SettingsGroup>}
    {section === 'backups' && <SettingsGroup title="Backups" description="Backups contain project memory, not repository source code."><label className="settings-field"><span>Backup reminder interval</span><select value={draft.backupIntervalHours} onChange={(event) => setDraft({ ...draft, backupIntervalHours: Number(event.target.value) })}><option value={6}>6 hours</option><option value={12}>12 hours</option><option value={24}>Daily</option><option value={168}>Weekly</option></select></label><button className="primary-button" disabled={busy} onClick={() => void backup()}>Create backup now</button><div className="backup-list">{backups.length ? backups.slice(0, 8).map((item) => <article key={item.path}><div><strong>{new Date(item.createdAt).toLocaleString()}</strong><small>{item.path}</small></div><span>{(item.sizeBytes / 1024).toFixed(0)} KB</span></article>) : <p className="settings-empty">No memory backups yet. AgentRelay creates the first one when project onboarding completes.</p>}</div></SettingsGroup>}
    {section === 'advanced' && <SettingsGroup title="Advanced" description="Support and privacy controls."><Toggle label="Include repository paths in diagnostics" detail="Source files and secret values are always excluded" checked={draft.diagnosticsIncludePaths} onChange={(value) => setDraft({ ...draft, diagnosticsIncludePaths: value })} /><Toggle label="Extended local diagnostics" checked={draft.advancedLogging} onChange={(value) => setDraft({ ...draft, advancedLogging: value })} /><button className="primary-button" disabled={busy} onClick={() => void diagnostics()}>Export diagnostics bundle</button><p className="diagnostic-exclusion">The ZIP includes version, settings, tool availability, health, and sanitized runtime summaries. It excludes source code, environment-variable values, credentials, the SQLite database, and command output.</p><button className="ghost-button" onClick={onOpenOnboarding}>Run onboarding again</button></SettingsGroup>}
  </div></div></section>
}

function SettingsGroup({ title, description, children }: { title: string; description: string; children: ReactNode }) {
  return <div className="settings-group"><header><h3>{title}</h3><p>{description}</p></header><div>{children}</div></div>
}

function Toggle({ label, detail, checked, onChange }: { label: string; detail?: string; checked: boolean; onChange: (checked: boolean) => void }) {
  return <label className="settings-toggle"><span><strong>{label}</strong>{detail && <small>{detail}</small>}</span><input type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} /><i /></label>
}

function StatusRow({ label, value }: { label: string; value: string }) {
  return <div className="settings-status-row"><span>{label}</span><strong>{value}</strong></div>
}
