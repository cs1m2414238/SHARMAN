import { invoke } from '@tauri-apps/api/core'

export type ProjectStatus = {
  repository: string
  branch: string
  modifiedFiles: string[]
  addedFiles: string[]
  deletedFiles: string[]
  lastCommit: string
  headCommit: string
  recentCommits: string[]
  diffStat: string[]
  scannedAt: string
}

export type GoalStatus = 'active' | 'completed' | 'archived'
export type TaskStatus = 'todo' | 'inProgress' | 'completed' | 'archived'
export type TaskPriority = 'low' | 'medium' | 'high' | 'critical'

export type WorkTask = {
  id: string
  goalId: string
  title: string
  description: string
  status: TaskStatus
  priority: TaskPriority
  assignedAgent: string | null
  sourceAgent: string | null
  createdAt: string
  completedAt: string | null
  assignmentStatus: TaskAssignmentStatus
  suggestedProvider: AgentProvider | null
  assignedProvider: AgentProvider | null
  routeReasons: string[]
  requiredCapabilities: Capability[]
  blockedBy: string[]
  dependsOn: string[]
  produces: string[]
  consumes: string[]
  claimedPaths: string[]
}

export type Goal = {
  id: string
  projectId: string
  title: string
  description: string
  status: GoalStatus
  createdAt: string
  updatedAt: string
  tasks: WorkTask[]
}

export type Decision = {
  id: string
  projectId: string
  summary: string
  reason: string
  alternatives: string[]
  affected: string[]
  status: 'active' | 'superseded' | 'reversed'
  createdBy: string
  supersededBy: string | null
  createdAt: string
}

export type ProjectEvent = {
  id: string
  projectId: string
  sessionId: string | null
  kind: string
  source: string
  payload: Record<string, unknown>
  createdAt: string
}

export type TimelineCategory = 'git' | 'goal' | 'task' | 'decision' | 'session' | 'context' | 'plan' | 'verification' | 'environment' | 'delivery' | 'conflict' | 'workspace' | 'other'

export type TimelineEntry = {
  id: string
  eventId: string
  sessionId: string | null
  timestamp: string
  category: TimelineCategory
  title: string
  summary: string
  source: string
  commit: string | null
  branch: string | null
}

export type TimelineSnapshot = {
  projectId: string
  repository: string
  entries: TimelineEntry[]
  firstTimestamp: string | null
  lastTimestamp: string | null
}

export type ExplanationCategory = 'planner' | 'assignment' | 'parallelSafety' | 'risk' | 'context' | 'conflict' | 'verification' | 'permission' | 'replan' | 'mergeReadiness' | 'freshness' | 'decision'

export type ExplanationRecord = {
  id: string
  projectId: string
  category: ExplanationCategory
  subjectType: string
  subjectId: string
  decision: string
  outcome: string
  reasons: Array<{ code: string; summary: string; detail: string; score: number | null }>
  evidenceRefs: Array<{ kind: string; id: string; label: string }>
  createdAt: string
}

export type ContextVersion = {
  id: string
  projectId: string
  sessionId: string | null
  taskId: string | null
  contextSnapshotHash: string
  markdown: string
  snapshot: ContextPackage['snapshot'] | null
  commit: string
  branch: string
  planId: string | null
  planVersion: number | null
  environmentFingerprint: string | null
  createdAt: string
}

export type ReplayState = {
  projectId: string
  repository: string
  point: { eventId: string | null; sessionId: string | null; timestamp: string; label: string }
  accuracy: 'exact' | 'legacyBestEffort'
  git: { commit: string; branch: string; worktree: string; dirty: boolean; modifiedFiles: string[]; addedFiles: string[]; deletedFiles: string[]; observedAt: string } | null
  goals: Goal[]
  decisions: Decision[]
  sessions: AgentSession[]
  files: Array<{ path: string; changeType: string; introducedAt: string; lastChangedAt: string; sessionIds: string[]; taskIds: string[] }>
  blockers: Array<{ id: string; kind: string; summary: string; introducedAt: string; resolvedAt: string | null; sessionId: string | null; taskId: string | null }>
  context: ContextVersion | null
  plan: PlanVersion | null
  verification: VerificationSuite | null
  environment: EnvironmentSnapshot | null
  events: ProjectEvent[]
  warnings: string[]
}

export type ReplayComparison = {
  left: ReplayState['point']
  right: ReplayState['point']
  newDecisions: string[]
  supersededDecisions: string[]
  completedTasks: string[]
  changedFiles: string[]
  newBlockers: string[]
  resolvedBlockers: string[]
  contextHashChanged: boolean
  planVersionChanged: boolean
  environmentChanged: boolean
}

export type HistoryExplanation = {
  entityType: string
  entityId: string
  title: string
  summary: string
  introducedAt: string | null
  introducedBy: string | null
  supersededAt: string | null
  evidence: Array<{ eventId: string; timestamp: string; kind: string; explanation: string; sessionId: string | null; taskId: string | null; verificationId: string | null }>
}

/** Open provider identifier supplied by the manifest registry. */
export type AgentProvider = string
export type Capability = 'architecture' | 'backend' | 'frontend' | 'refactoring' | 'research' | 'testing' | 'documentation' | 'rust' | 'typeScript' | 'security' | 'devOps'
export type TaskAssignmentStatus = 'unassigned' | 'suggested' | 'assigned' | 'running' | 'waiting' | 'blocked' | 'completed'

