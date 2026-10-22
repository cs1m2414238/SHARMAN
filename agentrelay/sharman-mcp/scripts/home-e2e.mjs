import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { createServer } from 'node:net';
import { createServer as createHttp2Server } from 'node:http2';
import { tmpdir } from 'node:os';
import path from 'node:path';

const projectDirectory = path.resolve(import.meta.dirname, '..');
const dataDirectory = await mkdtemp(path.join(tmpdir(), 'sharman-home-e2e-'));
let bridge = null;
let awsRequests = 0;
const awsSessions = new Set();
const unavailableAws = createHttp2Server((_request, response) => {
  awsRequests++;
  response.writeHead(503, { 'content-type': 'application/json' });
  response.end(JSON.stringify({ message: 'Controlled offline AWS fixture' }));
});
unavailableAws.on('session', (session) => {
  awsSessions.add(session);
  session.once('close', () => awsSessions.delete(session));
});
await new Promise((resolve) => unavailableAws.listen(0, '127.0.0.1', resolve));
const awsEndpoint = `http://127.0.0.1:${unavailableAws.address().port}`;

const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

async function freePort() {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      assert(address && typeof address !== 'string');
      server.close((error) => error ? reject(error) : resolve(address.port));
    });
  });
}

const port = await freePort();
const baseUrl = `http://127.0.0.1:${port}`;

