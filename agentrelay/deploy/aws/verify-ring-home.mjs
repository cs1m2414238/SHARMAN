import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '../..');
const base = new URL(process.argv[2]);
const local = base.hostname === '127.0.0.1';
assert(local ? base.protocol === 'http:' : base.origin === 'https://sh-62bba116286d48cfb495d1a3677193ba.ecs.ap-southeast-2.on.aws');
assert(base.pathname === '/' && !base.search && !base.hash && !base.username && !base.password);
const output = path.resolve(process.env.SHARMAN_PUBLIC_QA_OUTPUT || path.join(root, 'target/qa/product-copy-20261022'));
await mkdir(output, { recursive: true });
const requireTools = createRequire(path.join(root, 'target/home-qa-tools/package.json'));
const { chromium } = requireTools('playwright');
const axeSource = await readFile(requireTools.resolve('axe-core/axe.min.js'), 'utf8');
const get = async (route) => {
  const response = await fetch(new URL(route, base), { signal: AbortSignal.timeout(15_000) });
  assert.equal(response.status, 200, `${route} must be available`);
  return response.json();
};
const before = (await get('/api/home')).home;
const ring = await get('/api/ring/status');
assert.equal(ring.exposure, 'public_demo');
assert.equal(ring.playgroundConfigured, false);
assert.equal(ring.configured, false);
assert.equal(ring.localTestEnabled, false);
assert.equal(ring.playground.connectionState, 'unconfigured');
assert.equal('validation' in ring, false, 'Historical QA receipts belong in documentation');
const browser = await chromium.launch({ channel: 'chrome', headless: true });
const results = [];
try {
  for (const width of [1440, 390, 320]) {
    const context = await browser.newContext({ viewport: { width, height: 1000 }, reducedMotion: 'reduce' });
    const page = await context.newPage();
    const errors = [];
    const providerRequests = [];
    page.on('pageerror', (error) => errors.push(error.message));
    page.on('request', (request) => {
      const url = new URL(request.url());
      if (url.hostname === 'api.amazonvision.com' || url.pathname.startsWith('/api/ring/playground/')) providerRequests.push(url.pathname);
    });
    await page.goto(new URL('/home', base).href, { waitUntil: 'domcontentloaded', timeout: 45_000 });
    await page.getByText('MCP bridge online', { exact: true }).waitFor();
    const panel = page.locator('.ring-platform');
    await panel.getByText('No Ring devices connected', { exact: true }).waitFor();
    assert.equal(await panel.locator('.ring-connection').innerText(), 'Not connected');
    assert.doesNotMatch(await panel.innerText(), /validated|validation passed|credentials|not loaded|not checked|October 2026/i);
    assert.equal(await panel.locator('.ring-validation').count(), 0);
    assert.equal(await panel.locator('dl').count(), 0);
    assert.equal(await panel.getByRole('button', { name: 'Refresh Ring data', exact: true }).count(), 0);
    assert.equal(await panel.locator('tbody tr').count(), 0);
    for (const label of ['Expected delivery', 'Repeated visitor', 'Blocked unlock']) {
      assert.equal(await page.locator('.scenario-grid').getByRole('button', { name: new RegExp(label) }).isEnabled(), true);
    }
    assert.match(await page.locator('.door-title').innerText(), /SHARMAN SAFETY SIMULATIONS/);
    assert.match(await page.locator('.simulation-disclosure').innerText(), /Alexa\+ speech.*simulated/);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
    assert.equal(await page.evaluate(() => document.querySelector('.ring-platform').getBoundingClientRect().top < document.querySelector('.door-card').getBoundingClientRect().top), true);
    assert.deepEqual(providerRequests, [], 'Unconfigured public panel must not request Ring data');
    assert.deepEqual(errors, []);
    await page.addScriptTag({ content: axeSource });
    const accessibility = await page.evaluate(async () => window.axe.run('.home-view', { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21aa', 'wcag22aa'] } }));
    assert.deepEqual(accessibility.violations.map((item) => item.id), []);
    await page.screenshot({ path: path.join(output, `${local ? 'local' : 'public'}-${width}.png`), fullPage: true });
    results.push({ width, passed: true, noOverflow: true, accessibilityViolations: 0, providerRequests, errors });
    await context.close();
  }
} finally { await browser.close(); }
assert.deepEqual((await get('/api/home')).home, before, 'Rendering the Ring panel must not alter Home state');
const report = { passed: true, target: base.origin, ring, results, verifiedAt: new Date().toISOString(), writes: 'No simulator, reminder, permission or Ring-provider actions; read-only UI verification' };
await writeFile(path.join(output, `${local ? 'local' : 'public'}-ring-home.json`), JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify(report, null, 2));