export type AgentSession = {
  id: string
  projectId: string
  provider: AgentProvider
  agentName: string
  status: 'active' | 'completed' | 'interrupted'
  workingDirectory: string
  initialGitHead: string
  finalGitHead: string | null
  initialGitState: ProjectStatus | null
  finalGitState: ProjectStatus | null
  startedAt: string
  endedAt: string | null
  summaryStatus: string
  detectedPid: number | null
  automatic: boolean
  taskId: string | null
}

export type HandoffSnapshot = {
  id: string
  projectId: string
  sessionId: string
  previousAgent: string | null
  branch: string
  baseCommit: string
  finalCommit: string
  changedFiles: Array<{ path: string; changeType: 'modified' | 'created' | 'deleted' | 'renamed' }>
  commits: string[]
  completedTasks: Array<{ id: string; title: string; status: TaskStatus; priority: TaskPriority; assignedAgent: string | null }>
  activeTasks: Array<{ id: string; title: string; status: TaskStatus; priority: TaskPriority; assignedAgent: string | null }>
  recentDecisions: Array<{ id: string; summary: string; reason: string; status: string; createdBy: string }>
  blockers: Array<{ description: string }>
  recommendedNextActions: string[]
  nextRecommendedProvider: AgentProvider | null
  routingReasons: string[]
  startedAt: string
  endedAt: string
  createdAt: string
}

export type ConflictKind = 'fileOverlap' | 'decisionContradiction' | 'staleContext' | 'completedTaskDrift'
export type ConflictStatus = 'open' | 'acknowledged' | 'resolved'
export type FreshnessStatus = 'current' | 'stale' | 'superseded' | 'invalidated'

export type ConflictRecord = {
  id: string
  projectId: string
  fingerprint: string
  primarySessionId: string | null
  relatedSessionId: string | null
  kind: ConflictKind
  severity: 'info' | 'warning' | 'critical'
  title: string
  description: string
  affectedPaths: string[]
  relatedDecisionIds: string[]
  status: ConflictStatus
  detectedAt: string
  updatedAt: string
  resolvedAt: string | null
}

export type FreshnessRecord = {
  projectId: string
  subjectType: string
  subjectId: string
  status: FreshnessStatus
  reason: string
  supersededBy: string | null
  evaluatedAt: string
}

export type ProjectMemory = {
  project: {
    id: string
    path: string
    name: string
    branch: string
    lastCommit: string
    changeCount: number
    lastScannedAt: string
    trackingEnabled: boolean
    latestStatus: ProjectStatus | null
  }
  goals: Goal[]
  decisions: Decision[]
  events: ProjectEvent[]
  agentSessions: AgentSession[]
  lastAgentSession: AgentSession | null
  latestHandoff: HandoffSnapshot | null
  handoffs: HandoffSnapshot[]
  conflicts: ConflictRecord[]
  freshness: FreshnessRecord[]
  permissions: ProjectPermissions
  actionRuns: ActionRun[]
}

export type ProjectRecord = ProjectMemory['project']

export type AssistantProjectSummary = {
  id: string
  name: string
  path: string
  branch: string
  progressPercent: number
  completedTasks: number
  totalTasks: number
  activeTasks: number
  blockers: number
  activeAgents: number
  trackingEnabled: boolean
  lastScannedAt: string
}

export type AssistantDeviceProfile = {
  totalMemoryGb: number
  logicalCores: number
  cpu: string
  operatingSystem: string
  architecture: string
  tier: string
  recommendedModelSize: string
  recommendation: string
  recommendedModelName: string
  recommendedDownloadGb: number
  localRuntimeInstalled: boolean
  localModelInstalled: boolean
  localModelName: string | null
}

export type AssistantCapability = {
  id: string
  name: string
  status: 'ready' | 'ask' | 'optional' | 'notConnected' | 'setupRequired'
  detail: string
}

export type AssistantHomeSnapshot = {
  projects: AssistantProjectSummary[]
  device: AssistantDeviceProfile
  capabilities: AssistantCapability[]
}

export type AssistantFileResult = {
  name: string
  path: string
  kind: string
  modifiedAt: string
  sizeBytes: number
}

export type AssistantAction = {
  id: string
  label: string
  kind: 'openProject' | 'openPath' | 'openUrl' | 'openTrustedUrl' | 'playYoutube' | 'setupLocalAi'
  target: string
  requiresConfirmation: boolean
}

export type AssistantResponse = {
  conversationId: string
  intent: string
  title: string
  answer: string
  projects: AssistantProjectSummary[]
  files: AssistantFileResult[]
  actions: AssistantAction[]
  suggestions: string[]
  localOnly: boolean
  planSteps: AssistantPlanStep[]
  personalTasks: PersonalTask[]
  reminders: ScheduledReminder[]
  emailActions: EmailAction[]
  emailMessages: GmailMessageSummary[]
}

export type AssistantPlanStep = {
  tool: string
  summary: string
  permissionLevel: number
  status: string
}

export type PersonalTask = {
  id: string
  title: string
  status: string
  priority: string
  dueAt: string | null
  source: string
  createdAt: string
  updatedAt: string
}

export type ScheduledReminder = {
  id: string
  taskId: string | null
  title: string
  remindAt: string
  status: string
  createdAt: string
  firedAt: string | null
}

export type PersonalDashboard = {
  tasks: PersonalTask[]
  reminders: ScheduledReminder[]
  generatedAt: string
}

export type GmailSearchMode = 'important' | 'needsAction' | 'internship'

export type GmailMessageSummary = {
  id: string
  threadId: string
  subject: string
  sender: string
  snippet: string
  receivedAt: string
}

