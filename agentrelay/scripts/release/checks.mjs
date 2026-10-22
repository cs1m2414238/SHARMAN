import { createHash } from 'node:crypto';
import { lstat, readdir, readFile } from 'node:fs/promises';
import path from 'node:path';

export const ROOT = path.resolve(import.meta.dirname, '../..');
export const CANDIDATE_VERSION = '0.25.0-amazon-rc.1';
export const PROTOCOL_VERSION = '2025-11-25';
export const EXPECTED_TOOLS = [
  'sharman_get_tasks', 'sharman_assistant_request', 'sharman_get_action_items',
  'sharman_search_inbox', 'sharman_get_reminders', 'sharman_create_task',
  'sharman_create_reminder', 'sharman_open_allowed_url', 'sharman_home_get_status',
  'sharman_home_handle_ring_event', 'sharman_home_explain_with_bedrock',
];

// Deliberately do not inspect Windows app data, runtime workspaces or credentials.
// This is a bounded source scan, not a claim of exhaustive secret detection.
const EXCLUDED = new Set(['target', 'node_modules', 'dist', '.git', '.data', '.aws', 'workspace', '.agentrelay', '.agentrelay-worktrees', '.idea', '.vscode']);
const TEXT_EXTENSION = /\.(?:rs|toml|lock|json|[cm]?[jt]sx?|css|html|md|txt|ps1|ya?ml|nsh|sh)$/i;
const SENSITIVE_FILENAME = /^(?:\.env(?:\..+)?|credentials|client_secret.*\.json|token\.json|.*\.(?:pem|p12|pfx|key))$/i;
const isCredentialFile = (file) => SENSITIVE_FILENAME.test(path.basename(file)) && !/^\.env\.(?:example|sample|template)$/i.test(path.basename(file));

export async function sourceFiles(root = ROOT) {
  const files = [];
  async function walk(directory) {
    for (const entry of (await readdir(directory, { withFileTypes: true })).sort((a, b) => a.name.localeCompare(b.name))) {
      const filename = path.join(directory, entry.name);
      const relative = path.relative(root, filename).replaceAll(path.sep, '/');
      if (entry.isSymbolicLink()) throw new Error('SOURCE_SYMLINK_REQUIRES_REVIEW');
      if (entry.isDirectory()) { if (!EXCLUDED.has(entry.name)) await walk(filename); continue; }
      if (TEXT_EXTENSION.test(entry.name) || SENSITIVE_FILENAME.test(entry.name) || ['Dockerfile.amazon', '.dockerignore', '.gitignore'].includes(entry.name)) files.push(relative);
    }
  }
  await walk(root);
  // Include the trusted environment manifest without walking private runtime DBs.
  const environment = path.join(root, '.agentrelay/environment.toml');
  try {
    const metadata = await lstat(environment);
    if (metadata.isSymbolicLink()) throw new Error('SOURCE_SYMLINK_REQUIRES_REVIEW');
    if (metadata.isFile()) files.push('.agentrelay/environment.toml');
  } catch (error) { if (error.code !== 'ENOENT') throw error; }
  return files.sort();
}

