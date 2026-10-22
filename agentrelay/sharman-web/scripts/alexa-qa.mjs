import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtemp, mkdir, readFile, writeFile, rm } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '../..');
const requireTools = createRequire(path.join(root, 'target/home-qa-tools/package.json'));
const { chromium } = requireTools('playwright');
const executable = process.env.SHARMAN_NATIVE_EXECUTABLE;
assert(executable && path.isAbsolute(executable), 'Set SHARMAN_NATIVE_EXECUTABLE to the rebuilt native executable.');
const temporary = await mkdtemp(path.join(tmpdir(), 'sharman-alexa-qa-'));
const evidence = path.resolve(process.env.SHARMAN_ALEXA_QA_OUTPUT || path.join(root, 'target/qa/alexa-mcp'));
await mkdir(evidence, { recursive: true });
const findings = [];
const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));
let browser;
let bridge;
let web;

async function stopBridge() {
  if (!bridge || bridge.exitCode !== null) return;
  const stopped = new Promise((resolve) => bridge.once('exit', resolve));
  bridge.kill();
  await stopped;
  bridge = null;
}

async function freePort() {
  const server = createServer();
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const port = server.address().port;
  await new Promise((resolve) => server.close(resolve));
  return port;
}

async function nativeSnapshot(directory) {
  return new Promise((resolve, reject) => {
    const child = spawn(executable, ['--personal-bridge'], { windowsHide: true, env: { ...process.env, SHARMAN_NATIVE_DATA_DIR: directory }, stdio: ['pipe', 'pipe', 'pipe'] });
    let output = '';
    child.stdout.on('data', (chunk) => { output += chunk; });
    child.stderr.resume();
    child.on('error', reject);
    child.on('exit', (code) => {
      try { assert.equal(code, 0); const value = JSON.parse(output); assert.equal(value.ok, true); resolve(value.result); } catch (error) { reject(error); }
    });
    child.stdin.end(JSON.stringify({ tool: 'snapshot' }));
  });
}

