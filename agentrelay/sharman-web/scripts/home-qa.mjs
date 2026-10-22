// Run after both production builds:
// npm install --prefix target/home-qa-tools --no-save --no-audit --no-fund playwright axe-core
// node sharman-web/scripts/home-qa.mjs
import assert from 'node:assert/strict';
import { createHmac } from 'node:crypto';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtemp, mkdir, readFile, writeFile, rm } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const root = path.resolve(import.meta.dirname, '../..');
const requireTools = createRequire(path.join(root, 'target/home-qa-tools/package.json'));
const { chromium } = requireTools('playwright');
const axeSource = await readFile(requireTools.resolve('axe-core/axe.min.js'), 'utf8');
const outputDirectory = path.resolve(process.env.SHARMAN_HOME_QA_OUTPUT || path.join(root, 'target/qa/ring-home-ui-20261021'));
const dataDirectory = await mkdtemp(path.join(tmpdir(), 'sharman-home-ui-'));
await mkdir(outputDirectory, { recursive: true });
await writeFile(path.join(outputDirectory, 'summary.json'), JSON.stringify({ passed: false, state: 'running' }, null, 2));
let bridge;
let browser;
let online = false;
let heldRoute = null;
let loseResponseRoute = null;
let releaseRequest = null;
const requestCounts = new Map();
const findings = [];
const upstreamStatePath = path.join(dataDirectory, 'ring-upstream-state.json');
await writeFile(upstreamStatePath, JSON.stringify({ online: true, expired: false }));

const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const probe = createServer();
await new Promise((resolve) => probe.listen(0, '127.0.0.1', resolve));
const bridgePort = probe.address().port;
await new Promise((resolve) => probe.close(resolve));
const bridgeUrl = `http://127.0.0.1:${bridgePort}`;

