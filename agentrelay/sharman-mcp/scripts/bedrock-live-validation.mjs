// Opt-in live AWS acceptance. Uses synthetic Home facts and disposable local state.
// Run after both production builds: npm run validate:bedrock -- --live --ui
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { BedrockRuntimeClient, ConverseCommand } from '@aws-sdk/client-bedrock-runtime';
import { BedrockCareExplainer } from '../dist/bedrock.js';

assert(process.argv.includes('--live'), 'Pass --live explicitly; this makes a few small billable Bedrock calls.');
assert(!process.env.AWS_ACCESS_KEY_ID && !process.env.AWS_SECRET_ACCESS_KEY, 'Use the named temporary AWS profile, not static environment credentials.');
assert(!process.env.AWS_ENDPOINT_URL && !process.env.AWS_ENDPOINT_URL_BEDROCK_RUNTIME, 'Remove test endpoint overrides before live acceptance.');
process.env.AWS_PROFILE ||= 'sharman-amazon';
process.env.AWS_REGION = 'ap-southeast-2';
const modelId = 'global.amazon.nova-2-lite-v1:0';
const root = path.resolve(import.meta.dirname, '../..');
const evidence = path.join(root, 'target/qa/bedrock-live', new Date().toISOString().replace(/[:.]/g, '-'));
const directory = await mkdtemp(path.join(tmpdir(), 'sharman-bedrock-live-'));
const report = { passed: false, startedAt: new Date().toISOString(), profile: process.env.AWS_PROFILE, region: process.env.AWS_REGION, modelId, data: 'Synthetic Ring simulator only; no Gmail, native store or private source data', checks: [] };
let bridge;
let browser;
let baseUrl;
const client = new BedrockRuntimeClient({ region: process.env.AWS_REGION, maxAttempts: 1 });
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function request(route, body) {
  const response = await fetch(`${baseUrl}${route}`, {
    method: body === undefined ? 'GET' : 'POST',
    headers: { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(35_000),
  });
  assert(response.ok, `HTTP ${response.status}: ${route}`);
  return response.json();
}
async function mcp(method, params) {
  const response = await fetch(`${baseUrl}/mcp`, {
    method: 'POST', headers: { 'content-type': 'application/json', accept: 'application/json, text/event-stream', 'mcp-protocol-version': '2025-11-25' },
    body: JSON.stringify({ jsonrpc: '2.0', id: 1, method, params }), signal: AbortSignal.timeout(35_000),
  });
  assert(response.ok, `MCP HTTP ${response.status}`);
  const text = await response.text();
  const line = text.split(/\r?\n/).findLast((value) => value.startsWith('data:'));
  return JSON.parse(line ? line.slice(5).trim() : text);
}
async function startBridge(profile = process.env.AWS_PROFILE) {
  const probe = createServer();
  await new Promise((resolve) => probe.listen(0, '127.0.0.1', resolve));
  const port = probe.address().port;
  await new Promise((resolve) => probe.close(resolve));
  baseUrl = `http://127.0.0.1:${port}`;
  bridge = spawn(process.execPath, ['dist/server.js'], {
    cwd: path.join(root, 'sharman-mcp'), windowsHide: true, stdio: ['ignore', 'ignore', 'ignore'],
    env: { ...process.env, AWS_PROFILE: profile, AWS_EC2_METADATA_DISABLED: 'true', SHARMAN_AWS_BEDROCK_ENABLED: 'true', SHARMAN_BEDROCK_MODEL_ID: modelId, SHARMAN_MCP_HOST: '127.0.0.1', SHARMAN_MCP_PORT: String(port), SHARMAN_MCP_DATA_DIR: directory, SHARMAN_NATIVE_EXECUTABLE: '', SHARMAN_NATIVE_DATA_DIR: '', SHARMAN_WEB_DIST: path.join(root, 'sharman-web/dist') },
  });
  for (let attempt = 0; attempt < 200; attempt++) {
    assert.equal(bridge.exitCode, null, 'Isolated bridge exited during startup');
    try { if ((await fetch(`${baseUrl}/health`, { signal: AbortSignal.timeout(500) })).ok) return; } catch {}
    await delay(50);
  }
  throw new Error('Isolated bridge did not start');
}
async function stopBridge() {
  if (!bridge || bridge.exitCode !== null) return;
  const ended = once(bridge, 'exit');
  bridge.kill();
  await Promise.race([ended, delay(5_000)]);
  if (bridge.exitCode === null) { bridge.kill('SIGKILL'); await ended; }
  bridge = null;
}

try {
  await mkdir(evidence, { recursive: true });
  const tiny = await client.send(new ConverseCommand({
    modelId, system: [{ text: 'Return the requested JSON object only. No Markdown, code fences or explanation.' }],
    messages: [{ role: 'user', content: [{ text: 'Return exactly: {"status":"ok","service":"sharman-bedrock"}' }] }],
    inferenceConfig: { maxTokens: 80, temperature: 0 },
  }), { abortSignal: AbortSignal.timeout(30_000) });
  const raw = tiny.output?.message?.content?.map((block) => block.text || '').join('').trim();
  assert(raw, 'Nova returned no text');
  // Record a wrapper honestly; it is not equivalent to strict raw JSON.
  const fenced = /^```(?:json)?\s*([\s\S]*?)\s*```$/i.exec(raw);
  const parsed = JSON.parse(fenced ? fenced[1] : raw);
  assert.deepEqual(parsed, { status: 'ok', service: 'sharman-bedrock' });
  report.checks.push({ name: 'live_sdk_structured_prompt', passed: true, strictRawJson: !fenced, result: parsed, requestId: tiny.$metadata.requestId, usage: tiny.usage });
  console.log('Live Nova structured prompt passed.');

  await startBridge();
  assert.equal((await request('/api/aws/status')).connectionState, 'not_validated');
  await request('/api/home/simulate', { scenario: 'blocked_unlock' });
  const local = await request('/api/home/explain', {});
  const localMcp = await mcp('tools/call', { name: 'sharman_home_explain_with_bedrock', arguments: {} });
  assert.match(local.fallbackReason, /no AWS request/i);
  assert.match(localMcp.result.structuredContent.fallbackReason, /no AWS request/i);
  assert.equal((await request('/api/aws/status')).lastAttemptAt, null);
  report.checks.push({ name: 'local_only_rest_and_mcp', passed: true, awsAttemptRecorded: false });

  await request('/api/settings/processing-mode', { processingMode: 'cloud_allowed' });
  // Positive model-access proof uses an ordinary explanation. The denied-lock
  // case below independently proves that validated prose OR a rejected response
  // cannot authorize the forbidden action.
  await request('/api/home/simulate', { scenario: 'expected_delivery' });
  const before = await request('/api/home');
  const rest = await request('/api/home/explain', {});
  assert.equal(rest.mode, 'bedrock', `REST did not pass output validation: ${rest.fallbackReason}`);
  assert.equal(rest.modelId, modelId);
  assert.equal(rest.region, 'ap-southeast-2');
  const viaMcp = await mcp('tools/call', { name: 'sharman_home_explain_with_bedrock', arguments: {} });
  assert.equal(viaMcp.result.structuredContent.mode, 'bedrock', 'MCP did not return a validated live model explanation');
  assert.deepEqual(await request('/api/home'), before, 'Cloud output must not mutate the permission decision or workflow');
  const status = await request('/api/aws/status');
  assert.equal(status.connectionState, 'connected');
  assert(status.lastSuccessAt);
  const forbidden = await mcp('tools/call', { name: 'door.unlock', arguments: {} });
  assert(forbidden.error || forbidden.result?.isError, 'MCP must not expose door.unlock');
  const audit = await request('/api/audit');
  assert(audit.events.some((event) => event.permission === 'DENIED'));
  assert(audit.events.some((event) => event.tool === 'sharman_home_explain_with_bedrock' && /bedrock:/.test(event.result || event.resultSummary || event.summary || '')) || JSON.stringify(audit).includes('bedrock: grounded care explanation prepared'));
  report.checks.push({ name: 'live_sharman_rest_mcp_permission_boundary', passed: true, status, rest, mcp: viaMcp.result.structuredContent, homeDecision: before.home.lastWorkflow.decision, workflowUnchanged: true, doorUnlockExposed: false });
  console.log('Live SHARMAN REST/MCP explanation passed.');

  if (process.argv.includes('--ui')) {
    report.stage = 'connections_browser';
    const requireTools = createRequire(path.join(root, 'target/home-qa-tools/package.json'));
    const { chromium } = requireTools('playwright');
    browser = await chromium.launch({ headless: true, channel: 'chrome' });
    const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
    await page.goto(baseUrl);
    await page.getByRole('button', { name: 'Connections', exact: true }).click();
    const card = page.locator('.connection-grid article').filter({ has: page.getByRole('heading', { name: 'Amazon Bedrock', exact: true }) });
    await card.getByText('Connected', { exact: true }).waitFor();
    assert.match(await card.innerText(), /Nova 2 Lite.*live invocation verified/);
    await page.screenshot({ path: path.join(evidence, 'connections-live.png'), fullPage: true });
    report.checks.push({ name: 'connections_ui_live_status', passed: true, screenshot: 'connections-live.png' });
    await browser.close(); browser = null;
  }

  report.stage = 'denied_workflow';
  await request('/api/home/simulate', { scenario: 'blocked_unlock' });
  const deniedBefore = await request('/api/home');
  const deniedExplanation = await request('/api/home/explain', {});
  assert(['bedrock', 'local_fallback'].includes(deniedExplanation.mode));
  assert.match(deniedExplanation.text, /blocked|denied/i);
  if (deniedExplanation.mode === 'local_fallback') assert.match(deniedExplanation.fallbackReason, /validation/i, 'Do not count an unavailable model as output-validation evidence');
  assert.deepEqual(await request('/api/home'), deniedBefore);
  assert.equal(deniedBefore.home.lastWorkflow.decision, 'denied');
  assert(deniedBefore.home.lastWorkflow.agent.forbidden.includes('door.unlock'));
  report.checks.push({ name: 'live_denied_workflow_validation', passed: true, explanationMode: deniedExplanation.mode, fallbackReason: deniedExplanation.fallbackReason, permissionDecision: 'denied', workflowUnchanged: true });

  const invalid = new BedrockCareExplainer({ enabled: true, region: 'ap-southeast-2', modelId: 'sharman-invalid-validation-model' });
  const failed = await invalid.explain(deniedBefore.home.lastWorkflow, 'cloud_allowed');
  assert.equal(failed.mode, 'local_fallback');
  assert.equal(invalid.status().connectionState, 'unavailable');
  assert.equal(invalid.status().lastSuccessAt, null);
  assert.deepEqual(await request('/api/home'), deniedBefore);
  report.checks.push({ name: 'live_invalid_model_fallback', passed: true, status: invalid.status(), permissionDecisionUnchanged: true });

  await stopBridge();
  // Do not revoke/modify the owner's valid session. Isolate a missing profile.
  await startBridge('sharman-nonexistent-validation-profile');
  const missing = await request('/api/home/explain', {});
  assert.equal(missing.mode, 'local_fallback');
  assert.equal((await request('/api/aws/status')).connectionState, 'unavailable');
  assert.deepEqual(await request('/api/home'), deniedBefore);
  report.checks.push({ name: 'isolated_missing_credentials_and_restart', passed: true, fallback: true, workflowPreserved: true });
  report.passed = true;
} catch (error) {
  if (report.stage === 'connections_browser') console.error(error.stack);
  report.error = { code: error.name, message: error.name === 'AssertionError' || error.name === 'SyntaxError' ? error.message.slice(0, 500) : report.stage === 'connections_browser' ? 'Connections browser check failed; earlier AWS inference evidence is recorded separately.' : 'Live validation failed; inspect the AWS session/model access locally. Provider payload was not recorded.' };
  process.exitCode = 1;
} finally {
  client.destroy();
  if (browser) await browser.close();
  await stopBridge();
  report.finishedAt = new Date().toISOString();
  await writeFile(path.join(evidence, 'summary.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify({ passed: report.passed, checks: report.checks.map((check) => ({ name: check.name, passed: check.passed })), error: report.error, evidence }));
  const resolved = path.resolve(directory);
  assert(resolved.startsWith(`${path.resolve(tmpdir())}${path.sep}sharman-bedrock-live-`));
  await rm(resolved, { recursive: true, force: true });
}