try {
  browser = await chromium.launch({ channel: 'chrome', headless: true });
  for (let run = 1; run <= 3; run++) {
    const directory = path.join(temporary, `run-${run}`);
    const nativeDirectory = path.join(directory, 'native');
    const port = await freePort();
    const bridgeUrl = `http://127.0.0.1:${port}`;
    const startBridge = async () => {
      bridge = spawn(process.execPath, ['dist/server.js'], {
        cwd: path.join(root, 'sharman-mcp'), windowsHide: true,
        env: { ...process.env, SHARMAN_MCP_HOST: '127.0.0.1', SHARMAN_MCP_PORT: String(port), SHARMAN_MCP_DATA_DIR: path.join(directory, 'web'), SHARMAN_NATIVE_DATA_DIR: nativeDirectory, SHARMAN_NATIVE_EXECUTABLE: executable, SHARMAN_AWS_BEDROCK_ENABLED: 'false' },
        stdio: ['ignore', 'pipe', 'pipe'],
      });
      let output = '';
      bridge.stdout.on('data', (chunk) => { output += chunk; });
      bridge.stderr.on('data', (chunk) => { output += chunk; });
      for (let attempt = 0; attempt < 200; attempt++) {
        assert.equal(bridge.exitCode, null, output);
        try { if ((await fetch(`${bridgeUrl}/health`, { signal: AbortSignal.timeout(500) })).ok) return; } catch {}
        await delay(100);
      }
      throw new Error(`Bridge did not start: ${output}`);
    };
    await startBridge();
    const json = async (route, body) => {
      const response = await fetch(`${bridgeUrl}${route}`, { method: body ? 'POST' : 'GET', headers: { 'content-type': 'application/json' }, body: body ? JSON.stringify(body) : undefined });
      const value = await response.json();
      assert(response.ok, JSON.stringify(value));
      return value;
    };
    const mcp = async (name, args) => {
      const response = await fetch(`${bridgeUrl}/mcp`, { method: 'POST', headers: { accept: 'application/json, text/event-stream', 'content-type': 'application/json', 'mcp-protocol-version': '2025-11-25' }, body: JSON.stringify({ jsonrpc: '2.0', id: 100, method: 'tools/call', params: { name, arguments: args } }) });
      const raw = await response.text();
      const data = raw.split(/\r?\n/).findLast((line) => line.startsWith('data:'));
      return JSON.parse(data ? data.slice(5).trim() : raw);
    };
    const seedTask = await mcp('sharman_create_task', { title: 'Amazon MCP acceptance task' });
    assert.equal(seedTask.result.isError, undefined);
    const nativeBefore = await nativeSnapshot(nativeDirectory);
    assert.equal(nativeBefore.tasks.length, 1);

    const methods = [];
    const dist = path.join(root, 'sharman-web/dist');
    web = createServer(async (request, response) => {
      try {
        const route = new URL(request.url, 'http://localhost').pathname;
        if (route.startsWith('/api/') || route === '/mcp') {
          const chunks = [];
          for await (const chunk of request) chunks.push(chunk);
          const body = Buffer.concat(chunks);
          if (route === '/mcp' && body.length) {
            const value = JSON.parse(body.toString());
            methods.push({ method: value.method, tool: value.params?.name ?? null });
          }
          const upstream = await fetch(`${bridgeUrl}${request.url}`, { method: request.method, headers: { 'content-type': 'application/json', accept: 'application/json, text/event-stream', 'mcp-protocol-version': '2025-11-25' }, body: request.method === 'GET' ? undefined : body });
          response.writeHead(upstream.status, { 'content-type': upstream.headers.get('content-type') || 'application/json' });
          response.end(await upstream.text());
          return;
        }
        const file = route.startsWith('/assets/') ? path.resolve(dist, `.${route}`) : path.join(dist, 'index.html');
        assert(file.startsWith(`${dist}${path.sep}`));
        response.writeHead(200, { 'content-type': file.endsWith('.js') ? 'text/javascript' : file.endsWith('.css') ? 'text/css' : 'text/html' });
        response.end(await readFile(file));
      } catch (error) { response.writeHead(502, { 'content-type': 'application/json' }); response.end(JSON.stringify({ error: String(error) })); }
    });
    await new Promise((resolve) => web.listen(0, '127.0.0.1', resolve));
    const page = await browser.newPage({ viewport: { width: 1440, height: 1000 }, reducedMotion: 'reduce' });
    const errors = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.goto(`http://127.0.0.1:${web.address().port}/alexa`);
    await page.getByText(/tools available/).waitFor();
    assert.deepEqual(methods.slice(0, 3).map((item) => item.method), ['initialize', 'notifications/initialized', 'tools/list']);

    await page.getByRole('button', { name: 'What tasks do I have today?', exact: true }).click();
    await page.getByText('Amazon MCP acceptance task', { exact: true }).waitFor();
    await page.getByRole('button', { name: 'What needs my attention today?', exact: true }).click();
    await page.getByText(/DailyPlannerAgent completed in 6\/8/).waitFor();
    const requestedAt = Date.now();
    await page.getByRole('button', { name: 'Remind me in 2 minutes to take a break.', exact: true }).click();
    await page.getByText('Delivery: Windows app', { exact: true }).waitFor();
    await page.getByRole('button', { name: 'Run again', exact: true }).waitFor();
    await page.screenshot({ path: path.join(evidence, `run-${run}-native-reminder.png`), fullPage: true });
    const nativeAfter = await nativeSnapshot(nativeDirectory);
    assert.equal(nativeAfter.reminders.length, 1);
    const reminder = nativeAfter.reminders[0];
    assert.equal(reminder.title, 'take a break');
    const delayMs = new Date(reminder.remindAt).getTime() - requestedAt;
    assert(delayMs >= 115_000 && delayMs <= 130_000, `Relative reminder drift: ${delayMs}`);
    const replay = await mcp('sharman_create_reminder', { title: reminder.title, remindAt: reminder.remindAt });
    assert(replay.result?.structuredContent, JSON.stringify(replay));
    assert.equal(replay.result.structuredContent.created, false);
    assert.equal((await nativeSnapshot(nativeDirectory)).reminders.length, 1);
    await page.getByRole('button', { name: "Open today's LeetCode.", exact: true }).click();
    await page.getByRole('link', { name: /Launch LeetCode/ }).waitFor();
    await page.getByRole('button', { name: 'Run again', exact: true }).waitFor();
    await page.screenshot({ path: path.join(evidence, `run-${run}-alexa.png`), fullPage: true });
    assert(methods.filter((item) => item.tool === 'sharman_assistant_request').length === 4);

    const expected = await json('/api/home/simulate', { scenario: 'expected_delivery' });
    assert.equal(expected.agent.name, 'VisitorAgent');
    assert.equal(expected.decision, 'allowed');
    assert(expected.alexaMessage.length > 0);
    const denied = await json('/api/home/simulate', { scenario: 'blocked_unlock' });
    assert.equal(denied.decision, 'denied');
    assert(denied.agent.forbidden.includes('door.unlock'));
    const audit = await json('/api/audit');
    assert(audit.events.some((item) => item.permission === 'DENIED'));
    const unknown = await mcp('system.exec', { command: 'whoami' });
    assert(unknown.error || unknown.result?.isError);
    await stopBridge();
    await startBridge();
    assert.equal((await nativeSnapshot(nativeDirectory)).reminders[0].id, reminder.id);
    assert.equal((await json('/api/home')).home.lastWorkflow.decision, 'denied');
    assert.equal((await json('/api/status')).personalStorage, 'native_sqlite');
    assert.deepEqual(errors, []);
    findings.push({ run, passed: true, protocolVersion: '2025-11-25', browserRequests: methods, nativeReminder: { id: reminder.id, status: reminder.status, storage: 'native_sqlite' }, restartPreserved: true, forbiddenActionDenied: true, windowsToastObserved: false });
    await page.close();
    await new Promise((resolve) => web.close(resolve));
    web = null;
    await stopBridge();
    console.log(`Alexa/native MCP acceptance run ${run}/3 passed.`);
  }
  await writeFile(path.join(evidence, 'summary.json'), JSON.stringify({ passed: true, at: new Date().toISOString(), runs: findings, limitation: 'Windows toast appearance requires the running desktop app and an owner-visible check; these isolated tests prove native persistence and MCP/browser transport.' }, null, 2));
} finally {
  await stopBridge();
  if (browser) await browser.close();
  if (web) await new Promise((resolve) => web.close(resolve));
  const resolved = path.resolve(temporary);
  assert(resolved.startsWith(`${path.resolve(tmpdir())}${path.sep}sharman-alexa-qa-`));
  await rm(resolved, { recursive: true, force: true });
}
