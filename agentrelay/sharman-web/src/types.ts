export type Task = {
  id: string;
  title: string;
  dueAt: string | null;
  status: 'open' | 'completed';
  source: 'user' | 'gmail' | 'alexa';
  createdAt: string;
};

export type InboxAction = {
  sample?: boolean;
  id: string;
  messageId: string;
  sender: string;
  subject: string;
  summary: string;
  deadline: string | null;
  confidence: number;
  category: 'assessment' | 'interview' | 'application' | 'receipt';
  status: 'suggested' | 'accepted' | 'dismissed';
};

export type ProcessingMode = 'local_only' | 'hybrid' | 'cloud_allowed';

export type AuditEvent = {
  id: string;
  at: string;
  actor: 'user' | 'sharman' | 'alexa+';
  action: string;
  permission: 'READ' | 'SUGGEST' | 'WRITE_APPROVED' | 'HIGH_RISK' | 'DENIED';
  result: string;
  requestId?: string;
  parentAgent?: string;
  childAgent?: string | null;
  goal?: string;
  tool?: string;
  durationMs?: number;
  status?: 'allowed' | 'approval_required' | 'denied';
  denialReason?: string | null;
  requiredPermission?: string | null;
  delegatedCeiling?: string | null;
};

export type HomeScenario = 'expected_delivery' | 'repeated_unexpected' | 'blocked_unlock';

export type RingStatus = {
  exposure?: 'local' | 'public_demo';
  configured: boolean;
  localTestEnabled: boolean;
  playgroundConfigured: boolean;
  playground: { configured: boolean; connectionState: 'unconfigured' | 'not_checked' | 'connected' | 'unavailable'; lastSuccessAt: string | null };
};

export type RingPlaygroundDevice = { id: string; name: string };

export type RingPlaygroundSnapshot = {
  source: 'ring';
  environment: 'developer_playground';
  delivery: 'api_response';
  fetchedAt: string;
  device: RingPlaygroundDevice;
  status: { resourceId: string; online: boolean; reportedAt: string | null; responseAt: string; audioSnooze: { active: boolean | null; until: string | null } };
  events: Array<{ id: string; deviceId: string; eventType: string; startedAt: string; endedAt: string | null; cvDetections: Array<{ type: string; id: string }>; reviewed: boolean | null }>;
  hasMoreHistory: boolean;
};

export type HomeEvent = {
  id: string;
  at: string;
  source: 'ring-simulator' | 'ring';
  device: string;
  provenance?: 'sharman_simulator' | 'local_test' | 'signed_webhook' | 'ring_developer_playground';
  ring?: { ingestionMode: 'local_test' | 'signed_webhook' | 'ring_developer_playground'; delivery?: string; detectionType: string; sourceEventId: string; receivedAt: string; sequenceNumber: number };
  kind: 'visitor' | 'permission_denied' | 'motion' | 'system';
  title: string;
  summary: string;
  severity: 'normal' | 'attention' | 'blocked';
  expectedMatch: boolean;
  occurrenceCount: number;
};

export type PermissionDenialDetail = {
  action: string;
  requiredPermission: 'HIGH_RISK';
  delegatedCeiling: 'LOW_RISK' | 'READ_ONLY';
  reason: string;
};

export type TemporaryAgent = {
  id: string;
  name: 'VisitorAgent' | 'DailyPlannerAgent';
  parentAgentId?: string;
  goal: string;
  status: 'completed' | 'blocked';
  permissionCeiling?: 'READ_ONLY' | 'LOW_RISK';
  maxSteps?: number;
  stepsUsed?: number;
  timeoutMs?: number;
  permissions: string[];
  forbidden: string[];
  denialDetail?: PermissionDenialDetail | null;
  createdAt: string;
  terminatedAt: string;
};

export type DailyPriorityItem = {
  tier: 'CRITICAL' | 'HIGH' | 'MEDIUM' | 'LOW';
  title: string;
  detail: string;
  source: 'Gmail' | 'Sample Inbox' | 'SHARMAN' | 'Tasks';
};

export type DailyPlanDelegation = {
  id: string;
  parentAgentId: string;
  name: 'DailyPlannerAgent';
  goal: string;
  permissionCeiling: 'READ_ONLY';
  maxSteps: number;
  stepsUsed: number;
  timeoutMs: number;
  allowedCapabilities: string[];
  deniedCapabilities: string[];
  steps: Array<{ step: string; tool: string; status: 'completed' | 'skipped' }>;
  dataSource?: 'sample' | 'native_gmail' | 'unavailable';
  priorities: DailyPriorityItem[];
  status: 'terminated';
  createdAt: string;
  terminatedAt: string;
};

export type HomeState = {
  status: 'safe' | 'attention';
  residentHome: boolean;
  expectedVisit: { label: string; window: string; fromHour: number; toHour: number };
  events: HomeEvent[];
  agents: TemporaryAgent[];
  lastWorkflow: HomeWorkflow | null;
};

export type HomeWorkflow = {
  requestId?: string;
  scenario: HomeScenario | 'ring_event';
  event: HomeEvent;
  agent: TemporaryAgent;
  alexaMessage: string;
  suggestedAction: string | null;
  requiresApproval: boolean;
  decision: 'allowed' | 'approval_required' | 'denied';
};

export type BedrockStatus = {
  configured: boolean;
  connectionState: 'disabled' | 'not_validated' | 'connected' | 'unavailable' | 'validation_failed';
  lastAttemptAt: string | null;
  lastSuccessAt: string | null;
  lastErrorCode: string | null;
  region: string;
  modelId: string;
  credentialSource: 'AWS default provider chain';
  purpose: string;
};

export type CareExplanation = {
  mode: 'bedrock' | 'local_fallback';
  text: string;
  provider: 'Amazon Bedrock' | 'SHARMAN deterministic fallback';
  modelId: string | null;
  region: string | null;
  groundedFields: string[];
  fallbackReason: string | null;
};

export type BridgeStatus = {
  connected: boolean;
  mode: 'demo';
  personalStorage?: 'native_sqlite' | 'web_json';
  mailbox?: { connected: boolean; needsReconnect: boolean; source: 'native_gmail' | 'unavailable' };
  processingMode?: ProcessingMode;
  mcpEndpoint: string;
  counts: { tasks: number; actions: number; reminders: number };
  aws?: { bedrock: BedrockStatus };
};

export type ChatResult = {
  answer: string;
  intent: string;
  permission: string;
  plan: string[];
  tool: string | null;
  transport?: 'streamable_http_mcp';
  mcpTools?: string[];
  result: {
    scanned?: number | null;
    dataSource?: 'sample' | 'native_gmail' | 'unavailable';
    mailbox?: { connected: boolean; needsReconnect: boolean };
    error?: string | null;
    messages?: Array<{ id: string; threadId: string; subject: string; sender: string; snippet: string; receivedAt: string }>;
    actionable?: number;
    items?: InboxAction[];
    tasks?: Task[];
    reminders?: Array<{ id: string; title: string; remindAt: string }>;
    delegation?: DailyPlanDelegation;
    reminder?: {
      id: string;
      title: string;
      remindAt: string;
      sourceActionId: string | null;
      createdAt: string;
      storage?: 'native_sqlite' | 'web_json';
    };
    allowed?: boolean;
    url?: string;
    requiresLocalUserConfirmation?: boolean;
    opened?: boolean;
  };
};
