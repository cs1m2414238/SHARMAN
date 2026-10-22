import { createMcpExpressApp } from '@modelcontextprotocol/express';
import { toNodeHandler } from '@modelcontextprotocol/node';
import { createMcpHandler, McpServer } from '@modelcontextprotocol/server';
import type { NextFunction, Request, Response } from 'express';
import express from 'express';
import path from 'node:path';
import * as z from 'zod/v4';
import { BedrockCareExplainer } from './bedrock.js';
import { SharmanStore } from './store.js';
import { classifyEmailRequest, mailboxUnavailable, requestsSampleData, readMailbox } from './email.js';
import { RingAdapter, RingAuthenticationError, RingPayloadError } from './ring.js';
import { RingPlaygroundClient, RingPlaygroundError } from './ring-playground.js';

const host = process.env.SHARMAN_MCP_HOST || '127.0.0.1';
const port = Number(process.env.SHARMAN_MCP_PORT || process.env.PORT || 4179);
if (!Number.isInteger(port) || port < 1 || port > 65_535) throw new Error('Configure a valid TCP listening port.');
const store = new SharmanStore();
const bedrock = new BedrockCareExplainer();
const ringConfigured = Boolean(process.env.SHARMAN_RING_SIGNING_KEY && process.env.SHARMAN_RING_ACCOUNT_ID && process.env.SHARMAN_RING_DEVICE_ID);
const ringTestEvents = process.env.SHARMAN_RING_TEST_EVENTS_ENABLED === 'true';
const playground = new RingPlaygroundClient();
if ((ringTestEvents || playground.configured) && !['127.0.0.1', 'localhost', '::1'].includes(host)) {
  throw new Error('Local Ring test ingestion requires a loopback listener.');
}
const ring = new RingAdapter((event) => store.ingestRingEvent(event), undefined, ringConfigured ? {
  signingKey: process.env.SHARMAN_RING_SIGNING_KEY!, accountId: process.env.SHARMAN_RING_ACCOUNT_ID!, deviceId: process.env.SHARMAN_RING_DEVICE_ID!,
} : undefined);
if (store.native.enabled && !['127.0.0.1', 'localhost', '::1'].includes(host)) {
  throw new Error('The native personal bridge is available only on a loopback listener.');
}

const json = (value: unknown) => ({
  content: [{ type: 'text' as const, text: JSON.stringify(value, null, 2) }],
  structuredContent: value as Record<string, unknown>,
});

