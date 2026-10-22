import assert from 'node:assert/strict';
import { createHmac, randomUUID } from 'node:crypto';
import { mkdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { advanceStreak, CANDIDATE_VERSION, checkVersions, PROTOCOL_VERSION, ROOT, sourceFingerprint } from './release/checks.mjs';
import { artifactFingerprint, atomicJson, evidenceLock, jsonRequest, mcpClient, ownedBridge } from './release/runtime.mjs';
import { validateAudit, validateDenial, validateTools } from './release/contracts.mjs';

export function parseArgs(args) {
  const options = { runs: 1, mode: 'smoke', live: false, profile: 'sharman-amazon', output: path.join(ROOT, 'target/qa/judge-rehearsal') };
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === '--bedrock-live') options.live = true;
    else if (arg === '--help') options.help = true;
    else if (['--runs', '--mode', '--aws-profile', '--output'].includes(arg)) {
      const value = args[++i];
      assert(value && !value.startsWith('--'), `Missing value for ${arg}`);
      if (arg === '--runs') options.runs = Number(value);
      if (arg === '--mode') options.mode = value;
      if (arg === '--aws-profile') options.profile = value;
      if (arg === '--output') options.output = path.resolve(value);
    } else throw new Error(`Unknown rehearsal option: ${arg}`);
  }
  assert(Number.isInteger(options.runs) && options.runs >= 1 && options.runs <= 3, 'Use --runs 1, 2 or 3');
  assert(['smoke', 'candidate'].includes(options.mode), 'Use --mode smoke or candidate');
  assert(/^[A-Za-z0-9_.-]{1,80}$/.test(options.profile), 'Use a named AWS profile, not credentials');
  return options;
}

