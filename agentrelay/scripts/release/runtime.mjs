import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { once } from 'node:events';
import { mkdir, mkdtemp, open, readdir, readFile, realpath, rename, rm, unlink, writeFile } from 'node:fs/promises';
import { createServer as createHttp2Server } from 'node:http2';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { PROTOCOL_VERSION, ROOT } from './checks.mjs';

export const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
export const within = (parent, child) => { const relative = path.relative(parent, child); return relative !== '' && !relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative); };

export async function atomicJson(filename, value) {
  const temporary = `${filename}.${process.pid}.tmp`;
  await writeFile(temporary, JSON.stringify(value, null, 2) + '\n', { flag: 'wx' });
  try { await rename(temporary, filename); } catch (error) { await unlink(temporary).catch(() => {}); throw error; }
}

export async function evidenceLock(directory) {
  assert(within(path.join(ROOT, 'target/qa'), path.resolve(directory)), 'Evidence output must be inside target/qa, not a source/private directory');
  await mkdir(directory, { recursive: true });
  const filename = path.join(directory, '.runner.lock');
  let handle;
  try { handle = await open(filename, 'wx'); } catch { throw new Error('EVIDENCE_DIRECTORY_LOCKED: another runner or a stale lock needs review'); }
  await handle.writeFile(String(process.pid));
  return async () => { await handle.close(); await unlink(filename); };
}

export async function artifactFingerprint() {
  const hash = createHash('sha256');
  for (const relative of ['frontend/dist', 'sharman-web/dist', 'sharman-mcp/dist']) {
    const start = path.join(ROOT, relative);
    async function walk(directory) {
      for (const entry of (await readdir(directory, { withFileTypes: true })).sort((a, b) => a.name.localeCompare(b.name))) {
        const filename = path.join(directory, entry.name);
        assert(!entry.isSymbolicLink(), 'Build artifacts must not contain symlinks');
        if (entry.isDirectory()) await walk(filename);
        else hash.update(path.relative(ROOT, filename)).update('\0').update(await readFile(filename)).update('\0');
      }
    }
    await walk(start);
  }
  return hash.digest('hex');
}

export async function stopOwnedChild(child) {
  if (!child || !child.pid || child.exitCode !== null || child.signalCode !== null) return;
  const exited = once(child, 'exit');
  child.kill();
  await Promise.race([exited, delay(5_000)]);
  if (child.exitCode === null && child.signalCode === null) { child.kill('SIGKILL'); await Promise.race([exited, delay(5_000)]); }
  assert(child.exitCode !== null || child.signalCode !== null, 'Owned server failed to stop');
}