function createSharmanServer(): McpServer {
  const server = new McpServer(
    { name: 'sharman-local-supervisor', version: '0.23.0-alpha.1', websiteUrl: 'https://github.com/' },
    { capabilities: { tools: {} }, instructions: 'SHARMAN is a permission-aware personal productivity supervisor. Read tools never mutate external systems. Write tools represent explicit user-approved actions.' },
  );

  server.registerTool('sharman_get_tasks', {
    description: 'List SHARMAN tasks. Use this to answer what the user needs to do next.',
    inputSchema: z.object({ status: z.enum(['open', 'completed']).optional() }),
    annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true },
  }, async ({ status }) => {
    const tasks = await store.listTasks(status);
    await store.auditRead('sharman_get_tasks', `${tasks.length} task(s) returned`, 'alexa+');
    return json({ tasks });
  });

  server.registerTool('sharman_assistant_request', {
    description: 'Route an explicit user request through the bounded SHARMAN supervisor. Supports task reads, daily priorities, user-requested reminders and confirmed browser destinations.',
    inputSchema: z.object({ message: z.string().min(1).max(1000) }),
    annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: false },
  }, async ({ message }) => json(await executeAssistantCommand(message, 'alexa+')));

  server.registerTool('sharman_get_action_items', {
    description: 'Return actionable inbox items already extracted by SHARMAN using read-only mailbox access.',
    inputSchema: z.object({ sample: z.boolean().optional() }),
    annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true },
  }, async ({ sample }) => {
    const items = await store.actionItems(undefined, sample === true);
    await store.auditRead('sharman_get_action_items', `${items.length} actionable item(s) returned`, 'alexa+');
    return json({ items, dataSource: sample ? 'sample' : (await store.mailboxStatus()).source, mailbox: await store.mailboxStatus(), demoData: sample === true, mailboxPermission: 'READ_ONLY' });
  });

  server.registerTool('sharman_search_inbox', {
    description: 'Search previously indexed actionable email metadata. This tool never sends, deletes, or changes email.',
    inputSchema: z.object({ query: z.string().min(1).max(200), kind: z.enum(['status', 'recent', 'unread', 'actions']).optional(), mode: z.enum(['important', 'needsAction', 'internship']).optional(), sample: z.boolean().optional() }),
    annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true },
  }, async ({ query, kind, mode = 'needsAction', sample = false }) => {
    const result = await readMailbox(store, kind ?? 'actions', mode, sample);
    await store.auditRead('sharman_search_inbox', 'Bounded inbox request; message contents excluded from audit', 'alexa+');
    return json(kind === undefined && !sample && query ? { ...result, items: result.items.filter((item) => `${item.sender} ${item.subject} ${item.summary}`.toLowerCase().includes(query.toLowerCase())) } : result);
  });

  server.registerTool('sharman_get_reminders', {
    description: 'List scheduled SHARMAN reminders so the assistant can inspect upcoming reminder times.',
    inputSchema: z.object({}),
    annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true },
  }, async () => {
    const reminders = await store.listReminders();
    await store.auditRead('sharman_get_reminders', `${reminders.length} reminder(s) returned`, 'alexa+');
    return json({ reminders, permission: 'READ_ONLY' });
  });

  server.registerTool('sharman_create_task', {
    description: 'Create a local SHARMAN task after the user explicitly asks for it.',
    inputSchema: z.object({ title: z.string().min(2).max(240), dueAt: z.iso.datetime({ offset: true }).nullable().optional() }),
    annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: true },
  }, async ({ title, dueAt }) => json(await store.createTask(title, dueAt ?? null, 'alexa')));

  server.registerTool('sharman_create_reminder', {
    description: 'Create a local reminder after explicit user confirmation. Stable identity prevents duplicate reminders.',
    inputSchema: z.object({ title: z.string().min(2).max(240), remindAt: z.iso.datetime({ offset: true }), sourceActionId: z.string().nullable().optional() }),
    annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: true },
  }, async ({ title, remindAt, sourceActionId }) => json(await store.createReminder(title, remindAt, sourceActionId ?? null)));

  server.registerTool('sharman_open_allowed_url', {
    description: 'Validate a URL for user-approved opening. The MCP bridge never silently launches a browser.',
    inputSchema: z.object({ url: z.url(), dailyChallenge: z.boolean().optional() }),
    annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true },
  }, async ({ url, dailyChallenge }) => {
    const parsed = new URL(url);
    const allowed = parsed.protocol === 'https:' && parsed.hostname === 'leetcode.com' && !parsed.username && !parsed.password && !parsed.port && !parsed.search && !parsed.hash
      && (parsed.pathname === '/' || parsed.pathname === '/problemset/' || /^\/problems\/[a-z0-9-]+\/$/.test(parsed.pathname));
    const daily = allowed && dailyChallenge ? await resolveDailyChallenge() : null;
    await store.auditRead('sharman_open_allowed_url', allowed ? 'Approved LeetCode destination' : 'Rejected destination outside the browser allowlist', 'alexa+');
    return json({ allowed, url: daily?.url ?? url, dailyResolved: Boolean(daily), dailyTitle: daily?.title ?? null, requiresLocalUserConfirmation: true, opened: false });
  });

  server.registerTool('sharman_home_get_status', {
    description: 'Read Home events, source/verification metadata, bounded VisitorAgent results and prepared Alexa response. This tool does not deliver Alexa speech.',
    inputSchema: z.object({}),
    annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true },
  }, async () => {
    const home = await store.homeSnapshot();
    await store.auditRead('sharman_home_get_status', `${home.events.length} recent home event(s) returned`, 'alexa+');
    return json({ home, source: home.events[0]?.source === 'ring' ? home.events[0].ring?.ingestionMode === 'ring_developer_playground' ? 'Ring Developer Playground' : home.events[0].ring?.ingestionMode === 'signed_webhook' ? 'Ring signed webhook' : 'Ring-format local test' : 'Ring simulator', simulation: true, alexaSimulation: true });
  });

  server.registerTool('sharman_home_handle_ring_event', {
    description: 'Run a declared Home simulator scenario, or check the denied-unlock boundary for a recorded Ring event using eventId. This never controls a lock or contacts anyone.',
    inputSchema: z.object({ scenario: z.enum(['expected_delivery', 'repeated_unexpected', 'blocked_unlock']), eventId: z.string().min(1).max(100).optional() })
      .refine((value) => !value.eventId || value.scenario === 'blocked_unlock', 'Recorded events support only the denied-unlock permission check.'),
    annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: false },
  }, async ({ scenario, eventId }) => json(eventId ? await store.denyRingUnlock(eventId) : await store.simulateHomeScenario(scenario)));

  server.registerTool('sharman_home_explain_with_bedrock', {
    description: 'After an explicit user request, send only the latest structured Home workflow facts to Amazon Bedrock for a short accessible explanation. Bedrock never changes the deterministic decision or permissions.',
    inputSchema: z.object({}),
    annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true },
  }, async () => {
    const state = await store.snapshot();
    const workflow = state.home.lastWorkflow;
    if (!workflow) return json({ error: 'Run a Ring simulator scenario before requesting an explanation.' });
    const explanation = await bedrock.explain(workflow, state.processingMode ?? 'local_only');
    await store.auditRead('sharman_home_explain_with_bedrock', `${explanation.mode}: grounded care explanation prepared`);
    return json(explanation);
  });

  return server;
}

