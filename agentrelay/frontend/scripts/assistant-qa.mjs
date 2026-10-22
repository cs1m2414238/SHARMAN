// Native React UI acceptance with synthetic Tauri IPC only. Never starts Sharman,
// reads a private database, contacts Gmail/AI, or executes an OS action.
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
const output = path.join(root, 'target/qa/native-assistant-20261021');
await mkdir(output, { recursive: true });
const server = createServer(async (request, response) => {
  try {
    const url = new URL(request.url, 'http://localhost');
    const filename = path.resolve(dist, url.pathname === '/' ? 'index.html' : `.${decodeURIComponent(url.pathname)}`);
    if (!filename.startsWith(`${dist}${path.sep}`)) { response.writeHead(403).end(); return; }
    const data = await readFile(filename);
    response.setHeader('content-type', filename.endsWith('.js') ? 'text/javascript' : filename.endsWith('.css') ? 'text/css' : 'text/html');
    response.end(data);
  } catch { response.writeHead(404).end(); }
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const base = `http://127.0.0.1:${server.address().port}`;
const results = [];
const errors = [];
let completed = false;
let browser;

function fixture({ empty = false, connected = true, historyFailure = false, long = false, planner = false } = {}) {
  const at = '2026-10-20T08:30:00.000Z';
  const messages = [
    { id: '1', threadId: '123ab', subject: '[Synthetic] Design review tomorrow', sender: 'Design team <design@example.invalid>', snippet: 'The interface review is scheduled for tomorrow. The agenda and notes are ready for your review.', receivedAt: at },
    { id: '2', threadId: '456cd', subject: '[Synthetic] Weekly project update', sender: 'Updates <updates@example.invalid>', snippet: 'Here is the latest progress summary. No action or deadline was extracted from this message.', receivedAt: at },
  ];
  const response = { conversationId: 'inbox', intent: 'gmail_unread', title: 'Your unread email', answer: 'Found **2 unread messages**. You can review them below.\n\nGmail access is read-only. Nothing has been marked as read, changed, or sent.', projects: [], files: [], actions: [], suggestions: ['Show my recent emails', 'What tasks do I have today?'], localOnly: false, planSteps: [{ tool: 'gmail.list_unread', summary: 'Read unread message metadata without action filtering', permissionLevel: 0, status: 'completed' }], personalTasks: [], reminders: [], emailActions: [], emailMessages: messages };
  const note = { ...response, conversationId: 'notes', intent: 'chat', title: 'A useful next step', answer: 'Start with a **small, testable goal**.\n\n- Write down what success looks like.\n- Keep your context in `AGENT_CONTEXT.md`.\n- Verify the result before moving on.\n\n```rust\nlet approved = true;\n```', localOnly: true, planSteps: [], emailMessages: [] };
  if (planner) {
    Object.assign(response, { intent: 'personal_review', title: 'Your local planner', answer: 'Synthetic task/reminder preview. Review the exact item and confirm before changing it.', localOnly: true, emailMessages: [], planSteps: [{ tool: 'tasks.find', summary: 'Exact-title lookup only; no writes', permissionLevel: 0, status: 'awaiting-confirmation' }], personalTasks: [
      { id: 'task-1', title: 'Review proposal', status: 'todo', priority: 'normal', dueAt: at, source: 'synthetic', createdAt: at, updatedAt: at },
      { id: 'task-done', title: 'Completed research', status: 'completed', priority: 'normal', dueAt: null, source: 'synthetic', createdAt: at, updatedAt: at },
    ], reminders: [
      { id: 'reminder-1', taskId: 'task-1', title: 'Review proposal', remindAt: at, status: 'pending', createdAt: at, firedAt: null },
      { id: 'reminder-2', taskId: 'task-2', title: 'Take a break', remindAt: at, status: 'pending', createdAt: at, firedAt: null },
    ] });
  }
  const conversations = [
    { id: 'inbox', title: empty ? 'New conversation' : 'Review my unread inbox', updatedAt: at, turns: empty ? [] : [{ id: 1, request: 'Show my unread emails', response, createdAt: at }] },
    { id: 'notes', title: 'Plan a focused afternoon', updatedAt: at, turns: [{ id: 2, request: 'Help me pick the next step', response: note, createdAt: at }] },
  ];
  if (long) conversations[0].turns = Array.from({ length: 12 }, (_, index) => ({ id: index + 20, request: `Earlier question ${index + 1}`, response: { ...note, answer: note.answer.repeat(3) }, createdAt: at }));
  return { at, response, conversations, connected, historyFailure };
}

async function openFixture(width = 1440, options = {}, height = width < 500 ? 844 : 960) {
  const context = await browser.newContext({ viewport: { width, height }, timezoneId: 'Asia/Kolkata', reducedMotion: 'reduce' });
  await context.route('**/*', (route) => route.request().url().startsWith(base) ? route.continue() : route.abort());
  await context.addInitScript((seed) => {
    const stored = localStorage.getItem('native-ui-qa');
    const state = stored ? JSON.parse(stored) : { active: seed.conversations[0].id, conversations: seed.conversations, memories: [{ id: 1, content: 'I prefer concise answers.', createdAt: seed.at }], next: 10 };
    const qa = window.__nativeQa = { calls: [], failAsk: false, failReadAfterAsk: false, failNextMemory: seed.historyFailure, failInbox: false, copied: '' };
    const save = () => localStorage.setItem('native-ui-qa', JSON.stringify(state));
    const memory = (id = state.active) => ({ active: state.conversations.find((item) => item.id === id), conversations: state.conversations.map(({ turns, ...item }) => item), memories: state.memories });
    const personal = { tasks: seed.response.personalTasks.filter((task) => task.status === 'todo'), reminders: [...seed.response.reminders], generatedAt: seed.at };
    const inbox = () => ({ status: { connected: seed.connected, accountEmail: seed.connected ? 'demo@example.invalid' : null, lastSyncAt: seed.at, sync: { accountId: null, lastHistoryId: 'fixture-history', lastSuccessfulSync: seed.at, lastError: null }, credentialStorage: 'Synthetic fixture', setupHint: 'Connect Gmail in the installed desktop app with read-only access.' }, needsAction: [], upcomingDeadlines: [], dismissed: [], generatedAt: seed.at, scan: seed.connected ? { mode: 'important', scanned: 12, matched: 0 } : null });
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async (text) => { qa.copied = text; } } });
    window.__TAURI_INTERNALS__ = { invoke: async (cmd, args = {}) => {
      qa.calls.push({ cmd, args });
      let result;
      if (cmd === 'get_assistant_memory') {
        if (qa.failNextMemory) { qa.failNextMemory = false; throw new Error('Synthetic history read interruption'); }
        result = memory(args.conversationId || state.active);
      } else if (cmd === 'create_assistant_conversation') {
        state.active = `chat-${state.next++}`;
        state.conversations.unshift({ id: state.active, title: 'New conversation', updatedAt: seed.at, turns: [] });
        save(); result = memory();
      } else if (cmd === 'ask_sharman') {
        await new Promise((resolve) => setTimeout(resolve, 550));
        if (qa.failAsk) { qa.failAsk = false; throw new Error('Synthetic lost request acknowledgement'); }
        const conversation = state.conversations.find((item) => item.id === args.input.conversationId);
        const answer = { ...seed.response, conversationId: conversation.id, intent: 'chat', title: 'Here to help', localOnly: true, emailMessages: [], planSteps: [], answer: `You asked: ${args.input.message}\n\nThis is a **synthetic UI response**, not a live model or mailbox result.` };
        conversation.turns.push({ id: state.next++, request: args.input.message, response: answer, createdAt: seed.at });
        state.active = conversation.id; save();
        if (qa.failReadAfterAsk) { qa.failNextMemory = true; qa.failReadAfterAsk = false; }
        result = answer;
      } else if (cmd === 'forget_assistant_memory') {
        state.memories = args.memoryId == null ? [] : state.memories.filter((item) => item.id !== args.memoryId); save(); result = memory();
      } else if (cmd === 'get_assistant_home') result = { projects: [], capabilities: [], device: { totalMemoryGb: 24, logicalCores: 8, cpu: 'Synthetic test device', operatingSystem: 'Windows (fixture)', architecture: 'x64', tier: 'balanced', recommendedModelSize: '8B Q4', recommendation: 'Synthetic profile for UI validation.', recommendedModelName: 'qwen3:8b', recommendedDownloadGb: 5.2, localRuntimeInstalled: true, localModelInstalled: true, localModelName: 'qwen3:8b' } };
      else if (cmd === 'get_personal_dashboard') result = personal;
      else if (cmd === 'set_personal_task_completed' || cmd === 'dismiss_personal_reminder') {
        await new Promise((resolve) => setTimeout(resolve, 150));
        if (qa.failPersonalWrite) { qa.failPersonalWrite = false; throw new Error('Synthetic personal-store interruption'); }
        if (cmd === 'set_personal_task_completed') {
          personal.tasks = personal.tasks.filter((task) => task.id !== args.taskId);
          personal.reminders = personal.reminders.filter((reminder) => reminder.taskId !== args.taskId);
        } else personal.reminders = personal.reminders.filter((reminder) => reminder.id !== args.reminderId);
        result = personal;
      }
      else if (cmd === 'get_inbox_dashboard') { if (qa.failInbox) throw new Error('Synthetic inbox unavailable'); result = inbox(); }
      else if (cmd === 'get_assistant_audit') result = [];
      else if (cmd === 'get_application_settings') result = { defaultContextDepth: 'normal', defaultContextProfile: 'general' };
      else if (cmd === 'get_last_repository') result = null;
      else if (cmd === 'get_background_status') result = { running: true, detectedAgents: [], automaticSessionCount: 0 };
      else if (cmd === 'get_idle_timeout_minutes') result = 15;
      else if (cmd === 'get_sandbox_status') result = {};
      else if (cmd === 'get_integration_registry') result = { entries: [] };
      else if (cmd === 'open_gmail_message' || cmd === 'execute_assistant_action') result = null;
      else throw new Error(`Unmocked UI command: ${cmd}`);
      return structuredClone(result);
    } };
    document.addEventListener('DOMContentLoaded', () => {
      const label = document.createElement('div'); label.textContent = 'UI VALIDATION · SYNTHETIC DATA';
      label.setAttribute('data-qa-label', '');
      label.style.cssText = 'position:fixed;bottom:3px;right:9px;z-index:3000;padding:3px 7px;border-radius:4px;background:#1c2b23;color:#ceead5;font:10px system-ui;pointer-events:none';
      document.body.appendChild(label);
    });
  }, fixture(options));
  const page = await context.newPage();
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto(base);
  await page.getByRole('textbox', { name: 'Ask Sharman' }).waitFor();
  await page.waitForFunction(() => !document.querySelector('textarea')?.disabled || document.querySelector('[role="alert"]'));
  return { context, page };
}

