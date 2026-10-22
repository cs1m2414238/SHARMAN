import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { advanceStreak, CANDIDATE_VERSION, checkVersions, compareVersions, findSecrets, redactLog, scanSource, sourceFingerprint } from './checks.mjs';
import { decodeRpc, within } from './runtime.mjs';
import { parseArgs } from '../judge-rehearsal.mjs';
import { EXPECTED_TOOLS } from './checks.mjs';
import { validateAudit, validateDenial, validateTools } from './contracts.mjs';

async function fixture(operation) {
  const parent = await mkdtemp(path.join(tmpdir(), 'sharman-release-tests-'));
  try { return await operation(parent); }
  finally { assert(path.basename(parent).startsWith('sharman-release-tests-')); await rm(parent, { recursive: true, force: true }); }
}

test('versions fail on any mismatched runtime or lock value', () => {
  assert(compareVersions({ frontend: CANDIDATE_VERSION, health: CANDIDATE_VERSION }).passed);
  const result = compareVersions({ frontend: CANDIDATE_VERSION, health: 'old' });
  assert.equal(result.passed, false); assert.equal(result.mismatches[0].component, 'health');
  assert.equal(compareVersions({ lock: undefined }).passed, false);
  assert.equal(compareVersions({}).passed, false);
});

test('version inventory includes manifests, lock roots, health and MCP source/runtime', async () => fixture(async (root) => {
  await mkdir(path.join(root, 'frontend/src-tauri'), { recursive: true });
  await mkdir(path.join(root, 'sharman-mcp/src'), { recursive: true });
  await mkdir(path.join(root, 'sharman-web/src'), { recursive: true });
  await writeFile(path.join(root, 'Cargo.toml'), `[workspace.package]\nversion = "${CANDIDATE_VERSION}"\n`);
  await writeFile(path.join(root, 'Cargo.lock'), ['agentrelay-core', 'agentrelay-desktop'].map((name) => `[[package]]\nname = "${name}"\nversion = "${CANDIDATE_VERSION}"\n`).join('\n'));
  await writeFile(path.join(root, 'frontend/src-tauri/tauri.conf.json'), JSON.stringify({ version: CANDIDATE_VERSION }));
  for (const folder of ['frontend', 'sharman-mcp', 'sharman-web']) {
    await writeFile(path.join(root, folder, 'package.json'), JSON.stringify({ version: CANDIDATE_VERSION }));
    await writeFile(path.join(root, folder, 'package-lock.json'), JSON.stringify({ version: CANDIDATE_VERSION, packages: { '': { version: CANDIDATE_VERSION } } }));
  }
  await writeFile(path.join(root, 'sharman-mcp/src/server.ts'), `const server = { name: 'sharman-local-supervisor', version: '${CANDIDATE_VERSION}' }; app.get('/health', () => ({ version: '${CANDIDATE_VERSION}' }));`);
  await writeFile(path.join(root, 'sharman-web/src/mcp.ts'), `const clientInfo: { name: 'sharman-alexa-web', version: '${CANDIDATE_VERSION}' };`);
  assert((await checkVersions(root, { liveHealth: CANDIDATE_VERSION, liveMcpServer: CANDIDATE_VERSION })).passed);
  const lock = JSON.parse(await readFile(path.join(root, 'frontend/package-lock.json'), 'utf8')); lock.packages[''].version = 'old';
  await writeFile(path.join(root, 'frontend/package-lock.json'), JSON.stringify(lock));
  const failed = await checkVersions(root, { liveHealth: 'old' });
  assert(failed.mismatches.some((item) => item.component === 'frontend:lockRoot'));
  assert(failed.mismatches.some((item) => item.component === 'liveHealth'));
}));

test('secret findings contain only rule/path/line, never matched values', () => {
  const credential = 'AKIA' + 'B'.repeat(16);
  const findings = findSecrets(`const value = '${credential}';\nconst refresh_token = '${'1/' + '/' + 'Z'.repeat(36)}';`, 'fixture.ts');
  assert(findings.some((item) => item.rule === 'aws-access-key'));
  assert(findings.some((item) => item.rule === 'google-refresh-token'));
  assert(!JSON.stringify(findings).includes(credential));
  assert(findings.every((item) => Object.keys(item).sort().join(',') === 'file,line,rule'));
});

