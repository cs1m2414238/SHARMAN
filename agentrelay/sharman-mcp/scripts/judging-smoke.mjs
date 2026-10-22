import assert from 'node:assert/strict';
import { request as httpRequest } from 'node:http';
import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';

// Read-only by default. Opt in to disposable sample reminder/Home writes with
// --exercise-demo. Never point this harness at the personal Windows bridge.
const destination = process.argv[2];
if (!destination) throw new Error('Usage: node scripts/judging-smoke.mjs <base-url> [--exercise-demo] [--local-probes] [--report]');
const base = new URL(destination);
const local = ['localhost', '127.0.0.1', '[::1]'].includes(base.hostname);
assert(base.protocol === 'https:' || (local && base.protocol === 'http:'), 'Public validation requires HTTPS.');
assert(!base.username && !base.password && !base.search && !base.hash && base.pathname === '/', 'Supply only the service base URL.');
const exerciseDemo = process.argv.includes('--exercise-demo');
const localProbes = process.argv.includes('--local-probes');
assert(!localProbes || (local && base.protocol === 'http:'), 'Host-header probes are only for local HTTP validation.');
const checks = [];
let sequence = 0;

async function get(route, headers = {}) {
  return fetch(new URL(route, base), { headers, signal: AbortSignal.timeout(15_000) });
}

async function rpc(method, params = {}, notification = false) {
  const response = await fetch(new URL('/mcp', base), {
    method: 'POST',
    headers: {
      'content-type': 'application/json', accept: 'application/json, text/event-stream',
      origin: base.origin, 'mcp-protocol-version': '2025-11-25',
    },
    body: JSON.stringify({ jsonrpc: '2.0', ...(!notification && { id: ++sequence }), method, params }),
    signal: AbortSignal.timeout(15_000),
  });
  const raw = await response.text();
  assert(response.ok, `${method} HTTP status ${response.status}`);
  if (notification) return null;
  const event = raw.split(/\r?\n/).findLast((line) => line.startsWith('data:'));
  const envelope = JSON.parse(event ? event.slice(5).trim() : raw);
  assert(!envelope.error, `${method} returned a protocol error.`);
  return envelope.result;
}

async function call(name, args = {}) {
  const result = await rpc('tools/call', { name, arguments: args });
  assert(!result.isError, `${name} returned a tool error.`);
  assert(result.structuredContent, `${name} has no structured result.`);
  return result.structuredContent;
}

function localProbe(route, host) {
  return new Promise((resolve, reject) => {
    const request = httpRequest(new URL(route, base), { headers: { host }, timeout: 10_000 }, (response) => {
      response.resume();
      response.once('end', () => resolve(response.statusCode));
    });
    request.once('timeout', () => request.destroy(new Error('Host probe timed out.')));
    request.once('error', reject);
    request.end();
  });
}

// A restarted container can accept a TCP connection before Node is ready. Only
// the data-free GET health probe retries; tool calls/writes never auto-replay.
async function waitForHealth() {
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(new URL('/health', base), {
        signal: AbortSignal.timeout(Math.max(1, Math.min(2_000, deadline - Date.now()))),
      });
      if (response.status === 200) return response;
      await response.body?.cancel();
    } catch { /* A startup socket reset is safe to retry for this GET only. */ }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error('Health did not become ready within 15 seconds.');
}

const healthResponse = await waitForHealth();
assert.equal(healthResponse.status, 200);
const health = await healthResponse.json();
assert.equal(health.status, 'ok');
assert.equal(health.demoData, true);
const bootstrapStates = await Promise.all(Array.from({ length: 12 }, async () => {
  const response = await get('/api/status');
  assert.equal(response.status, 200, 'Concurrent cold-start status read failed.');
  return response.json();
}));
const status = bootstrapStates[0];
assert.equal(status.personalStorage, 'web_json', 'Refusing to exercise a personal/native service.');
assert.equal(status.processingMode, 'local_only');
assert.equal(status.aws.bedrock.configured, false, 'The public sample build must not invoke Bedrock.');
checks.push('health and sample-only/native-disconnected configuration');
assert(bootstrapStates.every((state) => state.personalStorage === 'web_json'));
checks.push('12 simultaneous first-load reads initialize memory without HTTP 500');

