import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { access, mkdir, readFile, realpath, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { CANDIDATE_VERSION, checkVersions, redactLog, ROOT, scanSource, sourceFingerprint } from './release/checks.mjs';
import { atomicJson, evidenceLock } from './release/runtime.mjs';
import { parseArgs, runRehearsals } from './judge-rehearsal.mjs';

async function npmCli() {
  const candidates = [process.env.npm_execpath, path.join(path.dirname(process.execPath), 'node_modules/npm/bin/npm-cli.js')].filter(Boolean);
  for (const directory of (process.env.PATH || '').split(path.delimiter)) {
    candidates.push(path.join(directory, 'node_modules/npm/bin/npm-cli.js'), path.resolve(directory, '../lib/node_modules/npm/bin/npm-cli.js'));
    if (process.platform !== 'win32') { try { candidates.push(await realpath(path.join(directory, 'npm'))); } catch {} }
  }
  for (const candidate of candidates) { if (!candidate.endsWith('npm-cli.js')) continue; try { await access(candidate); return candidate; } catch {} }
  throw new Error('NPM_CLI_NOT_FOUND');
}

async function commandStep(name, executable, args, directory, timeoutMs = 20 * 60_000) {
  const startedAt = new Date().toISOString();
  const start = performance.now();
  const env = { ...process.env, SHARMAN_NATIVE_EXECUTABLE: '', SHARMAN_NATIVE_DATA_DIR: '', SHARMAN_AWS_BEDROCK_ENABLED: 'false', AWS_EC2_METADATA_DISABLED: 'true' };
  for (const key of ['AWS_ACCESS_KEY_ID', 'AWS_SECRET_ACCESS_KEY', 'AWS_SESSION_TOKEN']) delete env[key];
  const child = spawn(executable, args, { cwd: ROOT, env, shell: false, windowsHide: true, detached: process.platform !== 'win32', stdio: ['ignore', 'pipe', 'pipe'] });
  const chunks = { stdout: '', stderr: '' };
  const cap = 256 * 1024;
  let truncated = false;
  for (const stream of ['stdout', 'stderr']) child[stream].on('data', (data) => {
    const remaining = cap - chunks[stream].length;
    if (data.length > remaining) truncated = true;
    if (remaining > 0) chunks[stream] += String(data).slice(0, remaining);
  });
  let timedOut = false;
  let spawnFailed = false;
  const timer = setTimeout(() => {
    timedOut = true;
    if (child.exitCode !== null || child.signalCode !== null || !child.pid) return;
    // Kill only the process tree created by this command, never by image name.
    if (process.platform === 'win32') {
      const killer = spawn('taskkill.exe', ['/PID', String(child.pid), '/T', '/F'], { windowsHide: true, stdio: 'ignore' });
      killer.on('error', () => child.kill());
    } else { try { process.kill(-child.pid, 'SIGKILL'); } catch { child.kill('SIGKILL'); } }
  }, timeoutMs);
  const code = await new Promise((resolve) => { child.once('error', () => { spawnFailed = true; resolve(null); }); child.once('close', resolve); });
  clearTimeout(timer);
  const status = !spawnFailed && !timedOut && code === 0 ? 'PASS' : 'FAIL';
  const slug = name.toLowerCase().replace(/[^a-z0-9]+/g, '-');
  for (const stream of ['stdout', 'stderr']) await writeFile(path.join(directory, `${slug}.${stream}.log`), redactLog(chunks[stream]) + (truncated ? '\n[Output truncated]\n' : ''));
  return { name, status, exitCode: code, startedAt, durationMs: Math.round(performance.now() - start), timedOut, spawnFailed, logs: [`${slug}.stdout.log`, `${slug}.stderr.log`] };
}

export async function validateAmazon(args = []) {
  // Accept only the rehearsal options, never arbitrary commands/shell text.
  const options = parseArgs(args);
  if (options.help) { console.log('npm run validate:amazon -- [--runs 1|2|3] [--bedrock-live] [--aws-profile sharman-amazon]\nBuild/test commands are local. Live AWS is explicitly opt-in. Exact RC versions are required; this does not create/tag/install a release or qualify a video.'); return { passed: true }; }
  const root = path.join(ROOT, 'target/qa/amazon-preflight');
  const release = await evidenceLock(root);
  const session = `${new Date().toISOString().replace(/[:.]/g, '-')}-${randomUUID().slice(0, 8)}`;
  const directory = path.join(root, session);
  const summary = { schemaVersion: 1, candidate: CANDIDATE_VERSION, startedAt: new Date().toISOString(), passed: false, readyForRehearsal: false, releaseCreated: false, recordingQualified: false, steps: [], evidence: path.relative(ROOT, directory).replaceAll(path.sep, '/'), manualGates: ['Live Gmail/reconnect/privacy evidence', 'Visible Windows notification', 'Reviewed source snapshot and final RC installer', 'Owner UI rehearsals and recording'] };
  let started;
  try {
    await mkdir(directory);
    await atomicJson(path.join(root, 'summary.json'), { ...summary, status: 'RUNNING' });
    console.log('\n=================================\n SHARMAN AMAZON RELEASE PREFLIGHT\n=================================');
    const scan = await scanSource();
    await atomicJson(path.join(directory, 'source-scan.json'), scan);
    summary.steps.push({ name: 'Source secret-pattern scan', status: scan.passed ? 'PASS' : 'FAIL', scannedFiles: scan.scannedFiles });
    console.log(`[${scan.passed ? 'PASS' : 'FAIL'}] Source secret-pattern scan`);
    if (!scan.passed) throw new Error('SOURCE_SCAN_FAILED'); // Do not compile/log source containing a suspected secret.
    started = await sourceFingerprint(); summary.sourceFingerprint = started;
    const npm = await npmCli();
    const commands = [
      ['Release runner tests', process.execPath, ['--test', 'scripts/release/checks.test.mjs']],
      ['Rust format', 'cargo', ['fmt', '--all', '--', '--check']],
      ['Rust tests', 'cargo', ['test', '--workspace', '--locked']],
      ['Clippy', 'cargo', ['clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings']],
      ['MCP tests', process.execPath, [npm, '--prefix', 'sharman-mcp', 'test']],
      ['MCP E2E', process.execPath, [npm, '--prefix', 'sharman-mcp', 'run', 'test:e2e']],
      ['Native frontend build', process.execPath, [npm, '--prefix', 'frontend', 'run', 'build']],
      ['Web production build', process.execPath, [npm, '--prefix', 'sharman-web', 'run', 'build']],
      ['MCP production build', process.execPath, [npm, '--prefix', 'sharman-mcp', 'run', 'build']],
    ];
    for (const [name, executable, commandArgs] of commands) {
      console.log(`[RUN ] ${name}`);
      const result = await commandStep(name, executable, commandArgs, directory);
      summary.steps.push(result); console.log(`[${result.status}] ${name} — ${(result.durationMs / 1000).toFixed(1)}s`);
    }
    const versions = await checkVersions();
    await atomicJson(path.join(directory, 'versions.json'), versions);
    summary.steps.push({ name: 'Version consistency', status: versions.passed ? 'PASS' : 'FAIL', expected: CANDIDATE_VERSION, mismatches: versions.mismatches });
    console.log(`[${versions.passed ? 'PASS' : 'FAIL'}] Version consistency — ${CANDIDATE_VERSION}`);
    // A failed build must never be followed by a rehearsal of stale artifacts.
    const buildsPassed = summary.steps.filter((step) => /build/.test(step.name)).every((step) => step.status === 'PASS');
    if (buildsPassed) {
      const rehearsal = await runRehearsals({ ...options, mode: 'candidate' });
      summary.steps.push({ name: 'Judge flow', status: rehearsal.passed ? 'PASS' : 'FAIL', report: rehearsal.evidence, streak: rehearsal.streak });
    } else summary.steps.push({ name: 'Judge flow', status: 'BLOCKED', reason: 'Production build failed; stale artifacts were not rehearsed.' });
    const unchanged = (await sourceFingerprint()) === started;
    summary.steps.push({ name: 'Unchanged source', status: unchanged ? 'PASS' : 'FAIL' });
    summary.passed = summary.steps.every((step) => step.status === 'PASS');
    summary.readyForRehearsal = summary.passed;
  } catch (error) {
    summary.error = { code: ['SOURCE_SCAN_FAILED', 'NPM_CLI_NOT_FOUND', 'SOURCE_SYMLINK_REQUIRES_REVIEW', 'CREDENTIAL_FILE_IN_SOURCE'].includes(error.message) ? error.message : 'PREFLIGHT_SETUP_OR_IO_FAILED', detail: 'Check the local evidence and source configuration. No raw private/provider payload was recorded.' };
  } finally {
    summary.finishedAt = new Date().toISOString();
    await atomicJson(path.join(directory, 'summary.json'), summary);
    await atomicJson(path.join(root, 'summary.json'), summary);
    await release();
  }
  console.log('\n=================================');
  for (const step of summary.steps) console.log(`${step.name.padEnd(29)} ${step.status}`);
  console.log(`Candidate target: v${CANDIDATE_VERSION}\n${summary.readyForRehearsal ? 'READY FOR OWNER REHEARSAL — manual live/installer/UI gates remain' : 'NOT READY — failed or blocked gates remain'}\nEvidence: ${directory}`);
  return summary;
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  try { if (!(await validateAmazon(process.argv.slice(2))).passed) process.exitCode = 1; }
  catch { console.error('Preflight could not start; check options, target/qa permissions or an existing .runner.lock.'); process.exitCode = 1; }
}