async function startBridge(enabled = false) {
  const output = [];
  bridge = spawn(process.execPath, ['dist/server.js'], {
    cwd: projectDirectory,
    env: {
      ...process.env,
      SHARMAN_MCP_HOST: '127.0.0.1',
      SHARMAN_MCP_PORT: String(port),
      SHARMAN_MCP_DATA_DIR: dataDirectory,
      SHARMAN_AWS_BEDROCK_ENABLED: String(enabled),
      AWS_REGION: 'us-east-1',
      AWS_ACCESS_KEY_ID: 'controlled-test-key',
      AWS_SECRET_ACCESS_KEY: 'controlled-test-secret',
      AWS_SESSION_TOKEN: '',
      AWS_PROFILE: '',
      AWS_EC2_METADATA_DISABLED: 'true',
      AWS_ENDPOINT_URL_BEDROCK_RUNTIME: awsEndpoint,
      AWS_IGNORE_CONFIGURED_ENDPOINT_URLS: 'false',
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  bridge.stdout.on('data', (chunk) => output.push(String(chunk)));
  bridge.stderr.on('data', (chunk) => output.push(String(chunk)));

  for (let attempt = 0; attempt < 300; attempt += 1) {
    if (bridge.exitCode !== null) {
      throw new Error(`Bridge exited before becoming healthy.\n${output.join('')}`);
    }
    try {
      const response = await fetch(`${baseUrl}/health`);
      if (response.ok) return;
    } catch {
      // The socket is expected to reject while the child process starts.
    }
    await delay(50);
  }
  throw new Error(`Bridge did not become healthy.\n${output.join('')}`);
}

async function stopBridge() {
  if (!bridge || bridge.exitCode !== null) return;
  const exited = new Promise((resolve) => bridge.once('exit', resolve));
  bridge.kill();
  await Promise.race([exited, delay(5_000)]);
  if (bridge.exitCode === null) {
    bridge.kill('SIGKILL');
    await exited;
  }
  bridge = null;
}

async function jsonRequest(route, options = {}) {
  const response = await fetch(`${baseUrl}${route}`, {
    ...options,
    headers: { 'content-type': 'application/json', ...options.headers },
  });
  const body = await response.json();
  assert(response.ok, `${options.method || 'GET'} ${route} failed: ${JSON.stringify(body)}`);
  return body;
}

async function rejectedJsonRequest(route, expectedStatus, options = {}) {
  const response = await fetch(`${baseUrl}${route}`, {
    ...options,
    headers: { 'content-type': 'application/json', ...options.headers },
  });
  const body = await response.json();
  assert.equal(response.status, expectedStatus, `${options.method || 'GET'} ${route} must return ${expectedStatus}`);
  return body;
}

async function mcpRequest(id, method, params) {
  const response = await fetch(`${baseUrl}/mcp`, {
    method: 'POST',
    headers: {
      accept: 'application/json, text/event-stream',
      'content-type': 'application/json',
      'mcp-protocol-version': '2025-11-25',
    },
    body: JSON.stringify({ jsonrpc: '2.0', id, method, params }),
  });
  const text = await response.text();
  assert(response.ok, `MCP ${method} failed: ${text}`);
  const dataLine = text.split(/\r?\n/).findLast((line) => line.startsWith('data:'));
  return JSON.parse(dataLine ? dataLine.slice(5).trim() : text);
}

try {
  await startBridge();

  const reset = await jsonRequest('/api/home/reset', { method: 'POST', body: '{}' });
  assert.equal(reset.home.events.length, 0, 'reset must start with an empty event timeline');

  const beforeInvalidScenario = await jsonRequest('/api/home');
  const auditBeforeInvalidScenario = await jsonRequest('/api/audit');
  const invalidScenario = await rejectedJsonRequest('/api/home/simulate', 400, {
    method: 'POST', body: JSON.stringify({ scenario: 'unknown_scenario' }),
  });
  assert.match(invalidScenario.error, /supported Ring simulator scenario/i);
  assert.deepEqual(await jsonRequest('/api/home'), beforeInvalidScenario, 'an invalid scenario must not mutate Home state');
  assert.deepEqual(await jsonRequest('/api/audit'), auditBeforeInvalidScenario, 'an invalid scenario must not append audit evidence');

  const expected = await jsonRequest('/api/home/simulate', {
    method: 'POST', body: JSON.stringify({ scenario: 'expected_delivery' }),
  });
  assert.equal(expected.decision, 'allowed');
  assert.equal(expected.event.expectedMatch, true);

  const firstReminder = await jsonRequest('/api/home/accept', {
    method: 'POST', body: JSON.stringify({ scenario: 'expected_delivery' }),
  });
  const repeatedReminder = await jsonRequest('/api/home/accept', {
    method: 'POST', body: JSON.stringify({ scenario: 'expected_delivery' }),
  });
  assert.equal(firstReminder.reminder.id, repeatedReminder.reminder.id, 'reminder approval must be idempotent');

  const repeatedVisitor = await jsonRequest('/api/home/simulate', {
    method: 'POST', body: JSON.stringify({ scenario: 'repeated_unexpected' }),
  });
  assert.equal(repeatedVisitor.decision, 'approval_required');
  const prepared = await jsonRequest('/api/home/accept', {
    method: 'POST', body: JSON.stringify({ scenario: 'repeated_unexpected' }),
  });
  assert.match(prepared.message, /Nothing was sent automatically/i);
  const preparedAgain = await jsonRequest('/api/home/accept', {
    method: 'POST', body: JSON.stringify({ scenario: 'repeated_unexpected' }),
  });
  assert.equal(preparedAgain.message, prepared.message, 'repeated approval must remain preparation-only');
  const auditAfterRepeatedApproval = await jsonRequest('/api/audit');
  const preparationApprovals = auditAfterRepeatedApproval.events.filter(
    (event) => event.action === 'Trusted-contact alert approved for preparation',
  );
  assert.equal(preparationApprovals.length, 2, 'each approval must be recorded without performing an external send');
  assert(preparationApprovals.every(
    (event) => event.permission === 'HIGH_RISK' && /Nothing was sent automatically/i.test(event.result),
  ));
  const statusAfterRepeatedApproval = await jsonRequest('/api/status');
  assert.equal(statusAfterRepeatedApproval.counts.reminders, 1, 'repeated visitor approval must not create unrelated actions');

  const blocked = await jsonRequest('/api/home/simulate', {
    method: 'POST', body: JSON.stringify({ scenario: 'blocked_unlock' }),
  });
  assert.equal(blocked.decision, 'denied');
  assert(blocked.agent.forbidden.includes('door.unlock'));
  const blockedState = await jsonRequest('/api/home');
  const blockedAudit = await jsonRequest('/api/audit');
  const blockedAcceptance = await rejectedJsonRequest('/api/home/accept', 400, {
    method: 'POST', body: JSON.stringify({ scenario: 'blocked_unlock' }),
  });
  assert.match(blockedAcceptance.error, /actionable home suggestion/i);
  assert.deepEqual(await jsonRequest('/api/home'), blockedState, 'a denied unlock cannot be accepted or mutate Home state');
  assert.deepEqual(await jsonRequest('/api/audit'), blockedAudit, 'a rejected unlock acceptance must not append approval evidence');
  assert(blockedAudit.events.some(
    (event) => event.action === 'Action denied'
      && event.permission === 'DENIED'
      && /door\.unlock/i.test(event.result),
  ), 'the audit trail must retain structured DENIED evidence for the blocked unlock');

  const explanation = await jsonRequest('/api/home/explain', { method: 'POST', body: '{}' });
  assert.equal(explanation.mode, 'local_fallback');
  assert.match(explanation.fallbackReason, /disabled/i);

  const initialized = await mcpRequest(1, 'initialize', {
    protocolVersion: '2025-11-25',
    capabilities: {},
    clientInfo: { name: 'sharman-home-e2e', version: '1.0.0' },
  });
  assert.equal(initialized.result.serverInfo.name, 'sharman-local-supervisor');
  const tools = await mcpRequest(2, 'tools/list', {});
  const toolNames = new Set(tools.result.tools.map((tool) => tool.name));
  for (const name of [
    'sharman_get_tasks', 'sharman_create_task', 'sharman_get_reminders', 'sharman_create_reminder',
    'sharman_search_inbox', 'sharman_get_action_items', 'sharman_home_get_status',
    'sharman_home_handle_ring_event', 'sharman_home_explain_with_bedrock',
  ]) {
    assert(toolNames.has(name), `MCP discovery is missing ${name}`);
  }
  const invalidToolInput = await mcpRequest(3, 'tools/call', {
    name: 'sharman_home_handle_ring_event',
    arguments: { scenario: 'unknown_scenario' },
  });
  assert.equal(invalidToolInput.result.isError, true, 'invalid MCP tool input must return a structured tool error');
  assert.match(invalidToolInput.result.content[0].text, /invalid|scenario|enum/i);
  const invalidToolRequest = await mcpRequest(4, 'tools/call', {});
  assert.equal(invalidToolRequest.error.code, -32602, 'an invalid tools/call envelope must return JSON-RPC Invalid params');
  const healthyAfterMcpError = await mcpRequest(5, 'tools/list', {});
  assert(Array.isArray(healthyAfterMcpError.result.tools), 'the MCP bridge must remain healthy after invalid tool input');

  // Verify the Alexa+ simulator routes judge-facing commands through the real local MCP endpoint.
  const alexaTasks = await jsonRequest('/api/assistant/chat', {
    method: 'POST',
    body: JSON.stringify({ message: 'What tasks do I have today?', surface: 'alexa' }),
  });
  assert.equal(alexaTasks.tool, 'sharman_get_tasks');
  assert.equal(alexaTasks.permission, 'READ_ONLY');
  assert.equal(alexaTasks.transport, 'streamable_http_mcp');
  assert.deepEqual(alexaTasks.mcpTools, ['sharman_get_tasks']);
  assert(Array.isArray(alexaTasks.result.tasks));

  const reminderRequestedAt = Date.now();
  const alexaReminder = await jsonRequest('/api/assistant/chat', {
    method: 'POST',
    body: JSON.stringify({ message: 'Remind me in 2 minutes to test SHARMAN.', surface: 'alexa' }),
  });
  assert.equal(alexaReminder.tool, 'sharman_create_reminder');
  assert.equal(alexaReminder.permission, 'WRITE_APPROVED');
  assert.equal(alexaReminder.transport, 'streamable_http_mcp');
  assert.deepEqual(alexaReminder.mcpTools, ['sharman_create_reminder']);
  assert.equal(alexaReminder.result.reminder.title, 'test SHARMAN');
  const relativeDelay = new Date(alexaReminder.result.reminder.remindAt).getTime() - reminderRequestedAt;
  assert(relativeDelay >= 115_000 && relativeDelay <= 125_000, 'the two-minute reminder must preserve the requested relative time');

  const alexaAttention = await jsonRequest('/api/assistant/chat', {
    method: 'POST',
    body: JSON.stringify({ message: 'What needs my attention today?', surface: 'alexa' }),
  });
  assert.equal(alexaAttention.tool, 'DailyPlannerAgent');
  assert.equal(alexaAttention.permission, 'READ_ONLY');
  assert.equal(alexaAttention.transport, 'streamable_http_mcp');
  assert.deepEqual(alexaAttention.mcpTools, ['sharman_get_action_items', 'sharman_get_tasks', 'sharman_get_reminders']);
  assert.equal(alexaAttention.result.delegation.stepsUsed, 6);
  assert.equal(alexaAttention.result.delegation.status, 'terminated');

  const alexaLeetCode = await jsonRequest('/api/assistant/chat', {
    method: 'POST',
    body: JSON.stringify({ message: "Open today's LeetCode.", surface: 'alexa' }),
  });
  assert.equal(alexaLeetCode.tool, 'sharman_open_allowed_url');
  assert.equal(alexaLeetCode.permission, 'LOW_RISK');
  assert.equal(alexaLeetCode.transport, 'streamable_http_mcp');
  assert.deepEqual(alexaLeetCode.mcpTools, ['sharman_open_allowed_url']);
  assert.equal(alexaLeetCode.result.allowed, true);
  assert.equal(alexaLeetCode.result.requiresLocalUserConfirmation, true);

  const mcpLeetcode = await mcpRequest(10, 'tools/call', {
    name: 'sharman_open_allowed_url',
    arguments: { url: 'https://leetcode.com/problemset/' },
  });
  assert.equal(mcpLeetcode.result.structuredContent.allowed, true);

  const deniedUrl = await mcpRequest(11, 'tools/call', {
    name: 'sharman_open_allowed_url', arguments: { url: 'https://leetcode.com.evil.test/problems/x/' },
  });
  assert.equal(deniedUrl.result.structuredContent.allowed, false);
  const unknownTool = await mcpRequest(12, 'tools/call', { name: 'system.exec', arguments: { command: 'whoami' } });
  assert(unknownTool.error || unknownTool.result?.isError, 'unknown execution tools must be rejected');
  const beforeInvalidReminder = await jsonRequest('/api/reminders');
  const invalidReminder = await mcpRequest(13, 'tools/call', {
    name: 'sharman_assistant_request', arguments: { message: 'Remind me in 2000 minutes to test SHARMAN' },
  });
  assert.equal(invalidReminder.result.isError, true);
  assert.deepEqual(await jsonRequest('/api/reminders'), beforeInvalidReminder, 'an invalid requested time must not create a reminder');

  await stopBridge();
  await startBridge();
  const recovered = await jsonRequest('/api/home');
  assert.equal(recovered.home.events.length, 3, 'home events must survive a bridge restart');
  assert.equal(recovered.home.lastWorkflow.scenario, 'blocked_unlock');
  const status = await jsonRequest('/api/status');
  assert.equal(status.counts.reminders, 2, 'the idempotent reminders must survive a bridge restart');
  const recoveredAudit = await jsonRequest('/api/audit');
  assert(recoveredAudit.events.some((event) => event.permission === 'DENIED'), 'DENIED evidence must survive a bridge restart');
  const recoveredReminders = await mcpRequest(6, 'tools/call', { name: 'sharman_get_reminders', arguments: {} });
  assert.equal(recoveredReminders.result.structuredContent.reminders.length, 2, 'MCP reminder reads must recover the approved reminders without duplication');

  await stopBridge();
  await startBridge(true);
  const localOnlyRest = await jsonRequest('/api/home/explain', { method: 'POST', body: '{}' });
  const localOnlyMcp = await mcpRequest(7, 'tools/call', { name: 'sharman_home_explain_with_bedrock', arguments: {} });
  assert.match(localOnlyRest.fallbackReason, /no AWS request/i);
  assert.match(localOnlyMcp.result.structuredContent.fallbackReason, /no AWS request/i);
  assert.equal(awsRequests, 0, 'Local Only must block AWS on both REST and MCP even when the adapter is enabled');

  await jsonRequest('/api/settings/processing-mode', { method: 'POST', body: JSON.stringify({ processingMode: 'hybrid' }) });
  const beforeUnavailableAws = await jsonRequest('/api/home');
  const unavailable = await jsonRequest('/api/home/explain', { method: 'POST', body: '{}' });
  assert.equal(unavailable.mode, 'local_fallback');
  assert.match(unavailable.fallbackReason, /unavailable/i);
  assert(awsRequests >= 1 && awsRequests <= 2, 'Hybrid must use the controlled SDK endpoint with a bounded retry count');
  assert.deepEqual(await jsonRequest('/api/home'), beforeUnavailableAws, 'unavailable AWS must not change the permission decision or Home workflow');

  console.log('SHARMAN Home E2E passed: invalid-input isolation, three scenarios, safe/idempotent approvals, DENIED audit evidence, MCP recovery, restart persistence, Local Only cloud exclusion, and controlled SDK unavailable fallback.');
} finally {
  await stopBridge();
  for (const session of awsSessions) session.destroy();
  await new Promise((resolve) => unavailableAws.close(resolve));
  const resolvedData = path.resolve(dataDirectory);
  assert(resolvedData.startsWith(`${path.resolve(tmpdir())}${path.sep}sharman-home-e2e-`));
  await rm(resolvedData, { recursive: true, force: true });
}