test('secret assignment placeholders do not mask structured provider tokens', () => {
  assert.equal(findSecrets('const api_key = "fixture-not-a-real-secret";').length, 0);
  assert(findSecrets(`const password = '${'sensitive'.repeat(3)}';`).length > 0);
  const token = 'ghp_' + 'A'.repeat(36);
  assert(findSecrets(`const value = '${token}';`).some((item) => item.rule === 'github-token'));
  assert(!redactLog(token).includes(token));
});

test('source scan excludes private/runtime build folders and flags credential files without reading values', async () => fixture(async (root) => {
  await mkdir(path.join(root, 'target')); await mkdir(path.join(root, 'workspace'));
  const key = 'AKIA' + 'C'.repeat(16);
  await writeFile(path.join(root, 'target/private.json'), JSON.stringify({ value: key }));
  await writeFile(path.join(root, 'workspace/private.json'), JSON.stringify({ value: key }));
  await writeFile(path.join(root, 'source.ts'), 'export const safe = true;');
  await writeFile(path.join(root, '.env.example'), 'SHARMAN_AWS_BEDROCK_ENABLED=false');
  assert((await scanSource(root)).passed);
  await writeFile(path.join(root, '.env'), `OWNER_VALUE=${key}`);
  const result = await scanSource(root);
  assert.equal(result.passed, false); assert.equal(result.findings[0].rule, 'credential-file-needs-exclusion');
  assert(!JSON.stringify(result).includes(key));
}));

test('source fingerprint changes with code, not generated evidence or documentation status', async () => fixture(async (root) => {
  await mkdir(path.join(root, 'target')); await mkdir(path.join(root, 'docs'));
  await writeFile(path.join(root, 'app.ts'), 'version one');
  const first = await sourceFingerprint(root);
  await writeFile(path.join(root, 'target/report.json'), '{}'); await writeFile(path.join(root, 'docs/progress.md'), 'updated');
  assert.equal(await sourceFingerprint(root), first);
  await writeFile(path.join(root, 'app.ts'), 'version two'); assert.notEqual(await sourceFingerprint(root), first);
  const second = await sourceFingerprint(root);
  await mkdir(path.join(root, '.agentrelay')); await writeFile(path.join(root, '.agentrelay/environment.toml'), 'profile = "nativeWindows"');
  assert.notEqual(await sourceFingerprint(root), second);
}));

test('consecutive counters reset on failure, changed source/artifacts, mode or AWS path', () => {
  const report = { passed: true, sourceFingerprint: 'source', artifactFingerprint: 'build', mode: 'candidate', bedrockLive: false, versionCheck: { passed: true } };
  const first = { ...report, streak: advanceStreak(null, report) };
  assert.equal(advanceStreak(first, report).candidate, 2);
  assert.deepEqual(advanceStreak(first, { ...report, passed: false }), { smoke: 0, candidate: 0 });
  for (const patch of [{ sourceFingerprint: 'new' }, { artifactFingerprint: 'new' }, { mode: 'smoke' }, { bedrockLive: true }]) assert.equal(advanceStreak(first, { ...report, ...patch }).smoke, 1);
  assert.equal(advanceStreak(first, { ...report, versionCheck: { passed: false } }).candidate, 0);
});

test('SSE/json MCP decoder selects the exact response ID and rejects missing results', () => {
  const stream = 'event: message\r\ndata: {"jsonrpc":"2.0","id":1,"result":{}}\r\n\r\nevent: message\r\ndata: {"jsonrpc":"2.0","id":2,"result":{"ok":true}}\r\n\r\n';
  assert.equal(decodeRpc(stream, 'text/event-stream', 2).result.ok, true);
  assert.equal(decodeRpc('{"jsonrpc":"2.0","id":3,"result":{}}', 'application/json', 3).id, 3);
  assert.throws(() => decodeRpc(stream, 'text/event-stream', 99), /NO_MATCHING/);
  assert.throws(() => decodeRpc('{"id":3,"result":{}}', 'application/json', 3));
  assert.throws(() => decodeRpc('{"jsonrpc":"2.0","id":3,"result":{},"error":{}}', 'application/json', 3));
});

