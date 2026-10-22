import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { ROOT } from '../../scripts/release/checks.mjs';
import { ownedBridge, jsonRequest, mcpClient } from '../../scripts/release/runtime.mjs';
import { validateAudit, validateDenial } from '../../scripts/release/contracts.mjs';

const metadataOnly = process.argv.includes('--metadata-only');
const directory = path.join(ROOT, 'target/qa', metadataOnly ? 'ring-playground-api-live' : 'ring-playground-live');
await mkdir(directory, { recursive: true });
const report = { passed: false, status: 'RUNNING', provenance: 'ring_developer_playground', hardwareVerified: false, alexaSpeechDelivered: false, checks: [], startedAt: new Date().toISOString() };
let bridge;
try {
  if (!process.env.RING_ACCESS_TOKEN || process.env.RING_ENVIRONMENT !== 'playground') {
    report.status = 'BLOCKED';
    report.reason = 'Set RING_ACCESS_TOKEN and RING_ENVIRONMENT=playground in the launching process. No token is accepted through chat or the web UI.';
  } else {
    bridge = await ownedBridge({ ringPlaygroundLive: true });
    const discovered = await jsonRequest(bridge.base, '/api/ring/playground/devices');
    assert(discovered.devices.length > 0, 'The Ring API returned no Playground devices.');
    report.checks.push('Official API device discovery'); report.deviceCount = discovered.devices.length;
    const deviceId = process.env.RING_DEVICE_ID || (discovered.devices.length === 1 ? discovered.devices[0].id : null);
    assert(deviceId, 'Set RING_DEVICE_ID to one of the discovered device IDs when several devices are available.');
    const snapshot = await jsonRequest(bridge.base, `/api/ring/playground/device?deviceId=${encodeURIComponent(deviceId)}`);
    assert.equal(snapshot.device.id, deviceId);
    assert.equal(typeof snapshot.status.online, 'boolean');
    assert(snapshot.status.resourceId && snapshot.status.responseAt);
    assert(Array.isArray(snapshot.events));
    assert(snapshot.events.every((event) => event.deviceId === deviceId));
    report.checks.push('Official API device status and event history normalized without inferring detections');
    const metadataHome = await jsonRequest(bridge.base, '/api/home');
    assert.equal(metadataHome.home.agents.length, 0);
    assert.equal(metadataHome.home.events.length, 0);
    report.checks.push('Read-only metadata panel leaves VisitorAgent and Home workflows unchanged');
    if (!metadataOnly) {
      const receipt = await jsonRequest(bridge.base, '/api/ring/playground/sync', { deviceId });
      assert.equal(receipt.status, 'ingested', 'Generate a Motion event in the official Playground before running this check.');
      assert.equal(receipt.normalized.event_type, 'motion_detected');
      assert(receipt.workflow, 'The latest event did not produce a bounded Motion workflow; select Motion in the Playground.');
      report.checks.push('Official API Motion metadata normalized and persisted');
      const mcp = mcpClient(bridge.base);
      await mcp.rpc('initialize', { protocolVersion: '2025-11-25', capabilities: {}, clientInfo: { name: 'sharman-ring-playground-live', version: '1.0.0' } });
      await mcp.rpc('notifications/initialized', {}, true);
      const result = await mcp.call('sharman_home_get_status');
      assert.equal(result.source, 'Ring Developer Playground');
      assert.equal(result.home.lastWorkflow.agent.name, 'VisitorAgent');
      assert.equal(result.home.lastWorkflow.agent.permissionCeiling, 'LOW_RISK');
      assert(result.home.lastWorkflow.alexaMessage);
      report.checks.push('MCP-visible VisitorAgent and prepared Alexa response');
      const denied = await mcp.call('sharman_home_handle_ring_event', { scenario: 'blocked_unlock', eventId: receipt.event.id });
      validateDenial(denied); validateAudit((await jsonRequest(bridge.base, '/api/audit')).events, denied);
      report.checks.push('Same-event HIGH_RISK request denied under LOW_RISK ceiling with correlated audit');
    }
    report.passed = true; report.status = 'PASS';
  }
} catch {
  report.status = 'FAIL';
  report.reason = `Live Playground verification failed. Check the local token and device selection${metadataOnly ? '' : ' and Motion event'}. No credentials or raw API payloads were recorded.`;
} finally {
  await bridge?.cleanup();
  report.finishedAt = new Date().toISOString();
  const name = `${report.startedAt.replace(/[:.]/g, '-')}.json`;
  await writeFile(path.join(directory, name), JSON.stringify(report, null, 2) + '\n');
  await writeFile(path.join(directory, 'summary.json'), JSON.stringify(report, null, 2) + '\n');
}
console.log(`Ring Developer Playground: ${report.status}; ${report.checks.length} verified checks. Hardware and Alexa speech are not claimed. Evidence: ${directory}`);
if (!report.passed) process.exitCode = 1;
