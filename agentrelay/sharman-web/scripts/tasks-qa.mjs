// Isolated production UI/backend check. Never touches native or personal data.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '../..');
const requireTools = createRequire(path.join(root, 'target/home-qa-tools/package.json'));
const { chromium } = requireTools('playwright');
const axeSource = await readFile(requireTools.resolve('axe-core/axe.min.js'), 'utf8');
const data = await mkdtemp(path.join(tmpdir(), 'sharman-tasks-ui-'));
const output = path.join(root, 'target/qa/tasks-ui');
await mkdir(output, { recursive: true });
const probe = createServer();
await new Promise((resolve) => probe.listen(0, '127.0.0.1', resolve));
const port = probe.address().port;
await new Promise((resolve) => probe.close(resolve));
const base = `http://127.0.0.1:${port}`;
const server = spawn(process.execPath, ['dist/server.js'], {
  cwd: path.join(root, 'sharman-mcp'), windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'],
  env: { ...process.env, SHARMAN_MCP_HOST: '127.0.0.1', SHARMAN_MCP_PORT: String(port), SHARMAN_MCP_DATA_DIR: data, SHARMAN_NATIVE_EXECUTABLE: '', SHARMAN_WEB_DIST: path.join(root, 'sharman-web/dist'), SHARMAN_AWS_BEDROCK_ENABLED: 'false' },
});
let logs = '';
server.stdout.on('data', (chunk) => { logs += chunk; });
server.stderr.on('data', (chunk) => { logs += chunk; });
let browser;
const audits = [];
async function audit(page, name) {
  await page.addScriptTag({ content: axeSource });
  const result = await page.evaluate(() => window.axe.run('.tasks-view', { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21aa', 'wcag22aa'] } }));
  const violations = result.violations.map(({ id, nodes }) => ({ id, targets: nodes.map(({ target }) => target) }));
  audits.push({ name, violations });
  assert.deepEqual(violations, [], name);
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false, `${name}: horizontal overflow`);
  await page.screenshot({ path: path.join(output, `${name}.png`), fullPage: true });
}
try {
  for (let i = 0; i < 100; i++) {
    assert.equal(server.exitCode, null, logs);
    try { if ((await fetch(`${base}/health`)).ok) break; } catch {}
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  browser = await chromium.launch({ channel: 'chrome', headless: true });
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 }, reducedMotion: 'reduce', timezoneId: 'Asia/Kolkata' });
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto(`${base}/tasks`);
  await page.getByText('MCP bridge online', { exact: true }).waitFor();
  await page.getByRole('heading', { name: 'Your next step starts here.' }).waitFor();
  assert.equal((await (await fetch(`${base}/api/tasks`)).json()).tasks.length, 0);
  assert.doesNotMatch(await page.locator('body').innerText(), /Polish SHARMAN demo video|Vidyut|Priyanshu|Start focus session/);
  await audit(page, 'empty-desktop');
  await page.getByRole('button', { name: 'New task', exact: true }).click();
  const form = page.getByRole('form', { name: 'Create task' });
  await form.getByLabel('Task name').fill('Review this week’s priorities');
  await audit(page, 'create-desktop');
  let posts = 0;
  let release;
  let lose = false;
  await page.route('**/api/tasks', async (route) => {
    if (route.request().method() !== 'POST') return route.continue();
    posts++;
    if (posts === 1) await new Promise((resolve) => { release = resolve; });
    if (lose) {
      lose = false;
      await route.fetch();
      return route.fulfill({ status: 502, contentType: 'application/json', body: JSON.stringify({ error: 'Response could not be confirmed.' }) });
    }
    await route.continue();
  });
  await form.getByRole('button', { name: 'Save task' }).click();
  await form.getByRole('button', { name: 'Saving…' }).waitFor();
  assert.equal(await form.getByRole('button', { name: 'Cancel', exact: true }).isDisabled(), true);
  assert.equal(posts, 1);
  release();
  await form.waitFor({ state: 'hidden' });
  await page.locator('.task-row').getByText('Review this week’s priorities', { exact: true }).waitFor();
  await audit(page, 'list-desktop');
  await page.reload();
  await page.locator('.task-row').getByText('Review this week’s priorities', { exact: true }).waitFor();
  assert.equal(new URL(page.url()).pathname, '/tasks');
  await page.setViewportSize({ width: 390, height: 844 });
  await audit(page, 'list-mobile');
  await page.getByRole('button', { name: 'New task', exact: true }).click();
  await form.getByLabel('Task name').fill('Check the upcoming calendar');
  await audit(page, 'create-mobile');
  lose = true;
  await form.getByRole('button', { name: 'Save task' }).click();
  await form.getByRole('alert').waitFor();
  assert.equal(await form.getByLabel('Task name').inputValue(), 'Check the upcoming calendar');
  assert.equal(posts, 2, 'failed writes must not automatically retry');
  await form.getByRole('button', { name: 'Save task' }).click();
  await form.waitFor({ state: 'hidden' });
  assert.equal((await (await fetch(`${base}/api/tasks`)).json()).tasks.length, 2, 'explicit retry deduplicates an already committed write');
  await page.getByRole('button', { name: 'Completed 0', exact: true }).click();
  await page.getByRole('heading', { name: 'No completed tasks yet.' }).waitFor();
  await page.setViewportSize({ width: 320, height: 700 });
  await audit(page, 'completed-small-mobile');
  await page.goto(`${base}/`);
  await page.getByRole('button', { name: 'What do I need to do today?' }).click();
  await page.getByText('DailyPlannerAgent created', { exact: true }).waitFor();
  assert.match(await page.locator('.delegation-receipt').innerText(), /Supervisor-confirmed.*terminated/is);
  for (const step of ['UNDERSTAND', 'PLAN', 'TOOL', 'RESULT']) assert.equal(await page.locator('.trace-flow').getByText(step, { exact: true }).isVisible(), true);
  await page.goto(`${base}/home`);
  await page.getByRole('button', { name: 'Blocked unlock: Prove the permission boundary' }).click();
  await page.locator('.permission-comparison dd').getByText('DENIED', { exact: true }).waitFor();
  assert.match(await page.locator('.permission-comparison').innerText(), /door\.unlock.*HIGH_RISK.*LOW_RISK.*DENIED/s);
  await page.locator('.workflow-line').getByText('Agent terminated').waitFor();
  assert.match(await page.locator('.aws-care-row').innerText(), /LOCAL SAFE FALLBACK ACTIVE/);
  assert.deepEqual(errors, []);
  await writeFile(path.join(output, 'summary.json'), JSON.stringify({ passed: true, audits, checks: ['empty seed', 'create via real backend', 'same-request guard', 'reload persistence', 'no false completion control', 'lost-response warning and explicit idempotent retry', '390/320px mobile', 'backend-confirmed delegation', 'four execution stages visible on mobile', 'Home explicit permission comparison'] }, null, 2));
  console.log(`Tasks / execution UI QA passed, ${audits.length} accessibility scans. Evidence: ${output}`);
} finally {
  await browser?.close();
  if (server.exitCode === null) { const exited = new Promise((resolve) => server.once('exit', resolve)); server.kill(); await exited; }
  assert(path.resolve(data).startsWith(`${path.resolve(tmpdir())}${path.sep}sharman-tasks-ui-`));
  await rm(data, { recursive: true, force: true });
}