export type EmailAction = {
  id: string
  messageId: string
  threadId: string
  actionType: 'assessment' | 'interview' | 'application' | 'deadline' | 'followUp' | 'review'
  title: string
  deadline: string | null
  company: string | null
  confidence: number
  evidence: string
  sender: string
  receivedAt: string
  state: string
  reminderCreated: boolean
}

export type GmailConnectionStatus = {
  connected: boolean
  accountEmail: string | null
  lastSyncAt: string | null
  sync: {
    accountId: string | null
    lastHistoryId: string | null
    lastSuccessfulSync: string | null
    lastError: string | null
  }
  credentialStorage: string
  setupHint: string
}

export type InboxDashboard = {
  status: GmailConnectionStatus
  needsAction: EmailAction[]
  upcomingDeadlines: EmailAction[]
  dismissed: EmailAction[]
  generatedAt: string
  scan: {
    mode: GmailSearchMode
    scanned: number
    matched: number
  } | null
}

export type AssistantAuditRecord = {
  id: number
  actor: string
  intent: string
  tool: string
  permission: string
  outcome: string
  summary: string
  occurredAt: string
}

export type AssistantTurn = {
  id: number
  request: string
  response: AssistantResponse
  createdAt: string
}

export type AssistantConversation = {
  id: string
  title: string
  updatedAt: string
  turns: AssistantTurn[]
}

export type AssistantConversationSummary = {
  id: string
  title: string
  updatedAt: string
}

export type AssistantMemoryItem = {
  id: number
  content: string
  createdAt: string
}

export type AssistantMemorySnapshot = {
  active: AssistantConversation
  conversations: AssistantConversationSummary[]
  memories: AssistantMemoryItem[]
}

export type ActionKind = 'runTests' | 'runBuild' | 'modifySource' | 'gitCommit' | 'gitPush' | 'deleteFiles'
export type ActionStatus = 'requested' | 'approvalRequired' | 'denied' | 'running' | 'succeeded' | 'failed' | 'unavailable'
export type PermissionMode = 'allow' | 'ask' | 'deny'

export type ProjectPermissions = {
  projectId: string
  readRepository: boolean
  readGit: boolean
  writeMemory: boolean
  writeContextFiles: boolean
  runTests: PermissionMode
  runBuild: PermissionMode
  modifySource: boolean
  gitCommit: boolean
  gitPush: boolean
  deleteFiles: boolean
  updatedAt: string
}

export type ActionRun = {
  id: string
  projectId: string
  sessionId: string | null
  requestedBy: string
  action: ActionKind
  status: ActionStatus
  command: string[]
  sandbox: string | null
  exitCode: number | null
  stdout: string
  stderr: string
  error: string | null
  requestedAt: string
  startedAt: string | null
  finishedAt: string | null
}

export type SandboxStatus = {
  configured: boolean
  name: string | null
  detail: string
}

export type AgentCapabilityProfile = {
  id: string
  projectId: string
  provider: AgentProvider
  displayName: string
  strengths: Capability[]
  allowedActions: ActionKind[]
  maxContextTokens: number
  enabled: boolean
  updatedAt: string
}

export type TaskRouteRecommendation = {
  taskId: string
  provider: AgentProvider
  displayName: string
  score: number
  reasons: string[]
  requiredCapabilities: Capability[]
}

export type WorkspaceLock = {
  id: string
  projectId: string
  taskId: string
  sessionId: string | null
  provider: AgentProvider
  pathPattern: string
  status: 'active' | 'released'
  acquiredAt: string
  releasedAt: string | null
}

export type ParallelAssessment = {
  leftTaskId: string
  rightTaskId: string
  safe: boolean
  reasons: string[]
}

export type OrchestrationSnapshot = {
  tasks: WorkTask[]
  profiles: AgentCapabilityProfile[]
  locks: WorkspaceLock[]
  recommendations: TaskRouteRecommendation[]
  parallelAssessments: ParallelAssessment[]
  nextRecommendedAgent: TaskRouteRecommendation | null
}

export type PlannedTaskKind = 'architecture' | 'implementation' | 'persistence' | 'testing' | 'documentation' | 'migrationConfig' | 'recovery'
export type PlannedTaskStatus = 'ready' | 'blocked' | 'materialized' | 'completed' | 'failed'
export type PlanStatus = 'valid' | 'invalid' | 'approved' | 'superseded'

export type PlannedTask = {
  id: string
  title: string
  description: string
  kind: PlannedTaskKind
  status: PlannedTaskStatus
  priority: TaskPriority
  requiredCapabilities: Capability[]
  dependsOn: string[]
  produces: string[]
  consumes: string[]
  affectedModules: string[]
  expectedFiles: string[]
  conflictRisk: RiskLevel
  riskScore: number
  riskReasons: string[]
  canRunInParallel: boolean
  parallelGroup: number
  suggestedProvider: AgentProvider | null
  suggestionReasons: string[]
  materializedTaskId: string | null
  recoveryFor: string | null
}

export type PlanVersion = {
  id: string
  projectId: string
  goalId: string
  version: number
  status: PlanStatus
  reason: string
  parentPlanId: string | null
  tasks: PlannedTask[]
  validationIssues: Array<{ code: string; severity: 'warning' | 'error'; taskIds: string[]; detail: string }>
  changedTaskIds: string[]
  changedDependencies: string[]
  createdAt: string
  approvedAt: string | null
}

export type PlannerSnapshot = {
  plans: PlanVersion[]
  activePlan: PlanVersion | null
}

export type PlanApprovalResult = {
  plan: PlanVersion
  tasks: WorkTask[]
}

export type TaskWorkspaceStatus = 'ready' | 'active' | 'validationPending' | 'validationPassed' | 'validationFailed' | 'mergeReady' | 'mergeConflict' | 'merged' | 'archived'

