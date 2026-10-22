import { useCallback, useEffect, useState } from 'react'
import type { ChangeEvent, FormEvent } from 'react'
import {
  acknowledgeConflict,
  analyzeImpact,
  approvePlan,
  approveTaskAssignment,
  approveWorkspaceMerge,
  cleanupTaskWorkspace,
  clearTaskAssignment,
  compareReplayPoints,
  compileContext,
  createGoal,
  createTask,
  endAgentSession,
  evaluateConflicts,
  explainHistory,
  generateSmartContext,
  generatePlan,
  getAuditTrail,
  getApplicationSettings,
  getBackgroundStatus,
  getCollaborativeExecutionSnapshot,
  getDeliverySnapshot,
  getEnvironmentDashboard,
  getIdleTimeoutMinutes,
  getIntegrationRegistry,
  getLastRepository,
  getProjectGraph,
  getProjectMemory,
  getTimeline,
  getVerificationSnapshot,
  getOrchestrationSnapshot,
  getPlannerSnapshot,
  getSandboxStatus,
  ingestExternalEvent,
  isDesktopRuntime,
  markSessionIdle,
  openContextFile,
  recordDecision,
  replayProject,
  replan,
  prepareTaskWorkspace,
  prepareWorkspaceMerge,
  requestAction,
  requestPluginAction,
  resolveConflict,
  runWorkspaceVerification,
  saveEnvironmentManifest,
  scanProject,
  suggestTaskRoute,
  setIdleTimeoutMinutes,
  setProjectTracking,
  startAgentSession,
  startTaskWorkspace,
  syncGithubDelivery,
  updateGoal,
  updateCapabilityProfile,
  updateApplicationSettings,
  updateProjectPermissions,
  updateTask,
  updateTaskOrchestration,
  validateTaskWorkspace,
  waiveVerificationGate,
  type ActionKind,
  type ApplicationSettings,
  type AgentCapabilityProfile,
  type Goal,
  type HistoryExplanation,
  type GraphNode,
  type ImpactAnalysis,
  type AgentProvider,
  type BackgroundStatus,
  type ContextDepth,
  type ContextPackage,
  type ContextProfile,
  type DeliverySnapshot,
  type EnvironmentDashboard,
  type EnvironmentManifest,
  type ExplanationRecord,
  type Capability,
  type CollaborativeExecutionSnapshot,
  type OrchestrationSnapshot,
  type PlannerSnapshot,
  type PermissionMode,
  type PlannedTask,
  type ProjectMemory,
  type ProjectGraph,
  type ProjectStatus,
  type ReplayComparison,
  type ReplayState,
  type RegistrySnapshot,
  type SandboxStatus,
  type TaskPriority,
  type TaskWorkspace,
  type TimelineSnapshot,
  type VerificationSnapshot,
  type VerificationSuite,
  type WorkspaceValidation,
  type WorkTask,
} from './api'
import { OnboardingWizard, PermissionModeControl, ProjectHealthDashboard, SettingsView } from './ProductUX'
import { AssistantHome } from './AssistantHome'

type View = 'home' | 'repository' | 'goals' | 'plan' | 'activity' | 'timeline' | 'audit' | 'handoff' | 'conflicts' | 'context' | 'actions' | 'orchestration' | 'execution' | 'environment' | 'integrations' | 'impact' | 'pullRequests' | 'ci' | 'settings'

const CAPABILITIES: Capability[] = ['architecture', 'backend', 'frontend', 'refactoring', 'research', 'testing', 'documentation', 'rust', 'typeScript', 'security', 'devOps']

function errorMessage(error: unknown) {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  return 'AgentRelay could not complete that action.'
}

function words(value: string) {
  return value.replace(/([a-z])([A-Z])/g, '$1 $2').replaceAll('_', ' ')
}

function splitList(value: string) {
  return value.split(',').map((item) => item.trim()).filter(Boolean)
}

function FileList({ files, emptyLabel }: { files: string[]; emptyLabel: string }) {
  if (files.length === 0) return <p className="empty-state">{emptyLabel}</p>
  return <ul className="file-list">{files.map((file) => <li key={file}><span className="file-icon" /><span>{file}</span></li>)}</ul>
}

function ImpactNodeList({ nodes, emptyLabel }: { nodes: GraphNode[]; emptyLabel: string }) {
  if (nodes.length === 0) return <p className="empty-state">{emptyLabel}</p>
  return <ul className="impact-node-list">{nodes.map((node) => <li key={node.id}><span className={`node-kind ${node.nodeType}`}>{words(node.nodeType)}</span><div><strong>{node.label}</strong><small>{node.path || node.id}{node.status ? ` · ${words(node.status)}` : ''}</small></div></li>)}</ul>
}

function GoalCard({ goal, busy, onChanged }: {
  goal: Goal
  busy: boolean
  onChanged: (action: () => Promise<unknown>) => Promise<void>
}) {
  const [editing, setEditing] = useState(false)
  const [addingTask, setAddingTask] = useState(false)
  const [title, setTitle] = useState(goal.title)
  const [description, setDescription] = useState(goal.description)
  const [taskTitle, setTaskTitle] = useState('')
  const [taskDescription, setTaskDescription] = useState('')
  const [priority, setPriority] = useState<TaskPriority>('medium')
  const [assignedAgent, setAssignedAgent] = useState('')

  async function saveGoal(event: FormEvent) {
    event.preventDefault()
    await onChanged(() => updateGoal({ id: goal.id, title, description, status: goal.status }))
    setEditing(false)
  }

  async function addTask(event: FormEvent) {
    event.preventDefault()
    await onChanged(() => createTask({
      goalId: goal.id,
      title: taskTitle,
      description: taskDescription,
      priority,
      assignedAgent: assignedAgent.trim() || null,
      sourceAgent: 'user',
    }))
    setTaskTitle('')
    setTaskDescription('')
    setAssignedAgent('')
    setAddingTask(false)
  }

  async function completeTask(task: WorkTask) {
    await onChanged(() => updateTask({
      id: task.id,
      title: task.title,
      description: task.description,
      status: task.status === 'completed' ? 'todo' : 'completed',
      priority: task.priority,
      assignedAgent: task.assignedAgent,
    }))
  }

  return (
    <article className={`goal-card ${goal.status === 'archived' ? 'archived' : ''}`}>
      <div className="goal-heading">
        <div><span className={`status-pill ${goal.status}`}>{goal.status}</span><h3>{goal.title}</h3><p>{goal.description || 'No description yet.'}</p></div>
        <div className="button-row">
          <button className="ghost-button" type="button" onClick={() => setEditing(!editing)}>Edit</button>
          {goal.status !== 'archived' && <button className="ghost-button danger" type="button" disabled={busy} onClick={() => void onChanged(() => updateGoal({ id: goal.id, title: goal.title, description: goal.description, status: 'archived' }))}>Archive</button>}
        </div>
      </div>

      {editing && <form className="inline-form" onSubmit={saveGoal}>
        <input value={title} onChange={(event) => setTitle(event.target.value)} required />
        <textarea value={description} onChange={(event) => setDescription(event.target.value)} placeholder="What outcome defines this goal?" />
        <div className="button-row"><button className="primary-button" disabled={busy}>Save goal</button><button className="ghost-button" type="button" onClick={() => setEditing(false)}>Cancel</button></div>
      </form>}

      <div className="task-list">
        {goal.tasks.length === 0 && <p className="empty-state">No tasks yet.</p>}
        {goal.tasks.map((task) => <div className={`task-row ${task.status === 'completed' ? 'done' : ''}`} key={task.id}>
          <button className="check-button" type="button" aria-label={task.status === 'completed' ? 'Reopen task' : 'Complete task'} disabled={busy} onClick={() => void completeTask(task)}>{task.status === 'completed' ? '✓' : ''}</button>
          <div className="task-copy"><strong>{task.title}</strong><span>{task.priority}{task.assignedAgent ? ` · ${task.assignedAgent}` : ''}{task.description ? ` · ${task.description}` : ''}</span></div>
        </div>)}
      </div>

      {goal.status === 'active' && <>
        <button className="add-link" type="button" onClick={() => setAddingTask(!addingTask)}>+ Add task</button>
        {addingTask && <form className="inline-form task-form" onSubmit={addTask}>
          <input value={taskTitle} onChange={(event) => setTaskTitle(event.target.value)} placeholder="Concrete next step" required />
          <textarea value={taskDescription} onChange={(event) => setTaskDescription(event.target.value)} placeholder="Optional details" />
          <div className="form-grid"><select value={priority} onChange={(event) => setPriority(event.target.value as TaskPriority)}><option value="low">Low priority</option><option value="medium">Medium priority</option><option value="high">High priority</option><option value="critical">Critical</option></select><input value={assignedAgent} onChange={(event) => setAssignedAgent(event.target.value)} placeholder="Assigned agent (optional)" /></div>
          <button className="primary-button" disabled={busy}>Create task</button>
        </form>}
      </>}
    </article>
  )
}

function selectedValues(event: ChangeEvent<HTMLSelectElement>) {
  return Array.from(event.currentTarget.selectedOptions, (option) => option.value)
}

function CapabilityProfileCard({ profile, busy, onSave }: {
  profile: AgentCapabilityProfile
  busy: boolean
  onSave: (profile: AgentCapabilityProfile) => Promise<void>
}) {
  const [displayName, setDisplayName] = useState(profile.displayName)
  const [strengths, setStrengths] = useState<Capability[]>(profile.strengths)
  const [maxTokens, setMaxTokens] = useState(profile.maxContextTokens)
  const [enabled, setEnabled] = useState(profile.enabled)
  const [allowedActions, setAllowedActions] = useState<ActionKind[]>(profile.allowedActions)

  useEffect(() => {
    setDisplayName(profile.displayName)
    setStrengths(profile.strengths)
    setMaxTokens(profile.maxContextTokens)
    setEnabled(profile.enabled)
    setAllowedActions(profile.allowedActions)
  }, [profile])

  async function save(event: FormEvent) {
    event.preventDefault()
    await onSave({ ...profile, displayName, strengths, maxContextTokens: maxTokens, enabled, allowedActions })
  }

  function toggleAction(action: ActionKind) {
    setAllowedActions((current) => current.includes(action)
      ? current.filter((item) => item !== action)
      : [...current, action])
  }

  return <form className={`profile-card ${enabled ? '' : 'disabled'}`} onSubmit={save}>
    <div className="profile-heading"><div><span>{profile.provider}</span><input value={displayName} onChange={(event) => setDisplayName(event.target.value)} required /></div><label className="enabled-check"><input type="checkbox" checked={enabled} onChange={(event) => setEnabled(event.target.checked)} /> Enabled</label></div>
    <label>Strengths<select multiple value={strengths} onChange={(event) => setStrengths(selectedValues(event) as Capability[])}>{CAPABILITIES.map((capability) => <option value={capability} key={capability}>{words(capability)}</option>)}</select></label>
    <label>Maximum context tokens<input type="number" min="512" max="128000" value={maxTokens} onChange={(event) => setMaxTokens(Number(event.target.value))} /></label>
    <div className="profile-actions"><label><input type="checkbox" checked={allowedActions.includes('runTests')} onChange={() => toggleAction('runTests')} /> Run tests</label><label><input type="checkbox" checked={allowedActions.includes('runBuild')} onChange={() => toggleAction('runBuild')} /> Run build</label></div>
    <button className="ghost-button" disabled={busy}>Save profile</button>
  </form>
}

function OrchestrationTaskCard({ task, allTasks, profiles, recommendation, busy, onSave, onSuggest, onApprove, onStart, onClear, onWhy }: {
  task: WorkTask
  allTasks: WorkTask[]
  profiles: AgentCapabilityProfile[]
  recommendation: OrchestrationSnapshot['recommendations'][number] | undefined
  busy: boolean
  onSave: (input: Parameters<typeof updateTaskOrchestration>[0]) => Promise<void>
  onSuggest: () => Promise<void>
  onApprove: (provider: AgentProvider) => Promise<void>
  onStart: () => Promise<void>
  onClear: () => Promise<void>
  onWhy: () => void
}) {
  const [editing, setEditing] = useState(false)
  const [provider, setProvider] = useState<AgentProvider>(task.suggestedProvider || task.assignedProvider || recommendation?.provider || 'codex')
  const [requiredCapabilities, setRequiredCapabilities] = useState<Capability[]>(task.requiredCapabilities)
  const [blockedBy, setBlockedBy] = useState(task.blockedBy)
  const [dependsOn, setDependsOn] = useState(task.dependsOn)
  const [produces, setProduces] = useState(task.produces.join(', '))
  const [consumes, setConsumes] = useState(task.consumes.join(', '))
  const [claimedPaths, setClaimedPaths] = useState(task.claimedPaths.join(', '))

  useEffect(() => {
    setProvider(task.suggestedProvider || task.assignedProvider || recommendation?.provider || 'codex')
    setRequiredCapabilities(task.requiredCapabilities)
    setBlockedBy(task.blockedBy)
    setDependsOn(task.dependsOn)
    setProduces(task.produces.join(', '))
    setConsumes(task.consumes.join(', '))
    setClaimedPaths(task.claimedPaths.join(', '))
  }, [task, recommendation])

  async function save(event: FormEvent) {
    event.preventDefault()
    await onSave({ taskId: task.id, requiredCapabilities, blockedBy, dependsOn, produces: splitList(produces), consumes: splitList(consumes), claimedPaths: splitList(claimedPaths) })
    setEditing(false)
  }

  const selectableTasks = allTasks.filter((candidate) => candidate.id !== task.id)
  return <article className={`orchestration-task ${task.assignmentStatus}`}>
    <div className="orchestration-task-main"><span className={`assignment-dot ${task.assignmentStatus}`} /><div><small>{words(task.assignmentStatus)}</small><strong>{task.title}</strong><p>{task.description || 'No task description.'}</p></div><span className="agent-chip">{task.assignedProvider || task.suggestedProvider || 'unassigned'}</span></div>
    {(task.claimedPaths.length > 0 || task.dependsOn.length > 0 || task.blockedBy.length > 0) && <div className="task-coordination-meta">{task.claimedPaths.map((path) => <code key={path}>lock {path}</code>)}{task.dependsOn.map((id) => <code key={`dep-${id}`}>depends {allTasks.find((item) => item.id === id)?.title || id.slice(0, 8)}</code>)}{task.blockedBy.map((id) => <code key={`block-${id}`}>blocked by {allTasks.find((item) => item.id === id)?.title || id.slice(0, 8)}</code>)}</div>}
    {(task.routeReasons.length > 0 || recommendation) && <div className="route-reason"><span>Why this route</span><ul>{(task.routeReasons.length ? task.routeReasons : recommendation?.reasons || []).map((reason) => <li key={reason}>{reason}</li>)}</ul></div>}
    <div className="task-route-controls"><select value={provider} onChange={(event) => setProvider(event.target.value as AgentProvider)}>{profiles.filter((profile) => profile.enabled).map((profile) => <option value={profile.provider} key={profile.id}>{profile.displayName}</option>)}</select><button className="ghost-button" disabled={busy || task.assignmentStatus === 'running' || task.assignmentStatus === 'completed'} onClick={() => void onSuggest()}>Suggest</button><button className="ghost-button" disabled={busy || task.assignmentStatus === 'running' || task.assignmentStatus === 'completed'} onClick={() => void onApprove(provider)}>Approve</button>{task.assignmentStatus === 'assigned' && <button className="primary-button" disabled={busy} onClick={() => void onStart()}>Prepare workspace</button>}{!['unassigned', 'running', 'completed'].includes(task.assignmentStatus) && <button className="ghost-button danger" disabled={busy} onClick={() => void onClear()}>Clear</button>}<button className="ghost-button" disabled={busy} onClick={() => setEditing(!editing)}>Coordinate</button><button className="why-button" type="button" onClick={onWhy}>Why?</button></div>
    {editing && <form className="task-orchestration-form" onSubmit={save}>
      <label>Required capabilities<select multiple value={requiredCapabilities} onChange={(event) => setRequiredCapabilities(selectedValues(event) as Capability[])}>{CAPABILITIES.map((capability) => <option value={capability} key={capability}>{words(capability)}</option>)}</select></label>
      <label>Depends on<select multiple value={dependsOn} onChange={(event) => setDependsOn(selectedValues(event))}>{selectableTasks.map((candidate) => <option value={candidate.id} key={candidate.id}>{candidate.title}</option>)}</select></label>
      <label>Explicitly blocked by<select multiple value={blockedBy} onChange={(event) => setBlockedBy(selectedValues(event))}>{selectableTasks.map((candidate) => <option value={candidate.id} key={candidate.id}>{candidate.title}</option>)}</select></label>
      <label>Produces<input value={produces} onChange={(event) => setProduces(event.target.value)} placeholder="schema, API contract" /></label>
      <label>Consumes<input value={consumes} onChange={(event) => setConsumes(event.target.value)} placeholder="architecture decision" /></label>
      <label>Workspace paths<input value={claimedPaths} onChange={(event) => setClaimedPaths(event.target.value)} placeholder="src/auth/*, tests/auth/*" /></label>
      <button className="primary-button" disabled={busy}>Save coordination metadata</button>
    </form>}
  </article>
}