async function snapshot(page, name) {
  await page.addScriptTag({ content: axe });
  const violations = await page.evaluate(async () => (await axe.run(document, { runOnly: ['wcag2a', 'wcag2aa', 'wcag21aa'] })).violations.map((item) => ({ id: item.id, nodes: item.nodes.map((node) => ({ target: node.target, summary: node.failureSummary })) })));
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth);
  const composer = await page.getByRole('button', { name: 'Send message', exact: true }).boundingBox();
  const visibleComposer = composer && composer.y >= 0 && composer.y + composer.height <= page.viewportSize().height;
  await page.screenshot({ path: path.join(output, `${name}.png`), fullPage: true });
  results.push({ name, axeViolations: violations, overflow, composerVisible: Boolean(visibleComposer) });
  assert.equal(overflow, false, `${name}: horizontal overflow`);
  assert(visibleComposer, `${name}: composer clipped`);
  assert.deepEqual(violations, [], `${name}: WCAG violations`);
}

try {
  browser = await chromium.launch({ channel: 'chrome', headless: true });
  const { page, context } = await openFixture();
  await snapshot(page, 'desktop-chat');
  assert.equal(await page.locator('.assistant-response-text').first().evaluate((element) => getComputedStyle(element).fontSize), '15px');
  await page.locator('.assistant-tool-trace > summary').click();
  await page.getByText('gmail.list_unread', { exact: true }).waitFor({ state: 'visible' });
  await page.getByRole('button', { name: 'Copy reply', exact: true }).first().click();
  assert.match(await page.evaluate(() => window.__nativeQa.copied), /2 unread messages/);
  await page.locator('.notice button').click();
  await page.getByRole('button', { name: /^Memory/ }).click();
  await snapshot(page, 'desktop-memory');
  await page.keyboard.press('Escape');
  assert.equal(await page.getByRole('button', { name: /^Memory/ }).evaluate((element) => element === document.activeElement), true);
  await page.getByRole('button', { name: /^Inbox/ }).click();
  await snapshot(page, 'desktop-inbox');
  await page.getByRole('button', { name: 'Close inbox', exact: true }).click();
  await page.getByRole('textbox', { name: 'Search conversations' }).fill('afternoon');
  assert.equal(await page.getByRole('navigation', { name: 'Saved conversations' }).getByRole('button').count(), 1);
  await page.getByRole('button', { name: /Plan a focused afternoon/ }).click();
  await page.getByText('small, testable goal', { exact: true }).waitFor();
  assert.equal(await page.locator('.assistant-response-text pre code').innerText(), 'let approved = true;\n');
  await page.getByRole('textbox', { name: 'Search conversations' }).fill('');
  await page.getByRole('button', { name: 'New conversation', exact: true }).click();
  await snapshot(page, 'desktop-welcome');
  const input = page.getByRole('textbox', { name: 'Ask Sharman' });
  await page.getByRole('button', { name: /Check my inbox/ }).click();
  assert.equal(await input.inputValue(), 'Show my unread emails');
  assert.equal(await page.evaluate(() => window.__nativeQa.calls.filter((item) => item.cmd === 'ask_sharman').length), 0, 'starter must prefill rather than execute');
  await input.fill('First line'); await input.press('Shift+Enter'); await input.type('Second line');
  assert.equal(await input.inputValue(), 'First line\nSecond line');
  await input.press('Enter');
  await page.getByRole('status').filter({ hasText: 'Working on your request' }).waitFor();
  await page.keyboard.press('Enter'); await page.keyboard.press('Enter');
  await page.waitForFunction(() => !document.querySelector('textarea').disabled);
  assert.equal(await page.evaluate(() => window.__nativeQa.calls.filter((item) => item.cmd === 'ask_sharman').length), 1, 'duplicate keys must not replay a command');
  await page.reload();
  await page.getByText('synthetic UI response', { exact: true }).waitFor();
  await page.evaluate(() => { window.__nativeQa.failReadAfterAsk = true; });
  await input.fill('A write was acknowledged'); await input.press('Enter');
  await page.getByRole('alert').filter({ hasText: 'request completed' }).waitFor();
  const beforeReload = await page.evaluate(() => window.__nativeQa.calls.filter((item) => item.cmd === 'ask_sharman').length);
  await page.getByRole('button', { name: 'Reload conversation', exact: true }).click();
  assert.equal(await page.evaluate(() => window.__nativeQa.calls.filter((item) => item.cmd === 'ask_sharman').length), beforeReload, 'recovery must be a read, not tool replay');
  await page.evaluate(() => { window.__nativeQa.failAsk = true; });
  await input.fill('Restore an unconfirmed draft'); await input.press('Enter');
  await page.getByRole('button', { name: 'Restore message', exact: true }).waitFor();
  await snapshot(page, 'desktop-recovery');
  await page.getByRole('button', { name: 'Restore message', exact: true }).click();
  assert.equal(await input.inputValue(), 'Restore an unconfirmed draft');
  await context.close();

  for (const [width, height] of [[1240, 760], [900, 600]]) {
    const sized = await openFixture(width, {}, height);
    await snapshot(sized.page, `native-${width}x${height}`);
    await sized.context.close();
  }
  const long = await openFixture(1440, { long: true });
  await long.page.getByRole('textbox', { name: 'Ask Sharman' }).fill('A new question');
  await long.page.getByRole('button', { name: 'Send message', exact: true }).click();
  await long.page.getByRole('status').filter({ hasText: 'Working on your request' }).waitFor();
  await long.page.locator('.assistant-conversation').evaluate((element) => { element.scrollTop = 0; });
  await long.page.waitForFunction(() => !document.querySelector('textarea').disabled);
  assert((await long.page.locator('.assistant-conversation').evaluate((element) => element.scrollTop)) < 100, 'a completed response must not steal the reader\'s scroll position');
  await long.page.getByRole('button', { name: 'Latest message', exact: true }).click();
  assert((await long.page.locator('.assistant-conversation').evaluate((element) => element.scrollHeight - element.scrollTop - element.clientHeight)) < 100);
  await snapshot(long.page, 'long-conversation');
  const unsafe = '<img src=x onerror=alert(1)> https://untrusted.example.invalid';
  await long.page.getByRole('textbox', { name: 'Ask Sharman' }).fill(unsafe);
  await long.page.getByRole('button', { name: 'Send message', exact: true }).click();
  await long.page.waitForFunction(() => !document.querySelector('textarea').disabled);
  assert.equal(await long.page.locator('.assistant-response-text img, .assistant-response-text a').count(), 0, 'model HTML/URLs must remain inert text');
  await long.context.close();

  for (const width of [390, 320]) {
    const mobile = await openFixture(width);
    await snapshot(mobile.page, `chat-${width}`);
    await mobile.page.getByRole('button', { name: 'Show conversations', exact: true }).click();
    await snapshot(mobile.page, `history-${width}`);
    await mobile.page.keyboard.press('Escape');
    await mobile.page.getByRole('button', { name: /^Memory/ }).click();
    const panel = mobile.page.getByRole('dialog', { name: 'Local memory', exact: true });
    await panel.waitFor();
    await mobile.page.keyboard.press('Tab');
    assert(await panel.evaluate((element) => element.contains(document.activeElement)));
    await mobile.page.keyboard.press('Escape');
    assert(await mobile.page.getByRole('button', { name: /^Memory/ }).evaluate((element) => element === document.activeElement));
    await mobile.page.getByRole('button', { name: /^Inbox/ }).click();
    await snapshot(mobile.page, `inbox-${width}`);
    await mobile.page.keyboard.press('Control+k');
    await mobile.page.getByRole('dialog', { name: 'Conversations', exact: true }).waitFor();
    assert.equal(await mobile.page.locator('#assistant-memory-panel, #assistant-inbox-panel').count(), 0, 'search shortcut must not stack context and history dialogs');
    await mobile.page.keyboard.press('Escape');
    await mobile.context.close();
  }
  const disconnected = await openFixture(1440, { connected: false });
  await disconnected.page.getByRole('button', { name: /^Inbox/ }).click();
  await snapshot(disconnected.page, 'gmail-disconnected');
  await disconnected.context.close();
  const failed = await openFixture(1440, { historyFailure: true });
  await failed.page.getByRole('button', { name: 'Reload conversation', exact: true }).click();
  await failed.page.waitForFunction(() => !document.querySelector('textarea').disabled);
  await failed.context.close();
  for (const width of [1440, 390, 320]) {
    const planner = await openFixture(width, { planner: true });
    const p = planner.page;
    await snapshot(p, `personal-review-${width}`);
    assert.equal(await p.getByRole('button', { name: 'Complete task: Completed research', exact: true }).count(), 0);
    const writes = () => p.evaluate(() => window.__nativeQa.calls.filter((item) => ['set_personal_task_completed', 'dismiss_personal_reminder'].includes(item.cmd)));
    assert.equal((await writes()).length, 0, 'rendering a preview must not change SQLite');
    const complete = p.getByRole('button', { name: 'Complete task: Review proposal', exact: true });
    p.once('dialog', (dialog) => dialog.dismiss());
    await complete.click();
    assert.equal((await writes()).length, 0, 'declining confirmation must not write');
    p.once('dialog', (dialog) => { assert.match(dialog.message(), /pending reminders/); return dialog.accept(); });
    await complete.click();
    await complete.waitFor({ state: 'detached' });
    assert.equal((await writes()).length, 1);
    assert.equal(await p.getByRole('button', { name: 'Cancel reminder: Review proposal', exact: true }).count(), 0, 'linked reminder control must disappear');
    const cancel = p.getByRole('button', { name: 'Cancel reminder: Take a break', exact: true });
    await p.evaluate(() => { window.__nativeQa.failPersonalWrite = true; });
    p.once('dialog', (dialog) => dialog.accept());
    await cancel.click();
    await p.getByRole('alert').filter({ hasText: 'Synthetic personal-store interruption' }).waitFor();
    assert.equal((await writes()).length, 2, 'failed writes must not auto-retry');
    await p.getByRole('alert').filter({ hasText: 'Synthetic personal-store interruption' }).getByRole('button', { name: 'Dismiss', exact: true }).click();
    p.once('dialog', (dialog) => { assert.match(dialog.message(), /task itself will stay open/); return dialog.accept(); });
    await cancel.click();
    await cancel.waitFor({ state: 'detached' });
    assert.equal((await writes()).length, 3);
    assert.equal((await writes())[2].args.reminderId, 'reminder-2');
    await planner.context.close();
  }
  assert.deepEqual(errors, []);
  completed = true;
  console.log(`Native assistant UI passed: ${results.length} WCAG/layout states, conversation search/switch/reload, draft starters, multiline input, duplicate prevention, no-replay recovery, formatting, copy and panel keyboard controls.`);
} finally {
  await writeFile(path.join(output, 'summary.json'), JSON.stringify({ passed: completed && results.length >= 15 && errors.length === 0 && results.every((item) => !item.overflow && !item.axeViolations.length && item.composerVisible), data: 'Synthetic IPC only; not evidence of live Gmail/LLM/installed-app behavior', results, errors }, null, 2));
  if (browser) await browser.close();
  await new Promise((resolve) => server.close(resolve));
}
