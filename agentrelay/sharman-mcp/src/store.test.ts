import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { SharmanStore } from './store.js';

async function fixture() {
  const directory = await mkdtemp(path.join(tmpdir(), 'sharman-home-'));
  return {
    directory,
    store: new SharmanStore(directory),
    cleanup: () => rm(directory, { recursive: true, force: true }),
  };
}

test('expected delivery creates a bounded temporary agent and persistent event', async () => {
  const { store, cleanup } = await fixture();
  try {
    const workflow = await store.simulateHomeScenario('expected_delivery');
    assert.equal(workflow.decision, 'allowed');
    assert.equal(workflow.event.expectedMatch, true);
    assert.equal(workflow.agent.status, 'completed');
    assert.ok(workflow.agent.permissions.includes('reminder.create'));
    assert.ok(workflow.agent.forbidden.includes('door.unlock'));

    const snapshot = await store.homeSnapshot();
    assert.equal(snapshot.events.length, 1);
    assert.equal(snapshot.agents.length, 1);
    assert.equal(snapshot.agents[0]?.terminatedAt, snapshot.agents[0]?.createdAt);
  } finally {
    await cleanup();
  }
});

test('expected delivery reminder acceptance is idempotent', async () => {
  const { store, cleanup } = await fixture();
  try {
    await store.simulateHomeScenario('expected_delivery');
    const first = await store.acceptHomeSuggestion('expected_delivery');
    const duplicate = await store.acceptHomeSuggestion('expected_delivery');
    assert.equal(first.reminder?.id, duplicate.reminder?.id);
    assert.equal((await store.snapshot()).reminders.length, 1);
  } finally {
    await cleanup();
  }
});

test('repeated unexpected visitor requests approval without sending a message', async () => {
  const { store, cleanup } = await fixture();
  try {
    const workflow = await store.simulateHomeScenario('repeated_unexpected');
    assert.equal(workflow.event.occurrenceCount, 2);
    assert.equal(workflow.event.expectedMatch, false);
    assert.equal(workflow.decision, 'approval_required');
    assert.equal(workflow.requiresApproval, true);
    assert.equal((await store.homeSnapshot()).status, 'attention');

    const accepted = await store.acceptHomeSuggestion('repeated_unexpected');
    assert.match(accepted.message, /prepared for review/i);
    assert.match(accepted.message, /nothing was sent automatically/i);
    assert.equal((await store.snapshot()).reminders.length, 0);
  } finally {
    await cleanup();
  }
});

test('door unlock remains denied and unavailable to VisitorAgent', async () => {
  const { store, cleanup } = await fixture();
  try {
    const workflow = await store.simulateHomeScenario('blocked_unlock');
    assert.equal(workflow.decision, 'denied');
    assert.equal(workflow.agent.status, 'blocked');
    assert.ok(workflow.agent.forbidden.includes('door.unlock'));
    assert.equal(workflow.agent.denialDetail?.requiredPermission, 'HIGH_RISK');
    assert.equal(workflow.agent.denialDetail?.delegatedCeiling, 'LOW_RISK');
    assert.match(workflow.agent.denialDetail?.reason || '', /cannot exceed delegated privileges/i);
    assert.equal(workflow.suggestedAction, null);
    const state = await store.snapshot();
    assert.equal(state.reminders.length, 0);
    assert.ok(state.audit.some((event) => event.action === 'Action denied' && event.permission === 'DENIED' && event.requiredPermission === 'HIGH_RISK'));
  } finally {
    await cleanup();
  }
});

test('reset returns the simulator to a clean safe baseline', async () => {
  const { store, cleanup } = await fixture();
  try {
    await store.simulateHomeScenario('repeated_unexpected');
    const reset = await store.resetHomeDemo();
    assert.equal(reset.status, 'safe');
    assert.deepEqual(reset.events, []);
    assert.deepEqual(reset.agents, []);
    assert.equal(reset.lastWorkflow, null);
    assert.equal(reset.expectedVisit.label, 'Medicine delivery');
  } finally {
    await cleanup();
  }
});

test('legacy demo state is upgraded with Home state on read', async () => {
  const { directory, store, cleanup } = await fixture();
  try {
    await writeFile(path.join(directory, 'sharman-demo.json'), JSON.stringify({ mode: 'demo', tasks: [], inbox: [], reminders: [], audit: [] }), 'utf8');
    const home = await store.homeSnapshot();
    assert.equal(home.status, 'safe');
    assert.equal(home.expectedVisit.window, '4:00–6:00 PM');
    const persisted = JSON.parse(await readFile(path.join(directory, 'sharman-demo.json'), 'utf8')) as { home?: unknown };
    assert.equal(persisted.home, undefined, 'read migration remains non-destructive until the next mutation');
  } finally {
    await cleanup();
  }
});