function WorkspaceCard({ workspace, task, validation, verification, busy, onStart, onEnd, onValidate, onVerify, onWaive, onPrepareMerge, onApproveMerge, onCleanup, onWhy }: {
  workspace: TaskWorkspace
  task: WorkTask | undefined
  validation: WorkspaceValidation | undefined
  verification: VerificationSuite | undefined
  busy: boolean
  onStart: () => Promise<void>
  onEnd: () => Promise<void>
  onValidate: () => Promise<void>
  onVerify: () => Promise<void>
  onWaive: (gateId: string) => Promise<void>
  onPrepareMerge: () => Promise<void>
  onApproveMerge: () => Promise<void>
  onCleanup: () => Promise<void>
  onWhy: (subjectType: string, subjectId: string) => void
}) {
  const verifiedHead = verification?.commit === workspace.headCommit && ['passed', 'waived'].includes(verification.status)
  return <article className={`workspace-execution-card ${workspace.status}`}>
    <div className="workspace-execution-heading">
      <div><span className={`workspace-status ${workspace.status}`}>{words(workspace.status)}</span><h3>{task?.title || `Task ${workspace.taskId.slice(0, 8)}`}</h3><p>{workspace.assignedProvider} · <code>{workspace.branch}</code></p></div>
      <span className="workspace-head">{workspace.headCommit.slice(0, 10)}</span>
    </div>
    <dl className="workspace-binding"><div><dt>Isolated path</dt><dd title={workspace.worktreePath}>{workspace.worktreePath}</dd></div><div><dt>Base commit</dt><dd><code>{workspace.baseCommit.slice(0, 12)}</code></dd></div><div><dt>Session</dt><dd>{workspace.sessionId?.slice(0, 8) || 'not started'}</dd></div></dl>
    {validation && <div className={`workspace-validation ${validation.status}`}><div><strong>Latest validation</strong><span>{validation.status} · {validation.changedFiles.length} changed file(s)</span></div><ul>{validation.checks.map((check) => <li className={check.status} key={`${validation.id}-${check.kind}`}><span>{check.status === 'passed' ? '✓' : check.status === 'failed' ? '!' : '–'}</span><div><strong>{words(check.kind)}</strong><small>{check.detail}</small></div></li>)}</ul></div>}
    {verification && <div className={`verification-suite ${verification.status}`}>
      <div className="verification-heading"><div><strong>Independent verification</strong><span>{words(verification.status)} · commit {verification.commit.slice(0, 10)} · {verification.environmentProfile}</span></div><div className="verification-heading-actions"><code title={verification.environmentFingerprint}>{verification.environmentFingerprint.slice(0, 12)}</code><button className="why-button" type="button" onClick={() => onWhy('verificationSuite', verification.id)}>Why?</button></div></div>
      <ul>{verification.gates.map((gate) => <li className={gate.status} key={gate.id}><span>{gate.status === 'passed' ? '✓' : gate.status === 'failed' ? '!' : gate.status === 'waived' ? '◇' : '·'}</span><div><strong>{gate.name}</strong><small>{gate.detail}{gate.exitCode !== null ? ` · exit ${gate.exitCode}` : ''}{gate.durationMs !== null ? ` · ${gate.durationMs}ms` : ''}</small>{gate.command.length > 0 && <code>{gate.command.join(' ')}</code>}</div><div className="verification-gate-actions"><button className="why-button" type="button" onClick={() => onWhy('verificationGate', gate.id)}>Why?</button>{gate.status === 'failed' && <button className="ghost-button" type="button" disabled={busy} onClick={() => void onWaive(gate.id)}>Waive</button>}</div></li>)}</ul>
      {verification.repairTaskId && <p className="repair-task-link">Repair task created: <code>{verification.repairTaskId.slice(0, 10)}</code></p>}
    </div>}
    <div className="workspace-actions">
      {workspace.status === 'ready' && <button className="primary-button" disabled={busy} onClick={() => void onStart()}>Start isolated session</button>}
      {workspace.status === 'active' && <button className="primary-button" disabled={busy} onClick={() => void onEnd()}>Finish agent session</button>}
      {['validationPending', 'validationFailed', 'mergeConflict'].includes(workspace.status) && <button className="primary-button" disabled={busy} onClick={() => void onValidate()}>Validate branch</button>}
      {workspace.status === 'validationPassed' && !verifiedHead && <button className="primary-button" disabled={busy} onClick={() => void onVerify()}>{verification?.status === 'failed' ? 'Verify again' : 'Run independent verification'}</button>}
      {workspace.status === 'validationPassed' && verifiedHead && <button className="primary-button" disabled={busy} onClick={() => void onPrepareMerge()}>Prepare merge</button>}
      {workspace.status === 'mergeReady' && <button className="primary-button merge-approval" disabled={busy} onClick={() => void onApproveMerge()}>Approve & merge</button>}
      {workspace.status === 'merged' && <button className="ghost-button" disabled={busy} onClick={() => void onCleanup()}>Clean up worktree</button>}
      {workspace.status === 'archived' && <span className="workspace-preserved">Worktree removed · branch and history preserved</span>}
    </div>
  </article>
}

function PlannedTaskCard({ task, tasks, busy, onFail }: {
  task: PlannedTask
  tasks: PlannedTask[]
  busy: boolean
  onFail: () => void
}) {
  const dependencies = task.dependsOn
    .map((id) => tasks.find((candidate) => candidate.id === id)?.title || id.slice(0, 8))
  return <article className={`planned-task-card ${task.status} ${task.conflictRisk}`}>
    <div className="planned-task-stage"><span>{task.parallelGroup + 1}</span><small>stage</small></div>
    <div className="planned-task-copy">
      <div className="planned-task-heading"><span className="node-kind task">{words(task.kind)}</span><span className={`status-pill ${task.status}`}>{words(task.status)}</span><span className={`plan-risk ${task.conflictRisk}`}>{task.riskScore} · {task.conflictRisk}</span></div>
      <h3>{task.title}</h3><p>{task.description}</p>
      <div className="plan-task-meta"><span><b>Agent</b>{task.suggestedProvider || 'No match'}</span><span><b>Depends on</b>{dependencies.join(', ') || 'None'}</span><span><b>Modules</b>{task.affectedModules.join(', ') || 'Scope review'}</span><span><b>Parallel</b>{task.canRunInParallel ? 'Eligible' : 'Serialized'}</span></div>
      <details><summary>Evidence and expected scope</summary><p>{task.suggestionReasons[0] || 'No routing evidence.'}</p><p>{task.riskReasons.join(' ')}</p><FileList files={task.expectedFiles} emptyLabel="No existing file matched; review scope before approval." /></details>
    </div>
    {task.materializedTaskId && !['completed', 'failed'].includes(task.status) && <button className="ghost-button danger" type="button" disabled={busy} onClick={onFail}>Report failure</button>}
  </article>
}