const SECRET_PATTERNS = [
  ['private-key', /-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----/g],
  ['aws-access-key', /\b(?:AKIA|ASIA)[A-Z0-9]{16}\b/g],
  ['google-api-key', /\bAIza[0-9A-Za-z_-]{35}\b/g],
  ['google-refresh-token', /\b1\/\/[A-Za-z0-9_-]{30,}\b/g],
  ['github-token', /\b(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,})\b/g],
];
const ASSIGNMENT = /["']?\b(?:aws_secret_access_key|client_secret|refresh_token|api_key|access_token|password)\b["']?\s*[:=]\s*["']([^"'\r\n]{12,})["']/gi;
const PLACEHOLDER = /^(?:controlled-test-|test[-_]|fixture[-_]|dummy[-_]|example[-_]|synthetic[-_]|your[-_]|<|\[|\$\{|process\.env\.)/i;

export function findSecrets(content, file = 'source') {
  const findings = [];
  const record = (rule, index) => findings.push({ file, line: content.slice(0, index).split('\n').length, rule });
  for (const [rule, pattern] of SECRET_PATTERNS) {
    for (const match of content.matchAll(new RegExp(pattern.source, pattern.flags))) record(rule, match.index);
  }
  for (const match of content.matchAll(new RegExp(ASSIGNMENT.source, ASSIGNMENT.flags))) {
    // These pre-existing Rust vault fixtures are intentionally non-credentials.
    // Do not exempt a test file/module wholesale or hide real provider tokens.
    const fixture = file === 'frontend/src-tauri/src/gmail.rs'
      && ['saved-refresh-token', 'top-secret-value', 'refresh-secret-token'].includes(match[1])
      && content.slice(content.lastIndexOf('\n', match.index) + 1, content.indexOf('\n', match.index)).includes('test.apps.googleusercontent.com');
    if (!PLACEHOLDER.test(match[1]) && !fixture) record('literal-secret-assignment', match.index);
  }
  return findings;
}

export async function scanSource(root = ROOT) {
  const files = await sourceFiles(root);
  const findings = [];
  for (const file of files) {
    if (isCredentialFile(file)) { findings.push({ file, line: 0, rule: 'credential-file-needs-exclusion' }); continue; }
    const filename = path.join(root, file);
    if ((await lstat(filename)).size > 5 * 1024 * 1024) { findings.push({ file, line: 0, rule: 'oversize-source-needs-review' }); continue; }
    findings.push(...findSecrets(await readFile(filename, 'utf8'), file));
  }
  return { passed: findings.length === 0, scannedFiles: files.length, findings, scope: 'Active/legacy source text; excludes ignored runtime data, dependencies, build outputs and private Windows data. Pattern scan, not a security certification.' };
}

export async function sourceFingerprint(root = ROOT) {
  const hash = createHash('sha256');
  // Documentation-only status updates do not change the executable candidate.
  for (const file of (await sourceFiles(root)).filter((file) => !file.startsWith('docs/') && !file.endsWith('.md') && !file.endsWith('.txt'))) {
    if (isCredentialFile(file)) throw new Error('CREDENTIAL_FILE_IN_SOURCE');
    if ((await lstat(path.join(root, file))).size > 5 * 1024 * 1024) throw new Error('SOURCE_TOO_LARGE_REQUIRES_REVIEW');
    hash.update(file).update('\0').update(await readFile(path.join(root, file))).update('\0');
  }
  return hash.digest('hex');
}

function capture(text, regex, label) {
  const match = regex.exec(text);
  if (!match) throw new Error(`VERSION_NOT_FOUND:${label}`);
  return match[1];
}

export function compareVersions(versions, expected = CANDIDATE_VERSION) {
  return { passed: Object.keys(versions).length > 0 && Object.values(versions).every((value) => value === expected), expected, versions, mismatches: Object.entries(versions).filter(([, value]) => value !== expected).map(([component, actual]) => ({ component, actual: actual ?? 'MISSING', expected })) };
}

export async function checkVersions(root = ROOT, runtime = {}, expected = CANDIDATE_VERSION) {
  const readJson = async (file) => JSON.parse(await readFile(path.join(root, file), 'utf8'));
  const cargo = await readFile(path.join(root, 'Cargo.toml'), 'utf8');
  const lock = await readFile(path.join(root, 'Cargo.lock'), 'utf8');
  const server = await readFile(path.join(root, 'sharman-mcp/src/server.ts'), 'utf8');
  const webClient = await readFile(path.join(root, 'sharman-web/src/mcp.ts'), 'utf8');
  const versions = {
    workspace: capture(cargo, /\[workspace\.package\]\s+version\s*=\s*"([^"]+)"/, 'workspace'),
    tauri: (await readJson('frontend/src-tauri/tauri.conf.json')).version,
    mcpServerSource: capture(server, /name:\s*'sharman-local-supervisor',\s*version:\s*'([^']+)'/, 'server'),
    healthSource: capture(server, /app\.get\('\/health'[\s\S]*?version:\s*'([^']+)'/, 'health'),
    webMcpClient: capture(webClient, /clientInfo:\s*\{\s*name:\s*'sharman-alexa-web',\s*version:\s*'([^']+)'/, 'web-client'),
  };
  for (const name of ['agentrelay-core', 'agentrelay-desktop']) {
    versions[`cargoLock:${name}`] = capture(lock, new RegExp(`\\[\\[package\\]\\]\\r?\\nname = "${name}"\\r?\\nversion = "([^"]+)"`), name);
  }
  for (const directory of ['frontend', 'sharman-mcp', 'sharman-web']) {
    const manifest = await readJson(`${directory}/package.json`);
    const npmLock = await readJson(`${directory}/package-lock.json`);
    versions[directory] = manifest.version;
    versions[`${directory}:lock`] = npmLock.version;
    versions[`${directory}:lockRoot`] = npmLock.packages?.['']?.version;
  }
  return compareVersions({ ...versions, ...runtime }, expected);
}

export function advanceStreak(previous, report) {
  if (!report.passed) return { smoke: 0, candidate: 0 };
  const same = previous?.passed && previous.sourceFingerprint === report.sourceFingerprint && previous.artifactFingerprint === report.artifactFingerprint && previous.mode === report.mode && previous.bedrockLive === report.bedrockLive;
  const smoke = (same ? previous.streak.smoke : 0) + 1;
  const candidate = report.mode === 'candidate' && report.versionCheck?.passed ? (same ? previous.streak.candidate : 0) + 1 : 0;
  return { smoke, candidate };
}

export function redactLog(text) {
  let safe = text;
  for (const [, pattern] of SECRET_PATTERNS) safe = safe.replace(new RegExp(pattern.source, pattern.flags), '[REDACTED]');
  return safe.replace(new RegExp(ASSIGNMENT.source, ASSIGNMENT.flags), (match, value) => match.replace(value, '[REDACTED]'));
}