export type TaskWorkspace = {
  id: string
  projectId: string
  taskId: string
  assignedProvider: AgentProvider
  branch: string
  worktreePath: string
  baseCommit: string
  headCommit: string
  status: TaskWorkspaceStatus
  sessionId: string | null
  latestValidationId: string | null
  createdAt: string
  updatedAt: string
  mergedAt: string | null
  archivedAt: string | null
}

export type ValidationCheck = {
  kind: 'cleanWorktree' | 'hasChanges' | 'baseAncestry' | 'parallelSafety' | 'mergeability' | 'tests' | 'build'
  status: 'passed' | 'failed' | 'skipped'
  detail: string
  actionRunId: string | null
}

export type WorkspaceValidation = {
  id: string
  workspaceId: string
  status: 'passed' | 'failed'
  validatedHead: string
  changedFiles: string[]
  checks: ValidationCheck[]
  createdAt: string
}

export type CollaborativeExecutionSnapshot = {
  workspaces: TaskWorkspace[]
  validations: WorkspaceValidation[]
  parallelAssessments: ParallelAssessment[]
}

export type WorkspaceValidationResult = {
  workspace: TaskWorkspace
  validation: WorkspaceValidation
  actionRuns: ActionRun[]
}

export type ExecutionProfile = 'nativeWindows' | 'sandboxedLocal' | 'container'
export type EnvironmentStatus = 'reproducible' | 'environmentDrift' | 'incomplete' | 'unmanaged'
export type VerificationStatus = 'notRun' | 'running' | 'passed' | 'failed' | 'waived'
export type VerificationGateKind = 'environment' | 'format' | 'lint' | 'tests' | 'build' | 'frontendBuild' | 'desktopBuild' | 'architecture' | 'acceptanceCriteria'

export type EnvironmentVariableRequirement = {
  name: string
  required: boolean
  secret: boolean
  value: string | null
}

export type EnvironmentManifest = {
  schemaVersion: number
  profile: ExecutionProfile
  workingDirectory: string
  containerImage: string | null
  disposableVerification: boolean
  runtimes: Record<string, string>
  packageManager: string | null
  systemTools: string[]
  commands: Record<string, string[]>
  environment: EnvironmentVariableRequirement[]
  cacheKinds: string[]
  architectureRules: string[]
}

export type EnvironmentSnapshot = {
  id: string
  projectId: string
  profile: ExecutionProfile
  manifestSource: string
  fingerprint: string
  status: EnvironmentStatus
  tools: Array<{ name: string; expected: string | null; detected: string | null; status: 'matched' | 'drift' | 'missing' | 'detected' }>
  environment: Array<{ name: string; required: boolean; secret: boolean; configured: boolean }>
  fingerprintInputs: Array<{ kind: string; path: string; digest: string }>
  drift: Array<{ subject: string; expected: string; detected: string; detail: string }>
  os: string
  architecture: string
  commit: string
  branch: string
  worktree: string
  capturedAt: string
}

export type EnvironmentCacheRecord = {
  id: string
  projectId: string
  kind: string
  fingerprint: string
  path: string
  status: 'valid' | 'invalidated'
  updatedAt: string
}

export type VerificationGate = {
  id: string
  kind: VerificationGateKind
  name: string
  required: boolean
  status: VerificationStatus
  command: string[]
  exitCode: number | null
  durationMs: number | null
  stdoutSummary: string
  stderrSummary: string
  detail: string
  startedAt: string | null
  finishedAt: string | null
  waiverReason: string | null
}

export type VerificationSuite = {
  id: string
  projectId: string
  taskId: string
  workspaceId: string
  status: VerificationStatus
  environmentId: string
  environmentFingerprint: string
  environmentProfile: ExecutionProfile
  commit: string
  branch: string
  worktree: string
  acceptanceCriteria: string[]
  impact: {
    changedFiles: string[]
    highestRisk: RiskLevel | null
    affectedModules: string[]
    testsToRun: string[]
  }
  gates: VerificationGate[]
  repairTaskId: string | null
  createdAt: string
  completedAt: string | null
}

export type VerificationSnapshot = {
  suites: VerificationSuite[]
  latest: VerificationSuite | null
}

export type EnvironmentDashboard = {
  manifest: EnvironmentManifest
  snapshot: EnvironmentSnapshot
  caches: EnvironmentCacheRecord[]
  latestVerification: VerificationSuite | null
}

export type DetectedAgent = {
  provider: AgentProvider
  pid: number
  executable: string
  workingDirectory: string | null
  commandLine: string[]
}

export type BackgroundStatus = {
  running: boolean
  paused: boolean
  detectedAgents: DetectedAgent[]
  watchedProjects: string[]
  automaticSessionCount: number
  idleTimeoutMinutes: number
  blockedTaskCount: number
  lastError: string | null
}

export type ApplicationSettings = {
  onboardingCompleted: boolean
  selectedAgents: AgentProvider[]
  launchAtStartup: boolean
  closeToTray: boolean
  trackingPaused: boolean
  notificationsEnabled: boolean
  notifyAgentSessions: boolean
  notifyConflicts: boolean
  notifyHandoffs: boolean
  notifyBlockers: boolean
  defaultContextDepth: ContextDepth
  defaultContextProfile: ContextProfile | 'auto'
  backupIntervalHours: number
  diagnosticsIncludePaths: boolean
  advancedLogging: boolean
}