async function oneRun(options, sequence) {
  const started = performance.now();
  const report = { schemaVersion: 1, sessionId: options.sessionId, run: sequence, mode: options.mode, candidate: CANDIDATE_VERSION, bedrockLive: options.live, passed: false, startedAt: new Date().toISOString(), scope: 'Automated API/MCP checks against a fresh owned server and disposable sample storage. Not real Gmail, Ring hardware, Windows toast, UI or release-install qualification.', checks: [], recordingQualified: false };
  let bridge;
  const check = async (id, label, operation) => {
    const start = performance.now();
    try {
      const evidence = await operation();
      report.checks.push({ id, label, status: 'PASS', durationMs: Math.round(performance.now() - start), evidence });
      console.log(`[PASS] ${label}`);
      return evidence;
    } catch {
      report.checks.push({ id, label, status: 'FAIL', durationMs: Math.round(performance.now() - start), error: 'Check failed; no provider payloads or credentials were recorded.' });
      console.log(`[FAIL] ${label}`);
      throw new Error(`REHEARSAL_CHECK_FAILED:${id}`);
    }
  };
  try {
    report.sourceFingerprint = await sourceFingerprint();
    report.artifactFingerprint = await artifactFingerprint();
    bridge = await ownedBridge({ live: options.live, profile: options.profile, ringFixture: true });
    const request = (route, body) => jsonRequest(bridge.base, route, body);
    const mcp = mcpClient(bridge.base);
    let health, initialized, workflow, audit, reminder;
    const version = JSON.parse(await readFile(path.join(ROOT, 'sharman-mcp/package.json'), 'utf8')).version;
    await check('health', 'Health', async () => {
      health = await request('/health');
      assert.equal(health.status, 'ok'); assert.equal(health.service, 'sharman-mcp');
      const status = await request('/api/status');
      assert.equal(status.personalStorage, 'web_json'); assert.equal(status.mailbox.connected, false);
      return { version: health.version, isolatedStorage: true, personalBridge: false };
    });
    await check('initialize', 'MCP initialize', async () => {
      const response = await mcp.rpc('initialize', { protocolVersion: PROTOCOL_VERSION, capabilities: {}, clientInfo: { name: 'sharman-judge-rehearsal', version } });
      assert(!response.error && response.result?.serverInfo); initialized = response.result;
      assert.equal(initialized.serverInfo.name, 'sharman-local-supervisor');
      await mcp.rpc('notifications/initialized', {}, true);
      return { serverVersion: initialized.serverInfo.version };
    });
    await check('protocol', `MCP protocol — ${PROTOCOL_VERSION}`, async () => { assert.equal(initialized.protocolVersion, PROTOCOL_VERSION); return { protocolVersion: initialized.protocolVersion }; });
    await check('tools', '11 expected tools discovered', async () => {
      const tools = []; let cursor;
      for (let page = 0; page < 20; page++) {
        const result = await mcp.rpc('tools/list', cursor ? { cursor } : {});
        assert(!result.error && Array.isArray(result.result?.tools));
        tools.push(...result.result.tools.map((tool) => tool.name)); cursor = result.result.nextCursor;
        if (!cursor) break;
      }
      assert(!cursor); validateTools(tools);
      return { tools };
    });
    await check('runtime-versions', 'Runtime versions match the built MCP package', async () => { assert.equal(health.version, version); assert.equal(initialized.serverInfo.version, version); return { version }; });
    report.versionCheck = await checkVersions(ROOT, { liveHealth: health.version, liveMcpServer: initialized.serverInfo.version });
    await check('tasks', 'Task create/read through MCP', async () => {
      const created = await mcp.call('sharman_create_task', { title: 'SHARMAN judge rehearsal task' });
      const tasks = await mcp.call('sharman_get_tasks', { status: 'open' });
      assert(tasks.tasks.some((task) => task.id === created.task.id));
      return { count: tasks.tasks.length };
    });
    await check('mailbox-provenance', 'Unconnected inbox never silently substitutes samples', async () => {
      const mail = await mcp.call('sharman_search_inbox', { query: 'Show recent emails', kind: 'recent', sample: false });
      assert.equal(mail.dataSource, 'unavailable'); assert.equal(mail.demoData, false); assert.equal(mail.items.length, 0); assert.equal(mail.messages.length, 0);
      return { source: mail.dataSource, privateMailboxQueried: false };
    });
    await check('daily-planner', 'DailyPlannerAgent completes with explicit sample provenance', async () => {
      const result = await mcp.call('sharman_assistant_request', { message: 'What needs my attention today? Show sample inbox.' });
      const agent = result.result.delegation;
      assert.equal(agent.name, 'DailyPlannerAgent'); assert.equal(agent.status, 'terminated');
      assert.equal(agent.permissionCeiling, 'READ_ONLY'); assert(agent.stepsUsed <= agent.maxSteps);
      assert.equal(agent.steps.length, agent.stepsUsed); assert(agent.terminatedAt);
      assert.equal(agent.dataSource, 'sample'); assert.match(result.answer, /sample.*not your real Gmail/i);
      assert(agent.priorities.some((item) => item.source === 'Sample Inbox'));
      assert(agent.deniedCapabilities.includes('system.exec'));
      return { dataSource: agent.dataSource, stepsUsed: agent.stepsUsed, maxSteps: agent.maxSteps, status: agent.status };
    });
    const reminderArgs = { title: 'SHARMAN isolated rehearsal reminder', remindAt: new Date(Date.now() + 120_000).toISOString(), sourceActionId: `${options.sessionId}:${sequence}` };
    await check('reminder', 'Reminder create', async () => {
      reminder = await mcp.call('sharman_create_reminder', reminderArgs);
      assert.equal(reminder.created, true); assert.equal(reminder.reminder.remindAt, reminderArgs.remindAt);
      return { created: true, storage: 'isolated_web_json', windowsNotificationTested: false };
    });
    await check('dedupe', 'Reminder duplicate guard', async () => {
      const again = await mcp.call('sharman_create_reminder', reminderArgs);
      assert.equal(again.created, false); assert.equal(again.reminder.id, reminder.reminder.id);
      const listed = await mcp.call('sharman_get_reminders'); assert.equal(listed.reminders.length, 1);
      return { storedReminders: listed.reminders.length };
    });
    await check('ring-event', 'Ring simulator event', async () => {
      workflow = await mcp.call('sharman_home_handle_ring_event', { scenario: 'expected_delivery' });
      assert.equal(workflow.event.source, 'ring-simulator'); assert.equal(workflow.event.kind, 'visitor');
      const state = await mcp.call('sharman_home_get_status'); assert(state.home.events.some((event) => event.id === workflow.event.id));
      return { simulation: true, eventPersisted: true };
    });
    await check('visitor-agent', 'VisitorAgent created with minimum permissions', async () => {
      assert.equal(workflow.agent.name, 'VisitorAgent'); assert.equal(workflow.agent.parentAgentId, 'sharman-supervisor');
      assert(workflow.agent.permissions.includes('ring.event.read')); assert(!workflow.agent.permissions.includes('door.unlock'));
      assert(workflow.agent.stepsUsed <= workflow.agent.maxSteps);
      return { name: workflow.agent.name, ceiling: workflow.agent.permissionCeiling };
    });
    let ringEventId;
    await check('ring-signed-fixture', 'Signed Ring-format fixture reaches VisitorAgent through HTTP', async () => {
      const now = Date.now();
      const payload = { meta: { version: '1.1', time: new Date(now).toISOString(), request_id: randomUUID(), account_id: 'fixture-ring-account' }, data: { id: randomUUID(), type: 'motion_detected', attributes: { source: 'fixture-ring-device', source_type: 'devices', timestamp: now, sub_type: 'human' } } };
      const body = JSON.stringify(payload, null, 2);
      const signature = `sha256=${createHmac('sha256', 'controlled-test-ring-key').update(body).digest('hex')}`;
      const response = await fetch(`${bridge.base}/webhooks/ring`, { method: 'POST', headers: { 'content-type': 'application/json', 'X-Signature': signature }, body, signal: AbortSignal.timeout(10_000) });
      assert.equal(response.status, 200); const receipt = await response.json(); ringEventId = receipt.eventId;
      const result = await mcp.call('sharman_home_get_status');
      assert.equal(result.home.lastWorkflow.event.id, ringEventId);
      assert.equal(result.home.lastWorkflow.event.ring.ingestionMode, 'signed_webhook');
      assert.equal(result.home.lastWorkflow.agent.name, 'VisitorAgent');
      return { signedLocalFixture: true, hardwareVerified: false, alexaSpeechDelivered: false };
    });
    await check('unlock-request', 'VisitorAgent door.unlock request recorded', async () => {
      workflow = await mcp.call('sharman_home_handle_ring_event', { scenario: 'blocked_unlock', eventId: ringEventId });
      assert.equal(workflow.agent.denialDetail.action, 'door.unlock');
      return { requestedAction: workflow.agent.denialDetail.action };
    });
    await check('required-permission', 'Required permission — HIGH_RISK', async () => { assert.equal(workflow.agent.denialDetail.requiredPermission, 'HIGH_RISK'); return { required: 'HIGH_RISK' }; });
    await check('delegated-ceiling', 'Delegated ceiling — LOW_RISK', async () => { assert.equal(workflow.agent.denialDetail.delegatedCeiling, 'LOW_RISK'); assert.equal(workflow.agent.permissionCeiling, 'LOW_RISK'); return { ceiling: 'LOW_RISK' }; });
    await check('denied', 'door.unlock DENIED and not exposed as an MCP tool', async () => {
      validateDenial(workflow);
      const forbidden = await mcp.rpc('tools/call', { name: 'door.unlock', arguments: {} });
      assert(forbidden.error || forbidden.result?.isError);
      return { decision: 'DENIED', actualLockControl: false };
    });
    await check('audit', 'Correlated denial audit recorded', async () => {
      audit = (await request('/api/audit')).events;
      report.denialRequestId = validateAudit(audit, workflow);
      return { denialAndDelegationCorrelated: true, requiredPermission: 'HIGH_RISK', delegatedCeiling: 'LOW_RISK' };
    });
    await check('termination', 'Worker terminated and receipt recorded', async () => {
      assert(workflow.agent.terminatedAt); assert(Date.parse(workflow.agent.terminatedAt) >= Date.parse(workflow.agent.createdAt));
      assert(audit.some((event) => event.requestId === report.denialRequestId && event.tool === 'agent.terminate'));
      return { terminated: true, auditCorrelated: true };
    });
    await check('local-only', 'Local Only blocks cloud on REST and MCP', async () => {
      const before = await request('/api/aws/status'); assert.equal(before.configured, true); assert.equal(before.lastAttemptAt, null);
      const rest = await request('/api/home/explain', {}); const viaMcp = await mcp.call('sharman_home_explain_with_bedrock');
      assert.equal(rest.mode, 'local_fallback'); assert.equal(viaMcp.mode, 'local_fallback');
      assert.match(rest.fallbackReason, /no AWS request/i); assert.match(viaMcp.fallbackReason, /no AWS request/i);
      assert.equal((await request('/api/aws/status')).lastAttemptAt, null); assert.equal(bridge.outboundAttempts(), 0);
      assert.deepEqual((await request('/api/home')).home.lastWorkflow, workflow);
      return { adapterEnabled: true, sdkAttemptsRecorded: 0, controlledEndpointRequests: 0, unchangedWorkflow: true };
    });
    await request('/api/settings/processing-mode', { processingMode: 'hybrid' });
    await mcp.call('sharman_home_handle_ring_event', { scenario: 'expected_delivery' });
    const beforeHybrid = await request('/api/home');
    if (options.live) {
      await check('hybrid-live', 'Hybrid — live Nova 2 Lite explanation', async () => {
        const explanation = await mcp.call('sharman_home_explain_with_bedrock');
        assert.equal(explanation.mode, 'bedrock'); assert.equal(explanation.modelId, 'global.amazon.nova-2-lite-v1:0'); assert.equal(explanation.region, 'ap-southeast-2');
        const status = await request('/api/aws/status'); assert.equal(status.connectionState, 'connected'); assert(status.lastSuccessAt);
        assert.deepEqual(await request('/api/home'), beforeHybrid);
        return { mode: 'bedrock', modelId: explanation.modelId, lastSuccessAt: status.lastSuccessAt, permissionStateUnchanged: true };
      });
    } else {
      await check('hybrid-controlled', 'Hybrid controlled outage preserves safe fallback', async () => {
        const result = await mcp.call('sharman_home_explain_with_bedrock');
        assert.equal(result.mode, 'local_fallback'); assert(bridge.outboundAttempts() >= 1 && bridge.outboundAttempts() <= 2);
        assert.deepEqual(await request('/api/home'), beforeHybrid);
        return { liveAws: false, controlledEndpointRequests: bridge.outboundAttempts(), unchangedWorkflow: true };
      });
      report.checks.push({ id: 'hybrid-live', label: 'Hybrid live Bedrock', status: 'SKIP', reason: 'Not opted in. Use --bedrock-live with a temporary AWS profile; a fallback is never counted as live inference.' });
      console.log('[SKIP] Hybrid live Bedrock — opt-in only');
    }
    if (options.mode === 'candidate') await check('candidate-versions', `All release versions — ${CANDIDATE_VERSION}`, async () => { assert(report.versionCheck.passed); return { expected: CANDIDATE_VERSION }; });
    await check('unchanged-candidate', 'Source and built artifacts unchanged during run', async () => { assert.equal(await sourceFingerprint(), report.sourceFingerprint); assert.equal(await artifactFingerprint(), report.artifactFingerprint); return { unchanged: true }; });
    report.passed = true;
  } catch (error) {
    report.error = { code: /^(?:REHEARSAL_CHECK_FAILED:[a-z-]+|SOURCE_SYMLINK_REQUIRES_REVIEW|CREDENTIAL_FILE_IN_SOURCE)$/.test(error.message) ? error.message : 'REHEARSAL_SETUP_OR_IO_FAILED', detail: 'Inspect the named gate and local source/build configuration. Raw HTTP/provider responses are intentionally excluded.' };
  } finally {
    try { await bridge?.cleanup(); report.cleanupPassed = true; } catch { report.cleanupPassed = false; report.passed = false; report.error = { code: 'OWNED_RESOURCE_CLEANUP_FAILED' }; }
    report.finishedAt = new Date().toISOString(); report.durationMs = Math.round(performance.now() - started);
  }
  return report;
}

