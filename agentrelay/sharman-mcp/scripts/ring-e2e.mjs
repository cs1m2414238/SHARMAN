import assert from 'node:assert/strict';
import { createHmac } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { ownedBridge, jsonRequest, mcpClient } from '../../scripts/release/runtime.mjs';
import { ROOT } from '../../scripts/release/checks.mjs';
import { validateAudit, validateDenial } from '../../scripts/release/contracts.mjs';

const directory = path.join(ROOT, 'target/qa/ring-integration');
await mkdir(directory, { recursive: true });
const report = { passed: false, provenance: 'locally signed synthetic fixtures', hardwareVerified: false, alexaSpeechDelivered: false, checks: [] };
let bridge;
const fixture = (id = 'fixture-human-event', subtype = 'human', source = 'fixture-ring-device') => ({
  meta: { version: '1.1', time: new Date().toISOString(), request_id: `request-${id}`, account_id: 'fixture-ring-account' },
  data: { id, type: 'motion_detected', attributes: { source, source_type: 'devices', timestamp: Date.now(), sub_type: subtype } },
});
const sign = (body) => `sha256=${createHmac('sha256', 'controlled-test-ring-key').update(body).digest('hex')}`;
const send = async (body, signature = sign(body)) => {
  const response = await fetch(`${bridge.base}/webhooks/ring`, { method: 'POST', headers: { 'content-type': 'application/json', 'X-Signature': signature }, body, signal: AbortSignal.timeout(10_000) });
  return { status: response.status, value: await response.json() };
};
const check = async (name, action) => { await action(); report.checks.push(name); console.log(`PASS ${name}`); };
try {
  bridge = await ownedBridge({ ringFixture: true });
  const mcp = mcpClient(bridge.base);
  await mcp.rpc('initialize', { protocolVersion: '2025-11-25', capabilities: {}, clientInfo: { name: 'ring-fixture-e2e', version: '1.0.0' } });
  await mcp.rpc('notifications/initialized', {}, true);
  await check('signature, binding and malformed-body rejection have no event side effects', async () => {
    const body = JSON.stringify(fixture());
    assert.equal((await send(body, `sha256=${'0'.repeat(64)}`)).status, 401);
    assert.equal((await send(JSON.stringify(fixture('wrong-device', 'human', 'other-device')))).status, 401);
    const wrongAccount = fixture('wrong-account'); wrongAccount.meta.account_id = 'other-account';
    assert.equal((await send(JSON.stringify(wrongAccount))).status, 401);
    assert.equal((await send('{broken-json')).status, 400);
    assert.equal((await send(' '.repeat(70_000))).status, 413);
    assert.equal((await jsonRequest(bridge.base, '/api/home')).home.events.length, 0);
  });
  const input = fixture();
  const raw = JSON.stringify(input, null, 2);
  let eventId;
  await check('raw signed event reaches bounded VisitorAgent and MCP-visible Alexa response', async () => {
    const response = await send(raw);
    assert.equal(response.status, 200); assert.equal(response.value.status, 'ingested');
    eventId = response.value.eventId;
    const status = await mcp.call('sharman_home_get_status');
    const workflow = status.home.lastWorkflow;
    assert.equal(status.source, 'Ring signed webhook');
    assert.equal(status.alexaSimulation, true);
    assert.equal(workflow.event.id, eventId);
    assert.equal(workflow.event.ring.ingestionMode, 'signed_webhook');
    assert.equal(workflow.agent.name, 'VisitorAgent');
    assert.equal(workflow.agent.permissionCeiling, 'LOW_RISK');
    assert(workflow.agent.forbidden.includes('door.unlock'));
    assert.match(workflow.alexaMessage, /no scheduled visit has been verified/i);
    assert(workflow.agent.terminatedAt);
  });
  await check('concurrent redelivery and restart preserve event identity without re-running workers', async () => {
    const before = await jsonRequest(bridge.base, '/api/home');
    const retry = structuredClone(input); retry.meta.request_id = 'new-delivery-request';
    const duplicates = await Promise.all(Array.from({ length: 6 }, () => send(JSON.stringify(retry))));
    assert(duplicates.every((result) => result.value.status === 'duplicate_ignored' && result.value.eventId === eventId));
    await bridge.restart();
    assert.equal((await send(raw)).value.status, 'duplicate_ignored');
    assert.deepEqual(await jsonRequest(bridge.base, '/api/home'), before);
  });
  await check('permission check for the recorded Ring event is denied and audit-correlated', async () => {
    const denied = await mcp.call('sharman_home_handle_ring_event', { scenario: 'blocked_unlock', eventId });
    validateDenial(denied);
    assert.equal(denied.event.source, 'ring');
    assert.equal(denied.event.ring.sourceEventId, input.data.id);
    validateAudit((await jsonRequest(bridge.base, '/api/audit')).events, denied);
    assert.equal((await jsonRequest(bridge.base, '/api/reminders')).reminders.length, 0);
  });
  await check('non-human motion records an event without creating a worker', async () => {
    const before = (await jsonRequest(bridge.base, '/api/home')).home;
    assert.equal((await send(JSON.stringify(fixture('vehicle-event', 'vehicle')))).status, 200);
    const after = (await jsonRequest(bridge.base, '/api/home')).home;
    assert.equal(after.agents.length, before.agents.length);
    assert.equal(after.events[0].kind, 'motion');
    assert.equal(after.lastWorkflow, null);
  });
  await check('local-test ingestion remains explicitly labelled', async () => {
    const result = await jsonRequest(bridge.base, '/api/ring/test-event', fixture('unsigned-local-test'));
    assert.equal(result.event.ring.ingestionMode, 'local_test');
    assert.equal(result.normalized.schema_version, 1);
    assert.equal(result.normalized.sequence_number, 3);
  });
  await check('owner-captured history and device status import without creating visitor detections', async () => {
    const captured = JSON.parse(await readFile(new URL('../test-fixtures/ring/event-history-on-demand.json', import.meta.url), 'utf8'));
    const before = (await jsonRequest(bridge.base, '/api/home')).home.agents.length;
    const imported = await jsonRequest(bridge.base, '/api/ring/playground/import', captured);
    assert.equal(imported.records.length, 9);
    assert(imported.records.every((record) => record.normalized.event_type === 'system_ping' && record.workflow === null));
    const sourceDeviceId = captured.data[0].relationships.source.data.id;
    const statusCapture = JSON.parse(await readFile(new URL('../test-fixtures/ring/device-status-online.json', import.meta.url), 'utf8'));
    const status = await jsonRequest(bridge.base, `/api/ring/playground/import?deviceId=${encodeURIComponent(sourceDeviceId)}`, statusCapture);
    assert.equal(status.normalized.payload.online, true);
    assert.equal(status.workflow, null);
    assert.equal((await jsonRequest(bridge.base, '/api/home')).home.agents.length, before);
  });
  await bridge.cleanup(); bridge = await ownedBridge();
  await check('unconfigured receivers and local-test ingestion are disabled by default', async () => {
    const status = await jsonRequest(bridge.base, '/api/ring/status');
    assert.equal(status.configured, false);
    assert.equal(status.playgroundConfigured, false);
    assert.equal(status.playground.connectionState, 'unconfigured');
    assert.equal('validation' in status, false, 'Product connection status must not contain a historical release receipt');
    assert.equal(status.exposure, 'local');
    assert.equal((await send(raw)).status, 503);
    const response = await fetch(`${bridge.base}/api/ring/test-event`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: raw });
    assert.equal(response.status, 404);
  });
  report.passed = true;
} catch (error) {
  report.error = error instanceof Error ? error.message : 'Ring E2E failed';
  process.exitCode = 1;
} finally {
  await bridge?.cleanup();
  await writeFile(path.join(directory, 'summary.json'), JSON.stringify(report, null, 2) + '\n');
}
console.log(`Ring E2E ${report.passed ? 'passed' : 'FAILED'}: ${report.checks.length} checks; local signed fixtures, no hardware or Alexa speech claim.`);