export type RecoveryReport = {
  recoveredAt: string
  databaseIntegrity: string
  migrationBackup: string | null
  interruptedSessions: number
  interruptedActions: number
  interruptedVerifications: number
  recoverableWorkspaces: number
  missingWorkspaces: number
  invalidatedCaches: number
  warnings: string[]
}

export type SystemToolStatus = {
  id: string
  name: string
  required: boolean
  detected: boolean
  version: string | null
  detail: string
}

export type RepositorySetupInspection = {
  repositoryPath: string
  name: string
  gitRepository: boolean
  detectedStack: string[]
  existingContext: boolean
  existingMemory: boolean
  suggestedQualityGates: string[]
  warnings: string[]
}

export type OnboardingInspection = {
  tools: SystemToolStatus[]
  detectedAgents: DetectedAgent[]
  repository: RepositorySetupInspection | null
}

export type BackupRecord = {
  path: string
  createdAt: string
  sizeBytes: number
}

export type ProjectOnboardingResult = {
  repositoryPath: string
  importedContextPath: string | null
  generatedContextPath: string
  backup: BackupRecord
}

export type HealthItem = {
  id: string
  label: string
  status: 'healthy' | 'warning' | 'actionRequired'
  value: string
  detail: string
}

export type ProjectHealthSnapshot = {
  repository: string
  score: number
  status: 'healthy' | 'warning' | 'actionRequired'
  items: HealthItem[]
  checkedAt: string
}

export type DiagnosticExportResult = {
  path: string
  sizeBytes: number
  includedFiles: string[]
  exclusions: string[]
}

export type PluginKind = 'provider' | 'developerTool' | 'automation'

export type PluginManifest = {
  id: string
  name: string
  version: string
  kind: PluginKind
  sdkApiVersion: number
  description: string
  enabled: boolean
  capabilities: {
    strengths: Capability[]
    allowedActions: ActionKind[]
    maxContextTokens: number
    codeEdit: boolean
    terminal: boolean
    planning: boolean
    documentation: boolean
  }
  detection: {
    processes: string[]
    commandContains: string[]
    rules: Array<{ process: string; commandContains: string[] }>
  }
  permissions: {
    readRepo: boolean
    writeRepo: boolean
    runCommands: boolean
  }
  adapters: {
    eventTypes: string[]
    actions: ActionKind[]
    handoffFormat: string | null
  }
}

export type RegistrySnapshot = {
  sdkApiVersion: number
  pluginDirectory: string
  entries: Array<{
    manifest: PluginManifest
    origin: 'builtIn' | 'directory'
    source: string
  }>
  loadErrors: string[]
}

export type NodeType = 'goal' | 'task' | 'file' | 'module' | 'test' | 'decision' | 'session'
export type EdgeType = 'contains' | 'touches' | 'belongsTo' | 'dependsOn' | 'validates' | 'affects' | 'modifiedBy' | 'blocks'
export type RiskLevel = 'low' | 'medium' | 'high' | 'critical'

export type GraphNode = {
  id: string
  nodeType: NodeType
  label: string
  path: string | null
  status: string | null
  metadata: Record<string, string>
}

export type GraphEdge = {
  source: string
  target: string
  edgeType: EdgeType
  evidence: string
}

export type ProjectGraph = {
  projectId: string
  repository: string
  nodes: GraphNode[]
  edges: GraphEdge[]
  stats: {
    goals: number
    tasks: number
    files: number
    modules: number
    tests: number
    decisions: number
    sessions: number
    edges: number
  }
  warnings: string[]
}

export type ImpactAnalysis = {
  target: GraphNode
  affectedModules: GraphNode[]
  dependentFiles: GraphNode[]
  affectedTasks: GraphNode[]
  validatingTests: GraphNode[]
  relevantDecisions: GraphNode[]
  relatedSessions: GraphNode[]
  activeConflicts: ConflictRecord[]
  testsToRun: string[]
  evidenceEdges: GraphEdge[]
  warning: string
  risk: {
    score: number
    level: RiskLevel
    factors: Array<{ key: string; label: string; points: number; detail: string }>
  }
}

export type CiCategory = 'build' | 'test' | 'lint' | 'security' | 'other'
export type CiStatus = 'queued' | 'inProgress' | 'completed'
export type CiConclusion = 'pending' | 'success' | 'failure' | 'neutral' | 'cancelled' | 'skipped' | 'timedOut' | 'actionRequired'

export type CiCheck = {
  id: string
  name: string
  category: CiCategory
  status: CiStatus
  conclusion: CiConclusion
  revision: string
  detailsUrl: string | null
  startedAt: string | null
  completedAt: string | null
}

export type ExternalIssue = {
  id: string
  number: number
  title: string
  description: string
  labels: string[]
  assignees: string[]
  state: 'open' | 'closed'
  linkedBranch: string | null
  url: string
  updatedAt: string
}

export type ReviewComment = {
  id: string
  author: string
  body: string
  state: string
  path: string | null
  line: number | null
  url: string
  createdAt: string
}

export type PullRequestState = {
  id: string
  number: number
  title: string
  description: string
  status: 'draft' | 'open' | 'closed' | 'merged'
  mergeState: 'unknown' | 'ready' | 'blocked' | 'behind' | 'unstable' | 'dirty'
  branch: string
  baseBranch: string
  headRevision: string
  author: string
  reviewers: string[]
  files: string[]
  commits: string[]
  linkedIssueNumbers: number[]
  ciChecks: CiCheck[]
  reviewComments: ReviewComment[]
  url: string
  updatedAt: string
}

export type ExternalLink = {
  id: string
  projectId: string
  localEntityType: 'task' | 'session' | 'decision'
  localEntityId: string
  provider: string
  externalEntityType: 'issue' | 'pullRequest' | 'reviewComment' | 'ciCheck'
  externalId: string
  externalUrl: string
  createdAt: string
  updatedAt: string
}

