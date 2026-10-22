// Narrow REL-013 rollout: mechanical copy-only edits to the verified deployed
// static bundle. Do NOT rebuild/publish the current pending backend/email work.
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import { cp, copyFile, lstat, mkdir, readFile, readdir, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { promisify } from 'node:util';

const run = promisify(execFile);
const root = path.resolve(import.meta.dirname, '../..');
const qa = path.join(root, 'target/qa/web-copy-deployment-20261021');
const baseline = path.join(qa, 'baseline');
const context = path.join(qa, 'overlay-context');
const baseImage = 'sharman-amazon-demo:tasks-ui-20261020';
const digest = 'sha256:8f847156467d9ba7b909a5b97c10e1c9f59d8ae9a5f7422c45afc0e95c187e20';
const inspected = JSON.parse((await run('docker', ['image', 'inspect', baseImage], { windowsHide: true })).stdout)[0];
assert(inspected.RepoDigests.includes(`926642724277.dkr.ecr.ap-southeast-2.amazonaws.com/sharman-amazon-demo@${digest}`));
assert.equal(inspected.Config.User, '65532:65532');

async function inventory(directory, relative = '') {
  const files = {};
  for (const entry of await readdir(directory)) {
    const filename = path.join(directory, entry);
    const info = await lstat(filename);
    assert(!info.isSymbolicLink(), 'Static overlay must not contain symlinks');
    const key = path.posix.join(relative, entry);
    if (info.isDirectory()) Object.assign(files, await inventory(filename, key));
    else {
      assert(info.isFile() && info.size < 5_000_000, 'Unexpected static payload');
      files[key] = createHash('sha256').update(await readFile(filename)).digest('hex');
    }
  }
  return files;
}
const before = await inventory(baseline);
const index = await readFile(path.join(baseline, 'index.html'), 'utf8');
const scripts = [...index.matchAll(/src="(\/assets\/[^"/]+\.js)"/g)];
assert.equal(scripts.length, 1, 'Expected the single deployed entry bundle');
const oldScript = scripts[0][1].slice(1);
let code = await readFile(path.join(baseline, oldScript), 'utf8');
const approved = [
  ['(0,V.jsxs)(`div`,{className:`demo-pill`,children:[(0,V.jsx)(`span`,{}),` AMAZON HACKATHON DEMO`]}),', ''],
  ['SHARMAN HOME · RING TRACK', 'SHARMAN HOME'],
  ['Hackathon simulation', 'Simulated home events'],
];
for (const [from, to] of approved) {
  assert.equal(code.split(from).length - 1, 1, `Expected exactly one approved source fragment: ${from}`);
  code = code.replace(from, to);
}
assert(!/AMAZON HACKATHON DEMO|RING TRACK|Hackathon simulation/.test(code));
assert(code.includes('Ring events and Alexa+ speech are simulated'));
const hash = createHash('sha256').update(code).digest('hex');
const newScript = `assets/index-wording-${hash.slice(0, 12)}.js`;
await mkdir(context); // Refuse reuse of an earlier context.
await cp(baseline, path.join(context, 'web'), { recursive: true, errorOnExist: true });
await rename(path.join(context, 'web', oldScript), path.join(context, 'web', newScript));
await writeFile(path.join(context, 'web', newScript), code);
await writeFile(path.join(context, 'web/index.html'), index.replace(`/${oldScript}`, `/${newScript}`));
await copyFile(path.join(root, 'deploy/aws/Dockerfile.wording-only'), path.join(context, 'Dockerfile'));
await run(process.execPath, ['--check', path.join(context, 'web', newScript)], { windowsHide: true });
const after = await inventory(path.join(context, 'web'));
assert.equal(Object.keys(before).length, Object.keys(after).length);
for (const [file, fingerprint] of Object.entries(before)) {
  if (file !== 'index.html' && file !== oldScript) assert.equal(after[file], fingerprint, `Unrelated asset changed: ${file}`);
}
const receipt = {
  timestamp: new Date().toISOString(), baseImage, baseDigest: digest, baseImageId: inspected.Id,
  context: path.relative(root, context), changedFiles: ['index.html', oldScript],
  newScript, approvedEdits: ['remove competition badge', 'shorten Home kicker', 'neutral simulation label'],
  simulationDisclosurePreserved: true, otherStaticAssetsUnchanged: true,
  runtimeFromUnchangedBaseImage: true, before, after,
};
await writeFile(path.join(qa, 'overlay-receipt.json'), JSON.stringify(receipt, null, 2));
console.log(JSON.stringify({ context: receipt.context, newScript, baseDigest: digest, backend: 'unchanged' }));