const mcp = createMcpHandler(createSharmanServer, { legacy: 'stateless' });
const allowedHosts = [host, 'localhost', '127.0.0.1', '[::1]', ...(process.env.SHARMAN_MCP_ALLOWED_HOSTS || '').split(',').map((item) => item.trim()).filter(Boolean)];
const app = express();
app.disable('x-powered-by');
// Hosting health probes may use an internal IP as Host. This public, data-free
// endpoint must work before the generated public hostname can be configured.
app.get('/health', (_request, response) => {
  response.setHeader('Cache-Control', 'no-store');
  response.json({ status: 'ok', ok: true, service: 'sharman-mcp', version: '0.23.0-alpha.1', protocol: 'Streamable HTTP', demoData: true });
});
// Mount before MCP's JSON parser: verification must use the exact wire bytes.
app.post('/webhooks/ring', express.raw({ type: 'application/json', limit: '64kb', inflate: false }), async (request, response) => {
  if (!ringConfigured) return response.status(503).json({ error: 'Ring webhook is not configured.' });
  if (!Buffer.isBuffer(request.body)) return response.status(415).json({ error: 'Send an application/json body.' });
  try {
    const receipt = await ring.ingestWebhook(request.body, request.get('X-Signature'));
    return response.status(200).json({ status: receipt.status, eventId: receipt.event.id });
  } catch (error) {
    const status = error instanceof RingAuthenticationError ? 401 : error instanceof RingPayloadError ? 400 : 503;
    return response.status(status).json({ error: status === 401 ? 'Ring webhook authentication failed.' : status === 400 ? 'Unsupported or malformed Ring event.' : 'Ring event could not be committed; retry delivery.' });
  }
});
app.use('/webhooks/ring', (error: { status?: number }, _request: Request, response: Response, _next: NextFunction) => {
  response.status(error.status === 413 ? 413 : 400).json({ error: 'Ring webhook body was rejected.' });
});
app.use(createMcpExpressApp({ host, allowedHosts, allowedOrigins: allowedHosts }));

type McpToolResult = Record<string, unknown>;

async function callLocalMcpTool(name: string, argumentsValue: Record<string, unknown>): Promise<McpToolResult> {
  const response = await fetch(`http://127.0.0.1:${port}/mcp`, {
    method: 'POST',
    headers: {
      accept: 'application/json, text/event-stream',
      'content-type': 'application/json',
      'mcp-protocol-version': '2025-11-25',
    },
    body: JSON.stringify({
      jsonrpc: '2.0',
      id: `alexa-${Date.now()}-${Math.random().toString(16).slice(2)}`,
      method: 'tools/call',
      params: { name, arguments: argumentsValue },
    }),
    signal: AbortSignal.timeout(10_000),
  });
  const raw = await response.text();
  if (!response.ok) throw new Error(`MCP ${name} failed (${response.status}).`);
  const dataLine = raw.split(/\r?\n/).findLast((line) => line.startsWith('data:'));
  const envelope = JSON.parse(dataLine ? dataLine.slice(5).trim() : raw) as {
    error?: { message?: string };
    result?: { isError?: boolean; structuredContent?: McpToolResult; content?: Array<{ text?: string }> };
  };
  if (envelope.error || envelope.result?.isError) {
    throw new Error(envelope.error?.message || envelope.result?.content?.[0]?.text || `MCP ${name} failed.`);
  }
  if (!envelope.result?.structuredContent) throw new Error(`MCP ${name} returned no structured result.`);
  return envelope.result.structuredContent;
}