export async function runRehearsals(options) {
  options = { ...options, sessionId: `${new Date().toISOString().replace(/[:.]/g, '-')}-${randomUUID().slice(0, 8)}` };
  const release = await evidenceLock(options.output);
  const directory = path.join(options.output, options.sessionId);
  const reports = [];
  try {
    await mkdir(directory);
    await atomicJson(path.join(options.output, 'summary.json'), { schemaVersion: 1, sessionId: options.sessionId, status: 'RUNNING', passed: false, recordingQualified: false, requestedRuns: options.runs, completedRuns: 0, streak: { smoke: 0, candidate: 0 }, evidence: path.relative(ROOT, directory).replaceAll(path.sep, '/') });
    // Mirror the requested run-N paths, but never let stale green runs masquerade
    // as this invocation. Full immutable-by-convention history stays per session.
    for (let run = 1; run <= 3; run++) await atomicJson(path.join(options.output, `run-${run}.json`), { sessionId: options.sessionId, run, status: 'NOT_RUN', recordingQualified: false });
    for (let run = 1; run <= options.runs; run++) {
      console.log(`\nSHARMAN AMAZON JUDGE REHEARSAL — run ${run}/${options.runs} (${options.mode})`);
      const report = await oneRun(options, run);
      report.streak = advanceStreak(reports.at(-1), report);
      reports.push(report);
      await atomicJson(path.join(directory, `run-${run}.json`), report);
      await atomicJson(path.join(options.output, `run-${run}.json`), report);
      const passed = report.checks.filter((check) => check.status === 'PASS').length;
      const failed = report.checks.filter((check) => check.status === 'FAIL').length;
      console.log(`Result: ${passed + failed ? `${passed}/${passed + failed} PASS` : 'FAILED before checks started'}; ${report.passed ? 'PASSED' : 'FAILED'}; ${(report.durationMs / 1000).toFixed(1)}s`);
      if (!report.passed) break; // A failure ends this clean-run sequence. Never auto-repair/replay it.
    }
    const summary = { schemaVersion: 1, sessionId: options.sessionId, mode: options.mode, candidate: CANDIDATE_VERSION, bedrockLive: options.live, passed: reports.length === options.runs && reports.every((report) => report.passed), requestedRuns: options.runs, completedRuns: reports.length, streak: reports.at(-1)?.streak || { smoke: 0, candidate: 0 }, recordingQualified: false, manualGates: ['Real Gmail sync/reconnect/privacy validation', 'Visible Windows notification', 'Reviewed release source snapshot and final installer', 'Approved data-source presentation and complete UI/owner rehearsals'], evidence: path.relative(ROOT, directory).replaceAll(path.sep, '/') };
    await atomicJson(path.join(directory, 'summary.json'), summary);
    await atomicJson(path.join(options.output, 'summary.json'), summary);
    console.log(`Evidence: ${directory}\nAutomated ${options.mode} evidence only. No RC/tag or recording qualification was created.`);
    return summary;
  } finally { await release(); }
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  try {
    const options = parseArgs(process.argv.slice(2));
    if (options.help) console.log('node scripts/judge-rehearsal.mjs [--runs 1|2|3] [--mode smoke|candidate] [--bedrock-live] [--aws-profile sharman-amazon] [--output target/qa/...]\nDefault: isolated sample-data smoke, no real AWS/Gmail/Windows notifications. --bedrock-live makes small billable AWS calls. Candidate mode requires 0.25.0-amazon-rc.1 everywhere.');
    else { const summary = await runRehearsals(options); if (!summary.passed) process.exitCode = 1; }
  } catch { console.error('Rehearsal could not start. Check options/builds, target/qa permissions, or an existing .runner.lock; no credentials were printed.'); process.exitCode = 1; }
}