test('CLI rejects unsafe/unknown destinations and invalid budgets', () => {
  assert.equal(parseArgs(['--runs', '3']).runs, 3);
  assert.equal(parseArgs([]).live, false);
  for (const args of [['--runs', '0'], ['--runs', '4'], ['--mode', 'green'], ['--base-url', 'https://example.invalid'], ['--runs']]) assert.throws(() => parseArgs(args));
  assert.equal(within(path.resolve('target/qa'), path.resolve('target/qa/../source')), false);
  assert.equal(within(path.resolve('target/qa'), path.resolve('target/qa/judge')), true);
});

test('MCP contracts reject missing, duplicated and unexpected dangerous tools', () => {
  validateTools(EXPECTED_TOOLS);
  assert.throws(() => validateTools(EXPECTED_TOOLS.slice(1)));
  assert.throws(() => validateTools([...EXPECTED_TOOLS, EXPECTED_TOOLS[0]]));
  assert.throws(() => validateTools([...EXPECTED_TOOLS, 'door.unlock']));
});

test('denial contract fails if risk/ceiling/result or authority is altered', () => {
  const workflow = { decision: 'denied', agent: { name: 'VisitorAgent', parentAgentId: 'sharman-supervisor', permissions: ['ring.event.read'], forbidden: ['door.unlock'], permissionCeiling: 'LOW_RISK', maxSteps: 6, stepsUsed: 3, denialDetail: { action: 'door.unlock', requiredPermission: 'HIGH_RISK', delegatedCeiling: 'LOW_RISK' } } };
  validateDenial(workflow);
  for (const patch of [{ permissionCeiling: 'HIGH_RISK' }, { permissions: ['door.unlock'] }, { forbidden: [] }, { stepsUsed: 7 }, { denialDetail: { ...workflow.agent.denialDetail, requiredPermission: 'LOW_RISK' } }]) assert.throws(() => validateDenial({ ...workflow, agent: { ...workflow.agent, ...patch } }));
  assert.throws(() => validateDenial({ ...workflow, decision: 'allowed' }));
});

test('audit/termination contract must reference the same request, not an older denial', () => {
  const at = '2026-10-20T12:00:00Z';
  const workflow = { event: { at }, agent: { createdAt: at, terminatedAt: at } };
  const denial = { tool: 'door.unlock', at, requestId: 'current', permission: 'DENIED', requiredPermission: 'HIGH_RISK', delegatedCeiling: 'LOW_RISK', denialReason: 'Above ceiling' };
  const events = [denial, { requestId: 'current', tool: 'agent.delegate' }, { requestId: 'current', tool: 'agent.terminate' }];
  assert.equal(validateAudit(events, workflow), 'current');
  assert.throws(() => validateAudit([denial, events[1], { ...events[2], requestId: 'old' }], workflow));
  assert.throws(() => validateAudit(events, { ...workflow, agent: { ...workflow.agent, terminatedAt: null } }));
});

test('the existing Rust dummy-vault exception is narrow and never exempts real provider tokens', () => {
  const line = JSON.stringify({ client_id: 'test.apps.googleusercontent.com', refresh_token: ['saved', 'refresh', 'token'].join('-') }) + '\n';
  assert.equal(findSecrets(line, 'frontend/src-tauri/src/gmail.rs').length, 0);
  assert(findSecrets(line, 'production.ts').length > 0);
  const provider = '1/' + '/' + 'P'.repeat(40);
  assert(findSecrets(line.replace('saved-refresh-token', provider), 'frontend/src-tauri/src/gmail.rs').some((item) => item.rule === 'google-refresh-token'));
});

test('Ring denial audit correlates by request ID when detection predates workflow evaluation', () => {
  const evaluatedAt = '2026-10-21T02:15:40Z';
  const workflow = { requestId: 'ring-permission-check', event: { at: '2026-10-21T02:15:30Z' }, agent: { createdAt: evaluatedAt, terminatedAt: evaluatedAt } };
  const events = [
    { tool: 'door.unlock', at: evaluatedAt, requestId: workflow.requestId, permission: 'DENIED', requiredPermission: 'HIGH_RISK', delegatedCeiling: 'LOW_RISK', denialReason: 'Above ceiling' },
    { tool: 'agent.delegate', requestId: workflow.requestId },
    { tool: 'agent.terminate', requestId: workflow.requestId },
  ];
  assert.equal(validateAudit(events, workflow), workflow.requestId);
  assert.throws(() => validateAudit(events, { ...workflow, requestId: 'different-workflow' }));
});
