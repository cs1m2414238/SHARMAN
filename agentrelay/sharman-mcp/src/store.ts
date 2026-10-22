import { createHash, randomUUID } from 'node:crypto';
import { mkdir, readFile, rename, unlink, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { NativePersonalBridge, type MailboxStatus } from './native.js';
import type { HomeEvent as NormalizedHomeEvent, NewHomeEvent } from './household-types.js';

export type TaskStatus = 'open' | 'completed';

export type SharmanTask = {
  id: string;
  title: string;
  dueAt: string | null;
  status: TaskStatus;
  source: 'user' | 'gmail' | 'alexa';
  createdAt: string;
};

export type InboxAction = {
  id: string;
  messageId: string;
  sender: string;
  subject: string;
  summary: string;
  deadline: string | null;
  confidence: number;
  category: string;
  sample?: boolean;
  status: 'suggested' | 'accepted' | 'dismissed';
};

export type Reminder = {
  id: string;
  title: string;
  remindAt: string;
  sourceActionId: string | null;
  createdAt: string;
  storage?: 'native_sqlite' | 'web_json';
  status?: string;
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
  argsSummary?: string;
  durationMs?: number;
  status?: 'allowed' | 'approval_required' | 'denied';
  denialReason?: string | null;
  requiredPermission?: string | null;
  delegatedCeiling?: string | null;
};

export type HomeScenario = 'expected_delivery' | 'repeated_unexpected' | 'blocked_unlock';

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

export type DailyPriorityTier = 'CRITICAL' | 'HIGH' | 'MEDIUM' | 'LOW';

export type DailyPriorityItem = {
  tier: DailyPriorityTier;
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
  dataSource: 'sample' | 'native_gmail' | 'unavailable';
  priorities: DailyPriorityItem[];
  status: 'terminated';
  createdAt: string;
  terminatedAt: string;
};

export type HomeState = {
  status: 'safe' | 'attention';
  residentHome: boolean;
  expectedVisit: {
    label: string;
    window: string;
    fromHour: number;
    toHour: number;
  };
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

export type BridgeState = {
  mode: 'demo';
  processingMode?: ProcessingMode;
  tasks: SharmanTask[];
  inbox: InboxAction[];
  reminders: Reminder[];
  audit: AuditEvent[];
  home: HomeState;
  ringReceipts?: Record<string, RingReceipt>;
  ringSequence?: number;
};

export type RingReceipt = { normalized: NormalizedHomeEvent; event: HomeEvent; workflow: HomeWorkflow | null };

const nextDay = (days: number, hour: number, minute = 0) => {
  const date = new Date();
  date.setDate(date.getDate() + days);
  date.setHours(hour, minute, 0, 0);
  return date.toISOString();
};

const seedHomeState = (): HomeState => ({
  status: 'safe',
  residentHome: true,
  expectedVisit: {
    label: 'Medicine delivery',
    window: '4:00–6:00 PM',
    fromHour: 16,
    toHour: 18,
  },
  events: [],
  agents: [],
  lastWorkflow: null,
});

const seedState = (): BridgeState => ({
  mode: 'demo',
  processingMode: 'local_only',
  tasks: [],
  inbox: [
    {
      id: 'action-amazon-oa',
      messageId: 'demo-amazon-oa-001',
      sender: 'Amazon University Recruiting',
      subject: 'Complete your SDE online assessment',
      summary: 'The assessment link expires tomorrow at 11:59 PM.',
      deadline: nextDay(1, 23, 59),
      confidence: 0.96,
      category: 'assessment',
      status: 'suggested',
    },
    {
      id: 'action-interview',
      messageId: 'demo-interview-002',
      sender: 'Talent Team',
      subject: 'Choose an interview slot',
      summary: 'Select one of the available interview times within 48 hours.',
      deadline: nextDay(2, 10),
      confidence: 0.91,
      category: 'interview',
      status: 'suggested',
    },
  ],
  reminders: [],
  audit: [
    { id: randomUUID(), at: new Date().toISOString(), actor: 'sharman', action: 'Demo workspace initialized', permission: 'READ', result: 'Sample data ready; no mailbox connected', parentAgent: 'sharman-supervisor', childAgent: null, status: 'allowed', durationMs: 2 },
  ],
  home: seedHomeState(),
});

const stableId = (...parts: Array<string | null>) => createHash('sha256').update(parts.join('|')).digest('hex').slice(0, 20);

// Only these fixed IDs were shipped as fixtures, incorrectly attributed to the
// user. Approved tasks have content-derived IDs; never remove by title alone.
const legacyTaskFixtures = new Map([
  ['task-portfolio', 'Polish SHARMAN demo video'],
  ['task-report', 'Finish Vidyut progress report'],
  ['task-gmail', 'Validate Gmail action extraction'],
]);

export class SharmanStore {
  readonly native = new NativePersonalBridge();
  private readonly file: string;
  private queue: Promise<unknown> = Promise.resolve();
  private initialization: Promise<void> | null = null;

  constructor(directory = process.env.SHARMAN_MCP_DATA_DIR || path.resolve('.data')) {
    this.file = path.join(directory, 'sharman-demo.json');
  }

  private async initialize(): Promise<void> {
    let existing: string;
    try {
      existing = await readFile(this.file, 'utf8');
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
      await this.save(seedState());
      return;
    }
    if (!this.native.enabled) {
      const state = JSON.parse(existing) as BridgeState;
      const actualTasks = state.tasks.filter((task) => task.source !== 'user' || legacyTaskFixtures.get(task.id) !== task.title);
      if (actualTasks.length !== state.tasks.length) {
        state.tasks = actualTasks;
        await this.save(state);
      }
    }
  }

  private async load(): Promise<BridgeState> {
    // The browser starts several reads at once. All reads and queued mutations
    // must await the same seed write; otherwise initialization races a rename or
    // overwrites the first approved task. A failed initialization can be retried.
    this.initialization ??= this.initialize().catch((error) => {
      this.initialization = null;
      throw error;
    });
    await this.initialization;
    const state = JSON.parse(await readFile(this.file, 'utf8')) as BridgeState;
    state.processingMode ??= 'local_only';
    state.home ??= seedHomeState();
    state.home.lastWorkflow ??= null;
    return state;
  }

  private async save(state: BridgeState): Promise<void> {
    await mkdir(path.dirname(this.file), { recursive: true });
    const temporary = `${this.file}.${process.pid}.${randomUUID()}.tmp`;
    try {
      await writeFile(temporary, `${JSON.stringify(state, null, 2)}\n`, { encoding: 'utf8', flag: 'wx', mode: 0o600 });
      await rename(temporary, this.file);
    } finally {
      await unlink(temporary).catch((error: NodeJS.ErrnoException) => {
        if (error.code !== 'ENOENT') throw error;
      });
    }
  }

  private mutate<T>(operation: (state: BridgeState) => T | Promise<T>): Promise<T> {
    const next = this.queue.then(async () => {
      const state = await this.load();
      const value = await operation(state);
      await this.save(state);
      return value;
    });
    this.queue = next.then(() => undefined, () => undefined);
    return next;
  }

  async snapshot(): Promise<BridgeState> {
    const state = await this.load();
    if (this.native.enabled) {
      state.tasks = await this.listTasks();
      state.reminders = await this.listReminders();
    }
    return state;
  }

  async listTasks(status?: TaskStatus): Promise<SharmanTask[]> {
    if (this.native.enabled) {
      const { tasks } = await this.native.snapshot();
      return tasks.map((task): SharmanTask => ({ ...task, status: task.status === 'completed' ? 'completed' : 'open', source: 'alexa' })).filter((task) => !status || task.status === status);
    }
    const state = await this.load();
    return state.tasks.filter((task) => !status || task.status === status);
  }

  async listReminders(): Promise<Reminder[]> {
    if (this.native.enabled) {
      const { reminders } = await this.native.snapshot();
      return reminders.map((reminder) => ({ ...reminder, sourceActionId: null, storage: 'native_sqlite' as const }));
    }
    const state = await this.load();
    return state.reminders;
  }

  async getProcessingMode(): Promise<ProcessingMode> {
    const state = await this.load();
    return state.processingMode ?? 'local_only';
  }

  async setProcessingMode(mode: ProcessingMode): Promise<ProcessingMode> {
    return this.mutate((state) => {
      state.processingMode = mode;
      state.audit.unshift({
        id: randomUUID(),
        at: new Date().toISOString(),
        actor: 'user',
        action: 'Processing mode updated',
        permission: 'WRITE_APPROVED',
        result: `Mode set to ${mode}; permission engine remains enforced locally`,
        parentAgent: 'sharman-supervisor',
        childAgent: null,
        tool: 'settings.processing_mode',
        status: 'allowed',
        durationMs: 2,
      });
      state.audit = state.audit.slice(0, 100);
      return mode;
    });
  }

  async runDailyPlannerAgent(actor: AuditEvent['actor'] = 'sharman', sample = false): Promise<DailyPlanDelegation> {
    const native = this.native.enabled ? await this.native.snapshot() : null;
    const mailbox = await this.mailboxStatus();
    const actions = await this.actionItems(undefined, sample);
    return this.mutate((state) => {
      const now = new Date().toISOString();
      const requestId = `req-${randomUUID().slice(0, 8)}`;
      const openTasks = native ? native.tasks.filter((task) => task.status === 'todo') : state.tasks.filter((task) => task.status === 'open');
      const reminders = native ? native.reminders.filter((reminder) => reminder.status === 'pending') : state.reminders;
      const priorities: DailyPriorityItem[] = [
        ...actions.slice(0, 2).map((item, index): DailyPriorityItem => ({ tier: index === 0 ? 'CRITICAL' : 'HIGH', title: item.subject, detail: item.summary, source: sample ? 'Sample Inbox' : 'Gmail' })),
        ...reminders.slice(0, 1).map((item): DailyPriorityItem => ({ tier: 'MEDIUM', title: item.title, detail: `Reminder scheduled · ${item.remindAt}`, source: 'SHARMAN' })),
        ...openTasks.slice(0, 2).map((item, index): DailyPriorityItem => ({ tier: index === 0 && reminders.length === 0 ? 'MEDIUM' : 'LOW', title: item.title, detail: item.dueAt ? `Task due · ${item.dueAt}` : 'Local task queue', source: 'Tasks' })),
      ];
      const delegation: DailyPlanDelegation = {
        id: `daily-planner-agent-${randomUUID().slice(0, 8)}`,
        parentAgentId: 'sharman-supervisor',
        name: 'DailyPlannerAgent',
        goal: 'Determine today’s priorities',
        permissionCeiling: 'READ_ONLY',
        maxSteps: 8,
        stepsUsed: 6,
        timeoutMs: 30_000,
        allowedCapabilities: [sample ? 'inbox.sample.read' : 'gmail.connection_status', 'inbox.actions', 'tasks.list', 'reminders.list'],
        deniedCapabilities: ['gmail.send', 'browser.submit', 'system.exec'],
        steps: [
          { step: '01', tool: sample ? 'inbox.sample.read' : 'gmail.connection_status', status: 'completed' },
          { step: '02', tool: 'inbox.actions', status: sample || mailbox.connected ? 'completed' : 'skipped' },
          { step: '03', tool: 'tasks.list', status: 'completed' },
          { step: '04', tool: 'reminders.list', status: 'completed' },
          { step: '05', tool: 'prioritize', status: 'completed' },
          { step: '06', tool: 'return_result', status: 'completed' },
        ],
        priorities,
        dataSource: sample ? 'sample' : mailbox.source,
        status: 'terminated',
        createdAt: now,
        terminatedAt: now,
      };
      state.audit.unshift(
        {
          id: randomUUID(),
          at: now,
          actor,
          action: 'DailyPlannerAgent terminated',
          permission: 'READ',
          result: `Returned ${priorities.length} prioritized items (6/8 steps used) and destroyed worker`,
          requestId,
          parentAgent: 'sharman-supervisor',
          childAgent: 'DailyPlannerAgent',
          goal: delegation.goal,
          tool: 'return_result',
          delegatedCeiling: 'READ_ONLY',
          durationMs: 18,
          status: 'allowed',
        },
        {
          id: randomUUID(),
          at: now,
          actor,
          action: 'DailyPlannerAgent created',
          permission: 'READ',
          result: 'Bounded READ capabilities delegated; gmail.send, browser.submit, system.exec denied',
          requestId,
          parentAgent: 'sharman-supervisor',
          childAgent: 'DailyPlannerAgent',
          goal: delegation.goal,
          tool: 'agent.delegate',
          delegatedCeiling: 'READ_ONLY',
          durationMs: 4,
          status: 'allowed',
        },
      );
      state.audit = state.audit.slice(0, 100);
      return delegation;
    });
  }

  async mailboxStatus(): Promise<MailboxStatus> {
    return this.native.enabled ? this.native.gmailStatus() : { connected: false, needsReconnect: false, source: 'unavailable' };
  }

  async actionItems(query?: string, sample = false): Promise<InboxAction[]> {
    const items = sample ? (await this.load()).inbox.map((item) => ({ ...item, sample: true }))
      : (await this.mailboxStatus()).connected ? this.normalizeActions((await this.native.gmailActions()).items) : [];
    const normalized = query?.trim().toLowerCase();
    return items.filter((item) => item.status !== 'dismissed' && (!normalized || `${item.sender} ${item.subject} ${item.summary}`.toLowerCase().includes(normalized)));
  }

  private normalizeActions(items: Awaited<ReturnType<NativePersonalBridge['gmailActions']>>['items']): InboxAction[] {
    return items.map((item) => ({ id: item.id, messageId: item.messageId, sender: item.sender, subject: item.title, summary: item.evidence, deadline: item.deadline, confidence: item.confidence, category: item.actionType, status: item.state === 'dismissed' ? 'dismissed' : item.reminderCreated ? 'accepted' : 'suggested', sample: false }));
  }

  async refreshActions(mode: string): Promise<{ items: InboxAction[]; scanned: number }> {
    const result = await this.native.gmailActions(true, mode);
    return { items: this.normalizeActions(result.items), scanned: result.scan?.scanned ?? 0 };
  }

  async createTask(title: string, dueAt: string | null, source: SharmanTask['source']): Promise<{ task: SharmanTask; created: boolean }> {
    if (this.native.enabled) {
      const result = await this.native.createTask(title, dueAt);
      await this.auditRead('sharman_create_task', 'Task persisted in the native SHARMAN store', 'alexa+');
      return { task: { ...result.task, status: result.task.status === 'completed' ? 'completed' : 'open', source }, created: result.created };
    }
    return this.mutate((state) => {
      const id = `task-${stableId(title.trim().toLowerCase(), dueAt)}`;
      const existing = state.tasks.find((task) => task.id === id);
      if (existing) return { task: existing, created: false };
      const task: SharmanTask = { id, title: title.trim(), dueAt, status: 'open', source, createdAt: new Date().toISOString() };
      state.tasks.unshift(task);
      state.audit.unshift({ id: randomUUID(), at: new Date().toISOString(), actor: source === 'alexa' ? 'alexa+' : 'user', action: 'Task created', permission: 'WRITE_APPROVED', result: task.title });
      return { task, created: true };
    });
  }

  async createReminder(title: string, remindAt: string, sourceActionId: string | null): Promise<{ reminder: Reminder; created: boolean }> {
    if (this.native.enabled) {
      const result = await this.native.createReminder(title, remindAt);
      await this.mutate((state) => {
        if (sourceActionId) {
          const item = state.inbox.find((action) => action.id === sourceActionId);
          if (item) item.status = 'accepted';
        }
        state.audit.unshift({ id: randomUUID(), at: new Date().toISOString(), actor: 'alexa+', action: 'Native reminder created', permission: 'WRITE_APPROVED', result: `${result.reminder.id} · native_sqlite · Windows scheduler` });
      });
      return { reminder: { ...result.reminder, sourceActionId, storage: 'native_sqlite' }, created: result.created };
    }
    return this.mutate((state) => {
      const id = `reminder-${stableId(sourceActionId, title.trim().toLowerCase(), remindAt)}`;
      const existing = state.reminders.find((reminder) => reminder.id === id);
      if (existing) return { reminder: existing, created: false };
      const reminder: Reminder = { id, title: title.trim(), remindAt, sourceActionId, createdAt: new Date().toISOString() };
      state.reminders.unshift(reminder);
      if (sourceActionId) {
        const source = state.inbox.find((item) => item.id === sourceActionId);
        if (source) source.status = 'accepted';
      }
      state.audit.unshift({ id: randomUUID(), at: new Date().toISOString(), actor: 'alexa+', action: 'Reminder created', permission: 'WRITE_APPROVED', result: `${reminder.title} · ${reminder.remindAt}` });
      return { reminder, created: true };
    });
  }

  async auditRead(tool: string, result: string, actor: AuditEvent['actor'] = 'sharman'): Promise<void> {
    await this.mutate((state) => {
      state.audit.unshift({ id: randomUUID(), at: new Date().toISOString(), actor, action: tool, permission: 'READ', result });
      state.audit = state.audit.slice(0, 100);
    });
  }

  async homeSnapshot(): Promise<HomeState> {
    const state = await this.load();
    return state.home;
  }

  async resetHomeDemo(): Promise<HomeState> {
    return this.mutate((state) => {
      state.home = seedHomeState();
      state.audit.unshift({
        id: randomUUID(),
        at: new Date().toISOString(),
        actor: 'user',
        action: 'Ring simulator reset',
        permission: 'WRITE_APPROVED',
        result: 'SHARMAN Home returned to its safe baseline',
      });
      return state.home;
    });
  }

  async simulateHomeScenario(scenario: HomeScenario): Promise<HomeWorkflow> {
    return this.mutate((state) => this.evaluateHomeScenario(state, scenario));
  }

  private evaluateHomeScenario(state: BridgeState, scenario: HomeWorkflow['scenario'], supplied?: HomeEvent): HomeWorkflow {
      const now = new Date().toISOString();
      const sharedPermissions = ['ring.event.read', 'home.context.read', 'alexa.notify'];
      const forbidden = ['door.unlock', 'system.exec', 'purchase'];
      let event: HomeEvent;
      let alexaMessage: string;
      let suggestedAction: string | null;
      let requiresApproval = false;
      let decision: HomeWorkflow['decision'] = 'allowed';

      if (scenario === 'expected_delivery') {
        event = {
          id: `home-event-${randomUUID()}`,
          at: now,
          source: 'ring-simulator',
          device: 'Front Door',
          kind: 'visitor',
          title: 'Visitor detected',
          summary: `Arrival matches the expected ${state.home.expectedVisit.label.toLowerCase()} window`,
          severity: 'normal',
          expectedMatch: true,
          occurrenceCount: 1,
        };
        alexaMessage = 'A visitor arrived during your expected medicine delivery window. Would you like a reminder in ten minutes to collect it?';
        suggestedAction = 'Create a 10-minute reminder';
      } else if (scenario === 'repeated_unexpected') {
        event = {
          id: `home-event-${randomUUID()}`,
          at: now,
          source: 'ring-simulator',
          device: 'Front Door',
          kind: 'visitor',
          title: 'Repeated unexpected visitor',
          summary: 'Two front-door events occurred within four minutes and no visit is scheduled',
          severity: 'attention',
          expectedMatch: false,
          occurrenceCount: 2,
        };
        alexaMessage = 'There have been two unexpected front-door events and no visitor is expected. Would you like me to prepare a message for your trusted contact?';
        suggestedAction = 'Prepare trusted-contact alert';
        requiresApproval = true;
        decision = 'approval_required';
        state.home.status = 'attention';
      } else if (scenario === 'blocked_unlock') {
        event = {
          id: `home-event-${randomUUID()}`,
          at: now,
          source: 'ring-simulator',
          device: 'Front Door',
          kind: 'permission_denied',
          title: 'Unsafe action blocked',
          summary: 'VisitorAgent requested door.unlock above its permission ceiling',
          severity: 'blocked',
          expectedMatch: false,
          occurrenceCount: 1,
        };
        alexaMessage = 'I blocked the door unlock request. VisitorAgent can explain and suggest actions, but it cannot control the lock.';
        suggestedAction = null;
        decision = 'denied';
      } else {
        if (!supplied) throw new Error('A recorded Ring event is required.');
        event = supplied;
        alexaMessage = supplied.title === 'Doorbell pressed'
          ? 'The configured Ring doorbell was pressed. No scheduled visit has been verified. No external action was taken.'
          : supplied.title === 'Package detected'
            ? 'The Ring device reported a package. No expected delivery has been verified. No external action was taken.'
          : supplied.title === 'Person detected'
            ? 'The configured Ring device reported a person. No scheduled visit has been verified. No external action was taken.'
            : 'The Ring Developer Playground reported motion. No person or scheduled visit has been identified. No external action was taken.';
        suggestedAction = null;
        state.home.status = 'attention';
      }

      if (supplied) {
        event = { ...supplied, kind: decision === 'denied' ? 'permission_denied' : supplied.kind, severity: decision === 'denied' ? 'blocked' : supplied.severity };
        if (scenario === 'blocked_unlock') {
          event.title = 'Unsafe action blocked';
          event.summary = 'VisitorAgent requested door.unlock above its permission ceiling';
        } else if (scenario === 'repeated_unexpected') {
          alexaMessage = `${event.occurrenceCount} events from the same configured Ring device occurred within four minutes. No scheduled visit has been verified. Would you like me to prepare a message for review?`;
        }
      }
      event.provenance = supplied?.ring?.ingestionMode ?? 'sharman_simulator';

      const requestId = `req-${randomUUID().slice(0, 8)}`;
      const denialDetail: PermissionDenialDetail | null = decision === 'denied' ? {
        action: 'door.unlock',
        requiredPermission: 'HIGH_RISK',
        delegatedCeiling: 'LOW_RISK',
        reason: 'Child agents cannot exceed delegated privileges',
      } : null;

      const agent: TemporaryAgent = {
        id: `visitor-agent-${randomUUID()}`,
        name: 'VisitorAgent',
        parentAgentId: 'sharman-supervisor',
        goal: 'Handle the current front-door event with minimum permissions',
        status: decision === 'denied' ? 'blocked' : 'completed',
        permissionCeiling: 'LOW_RISK',
        maxSteps: 6,
        stepsUsed: decision === 'denied' ? 3 : 4,
        timeoutMs: 30_000,
        permissions: scenario === 'expected_delivery' ? [...sharedPermissions, 'reminder.create'] : sharedPermissions,
        forbidden,
        denialDetail,
        createdAt: now,
        terminatedAt: now,
      };
      state.home.events.unshift(event);
      state.home.agents.unshift(agent);
      state.home.events = state.home.events.slice(0, 20);
      state.home.agents = state.home.agents.slice(0, 20);
      state.audit.unshift(
        {
          id: randomUUID(), at: now, actor: 'sharman', action: 'Temporary agent terminated', permission: 'READ', result: `${agent.name} returned a bounded result and was destroyed`, requestId, parentAgent: 'sharman-supervisor', childAgent: 'VisitorAgent', goal: agent.goal, tool: 'agent.terminate', delegatedCeiling: 'LOW_RISK', durationMs: 3, status: decision,
        },
        {
          id: randomUUID(), at: now, actor: 'sharman', action: decision === 'denied' ? 'Action denied' : 'Home workflow evaluated', permission: decision === 'denied' ? 'DENIED' : requiresApproval ? 'HIGH_RISK' : 'SUGGEST', result: event.summary, requestId, parentAgent: 'sharman-supervisor', childAgent: 'VisitorAgent', goal: agent.goal, tool: decision === 'denied' ? 'door.unlock' : 'ring.event.read', requiredPermission: decision === 'denied' ? 'HIGH_RISK' : 'LOW_RISK', delegatedCeiling: 'LOW_RISK', denialReason: denialDetail?.reason ?? null, durationMs: 11, status: decision,
        },
        {
          id: randomUUID(), at: now, actor: 'sharman', action: 'VisitorAgent created', permission: 'READ', result: `${agent.permissions.length} allowed capabilities; ${agent.forbidden.length} forbidden capabilities`, requestId, parentAgent: 'sharman-supervisor', childAgent: 'VisitorAgent', goal: agent.goal, tool: 'agent.delegate', delegatedCeiling: 'LOW_RISK', durationMs: 4, status: 'allowed',
        },
        {
          id: randomUUID(), at: now, actor: 'sharman', action: 'Ring event received', permission: 'READ', result: event.title, requestId, parentAgent: 'sharman-supervisor', childAgent: null, tool: 'ring.event.read', durationMs: 2, status: 'allowed',
        },
      );
      state.audit = state.audit.slice(0, 100);
      const workflow = { requestId, scenario, event, agent, alexaMessage, suggestedAction, requiresApproval, decision };
      state.home.lastWorkflow = workflow;
      return workflow;
  }

  async ingestRingEvent(input: NewHomeEvent): Promise<RingReceipt & { status: 'ingested' | 'duplicate_ignored' }> {
    const metadata = input.payload;
    if (input.source !== 'ring' || !metadata || !(typeof metadata.account_id === 'string' || (metadata.account_id === null && metadata.ingestion_mode === 'ring_developer_playground'))
      || typeof metadata.source_device_id !== 'string' || typeof metadata.detection_type !== 'string'
      || !['local_test', 'signed_webhook', 'ring_developer_playground'].includes(String(metadata.ingestion_mode))) {
      throw new Error('A normalized Ring event is required.');
    }
    const ingestionMode = metadata.ingestion_mode as 'local_test' | 'signed_webhook' | 'ring_developer_playground';
    const detectionType = metadata.detection_type;
    const key = createHash('sha256').update(JSON.stringify([metadata.account_id, metadata.source_device_id, input.source_event_id])).digest('hex');
    return this.mutate((state) => {
      const receipts = state.ringReceipts ??= {};
      const existing = receipts[key];
      if (existing) return { ...existing, status: 'duplicate_ignored' as const };
      const receivedAt = new Date().toISOString();
      const sequenceNumber = (state.ringSequence ?? 0) + 1;
      const normalized: NormalizedHomeEvent = {
        ...input, id: `ring-${key}`, schema_version: 1, received_at: receivedAt,
        sequence_number: sequenceNumber, validation_status: 'valid', payload: input.payload ?? null,
        source_event_hash: createHash('sha256').update(`ring|${input.source_event_id}`).digest('hex'),
      };
      const at = new Date(input.occurred_at).getTime();
      const visitor = input.event_type === 'doorbell_ring' || metadata.detection_type === 'human';
      const delegateMotion = input.event_type === 'motion_detected' && ['motion', 'unknown', 'package_delivery'].includes(detectionType);
      const occurrenceCount = visitor ? 1 + Object.values(receipts).filter((receipt) => {
        const prior = receipt.normalized;
        const delta = at - new Date(prior.occurred_at).getTime();
        return prior.payload?.account_id === metadata.account_id && prior.payload?.source_device_id === metadata.source_device_id
          && prior.payload?.ingestion_mode === ingestionMode && delta >= 0 && delta <= 240_000
          && (prior.event_type === 'doorbell_ring' || prior.payload?.detection_type === 'human');
      }).length : 1;
      const event: HomeEvent = {
        id: normalized.id, at: input.occurred_at, source: 'ring', device: 'Configured Ring device', kind: input.event_type === 'system_ping' ? 'system' : visitor ? 'visitor' : 'motion',
        provenance: ingestionMode,
        title: metadata.ring_event_type === 'on_demand' ? 'Live-view session recorded' : metadata.ring_event_type === 'device-status' ? metadata.online ? 'Ring device online' : 'Ring device offline' : input.event_type === 'doorbell_ring' ? 'Doorbell pressed' : detectionType === 'package_delivery' ? 'Package detected' : visitor ? 'Person detected' : 'Motion recorded',
        summary: input.event_type === 'system_ping' ? 'Ring session or device-status metadata recorded. No motion or visitor detection is inferred.' : visitor || delegateMotion ? 'Ring-format activity received. No scheduled visit has been verified.' : 'Motion recorded without a human classification; no VisitorAgent was created.',
        severity: visitor ? 'attention' : 'normal', expectedMatch: false, occurrenceCount,
        ring: { ingestionMode, delivery: typeof metadata.delivery === 'string' ? metadata.delivery : undefined, detectionType, sourceEventId: input.source_event_id, receivedAt, sequenceNumber },
      };
      let workflow: HomeWorkflow | null = null;
      if (visitor || delegateMotion) workflow = this.evaluateHomeScenario(state, occurrenceCount > 1 ? 'repeated_unexpected' : 'ring_event', event);
      else {
        state.home.events.unshift(event);
        state.home.events = state.home.events.slice(0, 20);
        state.home.lastWorkflow = null;
        state.audit.unshift({ id: randomUUID(), at: receivedAt, actor: 'sharman', action: 'Ring motion recorded', permission: 'READ', result: event.summary, tool: 'ring.event.read' });
        state.audit = state.audit.slice(0, 100);
      }
      state.ringSequence = sequenceNumber;
      const receipt: RingReceipt = { normalized, event: workflow?.event ?? event, workflow };
      receipts[key] = receipt;
      return { ...receipt, status: 'ingested' as const };
    });
  }

  async denyRingUnlock(eventId: string): Promise<HomeWorkflow> {
    return this.mutate((state) => {
      const receipt = Object.values(state.ringReceipts ?? {}).find((entry) => entry.event.id === eventId && entry.workflow);
      if (!receipt) throw new Error('A recorded visitor event is required.');
      return this.evaluateHomeScenario(state, 'blocked_unlock', receipt.event);
    });
  }

  async acceptHomeSuggestion(scenario: HomeScenario): Promise<{ message: string; reminder?: Reminder }> {
    if (scenario === 'expected_delivery') {
      const state = await this.load();
      const sourceEvent = state.home.events.find((event) => event.expectedMatch);
      const sourceId = sourceEvent?.id ?? 'home-expected-delivery';
      const baseline = sourceEvent ? new Date(sourceEvent.at).getTime() : Date.now();
      const remindAt = new Date(baseline + 10 * 60_000).toISOString();
      const created = await this.createReminder('Collect expected medicine delivery', remindAt, sourceId);
      return { message: 'Reminder created for ten minutes from now.', reminder: created.reminder };
    }
    return this.mutate((state) => {
      const message = 'Trusted-contact alert prepared for review. Nothing was sent automatically.';
      state.audit.unshift({
        id: randomUUID(),
        at: new Date().toISOString(),
        actor: 'user',
        action: 'Trusted-contact alert approved for preparation',
        permission: 'HIGH_RISK',
        result: message,
      });
      return { message };
    });
  }
}