export type DeliveryBlocker = {
  id: string
  taskId: string
  pullRequestNumber: number
  checkId: string
  summary: string
  url: string | null
  active: boolean
  detectedAt: string
  resolvedAt: string | null
}

export type DeliverySnapshot = {
  projectId: string
  provider: string
  repository: { provider: string; owner: string; name: string; webUrl: string }
  issues: ExternalIssue[]
  pullRequests: PullRequestState[]
  links: ExternalLink[]
  blockers: DeliveryBlocker[]
  syncedAt: string
}

export type DeliverySyncReport = {
  snapshot: DeliverySnapshot
  createdTasks: number
  updatedTasks: number
  createdLinks: number
  blockedTasks: number
  recordedEvents: number
}

export type ContextDepth = 'compact' | 'normal' | 'deep'
export type ContextProfile = 'balanced' | 'architecture' | 'implementation' | 'product'
export type ContextTier = 'critical' | 'active' | 'recent' | 'historical' | 'archived'

export type SourceReference = {
  key: string
  kind: 'project' | 'goal' | 'task' | 'decision' | 'conflict' | 'event' | 'session' | 'handoff' | 'gitCommit' | 'file'
  id: string
  label: string
}

export type ContextItem = {
  id: string
  kind: 'repository' | 'goal' | 'task' | 'decision' | 'conflict' | 'file' | 'commit' | 'blocker' | 'session' | 'event'
  title: string
  content: string
  tier: ContextTier
  relevanceScore: number
  freshness: FreshnessStatus | null
  recordedAt: string | null
  sources: SourceReference[]
  inclusionReasons: string[]
  estimatedTokens: number
}

export type ContextPackage = {
  snapshot: {
    projectName: string
    repository: string
    request: {
      projectId: string
      sessionId: string | null
      taskId: string | null
      tokenBudget: number
      profile: ContextProfile
    }
    items: ContextItem[]
    summary: {
      provider: string
      model: string
      bullets: Array<{ text: string; sourceKeys: string[] }>
    } | null
    compression: {
      mode: 'deterministicOnly' | 'nemotronAvailable' | 'nemotronValidated' | 'nemotronFallback'
      model: string | null
      detail: string
    }
    estimatedTokens: number
    omittedItemCount: number
    recommendedNextAction: string
  }
  markdown: string
}

export type ContextCompileInput = {
  repositoryPath: string
  sessionId: string | null
  taskId: string | null
  depth: ContextDepth
  profile: ContextProfile | null
}

export function isDesktopRuntime() {
  return '__TAURI_INTERNALS__' in window
}

export const getAssistantHome = () =>
  invoke<AssistantHomeSnapshot>('get_assistant_home')

export const getAssistantMemory = (conversationId: string | null = null) =>
  invoke<AssistantMemorySnapshot>('get_assistant_memory', { conversationId })

export const createAssistantConversation = () =>
  invoke<AssistantMemorySnapshot>('create_assistant_conversation')

export const forgetAssistantMemory = (memoryId: number | null = null) =>
  invoke<AssistantMemorySnapshot>('forget_assistant_memory', { memoryId })

export const getPersonalDashboard = () =>
  invoke<PersonalDashboard>('get_personal_dashboard')

export const setPersonalTaskCompleted = (taskId: string, completed: boolean) =>
  invoke<PersonalDashboard>('set_personal_task_completed', { taskId, completed })

export const dismissPersonalReminder = (reminderId: string) =>
  invoke<PersonalDashboard>('dismiss_personal_reminder', { reminderId })

export const getInboxDashboard = () =>
  invoke<InboxDashboard>('get_inbox_dashboard')

export const connectGmail = () =>
  invoke<GmailConnectionStatus>('connect_gmail')

export const disconnectGmail = () =>
  invoke<GmailConnectionStatus>('disconnect_gmail')

export const refreshGmailInbox = (mode: GmailSearchMode) =>
  invoke<InboxDashboard>('refresh_gmail_inbox', { mode })

export const dismissEmailAction = (actionId: string) =>
  invoke<InboxDashboard>('dismiss_email_action', { actionId })

export const createEmailReminder = (actionId: string) =>
  invoke<PersonalDashboard>('create_email_reminder', { actionId })

export const openGmailMessage = (threadId: string) =>
  invoke<void>('open_gmail_message', { threadId })

export const getAssistantAudit = () =>
  invoke<AssistantAuditRecord[]>('get_assistant_audit')

export const askSharman = (message: string, conversationId: string | null) =>
  invoke<AssistantResponse>('ask_sharman', { input: { message, conversationId } })

export const executeAssistantAction = (action: AssistantAction) =>
  invoke<void>('execute_assistant_action', { action })

export const installLocalAiRuntime = () =>
  invoke<AssistantDeviceProfile>('install_local_ai_runtime')

export const pullRecommendedLocalModel = () =>
  invoke<AssistantDeviceProfile>('pull_recommended_local_model')

export const scanProject = (repositoryPath: string) =>
  invoke<ProjectStatus>('scan_project', { repositoryPath })

export const getLastRepository = () => invoke<string | null>('get_last_repository')

export const getTrackedProjects = () => invoke<ProjectRecord[]>('get_tracked_projects')

export const getApplicationSettings = () => invoke<ApplicationSettings>('get_application_settings')

export const getStartupRecoveryReport = () =>
  invoke<RecoveryReport | null>('get_startup_recovery_report')

