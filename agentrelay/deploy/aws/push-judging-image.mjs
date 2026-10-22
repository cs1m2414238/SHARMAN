import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { request } from 'node:http';
import { promisify } from 'node:util';

// The Windows Docker credential helper can fail with "The stub received bad
// data". Use the documented Engine X-Registry-Auth header instead, keeping this
// short-lived ECR token in memory only. Never print it or change global config.
assert.equal(process.platform, 'win32', 'This deployment helper targets the verified Windows Docker Desktop pipe.');
const run = promisify(execFile);
const aws = 'C:\\Program Files\\Amazon\\AWSCLIV2\\aws.exe';
const repository = '926642724277.dkr.ecr.ap-southeast-2.amazonaws.com/sharman-amazon-demo';
const localImage = process.argv[2] || 'sharman-amazon-demo:tasks-ui-20261020';
assert(/^sharman-amazon-demo:[a-z0-9.-]+$/.test(localImage), 'Use a local SHARMAN judging image tag.');
const { stdout: inspected } = await run('docker', ['image', 'inspect', localImage, '--format', '{{.Id}}'], { windowsHide: true, timeout: 30_000 });
const digest = inspected.trim();
assert(/^sha256:[a-f0-9]{64}$/.test(digest));
const date = new Intl.DateTimeFormat('en-CA', { timeZone: 'Asia/Kolkata', year: 'numeric', month: '2-digit', day: '2-digit' }).format(new Date()).replaceAll('-', '');
const tag = `judging-${date}-${digest.slice(7, 15)}`;
await run('docker', ['tag', localImage, `${repository}:${tag}`], { windowsHide: true, timeout: 30_000 });
const { stdout } = await run(aws, ['ecr', 'get-login-password', '--profile', 'sharman-amazon', '--region', 'ap-southeast-2'], { windowsHide: true, timeout: 30_000 });
const password = stdout.trim();
assert(password.length > 20, 'No ECR authorization returned.');
const registryAuth = Buffer.from(JSON.stringify({ username: 'AWS', password, serveraddress: repository.split('/')[0] })).toString('base64url');

await new Promise((resolve, reject) => {
  const statuses = new Map();
  const push = request({
    socketPath: '\\\\.\\pipe\\dockerDesktopLinuxEngine', method: 'POST',
    path: `/v1.53/images/${encodeURIComponent(repository)}/push?tag=${tag}`,
    headers: { 'X-Registry-Auth': registryAuth }, timeout: 120_000,
  }, (response) => {
    if (response.statusCode !== 200) {
      response.resume();
      reject(new Error(`Docker push returned HTTP ${response.statusCode}.`));
      return;
    }
    let buffer = '';
    let failure = null;
    response.setEncoding('utf8');
    response.on('data', (chunk) => {
      buffer += chunk;
      let end;
      while ((end = buffer.indexOf('\n')) >= 0) {
        const line = buffer.slice(0, end).trim();
        buffer = buffer.slice(end + 1);
        if (!line) continue;
        try {
          const event = JSON.parse(line);
          if (event.error) failure = new Error(String(event.error).replaceAll(password, '[REDACTED]'));
          if (event.status && statuses.get(event.id || 'image') !== event.status) {
            statuses.set(event.id || 'image', event.status);
            console.log(`${event.id || 'image'}: ${String(event.status).replaceAll(password, '[REDACTED]')}`);
          }
        } catch { failure = new Error('Docker push returned an invalid progress record.'); }
      }
      if (buffer.length > 1_048_576) push.destroy(new Error('Docker push response exceeded its bound.'));
    });
    response.once('error', reject);
    response.once('end', () => failure ? reject(failure) : resolve());
  });
  push.once('timeout', () => push.destroy(new Error('Docker push stalled for two minutes.')));
  push.once('error', reject);
  push.end();
});
console.log(`Pushed ${repository}:${tag}; temporary registry credentials were not saved.`);