function reminderDetails(message: string, now = new Date()): { title: string; remindAt: string; spokenTime: string } {
  const relative = message.match(/\bin\s+(\d{1,4})\s+minutes?\b/i);
  const titleMatch = message.match(/\bto\s+(.+?)\s*[.!?]*$/i);
  const title = titleMatch?.[1]?.trim() || (/leetcode/i.test(message) ? 'Solve LeetCode' : 'Follow up on important task');
  if (relative) {
    const minutes = Number(relative[1]);
    if (!Number.isInteger(minutes) || minutes < 1 || minutes > 1_440) {
      throw new Error('Relative reminders must be between 1 minute and 24 hours.');
    }
    return {
      title,
      remindAt: new Date(now.getTime() + minutes * 60_000).toISOString(),
      spokenTime: `in ${minutes} minute${minutes === 1 ? '' : 's'}`,
    };
  }
  if (!/\bat\s+8(?::00)?\s*pm\b/i.test(message)) throw new Error('Specify a reminder time, such as in 2 minutes or at 8 PM.');
  const remindDate = new Date(now);
  remindDate.setHours(20, 0, 0, 0);
  if (remindDate.getTime() <= now.getTime()) remindDate.setDate(remindDate.getDate() + 1);
  return { title, remindAt: remindDate.toISOString(), spokenTime: 'at 8:00 PM' };
}

async function resolveDailyChallenge(): Promise<{ url: string; title: string } | null> {
  try {
    const response = await fetch('https://leetcode.com/graphql', {
      method: 'POST', headers: { 'content-type': 'application/json' }, signal: AbortSignal.timeout(5_000),
      body: JSON.stringify({ query: 'query questionOfToday { activeDailyCodingChallengeQuestion { link question { title } } }' }),
    });
    if (!response.ok) return null;
    const result = await response.json() as { data?: { activeDailyCodingChallengeQuestion?: { link?: string; question?: { title?: string } } } };
    const daily = result.data?.activeDailyCodingChallengeQuestion;
    if (!daily?.link || !/^\/problems\/[a-z0-9-]+\/$/.test(daily.link)) return null;
    return { url: `https://leetcode.com${daily.link}`, title: daily.question?.title?.slice(0, 240) || 'Daily challenge' };
  } catch { return null; }
}

app.use('/api', (request: Request, response: Response, next: NextFunction) => {
  const origin = request.headers.origin;
  if (origin && /^http:\/\/(localhost|127\.0\.0\.1)(:\d+)?$/.test(origin)) response.setHeader('Access-Control-Allow-Origin', origin);
  response.setHeader('Access-Control-Allow-Headers', 'Content-Type');
  response.setHeader('Access-Control-Allow-Methods', 'GET,POST,OPTIONS');
  if (request.method === 'OPTIONS') return response.status(204).end();
  next();
});