for (const route of ['/', '/alexa', '/home']) {
  const page = await get(route);
  assert.equal(page.status, 200);
  assert.match(page.headers.get('content-type') || '', /text\/html/);
  const html = await page.text();
  assert.match(html, /id="root"/);
  const asset = html.match(/src="([^\"]+\.js)"/);
  assert(asset, 'Production frontend script missing.');
  assert.equal((await get(asset[1])).status, 200);
}
checks.push('same-origin production web console, Alexa and Home routes/assets');
assert.equal((await get('/api/does-not-exist')).status, 404);
assert.equal((await get('/api/status', { origin: 'https://untrusted.example.invalid' })).status, 403);
checks.push('unknown API and cross-origin access rejected');
if (localProbes) {
  assert.equal(await localProbe('/health', '169.254.170.2'), 200);
  assert.equal(await localProbe('/api/status', 'untrusted.example.invalid'), 403);
  checks.push('internal hosting health probe allowed; unknown Host rejected');
}

const initialized = await rpc('initialize', {
  protocolVersion: '2025-11-25', capabilities: {}, clientInfo: { name: 'sharman-judging-smoke', version: '1.0' },
});
assert.equal(initialized.protocolVersion, '2025-11-25');
await rpc('notifications/initialized', {}, true);
const discovery = await rpc('tools/list');
const names = discovery.tools.map((tool) => tool.name);
assert.equal(names.length, 11);
assert(names.includes('sharman_assistant_request'));
assert(!names.includes('door.unlock') && !names.includes('system.exec'));
checks.push('real Streamable HTTP MCP initialize/notification/11-tool discovery');

const tasks = await call('sharman_assistant_request', { message: 'What tasks do I have today?' });
assert.equal(tasks.tool, 'sharman_get_tasks');
assert.equal(tasks.permission, 'READ_ONLY');
const plan = await call('sharman_assistant_request', { message: 'What needs my attention today?' });
assert.equal(plan.result.delegation.name, 'DailyPlannerAgent');
checks.push('supervisor task read and bounded DailyPlannerAgent delegation');
const browser = await call('sharman_assistant_request', { message: "Open today's LeetCode." });
assert.equal(browser.tool, 'sharman_open_allowed_url');
assert.equal(browser.permission, 'LOW_RISK');
assert.equal(browser.result.allowed, true);
assert.equal(browser.result.opened, false);
assert.equal(browser.result.requiresLocalUserConfirmation, true);
assert.match(browser.result.url, /^https:\/\/leetcode\.com\/(problemset\/|problems\/[a-z0-9-]+\/)$/);
assert.equal((await call('sharman_open_allowed_url', { url: 'https://untrusted.example.invalid/' })).allowed, false);
checks.push('approved LeetCode destination/fallback, explicit user click and rejected external URL');

if (exerciseDemo) {
  const response = await call('sharman_assistant_request', { message: 'Remind me in 2 minutes to test the judging deployment.' });
  assert.equal(response.permission, 'WRITE_APPROVED');
  const reminder = response.result.reminder;
  assert(reminder?.id && reminder.remindAt);
  const repeated = await call('sharman_create_reminder', { title: reminder.title, remindAt: reminder.remindAt, sourceActionId: null });
  assert.equal(repeated.reminder.id, reminder.id);
  assert((await call('sharman_get_reminders')).reminders.some((item) => item.id === reminder.id));
  const home = await call('sharman_home_handle_ring_event', { scenario: 'blocked_unlock' });
  assert.equal(home.decision, 'denied');
  assert.equal(home.agent.name, 'VisitorAgent');
  const audit = await (await get('/api/audit')).json();
  assert(audit.events.some((event) => event.permission === 'DENIED'));
  checks.push('explicit demo reminder, duplicate identity, VisitorAgent and audited door.unlock denial');
}

const report = {
  passed: true, target: base.origin, sampleDataOnly: true, exerciseDemo,
  toolCount: names.length, dailyChallengeResolved: browser.result.dailyResolved, checks, verifiedAt: new Date().toISOString(),
  limitations: ['No personal Gmail access', 'No Windows notification delivery', 'Demo state is shared and can reset when the container is replaced'],
};
if (process.argv.includes('--report')) {
  const reportFile = path.resolve('target/qa/aws-judging', exerciseDemo ? 'public-smoke.json' : 'public-postdeploy-smoke.json');
  await mkdir(path.dirname(reportFile), { recursive: true });
  await writeFile(reportFile, JSON.stringify(report, null, 2) + '\n');
}
console.log(JSON.stringify(report, null, 2));