async function startBridge() {
  // Exercise real server/UI routes using a mocked upstream API. The preload
  // stays in disposable QA storage and is never part of a production build.
  const fixtureModule = path.join(dataDirectory, 'ring-api-fixture.mjs');
  await writeFile(fixtureModule, `
import {readFile} from 'node:fs/promises';
const originalFetch = globalThis.fetch;
globalThis.fetch = async (input, options) => {
  const url = new URL(typeof input === 'string' ? input : input.url ?? String(input));
  if (url.origin !== 'https://api.amazonvision.com') return originalFetch(input, options);
  const state = JSON.parse(await readFile(${JSON.stringify(upstreamStatePath)}, 'utf8'));
  if (state.expired) return new Response('private-provider-auth-detail', {status:401});
  if (url.pathname === '/v1/devices') return new Response(JSON.stringify({data:[{type:'devices',id:'fixture-playground-device',attributes:{name:'Mock Playground camera'}}]}));
  if (url.pathname === '/v1/devices/fixture-playground-device/status') {
    const status = JSON.parse(await readFile(${JSON.stringify(path.join(root, 'sharman-mcp/test-fixtures/ring/device-status-online.json'))}, 'utf8'));
    status.data.attributes.online = state.online;
    return new Response(JSON.stringify(status));
  }
  if (url.pathname === '/v1/history/devices/fixture-playground-device/events' && url.searchParams.get('event_types')?.includes('on_demand')) {
    const history = JSON.parse(await readFile(${JSON.stringify(path.join(root, 'sharman-mcp/test-fixtures/ring/event-history-on-demand.json'))}, 'utf8'));
    for (const event of history.data) event.relationships.source.data.id = 'fixture-playground-device';
    return new Response(JSON.stringify(history));
  }
  if (url.pathname === '/v1/history/devices/fixture-playground-device/events') return new Response(JSON.stringify({data:[{type:'history-events',id:'fixture-playground-motion',attributes:{event_type:'motion',start:Date.now()},relationships:{source:{data:{type:'devices',id:'fixture-playground-device'}}}}]}));
  return new Response('{}', {status:404});
};
`);
  bridge = spawn(process.execPath, ['--import', pathToFileURL(fixtureModule).href, 'dist/server.js'], {
    cwd: path.join(root, 'sharman-mcp'),
    env: { ...process.env, RING_ACCESS_TOKEN: 'test-playground-access-token', RING_ENVIRONMENT: 'playground', SHARMAN_NATIVE_EXECUTABLE: '', SHARMAN_NATIVE_DATA_DIR: '', SHARMAN_MCP_HOST: '127.0.0.1', SHARMAN_MCP_PORT: String(bridgePort), SHARMAN_MCP_DATA_DIR: dataDirectory, SHARMAN_AWS_BEDROCK_ENABLED: 'false', SHARMAN_RING_TEST_EVENTS_ENABLED: 'true', SHARMAN_RING_SIGNING_KEY: 'controlled-test-ring-key', SHARMAN_RING_ACCOUNT_ID: 'fixture-ring-account', SHARMAN_RING_DEVICE_ID: 'fixture-ring-device' },
    windowsHide: true,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let output = '';
  bridge.stdout.on('data', (chunk) => { output += chunk; });
  bridge.stderr.on('data', (chunk) => { output += chunk; });
  for (let attempt = 0; attempt < 200; attempt++) {
    assert.equal(bridge.exitCode, null, output);
    try { if ((await fetch(`${bridgeUrl}/health`, { signal: AbortSignal.timeout(500) })).ok) return; } catch {}
    await delay(50);
  }
  throw new Error(`Bridge startup failed: ${output}`);
}

async function stopBridge() {
  if (!bridge || bridge.exitCode !== null) return;
  const exited = new Promise((resolve) => bridge.once('exit', resolve));
  bridge.kill();
  await exited;
  bridge = null;
}

const dist = path.join(root, 'sharman-web/dist');
const web = createServer(async (request, response) => {
  try {
    const route = new URL(request.url, 'http://localhost').pathname;
    if (route.startsWith('/api/')) {
      const key = `${request.method} ${route}`;
      requestCounts.set(key, (requestCounts.get(key) || 0) + 1);
      if (!online) {
        response.writeHead(503, { 'content-type': 'application/json' });
        response.end(JSON.stringify({ error: 'QA bridge offline' }));
        return;
      }
      if (heldRoute === route) await new Promise((resolve) => { releaseRequest = resolve; });
      const chunks = [];
      for await (const chunk of request) chunks.push(chunk);
      const result = await fetch(`${bridgeUrl}${request.url}`, {
        method: request.method,
        headers: { 'content-type': 'application/json' },
        body: request.method === 'GET' ? undefined : Buffer.concat(chunks),
        signal: AbortSignal.timeout(5_000),
      });
      if (loseResponseRoute === route) {
        loseResponseRoute = null;
        await result.text();
        // Return a gateway failure after the write commits. Closing an unused
        // socket can trigger Chrome's own transport retry before JavaScript
        // observes the failure, so an HTTP failure isolates application behavior.
        response.writeHead(502, { 'content-type': 'application/json' });
        response.end(JSON.stringify({ error: 'QA response lost after commit' }));
        return;
      }
      response.writeHead(result.status, { 'content-type': 'application/json' });
      response.end(await result.text());
      return;
    }
    const file = route.startsWith('/assets/') ? path.resolve(dist, `.${route}`) : path.join(dist, 'index.html');
    assert(file.startsWith(`${dist}${path.sep}`));
    const mime = file.endsWith('.js') ? 'text/javascript' : file.endsWith('.css') ? 'text/css' : 'text/html';
    response.writeHead(200, { 'content-type': mime });
    response.end(await readFile(file));
  } catch {
    if (!response.headersSent) response.writeHead(503, { 'content-type': 'application/json' });
    response.end(JSON.stringify({ error: 'QA bridge unavailable' }));
  }
});

async function auditHome(page, name) {
  assert.equal(await page.locator('.demo-pill').count(), 0, `${name}: no competition badge`);
  assert.equal((await page.locator('.home-heading .kicker').innerText()).trim(), 'SHARMAN HOME');
  assert.doesNotMatch(await page.locator('body').innerText(), /amazon hackathon demo|ring track|hackathon simulation/i);
  if (name.startsWith('ring-')) {
    await page.getByText('Ring event provenance', { exact: true }).waitFor();
    assert.match(await page.locator('.simulation-disclosure').innerText(), /Alexa\+ speech remains simulated/);
  } else {
    await page.getByText('SHARMAN safety simulations', { exact: true }).waitFor();
    assert.match(await page.locator('.simulation-disclosure').innerText(), /three safety scenarios and Alexa\+ speech are simulated/);
  }
  assert.match(await page.locator('.runtime-pill').getAttribute('title'), /No local LLM is connected/);
  await page.addScriptTag({ content: axeSource });
  const result = await page.evaluate(async () => window.axe.run('.home-view', {
    runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21aa', 'wcag22aa'] },
  }));
  // Bound each gradient by its lightest possible color per channel. This is
  // deliberately stricter than sampling one point and covers axe's gradient
  // color-contrast cases that require manual review.
  const contrast = await page.evaluate(() => {
    const parseColor = (value) => {
      const values = value.match(/[\d.]+/g)?.map(Number) || [];
      return [values[0] || 0, values[1] || 0, values[2] || 0, values[3] ?? 1];
    };
    const composite = (front, back) => back.map((channel, i) => front[i] * front[3] + channel * (1 - front[3]));
    const layers = (value) => {
      let depth = 0;
      let start = 0;
      const found = [];
      for (let i = 0; i < value.length; i++) {
        if (value[i] === '(') depth++;
        if (value[i] === ')') depth--;
        if (value[i] === ',' && depth === 0) { found.push(value.slice(start, i)); start = i + 1; }
      }
      return [...found, value.slice(start)];
    };
    const boundImages = (value, under) => layers(value).reverse().reduce((background, layer) => {
      const colors = [...layer.matchAll(/rgba?\([^)]+\)/g)].map(([color]) => parseColor(color));
      if (!colors.length) return background;
      const candidates = colors.map((color) => composite(color, background));
      if (layer.includes('transparent')) candidates.push(background);
      return background.map((_, i) => Math.max(...candidates.map((color) => color[i])));
    }, under);
    const luminance = (color) => color.map((channel) => {
      const value = channel / 255;
      return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
    }).reduce((total, channel, i) => total + channel * [0.2126, 0.7152, 0.0722][i], 0);
    const walker = document.createTreeWalker(document.querySelector('.home-view'), NodeFilter.SHOW_TEXT);
    const checked = new Set();
    const measurements = [];
    while (walker.nextNode()) {
      const element = walker.currentNode.parentElement;
      if (!walker.currentNode.textContent.trim() || checked.has(element) || element.closest('button:disabled') || !element.getClientRects().length) continue;
      checked.add(element);
      const ancestors = [];
      for (let parent = element; parent; parent = parent.parentElement) ancestors.unshift(parent);
      let background = [8, 10, 16];
      for (const parent of ancestors) {
        const style = getComputedStyle(parent);
        background = boundImages(style.backgroundImage, composite(parseColor(style.backgroundColor), background));
        const pseudo = getComputedStyle(parent, '::before');
        if (pseudo.content !== 'none' && pseudo.content !== 'normal') {
          background = boundImages(pseudo.backgroundImage, composite(parseColor(pseudo.backgroundColor), background));
        }
      }
      const style = getComputedStyle(element);
      const foreground = composite(parseColor(style.color), background);
      const ratio = (luminance(foreground) + 0.05) / (luminance(background) + 0.05);
      const large = parseFloat(style.fontSize) >= 24 || (parseFloat(style.fontSize) >= 18.66 && Number(style.fontWeight) >= 700);
      measurements.push({ text: walker.currentNode.textContent.trim().slice(0, 100), ratio: Number(ratio.toFixed(2)), minimum: large ? 3 : 4.5, foreground: style.color, backgroundBound: background.map((value) => Math.ceil(value)) });
    }
    return measurements;
  });
  findings.push({ name, violations: result.violations.map(({ id, nodes }) => ({ id, nodes: nodes.map(({ target, failureSummary }) => ({ target, failureSummary })) })), incomplete: result.incomplete.map(({ id, nodes }) => ({ id, targets: nodes.map(({ target }) => target) })), gradientContrast: contrast });
  await writeFile(path.join(outputDirectory, 'accessibility.json'), JSON.stringify(findings, null, 2));
  assert.deepEqual(result.violations.map(({ id, nodes }) => ({ id, targets: nodes.map(({ target }) => target) })), [], `${name}: Home accessibility violations`);
  assert(contrast.length > 0);
  assert.deepEqual(contrast.filter(({ ratio, minimum }) => ratio < minimum), [], `${name}: conservative text contrast bounds must meet AA`);
  const overflow = await page.locator('.home-view').evaluate((element) => element.scrollWidth > element.clientWidth);
  assert.equal(overflow, false, `${name}: Home content must not overflow`);
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({ path: path.join(outputDirectory, `${name}.png`), fullPage: true });
}

function hold(route) { heldRoute = route; }
function release() { heldRoute = null; releaseRequest?.(); releaseRequest = null; }

try {
  await startBridge();
  await new Promise((resolve) => web.listen(0, '127.0.0.1', resolve));
  browser = await chromium.launch({ channel: 'chrome', headless: true });
  const context = await browser.newContext({ viewport: { width: 1440, height: 1100 }, reducedMotion: 'reduce', timezoneId: 'Asia/Kolkata', locale: 'en-IN' });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto(`http://127.0.0.1:${web.address().port}/home`);
  const home = page.locator('.home-view');
  const scenarios = home.locator('.scenario-grid button');
  const retry = home.getByRole('button', { name: 'Retry connection' });
  await home.getByRole('alert').waitFor();
  assert.equal(await scenarios.evaluateAll((buttons) => buttons.every((button) => button.disabled)), true);
  assert.equal(await home.getByText('Bridge unavailable', { exact: true }).count(), 1);
  assert.equal(await home.getByText('Safe & monitored', { exact: true }).count(), 0);
  await auditHome(page, 'offline-desktop');

  online = true;
  await retry.click();
  await page.waitForFunction(() => !document.querySelector('.scenario-grid button').disabled);
  await page.getByText('MCP bridge online', { exact: true }).waitFor();
  await auditHome(page, 'ready-desktop');

  const expectedButton = home.getByRole('button', { name: 'Expected delivery: Event matches the 4–6 PM care window' });
  await page.keyboard.press('Tab');
  await expectedButton.focus();
  assert.equal(await expectedButton.evaluate((button) => getComputedStyle(button).outlineStyle), 'solid');
  await page.keyboard.press('Enter');
  const approve = home.getByRole('button', { name: 'Yes, remind me' });
  await approve.waitFor();
  await page.waitForFunction(() => document.querySelector('.home-view').getAttribute('aria-busy') === 'false');
  const expectedTitle = await home.locator('.event-main h2').textContent();
  await auditHome(page, 'expected-desktop');

  hold('/api/home/accept');
  await approve.evaluate((button) => { button.click(); button.click(); });
  await home.getByRole('button', { name: 'Confirming…' }).waitFor();
  assert.equal(await home.locator('button').evaluateAll((buttons) => buttons.every((button) => button.disabled)), true);
  assert.equal(requestCounts.get('POST /api/home/accept'), 1, 'same-frame duplicate approval must be suppressed');
  release();
  await home.getByRole('button', { name: 'Approved' }).waitFor();
  assert.equal((await (await fetch(`${bridgeUrl}/api/reminders`)).json()).reminders.length, 1);

  online = false;
  await home.getByRole('button', { name: 'Repeated visitor: Two events, no scheduled visitor' }).click();
  await home.getByRole('alert').waitFor();
  await page.locator('.bridge-pill').filter({ hasText: 'Bridge unavailable' }).waitFor();
  assert.equal(await home.locator('.event-main h2').textContent(), expectedTitle, 'offline failure must preserve the loaded event');
  assert.match(await home.getByRole('alert').textContent(), /could not be confirmed/);
  const failedWriteCount = requestCounts.get('POST /api/home/simulate');
  await retry.click();
  await page.waitForFunction(() => document.querySelector('.home-view').getAttribute('aria-busy') === 'false');
  assert.equal(requestCounts.get('POST /api/home/simulate'), failedWriteCount, 'reconnect must never replay a write');
  await auditHome(page, 'offline-preserved-desktop');

  online = true;
  hold('/api/home');
  await retry.click();
  await home.getByRole('button', { name: 'Reconnecting…' }).waitFor();
  await page.waitForFunction(() => document.querySelector('.home-view').getAttribute('aria-busy') === 'false', null, { timeout: 15_000 });
  assert.equal(await home.getByRole('alert').count(), 1, 'a hung read must time out into recoverable offline state');
  release();
  await stopBridge();
  await startBridge();
  await retry.click();
  await page.waitForFunction(() => !document.querySelector('.scenario-grid button').disabled);
  assert.equal(await home.getByRole('alert').count(), 0);
  assert.equal(await home.locator('.event-main h2').textContent(), expectedTitle, 'restart must recover the recorded workflow');
  assert.equal((await (await fetch(`${bridgeUrl}/api/reminders`)).json()).reminders.length, 1);
  await auditHome(page, 'reconnected-desktop');

  await home.getByRole('button', { name: 'Repeated visitor: Two events, no scheduled visitor' }).click();
  await home.getByRole('button', { name: 'Review action' }).waitFor();
  await page.waitForFunction(() => document.querySelector('.home-view').getAttribute('aria-busy') === 'false');
  await auditHome(page, 'attention-desktop');
  await home.getByRole('button', { name: 'Blocked unlock: Beyond the agent’s permissions' }).click();
  await home.locator('.alexa-home-card.denied').waitFor();
  await page.waitForFunction(() => document.querySelector('.home-view').getAttribute('aria-busy') === 'false');
  assert.equal(await home.getByRole('button', { name: 'Yes, remind me' }).count(), 0);
  await home.getByRole('button', { name: 'View local explanation' }).click();
  await home.locator('.aws-explanation').waitFor();
  await auditHome(page, 'denied-desktop');

  await page.setViewportSize({ width: 390, height: 844 });
  await auditHome(page, 'denied-mobile');
  await page.setViewportSize({ width: 320, height: 780 });
  await auditHome(page, 'denied-narrow-mobile');
  await home.getByRole('button', { name: 'Expected delivery: Event matches the 4–6 PM care window' }).click();
  await home.getByRole('button', { name: 'Yes, remind me' }).waitFor();
  await page.waitForFunction(() => document.querySelector('.home-view').getAttribute('aria-busy') === 'false');
  await auditHome(page, 'expected-narrow-mobile');

  // The server can finish a write even when its response is lost. The client
  // must report uncertainty and only retry when the user explicitly asks.
  loseResponseRoute = '/api/home/accept';
  await home.getByRole('button', { name: 'Yes, remind me' }).click();
  await home.getByRole('alert').waitFor();
  assert.match(await home.getByRole('alert').textContent(), /approval request could not be confirmed/i);
  assert.equal(requestCounts.get('POST /api/home/accept'), 2, 'lost approval responses must not cause automatic retries');
  assert.equal((await (await fetch(`${bridgeUrl}/api/reminders`)).json()).reminders.length, 2, 'the completed write must remain recorded even though its response was lost');
  await auditHome(page, 'approval-unconfirmed-mobile');
  await retry.click();
  await page.waitForFunction(() => !document.querySelector('.scenario-grid button').disabled);
  await home.getByRole('button', { name: 'Yes, remind me' }).click();
  await home.getByRole('button', { name: 'Approved' }).waitFor();
  assert.equal((await (await fetch(`${bridgeUrl}/api/reminders`)).json()).reminders.length, 2, 'an explicit retry must not duplicate the completed reminder');

  hold('/api/home/reset');
  await home.getByRole('button', { name: 'Reset SHARMAN Home demo' }).click();
  await home.getByRole('button', { name: 'Resetting SHARMAN Home demo' }).waitFor();
  assert.equal(await home.locator('button').evaluateAll((buttons) => buttons.every((button) => button.disabled)), true);
  release();
  await home.getByText('No activity yet').waitFor();
  await page.setViewportSize({ width: 1440, height: 1100 });
  await home.getByRole('button', { name: 'Send Ring-format test event' }).click();
  await home.getByText('Local test event.', { exact: false }).waitFor();
  await page.waitForFunction(() => document.querySelector('.home-view').getAttribute('aria-busy') === 'false');
  await auditHome(page, 'ring-local-test-desktop');
  const now = Date.now();
  const signedPayload = { meta: { version: '1.1', time: new Date(now).toISOString(), request_id: 'fixture-ui-request', account_id: 'fixture-ring-account' }, data: { id: 'fixture-ui-event', type: 'button_press', attributes: { source: 'fixture-ring-device', source_type: 'devices', timestamp: now } } };
  const raw = JSON.stringify(signedPayload, null, 2);
  const signedResponse = await fetch(`${bridgeUrl}/webhooks/ring`, { method: 'POST', headers: { 'content-type': 'application/json', 'X-Signature': `sha256=${createHmac('sha256', 'controlled-test-ring-key').update(raw).digest('hex')}` }, body: raw });
  assert.equal(signedResponse.status, 200);
  await home.getByRole('button', { name: 'Refresh events', exact: true }).click();
  await home.getByText('SIGNED RING WEBHOOK', { exact: false }).waitFor();
  await page.waitForFunction(() => document.querySelector('.home-view').getAttribute('aria-busy') === 'false');
  await auditHome(page, 'ring-signed-desktop');
  await home.getByRole('button', { name: 'Check unlock permission' }).click();
  await home.locator('.alexa-home-card.denied').waitFor();
  await page.waitForFunction(() => document.querySelector('.home-view').getAttribute('aria-busy') === 'false');
  await auditHome(page, 'ring-denied-desktop');
  await page.setViewportSize({ width: 320, height: 780 });
  await auditHome(page, 'ring-denied-narrow-mobile');
  await page.setViewportSize({ width: 1440, height: 1100 });
  const homeBeforeRead = await (await fetch(`${bridgeUrl}/api/home`)).json();
  await home.getByRole('button', { name: 'Refresh Ring data', exact: true }).click();
  await home.getByRole('combobox', { name: 'Ring Playground device' }).waitFor();
  await page.waitForFunction(() => document.querySelector('.home-view').getAttribute('aria-busy') === 'false');
  const platform = home.locator('.ring-platform');
  assert.equal(await platform.locator('.ring-connection').innerText(), 'Connected');
  assert.match(await platform.innerText(), /Device — Online/);
  assert.match(await platform.innerText(), /2026-10-21 16:35:31 UTC/);
  assert.match(await platform.innerText(), /2026-10-21 16:40:47 UTC/);
  assert.equal(await platform.locator('tbody tr').count(), 9);
  assert.match(await platform.innerText(), /on_demand/);
  assert.doesNotMatch(await platform.innerText(), /visitor detected|motion detected/i);
  assert.deepEqual((await (await fetch(`${bridgeUrl}/api/home`)).json()).home, homeBeforeRead.home, 'viewing Ring API metadata never starts a workflow or changes safety state');
  await auditHome(page, 'ring-playground-status-history-mocked-desktop');
  await page.setViewportSize({ width: 320, height: 780 });
  await auditHome(page, 'ring-playground-status-history-mocked-narrow-mobile');
  await writeFile(upstreamStatePath, JSON.stringify({ online: false, expired: false }));
  await platform.getByRole('button', { name: 'Refresh device status and history' }).click();
  await page.waitForFunction(() => document.querySelector('.home-view').getAttribute('aria-busy') === 'false');
  assert.match(await platform.innerText(), /Device — Offline/);
  assert.equal(await platform.locator('.ring-connection').innerText(), 'Connected', 'API connectivity is separate from device availability');
  await auditHome(page, 'ring-playground-device-offline-mocked-mobile');
  await writeFile(upstreamStatePath, JSON.stringify({ online: true, expired: true }));
  await platform.getByRole('button', { name: 'Refresh device status and history' }).click();
  await platform.getByRole('alert').waitFor();
  await page.waitForFunction(() => document.querySelector('.home-view').getAttribute('aria-busy') === 'false');
  assert.equal(await platform.locator('.ring-connection').innerText(), 'Unavailable');
  assert.match(await platform.innerText(), /last successful response/);
  assert.doesNotMatch(await platform.innerText(), /private-provider-auth-detail|test-playground-access-token/);
  assert.equal(await expectedButton.isEnabled(), true, 'Ring authentication failure never disables safety simulations');
  await auditHome(page, 'ring-playground-expired-token-mocked-mobile');
  await writeFile(upstreamStatePath, JSON.stringify({ online: true, expired: false }));
  await platform.getByRole('button', { name: 'Refresh device status and history' }).click();
  await page.waitForFunction(() => document.querySelector('.home-view').getAttribute('aria-busy') === 'false');
  assert.equal(await platform.getByRole('alert').count(), 0);
  await page.setViewportSize({ width: 1440, height: 1100 });
  await home.getByRole('button', { name: 'Import Playground Motion event' }).click();
  await home.getByText('Ring Developer Playground event history.', { exact: false }).waitFor();
  await page.waitForFunction(() => document.querySelector('.home-view').getAttribute('aria-busy') === 'false');
  assert.match(await home.locator('.alexa-home-card').innerText(), /no person.*identified/i);
  await auditHome(page, 'ring-playground-mocked-desktop');
  await page.setViewportSize({ width: 320, height: 780 });
  await auditHome(page, 'ring-playground-mocked-narrow-mobile');
  await writeFile(path.join(outputDirectory, 'mock-provenance.json'), JSON.stringify({ playgroundApi: 'Mocked official response shapes; no external Ring request.', hardwareVerified: false, alexaSpeechDelivered: false }, null, 2));
  assert.deepEqual(errors, [], 'Home must have no uncaught browser errors');
  await writeFile(path.join(outputDirectory, 'summary.json'), JSON.stringify({ passed: true, checks: ['cold offline', 'manual reconnect', 'keyboard and visible focus', 'same-frame approval deduplication', 'pending operation exclusion', 'preserved offline state', 'read timeout', 'restart recovery', 'no write replay', 'lost write response and idempotent explicit retry', 'permission denial', 'desktop/mobile accessibility'], requestCounts: Object.fromEntries(requestCounts), audits: findings.length }, null, 2));
  console.log(`HOME-013 UI QA passed: ${findings.length} accessibility scans plus offline, timeout, keyboard, concurrency and restart recovery. Evidence: ${outputDirectory}`);
} finally {
  release();
  await browser?.close();
  web.closeAllConnections();
  await new Promise((resolve) => web.close(resolve));
  await stopBridge();
  const resolvedData = path.resolve(dataDirectory);
  assert(resolvedData.startsWith(`${path.resolve(tmpdir())}${path.sep}sharman-home-ui-`));
  await rm(resolvedData, { recursive: true, force: true });
}
