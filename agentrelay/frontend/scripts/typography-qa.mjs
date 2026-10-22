// Production React/CSS with synthetic IPC only; no installed app, private
// database, Gmail, model or external tool is accessed by this test.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '../..');
const requireTools = createRequire(path.join(root, 'target/home-qa-tools/package.json'));
const { chromium } = requireTools('playwright');
const axe = await readFile(requireTools.resolve('axe-core/axe.min.js'), 'utf8');
const dist = path.join(root, 'frontend/dist');
const output = path.join(root, 'target/qa/native-typography-20261020');
await mkdir(output, { recursive: true });
const server = createServer(async (request, response) => {
  try {
    const url = new URL(request.url, 'http://localhost');
    const filename = path.resolve(dist, url.pathname === '/' ? 'index.html' : `.${decodeURIComponent(url.pathname)}`);
    if (!filename.startsWith(`${dist}${path.sep}`)) { response.writeHead(403).end(); return; }
    response.setHeader('content-type', filename.endsWith('.js') ? 'text/javascript' : filename.endsWith('.css') ? 'text/css' : 'text/html');
    response.end(await readFile(filename));
  } catch { response.writeHead(404).end(); }
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const base = `http://127.0.0.1:${server.address().port}`;
const results = [];
const errors = [];
const externalRequests = [];
let browser;
let completed = false;

try {
  browser = await chromium.launch({ channel: 'chrome', headless: true });
  const context = await browser.newContext({ viewport: { width: 1536, height: 960 }, reducedMotion: 'reduce' });
  await context.route('**/*', (route) => {
    if (route.request().url().startsWith(base)) return route.continue();
    externalRequests.push(route.request().url());
    return route.abort();
  });
  await context.addInitScript(() => {
    const at = '2026-10-20T08:30:00Z';
    const repository = 'C:\\Typography-Fixture\\SHARMAN';
    const status = { repository, branch: 'main', modifiedFiles: [], addedFiles: [], deletedFiles: [], recentCommits: [], diffStat: [], lastCommit: 'Synthetic typography fixture', headCommit: 'fixture', scannedAt: at };
    const providers = [
      ['claude-code', 'Claude Code', 'Anthropic Claude Code provider adapter', ['architecture', 'backend', 'refactoring', 'research', 'documentation']],
      ['codex', 'Codex', 'OpenAI Codex provider adapter', ['backend', 'testing', 'rust', 'refactoring']],
      ['copilot', 'GitHub Copilot', 'GitHub Copilot agent provider', ['frontend', 'typeScript', 'testing']],
      ['cursor', 'Cursor', 'Cursor editor and agent provider', ['frontend', 'typeScript', 'testing']],
      ['gemini', 'Gemini CLI', 'Google Gemini CLI provider adapter', ['research', 'documentation', 'architecture']],
    ];
    const registry = {
      sdkApiVersion: 1, pluginDirectory: 'C:\\Typography-Fixture\\AppData\\Roaming\\com.agentrelay.desktop\\plugins', loadErrors: [],
      entries: providers.map(([id, name, description, strengths]) => ({ origin: 'builtIn', source: 'Synthetic fixture', manifest: { id, name, description, version: '0.1.0', kind: 'provider', sdkApiVersion: 1, enabled: true, capabilities: { strengths, allowedActions: [], maxContextTokens: 6000, codeEdit: true, terminal: false, planning: true, documentation: true }, permissions: { readRepo: true, writeRepo: false, runCommands: false }, detection: { rules: [{ process: `${id}.exe`, commandContains: [] }], processes: [], commandContains: [] }, adapters: { eventTypes: ['*'], actions: id === 'gemini' ? [] : ['runTests', 'runBuild'], handoffFormat: null } } })),
    };
    const projectMemory = { project: { id: 'fixture', path: repository, name: 'Typography fixture', branch: 'main', changeCount: 0, trackingEnabled: false, lastCommit: 'fixture', lastScannedAt: at, latestStatus: status }, goals: [], decisions: [], events: [], agentSessions: [], lastAgentSession: null, latestHandoff: null, handoffs: [], conflicts: [], freshness: [], permissions: {}, actionRuns: [] };
    const settings = { onboardingCompleted: true, selectedAgents: [], launchAtStartup: false, closeToTray: true, trackingPaused: false, notificationsEnabled: false, defaultContextDepth: 'normal', defaultContextProfile: 'auto', backupIntervalHours: 24 };
    const conversation = { id: 'fixture', title: 'New conversation', updatedAt: at, turns: [] };
    window.__typographyQaCalls = [];
    window.__TAURI_INTERNALS__ = { invoke: async (cmd) => {
      window.__typographyQaCalls.push(cmd);
      const responses = {
        get_application_settings: settings, get_last_repository: repository, scan_project: status,
        get_project_memory: projectMemory, compile_context: { markdown: '# Synthetic context', snapshot: { items: [], warnings: [] } },
        get_orchestration_snapshot: { tasks: [], profiles: [], locks: [], recommendations: [], parallelAssessments: [] },
        get_collaborative_execution_snapshot: { workspaces: [], validations: [] }, get_planner_snapshot: { activePlan: null, plans: [] }, get_verification_snapshot: { suites: [] },
        get_integration_registry: registry, get_delivery_snapshot: null, get_background_status: { running: true, paused: false, detectedAgents: [], watchedProjects: [], automaticSessionCount: 0, blockedTaskCount: 0, idleTimeoutMinutes: 15 }, get_idle_timeout_minutes: 15, get_sandbox_status: {},
        get_project_health: { score: 100, status: 'healthy', items: [], checkedAt: at, repository },
        get_assistant_memory: { active: conversation, conversations: [conversation], memories: [] },
        get_assistant_home: { projects: [], capabilities: [], device: { totalMemoryGb: 16, logicalCores: 8, cpu: 'Synthetic device', operatingSystem: 'Windows fixture', architecture: 'x64', tier: 'balanced', recommendedModelSize: '8B', recommendation: 'Synthetic fixture', recommendedModelName: 'qwen3:8b', recommendedDownloadGb: 5, localRuntimeInstalled: false, localModelInstalled: false, localModelName: null } },
        get_personal_dashboard: { tasks: [], reminders: [], generatedAt: at }, get_inbox_dashboard: { status: { connected: false, sync: {}, credentialStorage: 'Synthetic fixture' }, needsAction: [], upcomingDeadlines: [], dismissed: [], generatedAt: at, scan: null }, get_assistant_audit: [],
      };
      if (!(cmd in responses)) throw new Error(`Unmocked typography command: ${cmd}`);
      return structuredClone(responses[cmd]);
    } };
    document.addEventListener('DOMContentLoaded', () => {
      const label = document.createElement('div');
      label.textContent = 'TYPOGRAPHY VALIDATION · SYNTHETIC DATA';
      label.style.cssText = 'position:fixed;bottom:4px;right:10px;z-index:3000;padding:4px 8px;background:#183323;color:#e2f7e8;border-radius:5px;font:12px Segoe UI,sans-serif;pointer-events:none';
      document.body.append(label);
    });
  });
  const page = await context.newPage();
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto(base);
  // The project loads asynchronously while Home is shown. Wait for its toolbar
  // rather than accidentally opening the first-run wizard before it is ready.
  await page.getByRole('button', { name: 'Settings', exact: true }).waitFor();
  await page.getByRole('button', { name: 'Projects', exact: true }).click();
  await page.getByRole('navigation', { name: 'Project views' }).getByRole('button', { name: /^integrations/i }).click();
  const panel = page.locator('.integrations-workspace');
  await panel.getByRole('heading', { name: 'Claude Code', exact: true }).waitFor();
  await page.addScriptTag({ content: axe });
  for (const [name, width, height] of [['desktop', 1536, 960], ['native', 1240, 820], ['minimum', 900, 680], ['zoom-reflow', 620, 820]]) {
    await page.setViewportSize({ width, height });
    await panel.locator('.github-sync-card').scrollIntoViewIfNeeded();
    const measurements = await panel.evaluate((element) => {
      const measured = [...element.querySelectorAll('p, span, small, em, code, label, button, input, select, h3')].filter((item) => item.getClientRects().length && item.textContent?.trim());
      return { font: getComputedStyle(element).fontFamily, descriptionSize: getComputedStyle(element.querySelector('.plugin-card > p')).fontSize, labelSize: getComputedStyle(element.querySelector('.plugin-tags span')).fontSize, titleSize: getComputedStyle(element.querySelector('.plugin-heading h3')).fontSize, codeFont: getComputedStyle(element.querySelector('.plugin-directory code')).fontFamily, minimumTextSize: Math.min(...measured.map((item) => parseFloat(getComputedStyle(item).fontSize))), panelOverflow: element.scrollWidth > element.clientWidth + 1, documentOverflow: document.documentElement.scrollWidth > innerWidth + 1, clippedControls: [...element.querySelectorAll('input, select, button')].some((item) => item.getBoundingClientRect().right > innerWidth + 1) };
    });
    const violations = await page.evaluate(async () => (await axe.run('.integrations-workspace', { runOnly: ['wcag2a', 'wcag2aa', 'wcag21aa'] })).violations.map(({ id, nodes }) => ({ id, targets: nodes.map(({ target }) => target) })));
    await panel.screenshot({ path: path.join(output, `${name}.png`) });
    results.push({ name, width, height, ...measurements, violations });
    assert.match(measurements.font, /Segoe UI/);
    assert.match(measurements.codeFont, /Consolas/);
    assert.equal(measurements.descriptionSize, '14px');
    assert.equal(measurements.labelSize, '12px');
    assert.equal(measurements.titleSize, '17px');
    assert(measurements.minimumTextSize >= 12, `${name}: text below 12px`);
    assert.equal(measurements.panelOverflow, false, `${name}: panel overflow`);
    assert.equal(measurements.clippedControls, false, `${name}: clipped controls`);
    if (width >= 900) assert.equal(measurements.documentOverflow, false, `${name}: window overflow`);
    assert.deepEqual(violations, [], `${name}: WCAG violations`);
  }
  assert.deepEqual(errors, []);
  assert.deepEqual(externalRequests, [], 'Native typography must not fetch remote fonts');
  assert((await page.evaluate(() => window.__typographyQaCalls)).every((cmd) => /^(get_|scan_project$|compile_context$)/.test(cmd)), 'No action was executed by typography QA');
  completed = true;
} catch (error) {
  errors.push(error.message);
  throw error;
} finally {
  await writeFile(path.join(output, 'summary.json'), JSON.stringify({ passed: completed, data: 'Synthetic IPC, actual production React/CSS; no private data or live actions', results, errors, externalRequests }, null, 2));
  await browser?.close();
  await new Promise((resolve) => server.close(resolve));
}
console.log(JSON.stringify({ passed: completed, states: results.length, output }));
