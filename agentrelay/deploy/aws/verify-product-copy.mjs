import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '../..');
const base = new URL(process.argv[2]);
const local = base.hostname === '127.0.0.1';
assert(local ? base.protocol === 'http:' : base.origin === 'https://sh-62bba116286d48cfb495d1a3677193ba.ecs.ap-southeast-2.on.aws');
assert(base.pathname === '/' && !base.search && !base.hash && !base.username && !base.password);
const output = path.join(root, 'target/qa/product-copy-20261022');
await mkdir(output, { recursive: true });
const requireTools = createRequire(path.join(root, 'target/home-qa-tools/package.json'));
const { chromium } = requireTools('playwright');
const axeSource = await readFile(requireTools.resolve('axe-core/axe.min.js'), 'utf8');
const forbidden = /live validated locally|official API validation|validation passed on|live invocation verified|not currently verified|Browser Allowlist Verified|Idempotent Record|test SHARMAN|Public demo credentials:|not a physical-device claim|hardware origin requires separate live verification|hackathon|release candidate|judge rehearsal/i;
const views = [
  ['Assistant', '.assistant-view'],
  ['Home', '.home-view'],
  ['Action Inbox', '.mailbox-notice'],
  ['Tasks', '.tasks-view'],
  ['Activity', '.audit-table'],
  ['Connections', '.connection-grid'],
  ['Alexa+', '.alexa-page'],
];
const browser = await chromium.launch({ channel: 'chrome', headless: true });
const results = [];
const context = await browser.newContext({ viewport: { width: 1440, height: 1000 }, reducedMotion: 'reduce' });
const page = await context.newPage();
const errors = [];
const providerRequests = [];
page.on('pageerror', (error) => errors.push(error.message));
page.on('request', (request) => {
  const url = new URL(request.url());
  if (url.hostname === 'api.amazonvision.com' || url.pathname.startsWith('/api/ring/playground/')) providerRequests.push(url.pathname);
});
try {
  for (const width of [1440, 390, 320]) {
    await page.setViewportSize({ width, height: 1000 });
    await page.goto(base.href, { waitUntil: 'domcontentloaded', timeout: 120_000 });
    await page.getByText('MCP bridge online', { exact: true }).waitFor();
    for (const [label, selector] of views) {
      const button = page.locator('.sidebar nav').getByRole('button', { name: label, exact: true });
      const openMenu = page.getByRole('button', { name: 'Open menu', exact: true });
      if (await openMenu.isVisible() && !await page.locator('.sidebar').evaluate((element) => element.classList.contains('mobile-open'))) await openMenu.click();
      await button.click();
      await page.locator(selector).first().waitFor();
      if (label === 'Alexa+') await page.getByText(/Connected to SHARMAN · 11 tools available/).waitFor();
      if (label === 'Home') {
        const ring = page.locator('.ring-platform');
        await ring.getByText('No Ring devices connected', { exact: true }).waitFor();
        assert.equal(await ring.locator('.ring-connection').innerText(), 'Not connected');
        assert.equal(await ring.locator('dl, .ring-validation, button').count(), 0);
        for (const scenario of ['Expected delivery', 'Repeated visitor', 'Blocked unlock']) assert(await page.locator('.scenario-grid').getByRole('button', { name: new RegExp(scenario) }).isEnabled());
      }
      assert(!forbidden.test(await page.locator('body').innerText()), `${label} includes QA or release copy`);
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false, `${label} overflows at ${width}px`);
      assert.deepEqual(errors, []);
      await page.addScriptTag({ content: axeSource });
      const audit = await page.evaluate(async () => window.axe.run('main', { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21aa', 'wcag22aa'] } }));
      assert.deepEqual(audit.violations.map((item) => ({ id: item.id, targets: item.nodes.map((node) => node.target) })), [], `${label} accessibility`);
      const slug = label.toLowerCase().replace(/[^a-z]+/g, '-');
      await page.screenshot({ path: path.join(output, `${local ? 'local' : 'public'}-${slug}-${width}.png`), fullPage: true, timeout: 60_000 });
      results.push({ view: label, width, passed: true, accessibilityViolations: 0, overflow: false });
    }
  }
  assert.deepEqual(providerRequests, [], 'Unconfigured Ring must not request provider data');
} finally { await browser.close(); }
const report = { passed: true, target: base.origin, checkedAt: new Date().toISOString(), views: views.length, scans: results.length, results, providerRequests, browserContextReusedAcrossResponsiveWidths: true, writes: 'Navigation and reads only; no command/scenario/reminder actions' };
await writeFile(path.join(output, `${local ? 'local' : 'public'}-all-views.json`), JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify(report, null, 2));