app.get('/api/status', async (_request, response) => {
  const state = await store.snapshot();
  const mailbox = await store.mailboxStatus();
  state.inbox = await store.actionItems();
  response.json({ connected: true, mode: state.mode, mailbox, processingMode: state.processingMode ?? 'local_only', personalStorage: store.native.enabled ? 'native_sqlite' : 'web_json', nativeNotifications: store.native.enabled ? 'Desktop app must be running; Windows notification permission is required' : 'Not connected to the Windows scheduler', mcpEndpoint: ['0.0.0.0', '::'].includes(host) ? '/mcp' : `http://${host}:${port}/mcp`, counts: { tasks: state.tasks.filter((task) => task.status === 'open').length, actions: state.inbox.filter((item) => item.status === 'suggested').length, reminders: state.reminders.length }, aws: { bedrock: bedrock.status() } });
});
app.get('/api/tasks', async (_request, response) => response.json({ tasks: await store.listTasks() }));
app.get('/api/reminders', async (_request, response) => response.json({ reminders: await store.listReminders() }));
app.get('/api/inbox/actions', async (request, response) => {
  const sample = request.query.sample === 'true';
  const mailbox = await store.mailboxStatus();
  response.json({ items: await store.actionItems(undefined, sample), permission: 'READ_ONLY', demoData: sample, mailbox, dataSource: sample ? 'sample' : mailbox.source });
});
app.get('/api/audit', async (_request, response) => response.json({ events: (await store.snapshot()).audit }));
app.get('/api/home', async (_request, response) => response.json({ home: await store.homeSnapshot(), simulation: true }));
app.get('/api/ring/status', (_request, response) => response.json({ configured: ringConfigured, playgroundConfigured: playground.configured, playground: playground.status(), localTestEnabled: ringTestEvents, webhookPath: '/webhooks/ring', hardwareVerified: false, alexaSpeechDelivery: false, exposure: ['127.0.0.1', 'localhost', '::1'].includes(host) ? 'local' : 'public_demo' }));
app.get('/api/ring/playground/devices', async (_request, response) => {
  try { return response.json({ devices: await playground.listDevices(), provenance: 'ring_developer_playground' }); }
  catch (error) { return response.status(error instanceof RingPlaygroundError ? error.status : 502).json({ error: error instanceof RingPlaygroundError ? error.message : 'Ring device discovery failed.' }); }
});
app.get('/api/ring/playground/device', async (request, response) => {
  const parsed = z.string().min(1).max(500).safeParse(request.query.deviceId);
  if (!parsed.success) return response.status(400).json({ error: 'Choose a Ring-discovered device.' });
  response.setHeader('Cache-Control', 'no-store');
  try { return response.json(await playground.inspectDevice(parsed.data)); }
  catch (error) { return response.status(error instanceof RingPlaygroundError ? error.status : 502).json({ error: error instanceof RingPlaygroundError ? error.message : 'Ring device metadata could not be loaded.' }); }
});
app.post('/api/ring/playground/sync', async (request, response) => {
  const parsed = z.object({ deviceId: z.string().min(1).max(500) }).safeParse(request.body);
  if (!parsed.success) return response.status(400).json({ error: 'Choose a Ring-discovered device.' });
  try {
    const event = await playground.latestEvent(parsed.data.deviceId);
    if (!event) return response.json({ status: 'no_events', provenance: 'ring_developer_playground', message: 'No motion or doorbell event was returned by Ring. Generate a Motion event in the Playground, then refresh.' });
    return response.json(await ring.ingestPlaygroundHistoryEvent(event, parsed.data.deviceId));
  } catch (error) { return response.status(error instanceof RingPlaygroundError ? error.status : 502).json({ error: error instanceof RingPlaygroundError ? error.message : 'Ring event metadata could not be normalized.' }); }
});
app.post('/api/ring/playground/history', async (request, response) => {
  const parsed = z.object({ deviceId: z.string().min(1).max(500) }).safeParse(request.body);
  if (!parsed.success) return response.status(400).json({ error: 'Choose a Ring-discovered device.' });
  try {
    const data = await playground.historyEvents(parsed.data.deviceId, true);
    return response.json({ status: 'imported', records: data.length ? await ring.ingestCapturedPlaygroundHistory({ data }, 'api_response') : [] });
  } catch (error) { return response.status(error instanceof RingPlaygroundError ? error.status : 502).json({ error: 'Ring history metadata could not be imported.' }); }
});
app.post('/api/ring/playground/device-status', async (request, response) => {
  const parsed = z.object({ deviceId: z.string().min(1).max(500) }).safeParse(request.body);
  if (!parsed.success) return response.status(400).json({ error: 'Choose a Ring-discovered device.' });
  try { return response.json(await ring.ingestPlaygroundDeviceStatus(await playground.deviceStatus(parsed.data.deviceId), parsed.data.deviceId)); }
  catch (error) { return response.status(error instanceof RingPlaygroundError ? error.status : 502).json({ error: 'Ring device status could not be normalized.' }); }
});
app.post('/api/ring/test-event', async (request, response) => {
  if (!ringTestEvents) return response.status(404).json({ error: 'Local Ring test ingestion is disabled.' });
  if (!['127.0.0.1', '::1', '::ffff:127.0.0.1'].includes(request.socket.remoteAddress ?? '')) return response.status(403).json({ error: 'Local test ingestion requires loopback.' });
  try { return response.status(200).json(await ring.ingestTestEvent(request.body)); }
  catch (error) { return response.status(error instanceof RingPayloadError ? 400 : 503).json({ error: 'Ring test event could not be ingested.' }); }
});
app.post('/api/ring/playground/import', async (request, response) => {
  if (!ringTestEvents || process.env.RING_ENVIRONMENT !== 'playground') return response.status(404).json({ error: 'Playground capture import is disabled.' });
  if (!['127.0.0.1', '::1', '::ffff:127.0.0.1'].includes(request.socket.remoteAddress ?? '')) return response.status(403).json({ error: 'Capture import requires loopback.' });
  try {
    if (Array.isArray(request.body?.data)) return response.json({ status: 'imported', records: await ring.ingestCapturedPlaygroundHistory(request.body) });
    if (request.body?.data?.type === 'device-status') {
      const context = z.string().min(1).max(500).safeParse(request.query.deviceId);
      if (!context.success) return response.status(400).json({ error: 'Device status requires the source device ID from discovery or the requested endpoint.' });
      return response.json(await ring.ingestPlaygroundDeviceStatus(request.body, context.data, 'user_supplied_capture'));
    }
    return response.json(await ring.ingestCapturedPlaygroundEvent(request.body));
  }
  catch (error) { return response.status(error instanceof RingPayloadError ? 400 : 503).json({ error: 'Ring event capture could not be imported.' }); }
});
app.get('/api/aws/status', (_request, response) => response.json(bedrock.status()));