test('DailyPlannerAgent runs bounded 6-step READ_ONLY delegation and terminates', async () => {
  const { store, cleanup } = await fixture();
  try {
    await store.createTask('Review project notes', null, 'user');
    await store.createTask('Prepare next steps', null, 'user');
    const delegation = await store.runDailyPlannerAgent('sharman', true);
    assert.equal(delegation.name, 'DailyPlannerAgent');
    assert.equal(delegation.permissionCeiling, 'READ_ONLY');
    assert.equal(delegation.maxSteps, 8);
    assert.equal(delegation.stepsUsed, 6);
    assert.equal(delegation.status, 'terminated');
    assert.deepEqual(delegation.steps.map((step) => step.tool), [
      'inbox.sample.read',
      'inbox.actions',
      'tasks.list',
      'reminders.list',
      'prioritize',
      'return_result',
    ]);
    assert.deepEqual(delegation.priorities.map((item) => item.tier), ['CRITICAL', 'HIGH', 'MEDIUM', 'LOW']);
    assert.equal(delegation.dataSource, 'sample');
    assert.ok(delegation.priorities.filter((item) => item.source === 'Sample Inbox').length === 2);
  } finally {
    await cleanup();
  }
});

test('new workspaces have no fabricated user tasks', async () => {
  const { store, cleanup } = await fixture();
  try {
    assert.deepEqual(await store.listTasks(), []);
    const plan = await store.runDailyPlannerAgent('sharman');
    assert.ok(plan.priorities.every((item) => item.source !== 'Tasks'));
  } finally { await cleanup(); }
});

test('legacy fixture cleanup preserves approved tasks, edited fixtures and other state across restart', async () => {
  const { directory, store, cleanup } = await fixture();
  try {
    // A user is allowed to create a task with the same title as an old sample.
    const approved = await store.createTask('Polish SHARMAN demo video', null, 'user');
    const state = await store.snapshot();
    const fixed = { dueAt: null, status: 'open' as const, source: 'user' as const, createdAt: new Date().toISOString() };
    state.tasks.push(
      { ...fixed, id: 'task-portfolio', title: 'Polish SHARMAN demo video' },
      { ...fixed, id: 'task-report', title: 'Finish Vidyut progress report' },
      { ...fixed, id: 'task-gmail', title: 'Validate Gmail action extraction', status: 'completed' },
      { ...fixed, id: 'task-report-edited', title: 'Finish Vidyut progress report' },
      { ...fixed, id: 'task-portfolio-custom', title: 'Personal task' },
    );
    await writeFile(path.join(directory, 'sharman-demo.json'), JSON.stringify(state), 'utf8');
    const restarted = new SharmanStore(directory);
    const migrated = await restarted.snapshot();
    assert.deepEqual(migrated.tasks.map((task) => task.id), [approved.task.id, 'task-report-edited', 'task-portfolio-custom']);
    assert.deepEqual(migrated.inbox, state.inbox);
    assert.deepEqual(migrated.audit, state.audit);
    assert.deepEqual(migrated.home, state.home);
    assert.deepEqual((await new SharmanStore(directory).snapshot()).tasks, migrated.tasks);
    // A renamed fixture with its original ID must also be preserved.
    migrated.tasks.push({ ...fixed, id: 'task-report', title: 'A task the user changed' });
    await writeFile(path.join(directory, 'sharman-demo.json'), JSON.stringify(migrated), 'utf8');
    assert.equal((await new SharmanStore(directory).snapshot()).tasks.at(-1)?.title, 'A task the user changed');
  } finally { await cleanup(); }
});

test('processing mode defaults to local_only and updates with audit record', async () => {
  const { store, cleanup } = await fixture();
  try {
    assert.equal(await store.getProcessingMode(), 'local_only');
    assert.equal(await store.setProcessingMode('hybrid'), 'hybrid');
    assert.equal(await store.getProcessingMode(), 'hybrid');
    assert.equal(await store.setProcessingMode('cloud_allowed'), 'cloud_allowed');
  } finally {
    await cleanup();
  }
});

test('parallel cold-start reads and approved writes initialize once without losing mutations', async () => {
  const { directory, store, cleanup } = await fixture();
  try {
    const readers = Array.from({ length: 24 }, () => [store.snapshot(), store.actionItems(), store.listTasks(), store.listReminders()]).flat();
    const writes = Array.from({ length: 8 }, (_, index) => store.createTask(`Cold-start approved task ${index}`, null, 'user'));
    const results = await Promise.allSettled([
      ...readers, ...writes,
      store.createReminder('Cold-start approved reminder', new Date(Date.now() + 3_600_000).toISOString(), null),
    ]);
    const failures = results.filter((result) => result.status === 'rejected');
    assert.equal(failures.length, 0, `Concurrent cold start rejected ${failures.length} operation(s).`);
    const state = await store.snapshot();
    assert.equal(state.tasks.filter((task) => task.title.startsWith('Cold-start approved task')).length, 8);
    assert.equal(state.reminders.filter((item) => item.title === 'Cold-start approved reminder').length, 1);
    const restarted = new SharmanStore(directory);
    assert.deepEqual((await restarted.snapshot()).tasks, state.tasks);
    assert.deepEqual((await restarted.snapshot()).reminders, state.reminders);
  } finally {
    await cleanup();
  }
});

test('corrupted existing memory fails closed instead of silently replacing project state', async () => {
  const { directory, store, cleanup } = await fixture();
  try {
    const file = path.join(directory, 'sharman-demo.json');
    await writeFile(file, '{invalid existing memory', 'utf8');
    await assert.rejects(store.snapshot(), SyntaxError);
    assert.equal(await readFile(file, 'utf8'), '{invalid existing memory');
  } finally {
    await cleanup();
  }
});