export async function ownedBridge({ live = false, profile = 'sharman-amazon', ringFixture = false, ringPlaygroundLive = false } = {}) {
  const temporaryRoot = await realpath(tmpdir());
  const directory = await mkdtemp(path.join(temporaryRoot, 'sharman-judge-rehearsal-'));
  let child;
  let attempts = 0;
  const sessions = new Set();
  const trap = createHttp2Server((_request, response) => {
    attempts++;
    response.writeHead(503, { 'content-type': 'application/json' });
    response.end(JSON.stringify({ message: 'Controlled rehearsal unavailability' }));
  });
  trap.on('session', (session) => { sessions.add(session); session.once('close', () => sessions.delete(session)); });
  await new Promise((resolve) => trap.listen(0, '127.0.0.1', resolve));
  const cleanup = async () => {
    try { await stopOwnedChild(child); }
    finally {
      for (const session of sessions) session.destroy();
      await new Promise((resolve) => trap.close(resolve));
      const resolved = await realpath(directory);
      assert.equal(path.dirname(resolved), temporaryRoot);
      assert(path.basename(resolved).startsWith('sharman-judge-rehearsal-'));
      await rm(resolved, { recursive: true, force: true });
    }
  };
  try {
    const probe = createServer();
    await new Promise((resolve) => probe.listen(0, '127.0.0.1', resolve));
    const port = probe.address().port;
    await new Promise((resolve) => probe.close(resolve));
    const env = { ...process.env, SHARMAN_MCP_HOST: '127.0.0.1', SHARMAN_MCP_PORT: String(port), SHARMAN_MCP_ALLOWED_HOSTS: '', SHARMAN_MCP_DATA_DIR: directory, SHARMAN_NATIVE_EXECUTABLE: '', SHARMAN_NATIVE_DATA_DIR: '', SHARMAN_WEB_DIST: path.join(ROOT, 'sharman-web/dist'), SHARMAN_AWS_BEDROCK_ENABLED: 'true', AWS_EC2_METADATA_DISABLED: 'true', AWS_REGION: 'ap-southeast-2', SHARMAN_BEDROCK_MODEL_ID: 'global.amazon.nova-2-lite-v1:0' };
    Object.assign(env, { RING_ACCESS_TOKEN: '', RING_ENVIRONMENT: '', SHARMAN_RING_SIGNING_KEY: '', SHARMAN_RING_ACCOUNT_ID: '', SHARMAN_RING_DEVICE_ID: '', SHARMAN_RING_TEST_EVENTS_ENABLED: 'false' });
    if (ringFixture) Object.assign(env, { RING_ENVIRONMENT: 'playground', SHARMAN_RING_SIGNING_KEY: 'controlled-test-ring-key', SHARMAN_RING_ACCOUNT_ID: 'fixture-ring-account', SHARMAN_RING_DEVICE_ID: 'fixture-ring-device', SHARMAN_RING_TEST_EVENTS_ENABLED: 'true' });
    if (ringPlaygroundLive) {
      assert(!ringFixture && process.env.RING_ENVIRONMENT === 'playground' && process.env.RING_ACCESS_TOKEN, 'Configure a fresh Playground token locally before live verification.');
      Object.assign(env, { RING_ENVIRONMENT: 'playground', RING_ACCESS_TOKEN: process.env.RING_ACCESS_TOKEN });
    }
    for (const name of Object.keys(env)) if (name.startsWith('AWS_ENDPOINT_URL')) delete env[name];
    if (live) {
      assert(!process.env.AWS_ACCESS_KEY_ID && !process.env.AWS_SECRET_ACCESS_KEY && !process.env.AWS_SESSION_TOKEN, 'Use a temporary named AWS profile, not credential values in the environment');
      env.AWS_PROFILE = profile;
      env.AWS_IGNORE_CONFIGURED_ENDPOINT_URLS = 'true';
    } else {
      Object.assign(env, { AWS_ACCESS_KEY_ID: 'controlled-test-key', AWS_SECRET_ACCESS_KEY: 'controlled-test-secret', AWS_SESSION_TOKEN: '', AWS_PROFILE: '', AWS_ENDPOINT_URL_BEDROCK_RUNTIME: `http://127.0.0.1:${trap.address().port}`, AWS_IGNORE_CONFIGURED_ENDPOINT_URLS: 'false' });
    }
    const start = async () => {
    child = spawn(process.execPath, ['dist/server.js'], { cwd: path.join(ROOT, 'sharman-mcp'), env, windowsHide: true, stdio: ['ignore', 'ignore', 'ignore'] });
    let spawnFailed = false;
    child.on('error', () => { spawnFailed = true; });
    const base = `http://127.0.0.1:${port}`;
    let healthy = false;
    for (let attempt = 0; attempt < 200; attempt++) {
      assert(!spawnFailed && child.exitCode === null && child.signalCode === null, 'Owned bridge exited during startup');
      try { if ((await fetch(`${base}/health`, { signal: AbortSignal.timeout(500) })).ok) { healthy = true; break; } } catch {}
      await delay(50);
    }
    assert(healthy, 'Owned bridge did not become healthy');
    };
    const base = `http://127.0.0.1:${port}`;
    await start();
    return { base, cleanup, restart: async () => { await stopOwnedChild(child); await start(); }, outboundAttempts: () => attempts, pid: child.pid };
  } catch (error) { await cleanup(); throw error; }
}

export async function jsonRequest(base, route, body) {
  const response = await fetch(`${base}${route}`, { method: body === undefined ? 'GET' : 'POST', headers: { 'content-type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(35_000) });
  assert(response.ok, `HTTP_${response.status}:${route}`);
  return response.json();
}

export function decodeRpc(text, contentType, id) {
  const messages = contentType.includes('text/event-stream')
    ? text.split(/\r?\n\r?\n/).map((event) => event.split(/\r?\n/).filter((line) => line.startsWith('data:')).map((line) => line.slice(5).trimStart()).join('\n')).filter(Boolean)
    : [text];
  for (const message of messages) {
    const envelope = JSON.parse(message);
    if (envelope.id !== id) continue;
    assert.equal(envelope.jsonrpc, '2.0', 'Invalid JSON-RPC response version');
    assert(Object.hasOwn(envelope, 'result') !== Object.hasOwn(envelope, 'error'), 'MCP must return exactly one of result/error');
    return envelope;
  }
  throw new Error('MCP_NO_MATCHING_RESPONSE');
}

export function mcpClient(base) {
  let nextId = 0;
  let session;
  async function rpc(method, params = {}, notification = false) {
    const id = ++nextId;
    const response = await fetch(`${base}/mcp`, { method: 'POST', headers: { accept: 'application/json, text/event-stream', 'content-type': 'application/json', 'mcp-protocol-version': PROTOCOL_VERSION, ...(session ? { 'mcp-session-id': session } : {}) }, body: JSON.stringify({ jsonrpc: '2.0', ...(notification ? {} : { id }), method, params }), signal: AbortSignal.timeout(35_000) });
    assert(response.ok, `MCP_HTTP_${response.status}:${method}`);
    session = response.headers.get('mcp-session-id') || session;
    if (notification) { await response.arrayBuffer(); return null; }
    return decodeRpc(await response.text(), response.headers.get('content-type') || '', id);
  }
  return {
    rpc,
    async call(name, args = {}) {
      const envelope = await rpc('tools/call', { name, arguments: args });
      assert(!envelope.error && !envelope.result?.isError && envelope.result?.structuredContent, `MCP_TOOL_FAILED:${name}`);
      return envelope.result.structuredContent;
    },
  };
}