app.post('/api/settings/processing-mode', async (request, response) => {
  const parsed = z.object({ processingMode: z.enum(['local_only', 'hybrid', 'cloud_allowed']) }).safeParse(request.body);
  if (!parsed.success) return response.status(400).json({ error: 'Choose local_only, hybrid, or cloud_allowed.' });
  const processingMode = await store.setProcessingMode(parsed.data.processingMode);
  response.json({ processingMode });
});

app.post('/api/home/simulate', async (request, response) => {
  const parsed = z.object({ scenario: z.enum(['expected_delivery', 'repeated_unexpected', 'blocked_unlock']) }).safeParse(request.body);
  if (!parsed.success) return response.status(400).json({ error: 'Choose a supported Ring simulator scenario.' });
  response.status(201).json(await store.simulateHomeScenario(parsed.data.scenario));
});

app.post('/api/home/accept', async (request, response) => {
  const parsed = z.object({ scenario: z.enum(['expected_delivery', 'repeated_unexpected']) }).safeParse(request.body);
  if (!parsed.success) return response.status(400).json({ error: 'Choose an actionable home suggestion.' });
  response.status(201).json(await store.acceptHomeSuggestion(parsed.data.scenario));
});

app.post('/api/home/reset', async (_request, response) => response.json({ home: await store.resetHomeDemo() }));
app.post('/api/home/check-permission', async (request, response) => {
  const parsed = z.object({ eventId: z.string().min(1).max(100) }).safeParse(request.body);
  if (!parsed.success) return response.status(400).json({ error: 'A recorded Ring event ID is required.' });
  try { return response.json(await store.denyRingUnlock(parsed.data.eventId)); }
  catch { return response.status(409).json({ error: 'A recorded visitor event is required.' }); }
});

app.post('/api/home/explain', async (_request, response) => {
  const state = await store.snapshot();
  const workflow = state.home.lastWorkflow;
  if (!workflow) return response.status(409).json({ error: 'Run a Ring simulator scenario before requesting an explanation.' });
  const explanation = await bedrock.explain(workflow, state.processingMode ?? 'local_only');
  await store.auditRead('sharman_home_explain_with_bedrock', `${explanation.mode}: grounded care explanation prepared`);
  response.json(explanation);
});

app.post('/api/tasks', async (request, response) => {
  const parsed = z.object({ title: z.string().min(2).max(240), dueAt: z.iso.datetime({ offset: true }).nullable().optional() }).safeParse(request.body);
  if (!parsed.success) return response.status(400).json({ error: 'A task title and valid optional ISO deadline are required.' });
  response.status(201).json(await store.createTask(parsed.data.title, parsed.data.dueAt ?? null, 'user'));
});