export const updateApplicationSettings = (settings: ApplicationSettings) =>
  invoke<ApplicationSettings>('update_application_settings', { settings })

export const chooseRepository = () => invoke<string | null>('choose_repository')

export const inspectOnboarding = (repositoryPath: string | null = null) =>
  invoke<OnboardingInspection>('inspect_onboarding', { repositoryPath })

export const completeProjectOnboarding = (input: {
  repositoryPath: string
  preserveExistingContext: boolean
  runTests: PermissionMode
  runBuild: PermissionMode
}) => invoke<ProjectOnboardingResult>('complete_project_onboarding', { input })

export const getProjectHealth = (repositoryPath: string) =>
  invoke<ProjectHealthSnapshot>('get_project_health', { repositoryPath })

export const listBackups = () => invoke<BackupRecord[]>('list_backups')

export const createBackup = () => invoke<BackupRecord>('create_backup')

export const exportDiagnostics = (repositoryPath: string | null) =>
  invoke<DiagnosticExportResult>('export_diagnostics', { repositoryPath })

export const getProjectMemory = (repositoryPath: string) =>
  invoke<ProjectMemory>('get_project_memory', { repositoryPath })

export const getAuditTrail = (repositoryPath: string) =>
  invoke<ExplanationRecord[]>('get_audit_trail', { repositoryPath })

export const getTimeline = (repositoryPath: string) =>
  invoke<TimelineSnapshot>('get_timeline', { repositoryPath })

export const replayProject = (repositoryPath: string, eventId: string) =>
  invoke<ReplayState>('replay_project', { input: { repositoryPath, eventId, sessionId: null, timestamp: null } })

export const compareReplayPoints = (repositoryPath: string, leftEventId: string, rightEventId: string) =>
  invoke<ReplayComparison>('compare_replay_points', { input: { repositoryPath, leftEventId, rightEventId } })

export const explainHistory = (repositoryPath: string, entityType: string, entityId: string, atEventId: string | null = null) =>
  invoke<HistoryExplanation>('explain_history', { input: { repositoryPath, entityType, entityId, atEventId } })

export const getProjectPermissions = (repositoryPath: string) =>
  invoke<ProjectPermissions>('get_project_permissions', { repositoryPath })

export const updateProjectPermissions = (input: {
  repositoryPath: string
  runTests: PermissionMode
  runBuild: PermissionMode
}) => invoke<ProjectPermissions>('update_project_permissions', { input })

export const getSandboxStatus = () => invoke<SandboxStatus>('get_sandbox_status')

export const getIntegrationRegistry = () =>
  invoke<RegistrySnapshot>('get_integration_registry')

export const getProjectGraph = (repositoryPath: string) =>
  invoke<ProjectGraph>('get_project_graph', { repositoryPath })

export const analyzeImpact = (repositoryPath: string, target: string) =>
  invoke<ImpactAnalysis>('analyze_impact', { input: { repositoryPath, target } })

export const getEnvironmentDashboard = (repositoryPath: string) =>
  invoke<EnvironmentDashboard>('get_environment_dashboard', { repositoryPath })

export const saveEnvironmentManifest = (repositoryPath: string, manifest: EnvironmentManifest) =>
  invoke<EnvironmentDashboard>('save_environment_manifest', { input: { repositoryPath, manifest } })

export const getVerificationSnapshot = (repositoryPath: string) =>
  invoke<VerificationSnapshot>('get_verification_snapshot', { repositoryPath })

export const runWorkspaceVerification = (workspaceId: string, approvalGranted: boolean) =>
  invoke<VerificationSuite>('run_workspace_verification', { input: { workspaceId, approvalGranted } })

export const waiveVerificationGate = (suiteId: string, gateId: string, reason: string) =>
  invoke<VerificationSuite>('waive_verification_gate', { input: { suiteId, gateId, reason } })

export const getDeliverySnapshot = (repositoryPath: string) =>
  invoke<DeliverySnapshot | null>('get_delivery_snapshot', { repositoryPath })

export const syncGithubDelivery = (repositoryPath: string, repository: string | null) =>
  invoke<DeliverySyncReport>('sync_github_delivery', { input: { repositoryPath, repository } })

export const ingestExternalEvent = (input: {
  repositoryPath: string
  integrationId: string
  eventType: string
  occurredAt: string | null
  payload: Record<string, unknown>
}) => invoke<ProjectEvent[]>('ingest_external_event', { input })

export const requestPluginAction = (input: {
  integrationId: string
  repositoryPath: string
  sessionId: string | null
  requestedBy: string
  action: ActionKind
  approvalGranted: boolean
}) => invoke<ActionRun>('request_plugin_action', { input })

export const requestAction = (input: {
  repositoryPath: string
  sessionId: string | null
  requestedBy: string
  action: ActionKind
  approvalGranted: boolean
}) => invoke<ActionRun>('request_action', { input })

export const createGoal = (input: {
  repositoryPath: string
  title: string
  description: string
}) => invoke<Goal>('create_goal', { input })

export const updateGoal = (input: {
  id: string
  title: string
  description: string
  status: GoalStatus
}) => invoke<Goal>('update_goal', { input })

export const createTask = (input: {
  goalId: string
  title: string
  description: string
  priority: TaskPriority
  assignedAgent: string | null
  sourceAgent: string | null
}) => invoke<WorkTask>('create_task', { input })

export const updateTask = (input: {
  id: string
  title: string
  description: string
  status: TaskStatus
  priority: TaskPriority
  assignedAgent: string | null
}) => invoke<WorkTask>('update_task', { input })

