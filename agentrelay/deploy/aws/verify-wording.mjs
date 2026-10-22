import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '../..');
const base = new URL(process.argv[2]);
const local = base.hostname === '127.0.0.1';
assert(local ? base.protocol === 'http:' : base.origin === 'https://sh-62bba116286d48cfb495d1a3677193ba.ecs.ap-southeast-2.on.aws');
assert(base.pathname === '/' && !base.search && !base.hash && !base.username && !base.password);
const output = path.join(root, 'target/qa/web-copy-deployment-20261021');
await mkdir(output, { recursive: true });
const requireTools = createRequire(path.join(root, 'target/home-qa-tools/package.json'));
const { chromium } = requireTools('playwright');
const health = await fetch(new URL('/health', base), { signal: AbortSignal.timeout(15000) });
assert.equal(health.status, 200);
const index = await (await fetch(new URL('/home', base))).text();
const bundle = index.match(/src="(\/assets\/index-wording-[a-f0-9]{12}\.js)"/);
assert(bundle, 'The updated cache-busted entry bundle is not live');
const js = await (await fetch(new URL(bundle[1], base))).text();
assert(!/AMAZON HACKATHON DEMO|RING TRACK|Hackathon simulation/.test(js));
assert(js.includes('Simulated home events'));
const browser = await chromium.launch({ channel: 'chrome', headless: true });
const results = [];
try {
  for (const width of [1440, 390]) {
    const context = await browser.newContext({ viewport: { width, height: 1000 }, reducedMotion: 'reduce' });
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.goto(new URL('/home', base).href);
    await page.getByText('MCP bridge online', { exact: true }).waitFor();
    assert.equal(await page.locator('.demo-pill').count(), 0);
    assert.equal((await page.locator('.home-heading .kicker').innerText()).trim(), 'SHARMAN HOME');
    assert.doesNotMatch(await page.locator('body').innerText(), /amazon hackathon demo|ring track|hackathon simulation/i);
    assert.match(await page.locator('.simulation-disclosure').innerText(), /Simulated home events.*Ring events and Alexa\+ speech are simulated/);
    assert.match(await page.locator('.runtime-pill').getAttribute('title'), /No local LLM is connected/);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
    assert.deepEqual(errors, []);
    await page.screenshot({ path: path.join(output, `${local ? 'local' : 'public'}-${width}.png`), fullPage: true });
    results.push({ width, wordingPassed: true, disclosuresPreserved: true, overflow: false, errors });
    await context.close();
  }
} finally { await browser.close(); }
const report = { passed: true, target: base.origin, entryBundle: bundle[1], results, verifiedAt: new Date().toISOString(), writes: 'No simulator/reminder/permission actions; render-only checks' };
await writeFile(path.join(output, `${local ? 'local' : 'public'}-wording.json`), JSON.stringify(report, null, 2));
console.log(JSON.stringify(report, null, 2));