function AuditView({ repositoryPath, focus, onClearFocus }: {
  repositoryPath: string
  focus: { subjectType: string; subjectId: string } | null
  onClearFocus: () => void
}) {
  const [records, setRecords] = useState<ExplanationRecord[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [category, setCategory] = useState('all')
  const [query, setQuery] = useState('')
  const [auditError, setAuditError] = useState<string | null>(null)
  const [auditLoading, setAuditLoading] = useState(true)

  useEffect(() => {
    let active = true
    setAuditLoading(true)
    void getAuditTrail(repositoryPath).then((next) => {
      if (!active) return
      setRecords(next)
      const focused = focus
        ? next.find((record) => record.subjectType === focus.subjectType && record.subjectId === focus.subjectId)
        : null
      setSelectedId(focused?.id || next[0]?.id || null)
    }).catch((reason) => active && setAuditError(errorMessage(reason)))
      .finally(() => active && setAuditLoading(false))
    return () => { active = false }
  }, [repositoryPath, focus?.subjectId, focus?.subjectType])

  const categories = [...new Set(records.map((record) => record.category))]
  const normalizedQuery = query.trim().toLowerCase()
  const filtered = records.filter((record) => {
    if (focus && (record.subjectType !== focus.subjectType || record.subjectId !== focus.subjectId)) return false
    if (category !== 'all' && record.category !== category) return false
    if (!normalizedQuery) return true
    return [record.decision, record.outcome, record.subjectType, record.subjectId, ...record.reasons.map((reason) => `${reason.summary} ${reason.detail}`)]
      .join(' ').toLowerCase().includes(normalizedQuery)
  })
  const selected = filtered.find((record) => record.id === selectedId)
    || filtered[0]
    || null

  return <section className="workspace-view audit-view">
    <div className="view-heading context-heading"><div><p className="section-label">EXPLAINABILITY + AUDIT TRAIL</p><h2>Every important choice keeps its evidence.</h2><p>Reasons come from deterministic rules, scores, policies, graph edges, and recorded state. Generated prose may summarize them later, but it cannot replace this evidence.</p></div><span className="audit-grounded-badge">STRUCTURED EVIDENCE</span></div>
    {auditError && <div className="error-panel compact">{auditError}</div>}
    {focus && <div className="audit-focus"><span>Showing explanations for <strong>{focus.subjectType}</strong> · <code>{focus.subjectId}</code></span><button className="ghost-button" onClick={onClearFocus}>Show all audit records</button></div>}
    <div className="audit-controls"><select value={category} onChange={(event) => setCategory(event.target.value)}><option value="all">All categories</option>{categories.map((item) => <option value={item} key={item}>{words(item)}</option>)}</select><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search decisions, reasons, evidence…" /><span>{filtered.length} record{filtered.length === 1 ? '' : 's'}</span></div>
    {auditLoading && <div className="empty-panel">Loading immutable explanation records…</div>}
    {!auditLoading && records.length === 0 && <div className="empty-panel">No audit records yet. Generate context, analyze impact, route a task, evaluate conflicts, run verification, or generate a plan.</div>}
    {!auditLoading && records.length > 0 && <div className="audit-layout">
      <div className="audit-list">{filtered.map((record) => <button type="button" className={selected?.id === record.id ? 'active' : ''} key={record.id} onClick={() => setSelectedId(record.id)}><span className={`audit-category ${record.category}`}>{words(record.category)}</span><strong>{record.decision}</strong><small>{record.subjectType} · {record.outcome} · {new Date(record.createdAt).toLocaleString()}</small></button>)}</div>
      <div className="audit-detail">{selected ? <>
        <header><div><span className={`audit-category ${selected.category}`}>{words(selected.category)}</span><h3>{selected.decision}</h3><p>{selected.subjectType} · <code>{selected.subjectId}</code></p></div><span className={`audit-outcome ${selected.outcome.toLowerCase()}`}>{words(selected.outcome)}</span></header>
        <section><p className="section-label">WHY</p><div className="audit-reasons">{selected.reasons.map((reason) => <article key={reason.code}><span>{reason.score === null ? '—' : `${reason.score > 0 ? '+' : ''}${reason.score}`}</span><div><strong>{reason.summary}</strong><p>{reason.detail}</p><code>{reason.code}</code></div></article>)}</div></section>
        <section><p className="section-label">EVIDENCE</p><div className="audit-evidence">{selected.evidenceRefs.map((evidence, index) => <article key={`${evidence.kind}-${evidence.id}-${index}`}><span>{words(evidence.kind)}</span><strong>{evidence.label}</strong><code>{evidence.id}</code></article>)}</div></section>
        <footer><span>Recorded {new Date(selected.createdAt).toLocaleString()}</span><code>{selected.id}</code></footer>
      </> : <div className="empty-panel">No record matches the current filter.</div>}</div>
    </div>}
  </section>
}

function TimelineView({ repositoryPath }: { repositoryPath: string }) {
  const [timeline, setTimeline] = useState<TimelineSnapshot | null>(null)
  const [replay, setReplay] = useState<ReplayState | null>(null)
  const [comparison, setComparison] = useState<ReplayComparison | null>(null)
  const [explanation, setExplanation] = useState<HistoryExplanation | null>(null)
  const [leftEventId, setLeftEventId] = useState('')
  const [rightEventId, setRightEventId] = useState('')
  const [entityType, setEntityType] = useState('decision')
  const [entityId, setEntityId] = useState('')
  const [busy, setBusy] = useState(false)
  const [timelineError, setTimelineError] = useState<string | null>(null)

  const selectPoint = useCallback(async (eventId: string) => {
    setBusy(true)
    setTimelineError(null)
    try {
      setReplay(await replayProject(repositoryPath, eventId))
    } catch (reason) {
      setTimelineError(errorMessage(reason))
    } finally {
      setBusy(false)
    }
  }, [repositoryPath])

  useEffect(() => {
    let active = true
    setBusy(true)
    void getTimeline(repositoryPath).then(async (next) => {
      if (!active) return
      setTimeline(next)
      const first = next.entries[0]?.eventId || ''
      const last = next.entries.at(-1)?.eventId || ''
      setLeftEventId(first)
      setRightEventId(last)
      if (last) await selectPoint(last)
    }).catch((reason) => active && setTimelineError(errorMessage(reason)))
      .finally(() => active && setBusy(false))
    return () => { active = false }
  }, [repositoryPath, selectPoint])

  async function comparePoints() {
    if (!leftEventId || !rightEventId) return
    setBusy(true)
    setTimelineError(null)
    try {
      setComparison(await compareReplayPoints(repositoryPath, leftEventId, rightEventId))
    } catch (reason) {
      setTimelineError(errorMessage(reason))
    } finally {
      setBusy(false)
    }
  }

  async function explainEntity(type = entityType, id = entityId) {
    if (!id.trim()) return
    setEntityType(type)
    setEntityId(id)
    setBusy(true)
    setTimelineError(null)
    try {
      setExplanation(await explainHistory(repositoryPath, type, id, replay?.point.eventId || null))
    } catch (reason) {
      setTimelineError(errorMessage(reason))
    } finally {
      setBusy(false)
    }
  }

  const options = timeline?.entries ?? []
  const taskCount = replay?.goals.reduce((count, goal) => count + goal.tasks.length, 0) ?? 0

  return <section className="workspace-view replay-view">
    <div className="view-heading context-heading"><div><p className="section-label">PROJECT TIME TRAVEL · READ ONLY</p><h2>Reconstruct what every agent knew.</h2><p>Replay projects immutable events into historical goals, tasks, decisions, Git state, context, environments, and verification evidence. The live repository is never checked out or changed.</p></div><span className="replay-safe-badge">REPLAY-SAFE</span></div>
    {timelineError && <div className="error-panel compact">{timelineError}</div>}
    {!timeline?.entries.length && <div className="empty-panel">No historical events have been recorded for this project.</div>}
    {timeline && timeline.entries.length > 0 && <>
      <div className="replay-layout">
        <aside className="replay-timeline"><div className="replay-panel-heading"><strong>Timeline</strong><span>{timeline.entries.length} events</span></div>{[...timeline.entries].reverse().map((entry) => <button type="button" key={entry.eventId} className={replay?.point.eventId === entry.eventId ? 'active' : ''} disabled={busy} onClick={() => void selectPoint(entry.eventId)}><span className={`timeline-category ${entry.category}`} /><div><strong>{entry.title}</strong><small>{new Date(entry.timestamp).toLocaleString()} · {entry.source}</small><p>{entry.summary}</p></div></button>)}</aside>
        <div className="replay-state">
          {!replay && <div className="empty-panel">Select a timeline point to reconstruct project state.</div>}
          {replay && <>
            <article className="replay-hero"><div><span className={`status-pill ${replay.accuracy}`}>{words(replay.accuracy)}</span><h3>{replay.point.label}</h3><p>{new Date(replay.point.timestamp).toLocaleString()}</p></div>{replay.git && <dl><div><dt>Commit</dt><dd>{replay.git.commit.slice(0, 12)}</dd></div><div><dt>Branch</dt><dd>{replay.git.branch}</dd></div><div><dt>Working tree</dt><dd>{replay.git.dirty ? 'Dirty' : 'Clean'}</dd></div></dl>}</article>
            {replay.warnings.map((warning) => <div className="replay-warning" key={warning}>{warning}</div>)}
            <div className="replay-metrics"><article><span>Goals</span><strong>{replay.goals.length}</strong></article><article><span>Tasks</span><strong>{taskCount}</strong></article><article><span>Decisions</span><strong>{replay.decisions.length}</strong></article><article><span>Files</span><strong>{replay.files.length}</strong></article><article><span>Blockers</span><strong>{replay.blockers.length}</strong></article></div>
            <div className="replay-card-grid">
              <article><p className="section-label">CONTEXT SEEN</p>{replay.context ? <><strong>{replay.context.contextSnapshotHash.slice(0, 16)}…</strong><span>Plan v{replay.context.planVersion ?? '—'} · env {replay.context.environmentFingerprint?.slice(0, 10) || 'not captured'}</span><details><summary>Open exact AGENT_CONTEXT.md</summary><pre>{replay.context.markdown}</pre></details></> : <p className="empty-state">No versioned context existed yet.</p>}</article>
              <article><p className="section-label">VERIFICATION</p>{replay.verification ? <><strong>{words(replay.verification.status)}</strong><span>{replay.verification.gates.filter((gate) => gate.status === 'passed').length}/{replay.verification.gates.length} gates passed · {replay.verification.commit.slice(0, 12)}</span></> : <p className="empty-state">No verification run existed yet.</p>}</article>
              <article><p className="section-label">ENVIRONMENT</p>{replay.environment ? <><strong>{words(replay.environment.status)}</strong><span>{replay.environment.fingerprint.slice(0, 16)}… · {replay.environment.profile}</span></> : <p className="empty-state">No environment fingerprint existed yet.</p>}</article>
              <article><p className="section-label">PLAN</p>{replay.plan ? <><strong>Version {replay.plan.version} · {words(replay.plan.status)}</strong><span>{replay.plan.tasks.length} planned tasks</span></> : <p className="empty-state">No plan existed yet.</p>}</article>
            </div>
            <div className="replay-entities"><article><div className="replay-panel-heading"><strong>Decisions</strong><span>{replay.decisions.length}</span></div>{replay.decisions.map((decision) => <button type="button" key={decision.id} onClick={() => void explainEntity('decision', decision.id)}><div><strong>{decision.summary}</strong><small>{decision.createdBy} · {decision.status}</small></div><span>Why?</span></button>)}</article><article><div className="replay-panel-heading"><strong>Files changed by then</strong><span>{replay.files.length}</span></div>{replay.files.map((file) => <button type="button" key={file.path} onClick={() => void explainEntity('file', file.path)}><div><strong>{file.path}</strong><small>{file.changeType} · {file.sessionIds.length} session(s)</small></div><span>Why?</span></button>)}</article></div>
          </>}
        </div>
      </div>
      <div className="replay-tools">
        <article><p className="section-label">COMPARE TWO POINTS</p><div className="replay-selects"><select value={leftEventId} onChange={(event) => setLeftEventId(event.target.value)}>{options.map((entry) => <option key={entry.eventId} value={entry.eventId}>{new Date(entry.timestamp).toLocaleString()} · {entry.title}</option>)}</select><span>→</span><select value={rightEventId} onChange={(event) => setRightEventId(event.target.value)}>{options.map((entry) => <option key={entry.eventId} value={entry.eventId}>{new Date(entry.timestamp).toLocaleString()} · {entry.title}</option>)}</select><button className="primary-button" disabled={busy} onClick={() => void comparePoints()}>Compare</button></div>{comparison && <div className="comparison-grid"><span><b>{comparison.newDecisions.length}</b> new decisions</span><span><b>{comparison.supersededDecisions.length}</b> superseded</span><span><b>{comparison.completedTasks.length}</b> tasks completed</span><span><b>{comparison.changedFiles.length}</b> files changed</span><span><b>{comparison.newBlockers.length}</b> new blockers</span><span><b>{comparison.resolvedBlockers.length}</b> resolved blockers</span></div>}</article>
        <article><p className="section-label">WHY DID THIS CHANGE?</p><div className="explain-controls"><select value={entityType} onChange={(event) => setEntityType(event.target.value)}><option value="decision">Decision</option><option value="task">Task</option><option value="file">File</option><option value="session">Session</option><option value="goal">Goal</option></select><input value={entityId} onChange={(event) => setEntityId(event.target.value)} placeholder="Entity ID or exact file path" /><button className="ghost-button" disabled={busy || !entityId.trim()} onClick={() => void explainEntity()}>Explain</button></div>{explanation && <div className="explanation"><h3>{explanation.title}</h3><p>{explanation.summary}</p>{explanation.evidence.map((item) => <div key={item.eventId}><span>{new Date(item.timestamp).toLocaleString()}</span><strong>{words(item.kind)}</strong><p>{item.explanation}</p></div>)}</div>}</article>
      </div>
    </>}
  </section>
}

export default function App() {
  const [view, setView] = useState<View>('home')
  const [repositoryPath, setRepositoryPath] = useState('')
  const [status, setStatus] = useState<ProjectStatus | null>(null)
  const [memory, setMemory] = useState<ProjectMemory | null>(null)
  const [applicationSettings, setApplicationSettings] = useState<ApplicationSettings | null>(null)
  const [showOnboarding, setShowOnboarding] = useState(false)
  const [healthRefresh, setHealthRefresh] = useState(0)
  const [context, setContext] = useState('')
  const [contextPackage, setContextPackage] = useState<ContextPackage | null>(null)
  const [contextDepth, setContextDepth] = useState<ContextDepth>('normal')
  const [contextProfile, setContextProfile] = useState<ContextProfile | 'auto'>('auto')
  const [contextTaskId, setContextTaskId] = useState('')
  const [expandedContextItemId, setExpandedContextItemId] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [goalTitle, setGoalTitle] = useState('')
  const [goalDescription, setGoalDescription] = useState('')
  const [decisionSummary, setDecisionSummary] = useState('')
  const [decisionReason, setDecisionReason] = useState('')
  const [alternatives, setAlternatives] = useState('')
  const [affected, setAffected] = useState('')
  const [decisionAuthor, setDecisionAuthor] = useState('user')
  const [supersedesDecisionId, setSupersedesDecisionId] = useState('')
  const [sessionProvider, setSessionProvider] = useState<AgentProvider>('codex')
  const [agentName, setAgentName] = useState('Codex')
  const [sessionSummary, setSessionSummary] = useState('')
  const [sessionBlockers, setSessionBlockers] = useState('')
  const [backgroundStatus, setBackgroundStatus] = useState<BackgroundStatus | null>(null)
  const [idleTimeout, setIdleTimeout] = useState(15)
  const [sandboxStatus, setSandboxStatus] = useState<SandboxStatus | null>(null)
  const [orchestration, setOrchestration] = useState<OrchestrationSnapshot | null>(null)
  const [planner, setPlanner] = useState<PlannerSnapshot | null>(null)
  const [plannerGoalId, setPlannerGoalId] = useState('')
  const [execution, setExecution] = useState<CollaborativeExecutionSnapshot | null>(null)
  const [verification, setVerification] = useState<VerificationSnapshot | null>(null)
  const [environment, setEnvironment] = useState<EnvironmentDashboard | null>(null)
  const [environmentLoading, setEnvironmentLoading] = useState(false)
  const [registry, setRegistry] = useState<RegistrySnapshot | null>(null)
  const [delivery, setDelivery] = useState<DeliverySnapshot | null>(null)
  const [githubRepository, setGithubRepository] = useState('')
  const [projectGraph, setProjectGraph] = useState<ProjectGraph | null>(null)
  const [impact, setImpact] = useState<ImpactAnalysis | null>(null)
  const [impactTarget, setImpactTarget] = useState('')
  const [impactLoading, setImpactLoading] = useState(false)
  const [auditFocus, setAuditFocus] = useState<{ subjectType: string; subjectId: string } | null>(null)
  const [integrationId, setIntegrationId] = useState('github')
  const [externalEventType, setExternalEventType] = useState('issue.updated')
  const [externalPayload, setExternalPayload] = useState('{\n  "number": 42,\n  "title": "Fix authentication flow",\n  "state": "open"\n}')
  const desktopRuntime = isDesktopRuntime()

  useEffect(() => {
    window.scrollTo(0, 0)
  }, [view])

  const refreshMemory = useCallback(async (path: string) => {
    const [nextMemory, nextContext, nextOrchestration, nextExecution, nextPlanner, nextVerification] = await Promise.all([
      getProjectMemory(path),
      compileContext({
        repositoryPath: path,
        sessionId: null,
        taskId: contextTaskId || null,
        depth: contextDepth,
        profile: contextProfile === 'auto' ? null : contextProfile,
      }),
      getOrchestrationSnapshot(path),
      getCollaborativeExecutionSnapshot(path),
      getPlannerSnapshot(path),
      getVerificationSnapshot(path),
    ])
    setMemory(nextMemory)
    setContextPackage(nextContext)
    setContext(nextContext.markdown)
    setOrchestration(nextOrchestration)
    setExecution(nextExecution)
    setPlanner(nextPlanner)
    setVerification(nextVerification)
    setPlannerGoalId((current) => current || nextMemory.goals.find((goal) => goal.status === 'active')?.id || '')
  }, [contextDepth, contextProfile, contextTaskId])

  const loadStatus = useCallback(async (path: string) => {
    const cleanPath = path.trim()
    if (!cleanPath) return
    setLoading(true)
    setError(null)
    setNotice(null)
    try {
      const result = await scanProject(cleanPath)
      setStatus(result)
      setRepositoryPath(result.repository)
      await refreshMemory(result.repository)
    } catch (requestError) {
      setError(errorMessage(requestError))
    } finally {
      setLoading(false)
    }
  }, [refreshMemory])

  useEffect(() => {
    if (!desktopRuntime) return
    void Promise.all([getApplicationSettings(), getLastRepository()]).then(([settings, path]) => {
      setApplicationSettings(settings)
      setContextDepth(settings.defaultContextDepth)
      setContextProfile(settings.defaultContextProfile)
      if (path) return loadStatus(path)
    }).catch((reason) => setError(errorMessage(reason)))
  }, [desktopRuntime, loadStatus])

  useEffect(() => {
    if (!desktopRuntime || view !== 'environment' || !status) return
    let active = true
    setEnvironmentLoading(true)
    void getEnvironmentDashboard(status.repository)
      .then((next) => { if (active) setEnvironment(next) })
      .catch((reason) => { if (active) setError(errorMessage(reason)) })
      .finally(() => { if (active) setEnvironmentLoading(false) })
    return () => { active = false }
  }, [desktopRuntime, status, view])

  useEffect(() => {
    if (!desktopRuntime) return
    let active = true
    const refresh = () => void getBackgroundStatus()
      .then((next) => {
        if (active) setBackgroundStatus(next)
      })
      .catch(() => undefined)
    refresh()
    void getIdleTimeoutMinutes().then((minutes) => active && setIdleTimeout(minutes))
    void getSandboxStatus().then((next) => active && setSandboxStatus(next))
    const timer = window.setInterval(refresh, 3000)
    return () => {
      active = false
      window.clearInterval(timer)
    }
  }, [desktopRuntime])

  useEffect(() => {
    if (!desktopRuntime || !status) return
    let active = true
    void getDeliverySnapshot(status.repository)
      .then((snapshot) => {
        if (!active) return
        setDelivery(snapshot)
        if (snapshot) setGithubRepository(`${snapshot.repository.owner}/${snapshot.repository.name}`)
      })
      .catch((reason) => active && setError(errorMessage(reason)))
    return () => { active = false }
  }, [desktopRuntime, status])

  useEffect(() => {
    if (!desktopRuntime) return
    void getIntegrationRegistry()
      .then(setRegistry)
      .catch((reason) => setError(errorMessage(reason)))
  }, [desktopRuntime])

  useEffect(() => {
    if (!desktopRuntime || view !== 'impact' || !status) return
    let active = true
    setImpactLoading(true)
    setError(null)
    void getProjectGraph(status.repository).then(async (graph) => {
      if (!active) return
      setProjectGraph(graph)
      const preferred = status.modifiedFiles[0]
        || status.addedFiles[0]
        || graph.nodes.find((node) => node.nodeType === 'file')?.path
        || graph.nodes.find((node) => node.nodeType === 'test')?.path
        || ''
      setImpactTarget(preferred)
      if (preferred) {
        const nextImpact = await analyzeImpact(status.repository, preferred)
        if (active) setImpact(nextImpact)
      } else {
        setImpact(null)
      }
    }).catch((reason) => {
      if (active) setError(errorMessage(reason))
    }).finally(() => {
      if (active) setImpactLoading(false)
    })
    return () => { active = false }
  }, [desktopRuntime, status, view])

  async function mutate(action: () => Promise<unknown>) {
    if (!status) return
    setLoading(true)
    setError(null)
    try {
      await action()
      await refreshMemory(status.repository)
    } catch (reason) {
      setError(errorMessage(reason))
    } finally {
      setLoading(false)
    }
  }

  function openAudit(subjectType: string, subjectId: string) {
    setAuditFocus({ subjectType, subjectId })
    setView('audit')
  }

  function scan(event: FormEvent) {
    event.preventDefault()
    if (!desktopRuntime) {
      setError('The persistent Rust core is available in the Tauri desktop app.')
      return
    }
    void loadStatus(repositoryPath)
  }

  async function addGoal(event: FormEvent) {
    event.preventDefault()
    await mutate(() => createGoal({ repositoryPath: status!.repository, title: goalTitle, description: goalDescription }))
    setGoalTitle('')
    setGoalDescription('')
  }

  async function createPlan(event: FormEvent) {
    event.preventDefault()
    if (!status || !plannerGoalId) return
    await mutate(() => generatePlan(status.repository, plannerGoalId))
    setNotice('Plan proposal generated from the current graph, decisions, context freshness, and configurable agent profiles.')
  }

  async function approveActivePlan() {
    const plan = planner?.activePlan
    if (!plan || plan.status !== 'valid') return
    if (!window.confirm(`Approve plan v${plan.version} and create ${plan.tasks.length} project tasks? Agent assignments will remain suggestions.`)) return
    await mutate(() => approvePlan(plan.id))
    setNotice('Plan approved. Tasks were added to the existing orchestration queue; agents were not launched.')
  }

  async function reportPlannedTaskFailure(task: PlannedTask) {
    const plan = planner?.activePlan
    if (!plan) return
    const reason = window.prompt(`Why did “${task.title}” fail?`, 'Deterministic workspace validation failed.')
    if (!reason?.trim()) return
    await mutate(() => replan(plan.id, task.id, reason.trim()))
    setNotice('A new plan version was proposed. The failed task and downstream tasks were blocked, and a recovery task was added.')
  }

  async function addDecision(event: FormEvent) {
    event.preventDefault()
    await mutate(() => recordDecision({
      repositoryPath: status!.repository,
      summary: decisionSummary,
      reason: decisionReason,
      alternatives: splitList(alternatives),
      affected: splitList(affected),
      createdBy: decisionAuthor,
      supersedesDecisionId: supersedesDecisionId || null,
    }))
    setDecisionSummary('')
    setDecisionReason('')
    setAlternatives('')
    setAffected('')
    setSupersedesDecisionId('')
  }

  async function regenerateContext() {
    if (!status) return
    setLoading(true)
    setError(null)
    try {
      const result = await generateSmartContext({
        repositoryPath: status.repository,
        sessionId: null,
        taskId: contextTaskId || null,
        depth: contextDepth,
        profile: contextProfile === 'auto' ? null : contextProfile,
      })
      setContextPackage(result)
      setContext(result.markdown)
      setNotice(result.snapshot.compression.mode === 'nemotronValidated'
        ? 'AGENT_CONTEXT.md regenerated with a validated Nemotron summary.'
        : 'AGENT_CONTEXT.md regenerated from deterministic grounded context.')
      setMemory(await getProjectMemory(status.repository))
    } catch (reason) {
      setError(errorMessage(reason))
    } finally {
      setLoading(false)
    }
  }

  async function previewContext() {
    if (!status) return
    setLoading(true)
    setError(null)
    try {
      const result = await compileContext({
        repositoryPath: status.repository,
        sessionId: null,
        taskId: contextTaskId || null,
        depth: contextDepth,
        profile: contextProfile === 'auto' ? null : contextProfile,
      })
      setContextPackage(result)
      setContext(result.markdown)
      setNotice('Relevance preview recomputed without writing a file or calling a model.')
    } catch (reason) {
      setError(errorMessage(reason))
    } finally {
      setLoading(false)
    }
  }

  async function copyContext() {
    try {
      await navigator.clipboard.writeText(context)
      setNotice('Context copied to clipboard.')
    } catch {
      setError('Clipboard access was unavailable.')
    }
  }

  async function startSession(event: FormEvent) {
    event.preventDefault()
    await mutate(() => startAgentSession({
      repositoryPath: status!.repository,
      provider: sessionProvider,
      agentName,
    }))
    setView('handoff')
  }

  async function endSession(event: FormEvent) {
    event.preventDefault()
    const active = memory?.agentSessions.find((session) => session.status === 'active')
    if (!active) return
    await mutate(() => endAgentSession({
      sessionId: active.id,
      summaryStatus: sessionSummary,
      blockers: sessionBlockers.split(/[\n,]/).map((item) => item.trim()).filter(Boolean),
    }))
    setSessionSummary('')
    setSessionBlockers('')
    setNotice('Session ended and deterministic handoff generated.')
  }

  async function saveIdleTimeout(event: FormEvent) {
    event.preventDefault()
    setLoading(true)
    setError(null)
    try {
      const saved = await setIdleTimeoutMinutes(idleTimeout)
      setIdleTimeout(saved)
      setNotice(`Automatic sessions now close after ${saved} idle minute${saved === 1 ? '' : 's'}.`)
    } catch (reason) {
      setError(errorMessage(reason))
    } finally {
      setLoading(false)
    }
  }

  async function setExecutionPermission(permission: 'runTests' | 'runBuild', mode: PermissionMode) {
    if (!memory || !status) return
    await mutate(() => updateProjectPermissions({
      repositoryPath: status.repository,
      runTests: permission === 'runTests' ? mode : memory.permissions.runTests,
      runBuild: permission === 'runBuild' ? mode : memory.permissions.runBuild,
    }))
    setHealthRefresh((current) => current + 1)
    setNotice(`${words(permission)} permission set to ${mode} for this project.`)
  }

  async function savePermissionModes(runTests: PermissionMode, runBuild: PermissionMode) {
    if (!status) return
    await mutate(() => updateProjectPermissions({ repositoryPath: status.repository, runTests, runBuild }))
    setHealthRefresh((current) => current + 1)
  }

  async function saveApplicationSettings(next: ApplicationSettings) {
    const saved = await updateApplicationSettings(next)
    setApplicationSettings(saved)
    setContextDepth(saved.defaultContextDepth)
    setContextProfile(saved.defaultContextProfile)
  }

  async function runControlledAction(action: 'runTests' | 'runBuild') {
    if (!status || !memory) return
    const mode = memory.permissions[action]
    if (mode === 'ask' && !window.confirm(`Allow AgentRelay to ${action === 'runTests' ? 'run the project tests' : 'run the project build'} this time?`)) return
    const activeSession = memory?.agentSessions.find((session) => session.status === 'active')
    await mutate(() => requestAction({
      repositoryPath: status.repository,
      sessionId: activeSession?.id ?? null,
      requestedBy: activeSession?.provider ?? 'user',
      action,
      approvalGranted: mode === 'ask',
    }))
    setNotice(`${words(action)} request finished. Its decision and result were recorded in project memory.`)
  }

  async function saveCapabilityProfile(profile: AgentCapabilityProfile) {
    if (!status) return
    await mutate(() => updateCapabilityProfile({
      repositoryPath: status.repository,
      provider: profile.provider,
      displayName: profile.displayName,
      strengths: profile.strengths,
      allowedActions: profile.allowedActions,
      maxContextTokens: profile.maxContextTokens,
      enabled: profile.enabled,
    }))
    setNotice(`${profile.displayName} capability profile updated.`)
  }

  async function createIsolatedWorkspace(task: WorkTask) {
    if (!status) return
    await mutate(() => prepareTaskWorkspace(status.repository, task.id))
    setNotice(`Isolated Git worktree prepared for “${task.title}”.`)
  }

  async function startIsolatedWorkspace(workspace: TaskWorkspace) {
    await mutate(() => startTaskWorkspace(workspace.id))
    setNotice(`Isolated ${workspace.assignedProvider} session started. AgentRelay did not launch an external agent.`)
  }

  async function finishIsolatedWorkspace(workspace: TaskWorkspace) {
    if (!workspace.sessionId) return
    await mutate(() => endAgentSession({
      sessionId: workspace.sessionId!,
      summaryStatus: 'Isolated task work completed; ready for validation.',
      blockers: [],
    }))
    setNotice('Session completed, handoff generated, and workspace queued for validation.')
  }

  async function validateIsolatedWorkspace(workspace: TaskWorkspace) {
    if (!memory) return
    const needsApproval = memory.permissions.runTests === 'ask' || memory.permissions.runBuild === 'ask'
    const approved = !needsApproval || window.confirm('Allow this validation run to execute the configured test/build gates?')
    await mutate(() => validateTaskWorkspace(
      workspace.id,
      memory.permissions.runTests === 'allow' || (memory.permissions.runTests === 'ask' && approved),
      memory.permissions.runBuild === 'allow' || (memory.permissions.runBuild === 'ask' && approved),
      approved,
    ))
    setNotice('Workspace validation recorded with deterministic Git and sandbox evidence.')
  }

  async function verifyIsolatedWorkspace(workspace: TaskWorkspace) {
    if (!memory) return
    const needsApproval = memory.permissions.runTests === 'ask' || memory.permissions.runBuild === 'ask'
    if (needsApproval && !window.confirm('Allow the independent verifier to run the configured project gates this time?')) return
    await mutate(() => runWorkspaceVerification(workspace.id, needsApproval))
    setNotice('Independent gates finished in the recorded environment. Merge readiness is tied to this exact commit.')
  }

  async function waiveVerification(workspace: TaskWorkspace, suite: VerificationSuite, gateId: string) {
    const reason = window.prompt('Document why this failed gate may be waived. The reason is permanent verification evidence.')?.trim()
    if (!reason) return
    await mutate(() => waiveVerificationGate(suite.id, gateId, reason))
    setNotice(`Verification waiver recorded for ${workspace.branch}. Required gates remain auditable.`)
  }

  async function persistEnvironmentManifest(event: FormEvent) {
    event.preventDefault()
    if (!status || !environment) return
    setEnvironmentLoading(true)
    setError(null)
    try {
      const next = await saveEnvironmentManifest(status.repository, environment.manifest)
      setEnvironment(next)
      await refreshMemory(status.repository)
      setNotice('Environment manifest saved to .agentrelay/environment.toml. Commit it before creating verifier worktrees.')
    } catch (reason) {
      setError(errorMessage(reason))
    } finally {
      setEnvironmentLoading(false)
    }
  }

  function updateEnvironmentManifest(patch: Partial<EnvironmentManifest>) {
    setEnvironment((current) => current ? { ...current, manifest: { ...current.manifest, ...patch } } : current)
  }

  async function prepareIsolatedMerge(workspace: TaskWorkspace) {
    await mutate(() => prepareWorkspaceMerge(workspace.id))
    setNotice('Merge candidate recomputed without changing the main checkout.')
  }

  async function approveIsolatedMerge(workspace: TaskWorkspace) {
    if (!window.confirm(`Merge ${workspace.branch} into the currently checked-out project branch?`)) return
    await mutate(() => approveWorkspaceMerge(workspace.id))
    if (status) setStatus(await scanProject(status.repository))
    setNotice('User-approved task branch merged. The isolated worktree is retained until cleanup.')
  }

  async function cleanupIsolatedWorkspace(workspace: TaskWorkspace) {
    await mutate(() => cleanupTaskWorkspace(workspace.id))
    setNotice('Merged worktree removed. Branch metadata, validation, session, and handoff history were preserved.')
  }

  async function importExternalEvent(event: FormEvent) {
    event.preventDefault()
    let payload: Record<string, unknown>
    try {
      const parsed: unknown = JSON.parse(externalPayload)
      if (!parsed || Array.isArray(parsed) || typeof parsed !== 'object') throw new Error('Payload must be a JSON object.')
      payload = parsed as Record<string, unknown>
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Payload must be valid JSON.')
      return
    }
    await mutate(() => ingestExternalEvent({
      repositoryPath: status!.repository,
      integrationId,
      eventType: externalEventType,
      occurredAt: null,
      payload,
    }))
    setNotice(`${integrationId} event normalized and appended through the core ledger.`)
  }

  async function runIntegrationAction(pluginId: string, action: 'runTests' | 'runBuild') {
    if (!status || !memory) return
    const mode = memory.permissions[action]
    if (mode === 'ask' && !window.confirm(`Allow ${pluginId} to request ${words(action)} this time?`)) return
    const activeSession = memory?.agentSessions.find((session) => session.status === 'active')
    await mutate(() => requestPluginAction({
      integrationId: pluginId,
      repositoryPath: status.repository,
      sessionId: activeSession?.id ?? null,
      requestedBy: activeSession?.provider ?? 'user',
      action,
      approvalGranted: mode === 'ask',
    }))
    setNotice(`${pluginId} proposed ${words(action)}; the core policy and sandbox handled the request.`)
  }

  async function synchronizeGithub() {
    if (!status) return
    setLoading(true)
    setError(null)
    try {
      const report = await syncGithubDelivery(status.repository, githubRepository.trim() || null)
      setDelivery(report.snapshot)
      setGithubRepository(`${report.snapshot.repository.owner}/${report.snapshot.repository.name}`)
      await refreshMemory(status.repository)
      setNotice(`GitHub synchronized: ${report.createdTasks} task(s) created, ${report.createdLinks} link(s) added, ${report.blockedTasks} task(s) blocked by CI.`)
    } catch (reason) {
      setError(errorMessage(reason))
    } finally {
      setLoading(false)
    }
  }

  async function runImpactAnalysis(event: FormEvent) {
    event.preventDefault()
    if (!status || !impactTarget.trim()) return
    setImpactLoading(true)
    setError(null)
    try {
      setImpact(await analyzeImpact(status.repository, impactTarget.trim()))
    } catch (reason) {
      setError(errorMessage(reason))
    } finally {
      setImpactLoading(false)
    }
  }

  async function finishOnboarding(path: string, settings: ApplicationSettings) {
    setApplicationSettings(settings)
    setContextDepth(settings.defaultContextDepth)
    setContextProfile(settings.defaultContextProfile)
    await loadStatus(path)
    setShowOnboarding(false)
    setView('repository')
    setHealthRefresh((current) => current + 1)
  }

  const changeCount = status ? status.modifiedFiles.length + status.addedFiles.length + status.deletedFiles.length : 0
  const unresolvedConflicts = memory?.conflicts.filter((conflict) => conflict.status !== 'resolved') ?? []
  const contextFreshness = memory?.freshness.find((item) => item.subjectType === 'context')

  return <main className={view === 'home' ? 'native-assistant-shell' : undefined}>
    <header className="topbar">
      <button className="brand" type="button" onClick={() => setView('home')}><span className="brand-mark"><span /><span /><span /></span><span>Sharman</span><small>AgentRelay inside</small></button>
      <div className="topbar-actions"><button className={`topbar-button ${view === 'home' ? 'active' : ''}`} type="button" onClick={() => setView('home')}>Home</button><button className={`topbar-button ${view !== 'home' && view !== 'settings' ? 'active' : ''}`} type="button" onClick={() => status ? setView('repository') : setShowOnboarding(true)}>Projects</button><div className="connection-state"><span className={desktopRuntime ? 'state-dot' : 'state-dot standby-dot'} />{desktopRuntime ? 'Local core connected' : 'Browser preview'}</div>{desktopRuntime && applicationSettings && <><button className="topbar-button" type="button" onClick={() => setShowOnboarding(true)}>Add project</button>{status && <button className={`topbar-button ${view === 'settings' ? 'active' : ''}`} type="button" onClick={() => setView('settings')}>Settings</button>}</>}</div>
    </header>

    {view !== 'home' && <><section className="hero compact-hero"><p className="eyebrow">AGENTRELAY PROJECT SPECIALIST</p><h1>One project state.<br />Every coding agent.</h1><p className="hero-copy">Git awareness, connected project impact, relevance-ranked context, and verifiable handoffs—now one specialist inside Sharman.</p></section>

    <form className="repository-picker" onSubmit={scan}><label htmlFor="repository-path">Repository path</label><div className="path-control"><input id="repository-path" value={repositoryPath} onChange={(event) => setRepositoryPath(event.target.value)} placeholder="C:\Projects\my-project" spellCheck={false} /><button disabled={loading || !repositoryPath.trim()}><span className={loading ? 'refresh-icon spinning' : 'refresh-icon'}>↻</span>{loading ? 'Working' : 'Connect'}</button></div><p>Optional project workspace · Read-only Git · Local SQLite truth</p></form></>}

    {applicationSettings && showOnboarding && <OnboardingWizard settings={applicationSettings} initialRepository={repositoryPath} canCancel onCancel={() => setShowOnboarding(false)} onFinished={finishOnboarding} />}

    {error && <section className="error-panel" role="alert"><div><p className="section-label">ACTION FAILED</p><h2>Sharman hit a problem</h2><p>{error}</p></div><button type="button" onClick={() => setError(null)}>Dismiss</button></section>}
    {notice && <div className="notice" role="status">{notice}<button onClick={() => setNotice(null)}>×</button></div>}

    {view === 'home' && <AssistantHome
      desktopRuntime={desktopRuntime}
      onAddProject={() => setShowOnboarding(true)}
      onOpenProject={async (path) => { await loadStatus(path); setView('repository') }}
      onError={setError}
      onNotice={setNotice}
    />}

    {view !== 'home' && status && memory ? <>
      <nav className="view-tabs" aria-label="Project views">{(['repository', 'goals', 'plan', 'orchestration', 'execution', 'environment', 'impact', 'integrations', 'pullRequests', 'ci', 'activity', 'timeline', 'audit', 'handoff', 'conflicts', 'context', 'actions', 'settings'] as View[]).map((item) => <button key={item} className={view === item ? 'active' : ''} onClick={() => setView(item)}>{words(item)}<span>{item === 'goals' ? memory.goals.filter((goal) => goal.status === 'active').length : item === 'plan' ? planner?.activePlan?.tasks.length || '' : item === 'orchestration' ? orchestration?.tasks.filter((task) => task.assignmentStatus === 'running').length || '' : item === 'execution' ? execution?.workspaces.filter((workspace) => !['merged', 'archived'].includes(workspace.status)).length || '' : item === 'environment' ? environment?.snapshot.status === 'reproducible' ? '✓' : environment?.snapshot.drift.length || '' : item === 'impact' ? projectGraph?.stats.edges || '' : item === 'integrations' ? registry?.entries.length || '' : item === 'pullRequests' ? delivery?.pullRequests.length || '' : item === 'ci' ? delivery?.blockers.filter((blocker) => blocker.active).length || '' : item === 'activity' || item === 'timeline' ? memory.events.length : item === 'conflicts' ? unresolvedConflicts.length : item === 'actions' ? memory.actionRuns.length : item === 'handoff' && memory.agentSessions.some((session) => session.status === 'active') ? 'live' : ''}</span></button>)}</nav>

      {unresolvedConflicts.length > 0 && <section className="conflict-banner" role="status"><div><p className="section-label">CONTEXT SAFETY CHECK</p><strong>{unresolvedConflicts.length} unresolved {unresolvedConflicts.length === 1 ? 'conflict' : 'conflicts'}</strong><span>{contextFreshness ? `Context is ${contextFreshness.status}: ${contextFreshness.reason}` : 'Review the detected project conflicts before handing work to another agent.'}</span></div><button className="ghost-button" type="button" onClick={() => setView('conflicts')}>Review conflicts</button></section>}

      {view === 'repository' && <section className="dashboard">
        <ProjectHealthDashboard repositoryPath={status.repository} refreshKey={healthRefresh} />
        <div className="overview-card"><div className="overview-topline"><div className="repository-copy"><p className="section-label">ACTIVE REPOSITORY</p><p className="repository">{status.repository}</p></div><div className="button-row"><button className={`tracking-button ${memory.project.trackingEnabled ? 'enabled' : ''}`} onClick={() => void mutate(() => setProjectTracking(status.repository, !memory.project.trackingEnabled))} disabled={loading}><span className="state-dot" />{memory.project.trackingEnabled ? 'Background tracking on' : 'Enable tracking'}</button><button className="refresh-button" onClick={() => void loadStatus(status.repository)} disabled={loading}><span className={loading ? 'refresh-icon spinning' : 'refresh-icon'}>↻</span>Refresh</button></div></div><div className="metric-grid"><div className="metric"><span>Branch</span><strong>{status.branch}</strong></div><div className="metric"><span>Working tree</span><strong>{changeCount} changes</strong></div><div className="metric"><span>Last commit</span><strong>{status.lastCommit}</strong></div></div></div>
        <article className="background-card"><div><p className="section-label">BACKGROUND SERVICE</p><h3>{backgroundStatus?.running ? 'Monitoring is active' : 'Starting background monitor'}</h3><p>{backgroundStatus?.detectedAgents.length ?? 0} supported agent process(es) detected · {backgroundStatus?.automaticSessionCount ?? 0} automatic session(s)</p>{backgroundStatus?.lastError && <small>{backgroundStatus.lastError}</small>}</div><form onSubmit={saveIdleTimeout}><label htmlFor="idle-timeout">Idle timeout</label><div><input id="idle-timeout" type="number" min="1" max="240" value={idleTimeout} onChange={(event) => setIdleTimeout(Number(event.target.value))} /><span>minutes</span><button className="ghost-button" disabled={loading}>Save</button></div></form></article>
        <div className="change-grid"><article className="change-card"><div className="card-heading"><div><span className="legend modified" />Modified</div><b>{status.modifiedFiles.length}</b></div><FileList files={status.modifiedFiles} emptyLabel="No modified files" /></article><article className="change-card"><div className="card-heading"><div><span className="legend added" />Added</div><b>{status.addedFiles.length}</b></div><FileList files={status.addedFiles} emptyLabel="No added files" /></article><article className="change-card"><div className="card-heading"><div><span className="legend deleted" />Deleted</div><b>{status.deletedFiles.length}</b></div><FileList files={status.deletedFiles} emptyLabel="No deleted files" /></article></div>
        <div className="detail-grid"><article className="detail-card"><p className="section-label">DIFF SUMMARY</p>{status.diffStat.length ? <pre>{status.diffStat.join('\n')}</pre> : <p className="empty-state">No unstaged diff.</p>}</article><article className="detail-card"><p className="section-label">RECENT COMMITS</p>{status.recentCommits.length ? <ol className="commit-list">{status.recentCommits.map((commit) => <li key={commit}>{commit}</li>)}</ol> : <p className="empty-state">No commits yet.</p>}</article></div>
      </section>}

      {view === 'goals' && <section className="workspace-view"><div className="view-heading"><div><p className="section-label">GOALS & TASKS</p><h2>Turn project intent into durable work.</h2></div></div><form className="compose-card" onSubmit={addGoal}><h3>Create goal</h3><div className="form-grid"><input value={goalTitle} onChange={(event) => setGoalTitle(event.target.value)} placeholder="Outcome to achieve" required /><input value={goalDescription} onChange={(event) => setGoalDescription(event.target.value)} placeholder="Definition of done" /></div><button className="primary-button" disabled={loading}>Add goal</button></form><div className="goal-stack">{memory.goals.length ? memory.goals.map((goal) => <GoalCard key={goal.id} goal={goal} busy={loading} onChanged={mutate} />) : <div className="empty-panel">No goals yet. Create the first project outcome above.</div>}</div></section>}

      {view === 'plan' && <section className="workspace-view plan-view">
        <div className="view-heading context-heading"><div><p className="section-label">AUTONOMOUS TASK DECOMPOSITION</p><h2>Propose first. Execute only after approval.</h2><p>The planner grounds its DAG in the current project graph, active decisions, freshness state, conflict ledger, and configurable capability profiles.</p></div>{planner?.activePlan && <div className={`plan-version-badge ${planner.activePlan.status}`}><span>ACTIVE PLAN</span><strong>v{planner.activePlan.version}</strong><small>{words(planner.activePlan.status)}</small></div>}</div>

        <form className="plan-generator" onSubmit={createPlan}><label htmlFor="planner-goal"><span>High-level goal</span><select id="planner-goal" value={plannerGoalId} onChange={(event) => setPlannerGoalId(event.target.value)} required><option value="">Select an active goal</option>{memory.goals.filter((goal) => goal.status === 'active').map((goal) => <option value={goal.id} key={goal.id}>{goal.title}</option>)}</select></label><button className="primary-button" disabled={loading || !plannerGoalId}>{planner?.activePlan ? 'Generate new version' : 'Generate task DAG'}</button></form>

        {planner?.activePlan ? <div className="plan-layout"><div className="plan-main">
          <article className="plan-summary"><div><span className={`status-pill ${planner.activePlan.status}`}>{words(planner.activePlan.status)}</span><h3>{memory.goals.find((goal) => goal.id === planner.activePlan?.goalId)?.title || 'Project plan'}</h3><p>{planner.activePlan.reason}</p></div><div className="plan-summary-actions"><span>{planner.activePlan.tasks.length} tasks · {Math.max(...planner.activePlan.tasks.map((task) => task.parallelGroup), 0) + 1} stages</span><button className="why-button" type="button" onClick={() => openAudit('plan', planner.activePlan!.id)}>Why this plan?</button>{planner.activePlan.status === 'valid' && <button className="primary-button" type="button" disabled={loading} onClick={() => void approveActivePlan()}>Approve plan & create tasks</button>}{planner.activePlan.status === 'approved' && <button className="ghost-button" type="button" onClick={() => setView('orchestration')}>Open orchestration →</button>}</div></article>

          {planner.activePlan.validationIssues.length > 0 && <article className="plan-validation"><p className="section-label">PLAN VALIDATION</p>{planner.activePlan.validationIssues.map((issue, index) => <div className={issue.severity} key={`${issue.code}-${index}`}><strong>{issue.severity === 'error' ? 'Blocked' : 'Review'} · {words(issue.code)}</strong><span>{issue.detail}</span></div>)}</article>}

          <div className="plan-dag">{planner.activePlan.tasks.map((task) => <PlannedTaskCard task={task} tasks={planner.activePlan!.tasks} busy={loading} onFail={() => void reportPlannedTaskFailure(task)} key={task.id} />)}</div>
        </div><aside className="plan-sidebar"><div className="coordination-panel"><p className="section-label">VERSION HISTORY</p>{planner.plans.filter((plan) => plan.goalId === planner.activePlan?.goalId).map((plan) => <div className={`plan-history-row ${plan.status}`} key={plan.id}><div><strong>Plan v{plan.version}</strong><span>{words(plan.status)} · {new Date(plan.createdAt).toLocaleString()}</span></div><p>{plan.reason}</p>{plan.changedDependencies.length > 0 && <small>{plan.changedDependencies.length} dependency change(s)</small>}</div>)}</div><div className="coordination-panel"><p className="section-label">CONTROL BOUNDARY</p><strong>User approval is mandatory</strong><p>Generation recommends tasks and agents. Approval creates ordinary AgentRelay tasks. Agent assignment, worktree creation, and merging remain separate approvals.</p></div></aside></div> : <div className="empty-panel">Select an active goal and generate a dependency-aware proposal. No task or agent will start automatically.</div>}
      </section>}

      {view === 'orchestration' && orchestration && <section className="workspace-view">
        <div className="view-heading context-heading"><div><p className="section-label">MULTI-AGENT ORCHESTRATION</p><h2>Recommend, approve, coordinate.</h2><p>Routing is deterministic and profile-driven. No agent is launched and no assignment is approved without you.</p></div>{orchestration.nextRecommendedAgent && <div className="next-agent-card"><span>NEXT RECOMMENDED AGENT</span><strong>{orchestration.nextRecommendedAgent.displayName}</strong><p>{orchestration.nextRecommendedAgent.reasons[0]}</p></div>}</div>
        <div className="orchestration-layout"><div className="goal-orchestration-tree"><p className="section-label">DELIVERY GRAPH</p>{memory.goals.filter((goal) => goal.status === 'active').map((goal) => <section className="orchestration-goal" key={goal.id}><div><span>Goal</span><h3>{goal.title}</h3></div><div className="orchestration-task-list">{goal.tasks.map((memoryTask) => {
          const task = orchestration.tasks.find((candidate) => candidate.id === memoryTask.id) || memoryTask
          const recommendation = orchestration.recommendations.find((candidate) => candidate.taskId === task.id)
          return <OrchestrationTaskCard key={task.id} task={task} allTasks={orchestration.tasks} profiles={orchestration.profiles} recommendation={recommendation} busy={loading}
            onSave={(input) => mutate(() => updateTaskOrchestration(input))}
            onSuggest={() => mutate(() => suggestTaskRoute(task.id))}
            onApprove={(provider) => mutate(() => approveTaskAssignment({ taskId: task.id, provider }))}
            onStart={() => createIsolatedWorkspace(task)}
            onClear={() => mutate(() => clearTaskAssignment(task.id))}
            onWhy={() => openAudit('task', task.id)} />
        })}</div></section>)}</div>
        <aside className="coordination-sidebar"><div className="coordination-panel"><p className="section-label">ACTIVE SOFT LOCKS</p>{orchestration.locks.filter((lock) => lock.status === 'active').length ? orchestration.locks.filter((lock) => lock.status === 'active').map((lock) => <div className="lock-row" key={lock.id}><code>{lock.pathPattern}</code><span>{lock.provider} · {orchestration.tasks.find((task) => task.id === lock.taskId)?.title}</span></div>) : <p className="empty-state">No workspace paths are claimed.</p>}</div>
        <div className="coordination-panel"><p className="section-label">PARALLEL SAFETY</p>{orchestration.parallelAssessments.length ? orchestration.parallelAssessments.slice(0, 12).map((assessment) => <div className={`parallel-row ${assessment.safe ? 'safe' : 'blocked'}`} key={`${assessment.leftTaskId}-${assessment.rightTaskId}`}><strong>{assessment.safe ? 'Can run together' : 'Keep sequential'}</strong><span>{orchestration.tasks.find((task) => task.id === assessment.leftTaskId)?.title} ↔ {orchestration.tasks.find((task) => task.id === assessment.rightTaskId)?.title}</span><p>{assessment.reasons[0]}</p>{!assessment.safe && <button className="why-button" type="button" onClick={() => openAudit('taskPair', [assessment.leftTaskId, assessment.rightTaskId].sort().join(':'))}>Why?</button>}</div>) : <p className="empty-state">Add at least two active tasks to compare them.</p>}</div></aside></div>
        <details className="profile-editor"><summary>Agent capability profiles</summary><div className="profile-grid">{orchestration.profiles.map((profile) => <CapabilityProfileCard key={profile.id} profile={profile} busy={loading} onSave={saveCapabilityProfile} />)}</div></details>
      </section>}

      {view === 'execution' && orchestration && execution && <section className="workspace-view execution-view">
        <div className="view-heading context-heading"><div><p className="section-label">COLLABORATIVE MULTI-AGENT EXECUTION</p><h2>One task. One branch. One isolated workspace.</h2><p>AgentRelay rejects unsafe parallel plans before creating a worktree, validates committed results, and changes the main checkout only after explicit merge approval.</p></div><div className="isolation-badge"><span>ISOLATION POLICY</span><strong>Git worktrees</strong><small>No shared mutable task directory</small></div></div>

        <div className="execution-metrics">
          <article><span>Live workspaces</span><strong>{execution.workspaces.filter((workspace) => !['merged', 'archived'].includes(workspace.status)).length}</strong></article>
          <article><span>Active agents</span><strong>{execution.workspaces.filter((workspace) => workspace.status === 'active').length}</strong></article>
          <article><span>Merge ready</span><strong>{execution.workspaces.filter((workspace) => workspace.status === 'mergeReady').length}</strong></article>
          <article><span>Archived</span><strong>{execution.workspaces.filter((workspace) => workspace.status === 'archived').length}</strong></article>
        </div>

        {orchestration.tasks.some((task) => task.assignmentStatus === 'assigned' && !execution.workspaces.some((workspace) => workspace.taskId === task.id && !['merged', 'archived'].includes(workspace.status))) && <article className="workspace-queue"><div><p className="section-label">APPROVED TASKS</p><h3>Ready for isolated workspace allocation</h3></div><div>{orchestration.tasks.filter((task) => task.assignmentStatus === 'assigned' && !execution.workspaces.some((workspace) => workspace.taskId === task.id && !['merged', 'archived'].includes(workspace.status))).map((task) => <button className="workspace-queue-item" disabled={loading} onClick={() => void createIsolatedWorkspace(task)} key={task.id}><span><strong>{task.title}</strong><small>{task.assignedProvider} · {task.claimedPaths.length ? task.claimedPaths.join(', ') : 'no path claim'}</small></span><b>Prepare worktree →</b></button>)}</div></article>}

        <div className="workspace-execution-grid">{execution.workspaces.length ? execution.workspaces.map((workspace) => <WorkspaceCard
          workspace={workspace}
          task={orchestration.tasks.find((task) => task.id === workspace.taskId)}
          validation={execution.validations.find((validation) => validation.id === workspace.latestValidationId)}
          verification={verification?.suites.find((suite) => suite.workspaceId === workspace.id)}
          busy={loading}
          onStart={() => startIsolatedWorkspace(workspace)}
          onEnd={() => finishIsolatedWorkspace(workspace)}
          onValidate={() => validateIsolatedWorkspace(workspace)}
          onVerify={() => verifyIsolatedWorkspace(workspace)}
          onWaive={(gateId) => {
            const suite = verification?.suites.find((candidate) => candidate.workspaceId === workspace.id)
            return suite ? waiveVerification(workspace, suite, gateId) : Promise.resolve()
          }}
          onPrepareMerge={() => prepareIsolatedMerge(workspace)}
          onApproveMerge={() => approveIsolatedMerge(workspace)}
          onCleanup={() => cleanupIsolatedWorkspace(workspace)}
          onWhy={openAudit}
          key={workspace.id}
        />) : <div className="empty-panel">Approve a routed task, then prepare its isolated worktree. Parallel agents never share the project checkout.</div>}</div>

        <article className="execution-safety"><p className="section-label">PARALLEL SAFETY MATRIX</p><div>{execution.parallelAssessments.map((assessment) => <div className={assessment.safe ? 'safe' : 'blocked'} key={`${assessment.leftTaskId}-${assessment.rightTaskId}`}><strong>{assessment.safe ? 'Parallel-safe' : 'Serialize'}</strong><span>{orchestration.tasks.find((task) => task.id === assessment.leftTaskId)?.title} ↔ {orchestration.tasks.find((task) => task.id === assessment.rightTaskId)?.title}</span><small>{assessment.reasons.join(' ')}</small></div>)}</div></article>
      </section>}

      {view === 'environment' && <section className="workspace-view environment-view">
        <div className="view-heading context-heading"><div><p className="section-label">REPRODUCIBLE EXECUTION</p><h2>Verify the commit, not the machine.</h2><p>The manifest fixes tool expectations and argv. Fingerprints cover toolchains, lockfiles, manifests, OS, and configured-variable presence—never secret values.</p></div>{environment && <div className={`environment-status-badge ${environment.snapshot.status}`}><span>ENVIRONMENT</span><strong>{words(environment.snapshot.status)}</strong><small>{environment.snapshot.fingerprint.slice(0, 12)}</small></div>}</div>

        {environmentLoading && !environment ? <div className="empty-panel">Inspecting runtimes, tools, lockfiles, and environment references…</div> : environment && <>
          <div className="environment-metrics"><article><span>Profile</span><strong>{words(environment.snapshot.profile)}</strong></article><article><span>Commit</span><strong>{environment.snapshot.commit.slice(0, 10)}</strong></article><article><span>Tool checks</span><strong>{environment.snapshot.tools.filter((tool) => tool.status === 'matched' || tool.status === 'detected').length}/{environment.snapshot.tools.length}</strong></article><article><span>Drift findings</span><strong>{environment.snapshot.drift.length}</strong></article></div>

          <div className="environment-layout"><form className="environment-manifest" onSubmit={persistEnvironmentManifest}>
            <div className="environment-panel-heading"><div><p className="section-label">ENVIRONMENT MANIFEST</p><h3>.agentrelay/environment.toml</h3></div><button className="primary-button" disabled={environmentLoading}>Save manifest</button></div>
            <div className="form-grid"><label>Execution profile<select value={environment.manifest.profile} onChange={(event) => updateEnvironmentManifest({ profile: event.target.value as EnvironmentManifest['profile'] })}><option value="nativeWindows">Native Windows</option><option value="sandboxedLocal">Sandboxed local</option><option value="container">Container</option></select></label><label>Working directory<input value={environment.manifest.workingDirectory} onChange={(event) => updateEnvironmentManifest({ workingDirectory: event.target.value })} placeholder="." /></label></div>
            {environment.manifest.profile === 'container' && <label>Container image<input value={environment.manifest.containerImage || ''} onChange={(event) => updateEnvironmentManifest({ containerImage: event.target.value || null })} placeholder="rust:1.98-bookworm" required /></label>}
            <label className="check-line"><input type="checkbox" checked={environment.manifest.disposableVerification} onChange={(event) => updateEnvironmentManifest({ disposableVerification: event.target.checked })} />Use a disposable verification cache and destroy it after gates finish</label>
            <div className="manifest-section"><h4>Required runtimes</h4>{Object.entries(environment.manifest.runtimes).map(([name, version]) => <label key={name}>{name}<input value={version} onChange={(event) => updateEnvironmentManifest({ runtimes: { ...environment.manifest.runtimes, [name]: event.target.value } })} /></label>)}</div>
            <div className="manifest-section"><h4>Fixed verification commands</h4>{Object.entries(environment.manifest.commands).map(([name, command]) => <div className="command-row" key={name}><span>{words(name)}</span><code>{command.join(' ')}</code></div>)}</div>
            <p className="manifest-note">Commands are stored as argv arrays and never passed through a shell. Secret requirements store names and configured state only.</p>
          </form>

          <div className="environment-evidence">
            <article className="environment-panel"><p className="section-label">TOOLCHAIN OBSERVATIONS</p>{environment.snapshot.tools.length ? <ul className="tool-observation-list">{environment.snapshot.tools.map((tool) => <li className={tool.status} key={tool.name}><span>{tool.status === 'matched' || tool.status === 'detected' ? '✓' : '!'}</span><div><strong>{tool.name}</strong><small>Expected {tool.expected || 'available'} · detected {tool.detected || 'missing'}</small></div></li>)}</ul> : <p className="empty-state">No runtime or system-tool requirement is configured.</p>}</article>
            <article className="environment-panel"><p className="section-label">VARIABLE REFERENCES</p>{environment.snapshot.environment.length ? <ul className="secret-reference-list">{environment.snapshot.environment.map((variable) => <li key={variable.name}><div><strong>{variable.name}</strong><small>{variable.secret ? 'secret reference' : 'public configuration'}{variable.required ? ' · required' : ' · optional'}</small></div><span className={variable.configured ? 'configured' : 'missing'}>{variable.configured ? 'configured' : 'not configured'}</span></li>)}</ul> : <p className="empty-state">No environment-variable references are required.</p>}</article>
            {environment.snapshot.drift.length > 0 && <article className="environment-panel drift-panel"><p className="section-label">ENVIRONMENT DRIFT</p>{environment.snapshot.drift.map((drift) => <div key={`${drift.subject}-${drift.expected}`}><strong>{drift.subject}</strong><span>{drift.expected} → {drift.detected}</span><p>{drift.detail}</p></div>)}</article>}
            <article className="environment-panel"><p className="section-label">FINGERPRINT + CACHE PROVENANCE</p><code className="fingerprint-value">{environment.snapshot.fingerprint}</code><p>{environment.snapshot.os} · {environment.snapshot.architecture} · {environment.snapshot.worktree}</p>{environment.caches.length ? environment.caches.map((cache) => <div className={`cache-row ${cache.status}`} key={cache.id}><div><strong>{cache.kind}</strong><small>{cache.path}</small></div><span>{cache.status}</span></div>) : <p className="empty-state">Caches are allocated when independent verification runs.</p>}</article>
            <article className="environment-panel"><p className="section-label">LAST VERIFIED</p>{environment.latestVerification ? <><div className={`last-verification ${environment.latestVerification.status}`}><strong>{words(environment.latestVerification.status)}</strong><span>Commit {environment.latestVerification.commit.slice(0, 12)}</span></div><p>{environment.latestVerification.gates.filter((gate) => gate.status === 'passed').length} passed · {environment.latestVerification.gates.filter((gate) => gate.status === 'waived').length} waived · {environment.latestVerification.gates.filter((gate) => gate.status === 'failed').length} failed</p><button className="why-button" type="button" onClick={() => openAudit('verificationSuite', environment.latestVerification!.id)}>Why?</button></> : <p className="empty-state">No workspace has independent verification evidence yet.</p>}</article>
          </div></div>
        </>}
      </section>}

      {view === 'impact' && <section className="workspace-view impact-view">
        <div className="view-heading context-heading">
          <div><p className="section-label">PROJECT GRAPH + IMPACT ANALYSIS</p><h2>Know what could break before you edit.</h2><p>Relationships and risk are inferred deterministically from Git, source imports, tasks, tests, decisions, sessions, conflicts, and action history.</p></div>
          {impact && <div className={`risk-badge ${impact.risk.level}`}><span>CHANGE RISK</span><strong>{impact.risk.score}</strong><small>{impact.risk.level}</small><button className="why-button" type="button" onClick={() => openAudit('file', impact.target.id)}>Why?</button></div>}
        </div>

        {projectGraph && <div className="graph-metrics">
          {([['Modules', projectGraph.stats.modules], ['Files', projectGraph.stats.files], ['Tests', projectGraph.stats.tests], ['Tasks', projectGraph.stats.tasks], ['Sessions', projectGraph.stats.sessions], ['Edges', projectGraph.stats.edges]] as const).map(([label, value]) => <article key={label}><span>{label}</span><strong>{value}</strong></article>)}
        </div>}

        <form className="impact-search" onSubmit={runImpactAnalysis}>
          <label htmlFor="impact-target"><span>File to assess</span><input id="impact-target" list="impact-targets" value={impactTarget} onChange={(event) => setImpactTarget(event.target.value)} placeholder="src/storage.rs" spellCheck={false} required /></label>
          <datalist id="impact-targets">{projectGraph?.nodes.filter((node) => (node.nodeType === 'file' || node.nodeType === 'test') && node.path).map((node) => <option value={node.path!} key={node.id}>{node.label}</option>)}</datalist>
          <button className="primary-button" disabled={impactLoading || !impactTarget.trim()}>{impactLoading ? 'Tracing impact…' : 'Analyze before edit'}</button>
        </form>

        {impactLoading && !impact && <div className="empty-panel">Building the repository graph and tracing dependents…</div>}
        {impact && <>
          <article className={`impact-warning ${impact.risk.level}`}><div><span>{words(impact.risk.level)} risk · {impact.target.nodeType}</span><h3>{impact.target.path || impact.target.label}</h3><p>{impact.warning}</p></div><strong>{impact.risk.score}<small>/100</small></strong></article>

          <div className="impact-layout">
            <div className="impact-results">
              <article className="impact-panel"><div className="impact-panel-heading"><span>01</span><h3>Affected modules</h3><b>{impact.affectedModules.length}</b></div><ImpactNodeList nodes={impact.affectedModules} emptyLabel="No module boundary was affected." /></article>
              <article className="impact-panel"><div className="impact-panel-heading"><span>02</span><h3>Dependent files</h3><b>{impact.dependentFiles.length}</b></div><ImpactNodeList nodes={impact.dependentFiles} emptyLabel="No static dependents were found." /></article>
              <article className="impact-panel"><div className="impact-panel-heading"><span>03</span><h3>Active work</h3><b>{impact.affectedTasks.length}</b></div><ImpactNodeList nodes={impact.affectedTasks} emptyLabel="No active task touches this area." /></article>
              <article className="impact-panel"><div className="impact-panel-heading"><span>04</span><h3>Tests to run</h3><b>{impact.validatingTests.length}</b></div><ImpactNodeList nodes={impact.validatingTests} emptyLabel="No mapped test: use the project test suite." />{impact.testsToRun.length > 0 && <div className="tests-to-run">{impact.testsToRun.map((test) => <code key={test}>{test}</code>)}</div>}</article>
              <article className="impact-panel"><div className="impact-panel-heading"><span>05</span><h3>Relevant decisions</h3><b>{impact.relevantDecisions.length}</b></div><ImpactNodeList nodes={impact.relevantDecisions} emptyLabel="No active architectural decision maps here." /></article>
              <article className="impact-panel"><div className="impact-panel-heading"><span>06</span><h3>Related sessions</h3><b>{impact.relatedSessions.length}</b></div><ImpactNodeList nodes={impact.relatedSessions} emptyLabel="No agent session is touching related code." /></article>
            </div>

            <aside className="impact-sidebar">
              <article className="risk-factors"><p className="section-label">DETERMINISTIC RISK SCORE</p>{impact.risk.factors.map((factor) => <div key={factor.key}><span><strong>{factor.label}</strong><small>{factor.detail}</small></span><b>+{factor.points}</b></div>)}</article>
              <article className="impact-conflicts"><p className="section-label">ACTIVE CONFLICTS</p>{impact.activeConflicts.length ? impact.activeConflicts.map((conflict) => <div key={conflict.id}><strong>{conflict.title}</strong><span>{conflict.description}</span></div>) : <p className="empty-state">No active coordination conflict overlaps this impact cone.</p>}</article>
            </aside>
          </div>

          <details className="impact-evidence"><summary>Why AgentRelay connected these items · {impact.evidenceEdges.length} evidence edges</summary><div>{impact.evidenceEdges.map((edge, index) => {
            const source = projectGraph?.nodes.find((node) => node.id === edge.source)?.label || edge.source
            const target = projectGraph?.nodes.find((node) => node.id === edge.target)?.label || edge.target
            return <p key={`${edge.source}-${edge.target}-${edge.edgeType}-${index}`}><strong>{source}</strong><span>{words(edge.edgeType)}</span><strong>{target}</strong><small>{edge.evidence}</small></p>
          })}</div></details>
        </>}
        {projectGraph?.warnings.map((warning) => <p className="graph-warning" key={warning}>{warning}</p>)}
      </section>}

      {view === 'integrations' && registry && <section className="workspace-view integrations-workspace">
        <div className="view-heading context-heading"><div><p className="section-label">PROVIDER / PLUGIN SDK</p><h2>Extend the edge. Protect the core.</h2><p>Manifests declare detection, capabilities, events, and action proposals. Plugins never receive SQLite access or bypass project permissions.</p></div><div className="sdk-badge"><span>SDK API</span><strong>v{registry.sdkApiVersion}</strong><small>{registry.entries.length} registered</small></div></div>
        <article className="github-sync-card">
          <div><p className="section-label">GITHUB DELIVERY BRIDGE</p><h3>{delivery ? `${delivery.repository.owner}/${delivery.repository.name}` : 'Connect issues, pull requests, reviews, and CI'}</h3><p>Read-only GitHub data is normalized by the adapter. Only AgentRelay Core can create tasks, links, blockers, or ledger events.</p>{delivery && <small>Last synchronized {new Date(delivery.syncedAt).toLocaleString()} · {delivery.issues.length} issues · {delivery.pullRequests.length} pull requests</small>}</div>
          <div className="github-sync-controls"><label>Repository override<input value={githubRepository} onChange={(event) => setGithubRepository(event.target.value)} placeholder="owner/repository · optional when origin is GitHub" spellCheck={false} /></label><button className="primary-button" disabled={loading} onClick={() => void synchronizeGithub()}>{loading ? 'Synchronizing…' : 'Sync GitHub'}</button></div>
        </article>
        <article className="plugin-directory"><div><span>DROP-IN MANIFEST DIRECTORY</span><code>{registry.pluginDirectory || 'Built-in registry only'}</code></div><p>Add a validated <code>.toml</code> manifest and restart AgentRelay. Duplicate IDs and incompatible SDK versions are rejected.</p></article>
        {registry.loadErrors.length > 0 && <div className="registry-errors"><strong>Manifest load errors</strong>{registry.loadErrors.map((message) => <code key={message}>{message}</code>)}</div>}
        <div className="registry-groups">{([
          ['provider', 'Providers'],
          ['developerTool', 'Developer tools'],
          ['automation', 'Automation'],
        ] as const).map(([kind, label]) => <section className="registry-group" key={kind}><div className="registry-group-heading"><p className="section-label">{label}</p><span>{registry.entries.filter((entry) => entry.manifest.kind === kind).length}</span></div><div className="registry-grid">{registry.entries.filter((entry) => entry.manifest.kind === kind).map(({ manifest, origin }) => <article className="plugin-card" key={manifest.id}><div className="plugin-heading"><div><span>{manifest.id}</span><h3>{manifest.name}</h3></div><em>{origin === 'builtIn' ? 'built in' : 'directory'}</em></div><p>{manifest.description || 'No description supplied.'}</p><div className="plugin-meta"><code>v{manifest.version}</code><code>{manifest.permissions.readRepo ? 'read repo' : 'no repo access'}</code><code>{manifest.permissions.writeRepo ? 'write requested' : 'no direct writes'}</code></div>{manifest.capabilities.strengths.length > 0 && <div className="plugin-tags">{manifest.capabilities.strengths.map((capability) => <span key={capability}>{words(capability)}</span>)}</div>}{manifest.detection.rules.length > 0 && <small>{manifest.detection.rules.length} process detection rule(s)</small>}{manifest.adapters.eventTypes.length > 0 && <small>{manifest.adapters.eventTypes.length === 1 && manifest.adapters.eventTypes[0] === '*' ? 'Accepts declared provider events' : `${manifest.adapters.eventTypes.length} normalized event type(s)`}</small>}{manifest.adapters.actions.length > 0 && <div className="plugin-actions">{manifest.adapters.actions.map((action) => action === 'runTests' || action === 'runBuild' ? <button className="ghost-button" disabled={loading} key={action} onClick={() => void runIntegrationAction(manifest.id, action)}>{words(action)}</button> : <code key={action}>{words(action)}</code>)}</div>}</article>)}</div></section>)}</div>
        <form className="compose-card external-event-form" onSubmit={importExternalEvent}><div><p className="section-label">NORMALIZATION TEST BENCH</p><h3>Ingest an external event</h3><p>The selected adapter validates the event type, wraps its source identity, and returns a normalized proposal. Only the core appends it to the immutable project ledger.</p></div><div className="form-grid"><label>Integration<select value={integrationId} onChange={(event) => { setIntegrationId(event.target.value); const manifest = registry.entries.find((entry) => entry.manifest.id === event.target.value)?.manifest; const first = manifest?.adapters.eventTypes.find((item) => item !== '*'); if (first) setExternalEventType(first) }}>{registry.entries.filter((entry) => entry.manifest.adapters.eventTypes.length > 0).map((entry) => <option value={entry.manifest.id} key={entry.manifest.id}>{entry.manifest.name}</option>)}</select></label><label>Event type<input value={externalEventType} onChange={(event) => setExternalEventType(event.target.value)} placeholder="issue.updated" required /></label></div><label>Raw provider payload<textarea className="json-input" value={externalPayload} onChange={(event) => setExternalPayload(event.target.value)} spellCheck={false} required /></label><button className="primary-button" disabled={loading}>Normalize and record</button></form>
      </section>}

      {view === 'pullRequests' && <section className="workspace-view delivery-view">
        <div className="view-heading context-heading"><div><p className="section-label">PULL REQUEST DELIVERY STATE</p><h2>Local agent work, connected to delivery.</h2><p>Tasks and sessions are linked bidirectionally to GitHub records. Reviews become durable work instead of disappearing into a comment thread.</p></div>{delivery && <button className="primary-button" disabled={loading} onClick={() => void synchronizeGithub()}>Sync now</button>}</div>
        {!delivery && <div className="empty-panel">No GitHub delivery snapshot yet. Open Integrations and synchronize a repository.</div>}
        {delivery && <>
          <div className="delivery-metrics"><article><span>Open</span><strong>{delivery.pullRequests.filter((pullRequest) => pullRequest.status === 'open').length}</strong></article><article><span>Draft</span><strong>{delivery.pullRequests.filter((pullRequest) => pullRequest.status === 'draft').length}</strong></article><article><span>Merged</span><strong>{delivery.pullRequests.filter((pullRequest) => pullRequest.status === 'merged').length}</strong></article><article><span>Review tasks</span><strong>{delivery.links.filter((link) => link.externalEntityType === 'reviewComment').length}</strong></article><article><span>Active blockers</span><strong>{delivery.blockers.filter((blocker) => blocker.active).length}</strong></article></div>
          <div className="pull-request-stack">{delivery.pullRequests.length ? delivery.pullRequests.map((pullRequest) => {
            const prLinks = delivery.links.filter((link) => link.externalEntityType === 'pullRequest' && link.externalId === String(pullRequest.number))
            const taskIds = new Set(prLinks.filter((link) => link.localEntityType === 'task').map((link) => link.localEntityId))
            const sessionIds = new Set(prLinks.filter((link) => link.localEntityType === 'session').map((link) => link.localEntityId))
            const tasks = memory.goals.flatMap((goal) => goal.tasks).filter((task) => taskIds.has(task.id))
            const sessions = memory.agentSessions.filter((session) => sessionIds.has(session.id))
            const decisions = memory.decisions.filter((decision) => decision.status === 'active' && decision.affected.some((reference) => pullRequest.files.some((file) => file.toLowerCase().includes(reference.toLowerCase()) || reference.toLowerCase().includes(file.toLowerCase()))))
            const blockers = delivery.blockers.filter((blocker) => blocker.pullRequestNumber === pullRequest.number && blocker.active)
            return <article className={`pull-request-card ${pullRequest.status}`} key={pullRequest.id}>
              <div className="pr-heading"><div><span>PR #{pullRequest.number} · {pullRequest.status}</span><h3>{pullRequest.title}</h3><p>{pullRequest.branch} → {pullRequest.baseBranch} · {pullRequest.author}</p></div><a href={pullRequest.url} target="_blank" rel="noreferrer">Open GitHub ↗</a></div>
              <div className="pr-summary"><div><span>Agent sessions</span><strong>{sessions.length}</strong><small>{sessions.map((session) => session.agentName).join(' · ') || 'No linked sessions'}</small></div><div><span>Tasks</span><strong>{tasks.length}</strong><small>{tasks.filter((task) => task.status === 'completed').length} completed · {tasks.filter((task) => task.assignmentStatus === 'blocked').length} blocked</small></div><div><span>CI</span><strong>{pullRequest.ciChecks.length}</strong><small>{pullRequest.ciChecks.filter((check) => check.conclusion === 'success').length} passed · {pullRequest.ciChecks.filter((check) => ['failure', 'timedOut', 'actionRequired'].includes(check.conclusion)).length} failed</small></div><div><span>Review</span><strong>{pullRequest.reviewComments.length}</strong><small>{pullRequest.reviewers.join(' · ') || 'No reviewers'}</small></div></div>
              <div className="pr-detail-grid"><section><p className="section-label">CI CHECKS</p>{pullRequest.ciChecks.length ? pullRequest.ciChecks.map((check) => <div className={`ci-check-row ${check.conclusion}`} key={check.id}><span>{check.conclusion === 'success' ? '✓' : check.conclusion === 'pending' ? '…' : '×'}</span><div><strong>{check.name}</strong><small>{words(check.category)} · {words(check.status)}</small></div></div>) : <p className="empty-state">No check runs found.</p>}</section><section><p className="section-label">LINKED TASKS + AGENTS</p>{tasks.length ? tasks.map((task) => <div className="delivery-link-row" key={task.id}><strong>{task.title}</strong><span>{words(task.assignmentStatus)}{task.assignedProvider ? ` · ${task.assignedProvider}` : ''}</span></div>) : <p className="empty-state">No issue, review, or path-linked tasks.</p>}{sessions.map((session) => <div className="delivery-link-row agent" key={session.id}><strong>{session.agentName}</strong><span>{session.provider} · {session.status}</span></div>)}</section><section><p className="section-label">RELEVANT DECISIONS</p>{decisions.length ? decisions.map((decision) => <div className="delivery-link-row" key={decision.id}><strong>{decision.summary}</strong><span>{decision.createdBy}</span></div>) : <p className="empty-state">No active decision references changed files.</p>}</section></div>
              {blockers.length > 0 && <div className="pr-blocker-strip"><strong>Delivery blocked</strong>{blockers.map((blocker) => <span key={blocker.id}>{blocker.summary}</span>)}</div>}
            </article>
          }) : <div className="empty-panel">GitHub returned no pull requests.</div>}</div>
        </>}
      </section>}

      {view === 'ci' && <section className="workspace-view delivery-view">
        <div className="view-heading context-heading"><div><p className="section-label">CI + AUTOMATIC BLOCKER HANDLING</p><h2>Failures become coordinated work.</h2><p>Check runs are normalized into build, test, lint, and security events. A failing check blocks every linked active task and becomes part of the next handoff.</p></div>{delivery && <button className="primary-button" disabled={loading} onClick={() => void synchronizeGithub()}>Refresh CI</button>}</div>
        {!delivery && <div className="empty-panel">Synchronize GitHub from Integrations to load CI state.</div>}
        {delivery && <div className="ci-layout"><div className="ci-pr-list">{delivery.pullRequests.map((pullRequest) => <article className="ci-pr-card" key={pullRequest.id}><div><span>PR #{pullRequest.number}</span><h3>{pullRequest.title}</h3><code>{pullRequest.headRevision.slice(0, 12)}</code></div><div className="ci-check-stack">{pullRequest.ciChecks.length ? pullRequest.ciChecks.map((check) => <div className={`ci-check-row ${check.conclusion}`} key={check.id}><span>{check.conclusion === 'success' ? '✓' : ['failure', 'timedOut', 'actionRequired'].includes(check.conclusion) ? '×' : '…'}</span><div><strong>{check.name}</strong><small>{words(check.category)} · {words(check.conclusion)}</small></div>{check.detailsUrl && <a href={check.detailsUrl} target="_blank" rel="noreferrer">Details</a>}</div>) : <p className="empty-state">No GitHub check runs for this revision.</p>}</div></article>)}</div><aside className="ci-sidebar"><article><p className="section-label">ACTIVE BLOCKERS</p>{delivery.blockers.filter((blocker) => blocker.active).length ? delivery.blockers.filter((blocker) => blocker.active).map((blocker) => { const task = memory.goals.flatMap((goal) => goal.tasks).find((candidate) => candidate.id === blocker.taskId); return <div className="ci-blocker" key={blocker.id}><span>PR #{blocker.pullRequestNumber}</span><strong>{task?.title || 'Linked task'}</strong><p>{blocker.summary}</p></div> }) : <p className="empty-state">No task is blocked by current CI.</p>}</article><article><p className="section-label">NORMALIZED DELIVERY EVENTS</p>{memory.events.filter((event) => ['buildStarted', 'buildPassed', 'buildFailed', 'testFailed', 'lintFailed', 'securityCheckFailed', 'prReady', 'prMerged'].includes(event.kind)).slice(0, 18).map((event) => <div className="delivery-event" key={event.id}><strong>{words(event.kind)}</strong><span>{new Date(event.createdAt).toLocaleString()}</span></div>)}</article></aside></div>}
      </section>}

      {view === 'activity' && <section className="workspace-view"><div className="view-heading"><div><p className="section-label">PROJECT LEDGER</p><h2>Decisions and immutable activity.</h2></div></div><div className="activity-layout"><div><form className="compose-card" onSubmit={addDecision}><h3>Record decision</h3><input value={decisionSummary} onChange={(event) => setDecisionSummary(event.target.value)} placeholder="Decision summary" required /><textarea value={decisionReason} onChange={(event) => setDecisionReason(event.target.value)} placeholder="Why was this chosen?" required /><div className="form-grid"><input value={alternatives} onChange={(event) => setAlternatives(event.target.value)} placeholder="Alternatives, comma separated" /><input value={affected} onChange={(event) => setAffected(event.target.value)} placeholder="Affected files/components" /></div><input value={decisionAuthor} onChange={(event) => setDecisionAuthor(event.target.value)} placeholder="Created by" required /><select value={supersedesDecisionId} onChange={(event) => setSupersedesDecisionId(event.target.value)}><option value="">Does not supersede an earlier decision</option>{memory.decisions.filter((decision) => decision.status === 'active').map((decision) => <option key={decision.id} value={decision.id}>Supersedes: {decision.summary}</option>)}</select><button className="primary-button" disabled={loading}>Record decision</button></form><div className="decision-stack">{memory.decisions.map((decision) => <article className={`decision-card ${decision.status}`} key={decision.id}><span>{decision.createdBy} · {decision.status}</span><h3>{decision.summary}</h3><p>{decision.reason}</p>{decision.affected.length > 0 && <small>Affects {decision.affected.join(', ')}</small>}<button className="why-button" type="button" onClick={() => openAudit('decision', decision.id)}>Why?</button></article>)}</div></div><div className="timeline">{memory.events.map((event) => <article className="event-row" key={event.id}><span className="event-dot" /><div><strong>{words(event.kind)}</strong><p>{event.source} · {new Date(event.createdAt).toLocaleString()}</p><code>{JSON.stringify(event.payload)}</code></div></article>)}</div></div></section>}

      {view === 'timeline' && <TimelineView repositoryPath={status.repository} />}

      {view === 'audit' && <AuditView repositoryPath={status.repository} focus={auditFocus} onClearFocus={() => setAuditFocus(null)} />}

      {view === 'handoff' && <section className="workspace-view">
        <div className="view-heading context-heading"><div><p className="section-label">AGENT SESSION RELAY</p><h2>Leave the next agent a clean handoff.</h2><p>Session baselines and deltas come from read-only Git observation.</p></div><div className="button-row"><button className="ghost-button" onClick={() => void copyContext()}>Copy Context</button><button className="ghost-button" onClick={() => status && void openContextFile(status.repository).catch((reason) => setError(errorMessage(reason)))}>Open AGENT_CONTEXT.md</button><button className="primary-button" disabled={loading} onClick={() => void regenerateContext()}>Regenerate</button></div></div>
        {(() => {
          const active = memory.agentSessions.find((session) => session.status === 'active')
          if (active) return <div className="session-layout"><article className="active-session-card"><div className="live-label"><span className="state-dot" />{active.automatic ? 'Automatically detected' : 'Session active'}</div><h3>{active.agentName}</h3><p>{active.provider}{active.detectedPid ? ` · PID ${active.detectedPid}` : ''} · started {new Date(active.startedAt).toLocaleString()}</p><dl><div><dt>Branch</dt><dd>{active.initialGitState?.branch || 'unknown'}</dd></div><div><dt>Starting HEAD</dt><dd>{active.initialGitHead ? active.initialGitHead.slice(0, 12) : 'no commits'}</dd></div><div><dt>Working directory</dt><dd>{active.workingDirectory}</dd></div></dl><button className="ghost-button" disabled={loading} onClick={() => void mutate(() => markSessionIdle(active.id))}>Mark idle</button></article><form className="compose-card end-session-form" onSubmit={endSession}><h3>End session and build handoff</h3><textarea value={sessionSummary} onChange={(event) => setSessionSummary(event.target.value)} placeholder="What was accomplished?" /><textarea value={sessionBlockers} onChange={(event) => setSessionBlockers(event.target.value)} placeholder="Blockers, one per line" /><button className="primary-button" disabled={loading}>End Session</button></form></div>
          return <form className="compose-card start-session-form" onSubmit={startSession}><h3>Start new agent session</h3><p>AgentRelay captures the current branch, HEAD, and working tree as the baseline.</p><div className="form-grid"><select value={sessionProvider} onChange={(event) => { setSessionProvider(event.target.value); const name = registry?.entries.find((entry) => entry.manifest.id === event.target.value)?.manifest.name; if (name) setAgentName(name) }}>{registry?.entries.filter((entry) => entry.manifest.kind === 'provider' && entry.manifest.enabled).map((entry) => <option value={entry.manifest.id} key={entry.manifest.id}>{entry.manifest.name}</option>)}<option value="unknown">Other / unknown</option></select><input value={agentName} onChange={(event) => setAgentName(event.target.value)} placeholder="Agent name" required /></div><button className="primary-button" disabled={loading}>Start Agent Session</button></form>
        })()}
        {memory.latestHandoff ? <article className="handoff-card"><div className="handoff-title"><div><p className="section-label">LATEST HANDOFF</p><h3>{memory.latestHandoff.previousAgent || 'Unknown agent'} → next agent</h3></div><span>{memory.latestHandoff.branch}</span></div><div className="handoff-metrics"><div><span>Duration</span><strong>{Math.max(0, Math.round((new Date(memory.latestHandoff.endedAt).getTime() - new Date(memory.latestHandoff.startedAt).getTime()) / 60000))} min</strong></div><div><span>Files changed</span><strong>{memory.latestHandoff.changedFiles.length}</strong></div><div><span>Commits</span><strong>{memory.latestHandoff.commits.length}</strong></div></div><div className="handoff-grid"><div><h4>Files changed</h4>{memory.latestHandoff.changedFiles.length ? <ul>{memory.latestHandoff.changedFiles.map((file) => <li key={`${file.changeType}-${file.path}`}><b>{file.changeType[0].toUpperCase()}</b>{file.path}</li>)}</ul> : <p>No net file changes.</p>}</div><div><h4>Tasks completed</h4>{memory.latestHandoff.completedTasks.length ? <ul>{memory.latestHandoff.completedTasks.map((task) => <li key={task.id}>{task.title}</li>)}</ul> : <p>No tasks completed in this session.</p>}<h4>Tasks remaining</h4><ul>{memory.latestHandoff.activeTasks.map((task) => <li key={task.id}>{task.title}</li>)}</ul></div><div><h4>Decisions made</h4>{memory.latestHandoff.recentDecisions.length ? <ul>{memory.latestHandoff.recentDecisions.map((decision) => <li key={decision.id}>{decision.summary}</li>)}</ul> : <p>No decisions recorded.</p>}<h4>Blockers</h4>{memory.latestHandoff.blockers.length ? <ul>{memory.latestHandoff.blockers.map((blocker) => <li key={blocker.description}>{blocker.description}</li>)}</ul> : <p>No blockers.</p>}</div></div><div className="next-actions"><span>Suggested next step</span><strong>{memory.latestHandoff.recommendedNextActions[0]}</strong>{memory.latestHandoff.nextRecommendedProvider && <em>Route to {words(memory.latestHandoff.nextRecommendedProvider)} · {memory.latestHandoff.routingReasons[0]}</em>}</div></article> : <div className="empty-panel">No agent sessions yet. Start Claude, Codex, or another configured agent inside this repository and AgentRelay will detect it automatically.</div>}
        {memory.agentSessions.length > 0 && <div className="session-history"><p className="section-label">SESSION HISTORY</p>{memory.agentSessions.map((session) => <div key={session.id}><strong>{session.agentName}</strong><span>{session.provider} · {session.status} · {new Date(session.startedAt).toLocaleString()}</span></div>)}</div>}
      </section>}

      {view === 'conflicts' && <section className="workspace-view">
        <div className="view-heading context-heading"><div><p className="section-label">CONFLICT & FRESHNESS ENGINE</p><h2>Know when shared context stops being safe.</h2><p>Rules are evaluated from Git, sessions, handoffs, tasks, and explicit decision relationships—never from model guesses.</p></div><button className="primary-button" disabled={loading} onClick={() => void mutate(() => evaluateConflicts(status.repository))}>Re-evaluate</button></div>
        <div className="freshness-strip">{memory.freshness.filter((item) => item.subjectType === 'context' || item.status !== 'current').map((item) => <article key={`${item.subjectType}-${item.subjectId}`}><span>{item.subjectType}</span><strong className={`freshness ${item.status}`}>{item.status}</strong><p>{item.reason}</p></article>)}</div>
        <div className="conflict-stack">{memory.conflicts.length ? memory.conflicts.map((conflict) => <article className={`conflict-card ${conflict.severity} ${conflict.status}`} key={conflict.id}><div className="conflict-card-heading"><div><span className="conflict-kind">{words(conflict.kind)}</span><h3>{conflict.title}</h3></div><span className={`status-pill ${conflict.status}`}>{conflict.status}</span></div><p>{conflict.description}</p>{conflict.affectedPaths.length > 0 && <ul>{conflict.affectedPaths.map((path) => <li key={path}>{path}</li>)}</ul>}<div className="button-row"><button className="why-button" type="button" onClick={() => openAudit('conflict', conflict.id)}>Why?</button>{conflict.status === 'open' && <button className="ghost-button" disabled={loading} onClick={() => void mutate(() => acknowledgeConflict(conflict.id))}>Acknowledge</button>}{conflict.status !== 'resolved' && <button className="ghost-button" disabled={loading} onClick={() => void mutate(() => resolveConflict(conflict.id))}>Resolve</button>}</div></article>) : <div className="empty-panel">No conflicts detected. Project context is internally consistent.</div>}</div>
      </section>}

      {view === 'context' && <section className="workspace-view">
        <div className="view-heading context-heading"><div><p className="section-label">SMART CONTEXT COMPILER</p><h2>Only what the next agent needs.</h2><p>Freshness and relevance are deterministic. Nemotron can compress selected facts, but every summary bullet must cite a source.</p></div><div className="button-row"><button className="ghost-button" onClick={() => void copyContext()}>Copy</button><button className="primary-button" disabled={loading} onClick={() => void regenerateContext()}>Generate file</button></div></div>
        <div className="context-controls">
          <label>Budget<select value={contextDepth} onChange={(event) => setContextDepth(event.target.value as ContextDepth)}><option value="compact">Compact · 2k</option><option value="normal">Normal · 6k</option><option value="deep">Deep · 12k</option></select></label>
          <label>Profile<select value={contextProfile} onChange={(event) => setContextProfile(event.target.value as ContextProfile | 'auto')}><option value="auto">Auto from agent</option><option value="balanced">Balanced</option><option value="architecture">Architecture</option><option value="implementation">Implementation</option><option value="product">Product</option></select></label>
          <label>Focus task<select value={contextTaskId} onChange={(event) => setContextTaskId(event.target.value)}><option value="">Automatic active task</option>{memory.goals.flatMap((goal) => goal.tasks).filter((task) => task.status === 'todo' || task.status === 'inProgress').map((task) => <option value={task.id} key={task.id}>{task.title}</option>)}</select></label>
          <button className="ghost-button" disabled={loading} onClick={() => void previewContext()}>Recompute preview</button>
        </div>
        {contextPackage && <>
          <div className="context-metrics"><article><span>Profile</span><strong>{contextPackage.snapshot.request.profile}</strong></article><article><span>Estimated</span><strong>{contextPackage.snapshot.estimatedTokens.toLocaleString()} / {contextPackage.snapshot.request.tokenBudget.toLocaleString()}</strong></article><article><span>Selected</span><strong>{contextPackage.snapshot.items.length} items</strong></article><article><span>Omitted</span><strong>{contextPackage.snapshot.omittedItemCount}</strong></article></div>
          <article className={`compression-card ${contextPackage.snapshot.compression.mode}`}><div><span>{words(contextPackage.snapshot.compression.mode)}</span><strong>{contextPackage.snapshot.compression.model || 'Deterministic compiler'}</strong></div><p>{contextPackage.snapshot.compression.detail}</p></article>
          {contextPackage.snapshot.summary && <article className="summary-card"><p className="section-label">VALIDATED NEMOTRON SUMMARY</p><ul>{contextPackage.snapshot.summary.bullets.map((bullet, index) => <li key={`${bullet.text}-${index}`}><span>{bullet.text}</span><small>{bullet.sourceKeys.join(' · ')}</small></li>)}</ul></article>}
          <div className="context-item-stack">{contextPackage.snapshot.items.map((item) => {
            const expanded = expandedContextItemId === item.id
            return <article className={`context-item ${item.tier}`} key={item.id}><button type="button" className="context-item-heading" onClick={() => setExpandedContextItemId(expanded ? null : item.id)} aria-expanded={expanded}><span className={`tier-pill ${item.tier}`}>{item.tier}</span><div><strong>{item.title}</strong><small>{words(item.kind)} · score {item.relevanceScore} · ~{item.estimatedTokens} tokens</small></div><b>{expanded ? '−' : '?'}</b></button><p>{item.content}</p><div className="source-row">{item.sources.map((source) => <code key={source.key}>{source.label}</code>)}</div>{expanded && <div className="why-panel"><span>Included because</span><ul>{item.inclusionReasons.map((reason) => <li key={reason}>{reason}</li>)}</ul>{item.recordedAt && <small>Recorded {new Date(item.recordedAt).toLocaleString()}</small>}<button className="why-button" type="button" onClick={() => openAudit('contextItem', item.id)}>Open audit evidence</button></div>}</article>
          })}</div>
        </>}
        <details className="markdown-details"><summary>Preview generated Markdown</summary><pre className="context-preview">{context}</pre></details>
      </section>}

      {view === 'settings' && applicationSettings && <SettingsView
        settings={applicationSettings}
        repositoryPath={status.repository}
        memory={memory}
        background={backgroundStatus}
        registry={registry}
        environment={environment}
        delivery={delivery}
        verification={verification}
        sandbox={sandboxStatus}
        onSaveSettings={saveApplicationSettings}
        onSavePermissions={savePermissionModes}
        onOpenOnboarding={() => setShowOnboarding(true)}
        onNotice={setNotice}
        onError={setError}
      />}

      {view === 'actions' && <section className="workspace-view">
        <div className="view-heading context-heading"><div><p className="section-label">CONTROLLED AGENT ACTIONS</p><h2>Permission first. Sandbox always.</h2><p>Agents request named capabilities. AgentRelay selects a fixed command, enforces project policy, and records the result.</p></div><div className={`sandbox-badge ${sandboxStatus?.configured ? 'configured' : ''}`}><span>{sandboxStatus?.configured ? 'READY' : 'NOT CONFIGURED'}</span><strong>{sandboxStatus?.name || 'Token Factory Sandbox'}</strong></div></div>
        <article className="sandbox-status"><div><p className="section-label">EXECUTION BOUNDARY</p><h3>{sandboxStatus?.configured ? 'Remote sandbox configured' : 'Remote execution is safely unavailable'}</h3><p>{sandboxStatus?.detail || 'Reading sandbox configuration…'}</p></div><code>No local shell fallback</code></article>
        <div className="button-row permission-audit-link"><button className="why-button" type="button" onClick={() => openAudit('projectPermissions', memory.project.id)}>Why these permissions?</button></div><div className="permission-grid">
          {([['Read repository', 'Core capability'], ['Read Git', 'Core capability'], ['Write AgentRelay memory', 'Core capability'], ['Write context files', 'Core capability']] as const).map(([label, detail]) => <article className="permission-card enabled" key={label}><div><strong>{label}</strong><span>{detail}</span></div><span className="permission-state">✓</span></article>)}
          <article className={`permission-card ${memory.permissions.runTests !== 'deny' ? 'enabled' : ''}`}><div><strong>Run tests</strong><span>Per-project policy</span></div><PermissionModeControl value={memory.permissions.runTests} disabled={loading} onChange={(mode) => void setExecutionPermission('runTests', mode)} /></article>
          <article className={`permission-card ${memory.permissions.runBuild !== 'deny' ? 'enabled' : ''}`}><div><strong>Run build</strong><span>Per-project policy</span></div><PermissionModeControl value={memory.permissions.runBuild} disabled={loading} onChange={(mode) => void setExecutionPermission('runBuild', mode)} /></article>
          {['Modify source', 'Git commit', 'Git push', 'Delete files'].map((label) => <article className="permission-card" key={label}><div><strong>{label}</strong><span>Denied by core policy</span></div><span className="permission-state">—</span></article>)}
        </div>
        <div className="action-launcher"><div><p className="section-label">REQUEST AN ACTION</p><h3>Commands are derived from trusted project manifests.</h3><p>For this Rust workspace, AgentRelay resolves tests to <code>cargo test --workspace</code> and builds to <code>cargo build --workspace</code>.</p></div><div className="button-row"><button className="ghost-button" disabled={loading} onClick={() => void runControlledAction('runTests')}>Run tests</button><button className="primary-button" disabled={loading} onClick={() => void runControlledAction('runBuild')}>Run build</button></div></div>
        <div className="action-history"><p className="section-label">ACTION HISTORY</p>{memory.actionRuns.length ? memory.actionRuns.map((run) => <article className={`action-run ${run.status}`} key={run.id}><div className="action-run-heading"><div><span>{words(run.action)}</span><strong>{run.command.length ? run.command.join(' ') : 'No command selected'}</strong></div><div className="action-audit-controls"><span className={`status-pill ${run.status}`}>{run.status}</span><button className="why-button" type="button" onClick={() => openAudit('actionRun', run.id)}>Why?</button></div></div><p>{run.sandbox || 'Policy gate'} · requested by {run.requestedBy} · {new Date(run.requestedAt).toLocaleString()}</p>{run.exitCode !== null && <small>Exit code {run.exitCode}</small>}{run.error && <pre>{run.error}</pre>}{run.stdout && <details><summary>Standard output</summary><pre>{run.stdout}</pre></details>}{run.stderr && <details><summary>Standard error</summary><pre>{run.stderr}</pre></details>}</article>) : <div className="empty-panel">No action requests yet. Enable a permission before requesting tests or a build.</div>}</div>
      </section>}
    </> : view !== 'home' && !error && <section className="ready-panel"><div className="ready-mark">AR</div><div><p className="section-label">OPTIONAL PROJECT SPECIALIST</p><h2>Connect a repository when you need AgentRelay.</h2><p>Sharman works without project setup. Adding a repository enables durable task progress, coding-agent memory, and verified handoffs.</p></div></section>}

    <footer><span>Sharman · Rust · Tauri 2 · Local-first</span><span>{status ? `AgentRelay updated ${new Date(status.scannedAt).toLocaleTimeString()}` : 'Assistant first · projects optional'}</span></footer>
  </main>
}