app.post('/api/reminders', async (request, response) => {
  const parsed = z.object({ title: z.string().min(2).max(240), remindAt: z.iso.datetime({ offset: true }), sourceActionId: z.string().nullable().optional() }).safeParse(request.body);
  if (!parsed.success) return response.status(400).json({ error: 'A title and valid ISO reminder time are required.' });
  response.status(201).json(await store.createReminder(parsed.data.title, parsed.data.remindAt, parsed.data.sourceActionId ?? null));
});

app.post('/api/assistant/chat', async (request, response) => {
  const parsed = z.object({ message: z.string().min(1).max(1000), surface: z.enum(['web', 'alexa']).default('web') }).safeParse(request.body);
  if (!parsed.success) return response.status(400).json({ error: 'A message is required.' });
  try {
    response.json(await executeAssistantCommand(parsed.data.message, parsed.data.surface === 'alexa' ? 'alexa+' : 'sharman'));
  } catch (error) {
    response.status(502).json({ error: error instanceof Error ? error.message : 'SHARMAN could not confirm the command result.' });
  }
});

async function executeAssistantCommand(originalMessage: string, actor: 'alexa+' | 'sharman') {
  const message = originalMessage.toLowerCase();
  const transport = 'streamable_http_mcp';

  if (/(what do i need to do today|what needs my attention today|plan my day|prioritize my day)/.test(message)) {
    const sample = requestsSampleData(message);
    const [delegation, actionResult, taskResult, reminderResult] = await Promise.all([
      store.runDailyPlannerAgent(actor, sample),
      callLocalMcpTool('sharman_get_action_items', { sample }),
      callLocalMcpTool('sharman_get_tasks', { status: 'open' }),
      callLocalMcpTool('sharman_get_reminders', {}),
    ]);
    const items = actionResult.items as Awaited<ReturnType<SharmanStore['actionItems']>>;
    const tasks = taskResult.tasks as Awaited<ReturnType<SharmanStore['listTasks']>>;
    return {
      answer: `${sample ? 'Sample-data plan, not your real Gmail. ' : actionResult.mailbox && !(actionResult.mailbox as { connected: boolean }).connected ? 'Gmail is not connected; this plan excludes your inbox. ' : ''}DailyPlannerAgent completed in ${delegation.stepsUsed}/${delegation.maxSteps} steps under a READ_ONLY ceiling and was terminated. Top priority: ${delegation.priorities[0]?.title || 'No priorities in the available sources'}.`,
      intent: 'daily_plan_delegation',
      permission: 'READ_ONLY',
      plan: ['Create DailyPlannerAgent (READ_ONLY ceiling, 8-step budget)', 'Check inbox availability and read only available sources', 'Prioritize available items & terminate worker'],
      tool: 'DailyPlannerAgent',
      transport,
      mcpTools: ['sharman_get_action_items', 'sharman_get_tasks', 'sharman_get_reminders'],
      result: { dataSource: delegation.dataSource, mailbox: actionResult.mailbox, actionable: items.length, items, tasks, reminders: reminderResult.reminders, delegation },
    };
  }

  if (/(remind me|create reminder|set a reminder)/.test(message)) {
    const details = reminderDetails(originalMessage);
    const reminderResult = await callLocalMcpTool('sharman_create_reminder', {
      title: details.title,
      remindAt: details.remindAt,
      sourceActionId: null,
    });
    return {
      answer: `I have scheduled an approved reminder for "${details.title}" ${details.spokenTime}.`,
      intent: 'create_reminder',
      permission: 'WRITE_APPROVED',
      plan: ['Extract reminder time & title', 'Enforce WRITE_APPROVED permission', 'Store idempotent reminder record'],
      tool: 'sharman_create_reminder',
      transport,
      mcpTools: ['sharman_create_reminder'],
      result: { reminder: reminderResult.reminder },
    };
  }

  if (/(open.*leetcode|leetcode)/.test(message)) {
    const url = 'https://leetcode.com/problemset/';
    const toolResult = await callLocalMcpTool('sharman_open_allowed_url', { url, dailyChallenge: true });
    return {
      answer: toolResult.dailyResolved ? `Today’s LeetCode challenge: ${toolResult.dailyTitle}. Confirm to open the verified challenge page.` : 'LeetCode’s daily resolver is unavailable. Confirm to open the official problemset, where the Daily Challenge is listed.',
      intent: 'browser_navigation',
      permission: 'LOW_RISK',
      plan: ['Inspect destination URL', 'Check browser allowlist (HTTPS domain: leetcode.com)', 'Require explicit local confirmation'],
      tool: 'sharman_open_allowed_url',
      transport,
      mcpTools: ['sharman_open_allowed_url'],
      result: toolResult,
    };
  }

  const email = classifyEmailRequest(message);
  if (email) {
    const toolResult = await callLocalMcpTool('sharman_search_inbox', { query: originalMessage.slice(0, 200), ...email });
    const mailbox = toolResult.mailbox as { connected: boolean; needsReconnect: boolean };
    const items = (toolResult.items ?? []) as unknown[];
    const messages = (toolResult.messages ?? []) as unknown[];
    const answer = email.sample ? `Sample inbox preview only: ${items.length} example action items. These are not your emails and no live Gmail search was performed.`
      : !mailbox.connected || mailbox.needsReconnect ? mailboxUnavailable(mailbox.needsReconnect)
        : toolResult.error ? 'The Gmail request failed. Check your connection and retry; this is recoverable and no sample messages were substituted.'
          : email.kind === 'status' ? 'Gmail is connected privately with read-only access.'
            : email.kind === 'recent' || email.kind === 'unread' ? messages.length ? `Found ${messages.length} ${email.kind} Gmail messages. Showing actual bounded metadata, not just action items.` : `No ${email.kind} messages found.`
              : `Scanned: ${toolResult.scanned} message(s). Action Inbox items: ${items.length}. Verify extracted deadlines before creating reminders.`;
    return {
      answer,
      intent: email.sample ? 'sample_inbox_preview' : `gmail_${email.kind}`,
      permission: 'READ_ONLY',
      plan: ['Check private Gmail connection', email.sample ? 'Load explicitly requested sample data' : 'Read metadata only if connected', 'Report source and actual result counts'],
      tool: 'sharman_search_inbox',
      transport,
      mcpTools: ['sharman_search_inbox'],
      result: { ...toolResult, actionable: items.length },
    };
  }

  if (/(task|to.?do|what.*next)/.test(message)) {
    const toolResult = await callLocalMcpTool('sharman_get_tasks', { status: 'open' });
    const tasks = toolResult.tasks as Awaited<ReturnType<SharmanStore['listTasks']>>;
    return { answer: `You have ${tasks.length} open tasks. ${tasks[0] ? `Your next item is “${tasks[0].title}”.` : ''}`.trim(), intent: 'task_review', permission: 'READ_ONLY', plan: ['Load local tasks', 'Sort open items by deadline'], tool: 'sharman_get_tasks', transport, mcpTools: ['sharman_get_tasks'], result: { tasks } };
  }

  return { answer: 'I can review actionable email, list tasks, delegate a daily priority plan, and create an approved reminder. Try “What do I need to do today?”', intent: 'capability_help', permission: 'READ_ONLY', plan: ['Match request to allowed capabilities'], tool: null, result: {} };
}