export const updateTaskOrchestration = (input: {
  taskId: string
  requiredCapabilities: Capability[]
  blockedBy: string[]
  dependsOn: string[]
  produces: string[]
  consumes: string[]
  claimedPaths: string[]
}) => invoke<WorkTask>('update_task_orchestration', { input })

export const getOrchestrationSnapshot = (repositoryPath: string) =>
  invoke<OrchestrationSnapshot>('get_orchestration_snapshot', { repositoryPath })

export const getPlannerSnapshot = (repositoryPath: string) =>
  invoke<PlannerSnapshot>('get_planner_snapshot', { repositoryPath })

export const generatePlan = (repositoryPath: string, goalId: string) =>
  invoke<PlanVersion>('generate_plan', { input: { repositoryPath, goalId } })

export const approvePlan = (planId: string) =>
  invoke<PlanApprovalResult>('approve_plan', { input: { planId } })

export const replan = (planId: string, failedPlanTaskId: string, failureReason: string) =>
  invoke<PlanVersion>('replan', { input: { planId, failedPlanTaskId, failureReason } })

export const updateCapabilityProfile = (input: {
  repositoryPath: string
  provider: AgentProvider
  displayName: string
  strengths: Capability[]
  allowedActions: ActionKind[]
  maxContextTokens: number
  enabled: boolean
}) => invoke<AgentCapabilityProfile>('update_capability_profile', { input })

export const suggestTaskRoute = (taskId: string) =>
  invoke<WorkTask>('suggest_task_route', { taskId })

export const approveTaskAssignment = (input: { taskId: string; provider: AgentProvider }) =>
  invoke<WorkTask>('approve_task_assignment', { input })

export const clearTaskAssignment = (taskId: string) =>
  invoke<WorkTask>('clear_task_assignment', { taskId })

export const startRoutedTask = (input: { repositoryPath: string; taskId: string }) =>
  invoke<AgentSession>('start_routed_task', { input })

export const getCollaborativeExecutionSnapshot = (repositoryPath: string) =>
  invoke<CollaborativeExecutionSnapshot>('get_collaborative_execution_snapshot', { repositoryPath })

export const prepareTaskWorkspace = (repositoryPath: string, taskId: string) =>
  invoke<TaskWorkspace>('prepare_task_workspace', { input: { repositoryPath, taskId } })

export const startTaskWorkspace = (workspaceId: string) =>
  invoke<AgentSession>('start_task_workspace', { input: { workspaceId } })

export const validateTaskWorkspace = (workspaceId: string, runTests: boolean, runBuild: boolean, approvalGranted: boolean) =>
  invoke<WorkspaceValidationResult>('validate_task_workspace', { input: { workspaceId, runTests, runBuild, approvalGranted } })

export const prepareWorkspaceMerge = (workspaceId: string) =>
  invoke<TaskWorkspace>('prepare_workspace_merge', { input: { workspaceId } })

export const approveWorkspaceMerge = (workspaceId: string) =>
  invoke<TaskWorkspace>('approve_workspace_merge', { input: { workspaceId } })

export const cleanupTaskWorkspace = (workspaceId: string) =>
  invoke<TaskWorkspace>('cleanup_task_workspace', { input: { workspaceId } })

export const recordDecision = (input: {
  repositoryPath: string
  summary: string
  reason: string
  alternatives: string[]
  affected: string[]
  createdBy: string
  supersedesDecisionId: string | null
}) => invoke<Decision>('record_decision', { input })

export const getContextPreview = (repositoryPath: string) =>
  invoke<string>('get_context_preview', { repositoryPath })

export const generateContext = (repositoryPath: string) =>
  invoke<string>('generate_context', { repositoryPath })

export const compileContext = (input: ContextCompileInput) =>
  invoke<ContextPackage>('compile_context', { input })

export const generateSmartContext = (input: ContextCompileInput) =>
  invoke<ContextPackage>('generate_smart_context', { input })

export const startAgentSession = (input: {
  repositoryPath: string
  provider: AgentProvider
  agentName: string
}) => invoke<AgentSession>('start_agent_session', { input: { ...input, taskId: null } })

export const endAgentSession = (input: {
  sessionId: string
  summaryStatus: string
  blockers: string[]
}) => invoke<HandoffSnapshot>('end_agent_session', { input })

export const getActiveSession = (repositoryPath: string) =>
  invoke<AgentSession | null>('get_active_session', { repositoryPath })

export const getLatestHandoff = (repositoryPath: string) =>
  invoke<HandoffSnapshot | null>('get_latest_handoff', { repositoryPath })

export const markSessionIdle = (sessionId: string) =>
  invoke<AgentSession>('mark_session_idle', { sessionId })

export const evaluateConflicts = (repositoryPath: string) =>
  invoke<ConflictRecord[]>('evaluate_conflicts', { repositoryPath })

export const acknowledgeConflict = (conflictId: string) =>
  invoke<ConflictRecord>('acknowledge_conflict', { conflictId })

export const resolveConflict = (conflictId: string) =>
  invoke<ConflictRecord>('resolve_conflict', { conflictId })

export const setProjectTracking = (repositoryPath: string, enabled: boolean) =>
  invoke<ProjectMemory['project']>('set_project_tracking', { repositoryPath, enabled })

export const getIdleTimeoutMinutes = () => invoke<number>('get_idle_timeout_minutes')

export const setIdleTimeoutMinutes = (minutes: number) =>
  invoke<number>('set_idle_timeout_minutes', { minutes })

export const getBackgroundStatus = () => invoke<BackgroundStatus>('get_background_status')

export const openContextFile = (repositoryPath: string) =>
  invoke<void>('open_context_file', { repositoryPath })