app.all('/mcp', (request, response) => void toNodeHandler(mcp)(request, response, request.body));

if (process.env.SHARMAN_WEB_DIST) {
  const directory = path.resolve(process.env.SHARMAN_WEB_DIST);
  app.use(express.static(directory));
  app.get('/{*page}', (request, response) => {
    if (request.path.startsWith('/api/')) return response.status(404).json({ error: 'Unknown SHARMAN API route.' });
    response.sendFile(path.join(directory, 'index.html'));
  });
}

const httpServer = app.listen(port, host, () => {
  console.log(`SHARMAN MCP bridge listening at http://${host}:${port}/mcp`);
  console.log(`SHARMAN web API listening at http://${host}:${port}/api`);
  console.log('Demo mode is active; sample mailbox data is clearly labeled and no Gmail account is accessed.');
});

// Node is PID 1 in the judging image. Handle the hosting platform's stop signal
// explicitly so an ordinary redeploy need not wait for a forced container kill.
for (const signal of ['SIGTERM', 'SIGINT'] as const) {
  process.once(signal, () => {
    const deadline = setTimeout(() => {
      httpServer.closeAllConnections();
      process.exit(1);
    }, 8_000).unref();
    httpServer.close(() => {
      clearTimeout(deadline);
      process.exit(0);
    });
    httpServer.closeIdleConnections();
  });
}
