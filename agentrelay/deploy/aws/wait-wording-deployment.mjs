import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { promisify } from 'node:util';
import { setTimeout as delay } from 'node:timers/promises';

const run = promisify(execFile);
const aws = 'C:\\Program Files\\Amazon\\AWSCLIV2\\aws.exe';
const arn = 'arn:aws:ecs:ap-southeast-2:926642724277:service-deployment/sharman-judging/sharman-amazon-demo/uam-65TUF32XQqPW8IecX';
const reportFile = path.resolve(import.meta.dirname, '../../target/qa/web-copy-deployment-20261021/rollout.json');
const deadline = Date.now() + 12 * 60_000;
const timeline = [];
let previous;
while (Date.now() < deadline) {
  const { stdout } = await run(aws, ['ecs', 'describe-service-deployments', '--profile', 'sharman-amazon', '--region', 'ap-southeast-2', '--service-deployment-arns', arn, '--output', 'json'], { windowsHide: true, timeout: 20_000, env: { ...process.env, AWS_PAGER: '' } });
  const response = JSON.parse(stdout);
  assert(!response.failures?.length, 'AWS could not describe the exact deployment');
  const state = response.serviceDeployments[0];
  assert.equal(state.serviceDeploymentArn, arn);
  const progress = {
    status: state.status, stage: state.lifecycleStage,
    productionTraffic: state.targetServiceRevision.requestedProductionTrafficWeight,
    newRunning: state.targetServiceRevision.runningTaskCount,
    pending: state.targetServiceRevision.pendingTaskCount,
    alarmStatus: state.alarms?.status, triggeredAlarms: state.alarms?.triggeredAlarmNames,
  };
  if (JSON.stringify(progress) !== previous) {
    timeline.push({ observedAt: new Date().toISOString(), ...progress });
    console.log(JSON.stringify(timeline.at(-1)));
    previous = JSON.stringify(progress);
  }
  await writeFile(reportFile, JSON.stringify({ passed: state.status === 'SUCCESSFUL', arn, timeline, finalState: state }, null, 2));
  if (state.status === 'SUCCESSFUL') {
    assert.equal(progress.productionTraffic, 100);
    assert.equal(progress.newRunning, 1);
    assert.equal(progress.pending, 0);
    assert.deepEqual(progress.triggeredAlarms, []);
    console.log('Exact ECS deployment completed successfully with rollback protection intact.');
    process.exit(0);
  }
  assert.equal(state.status, 'IN_PROGRESS', `Deployment did not succeed: ${state.status}`);
  assert.deepEqual(progress.triggeredAlarms, []);
  await delay(30_000);
}
throw new Error('Deployment did not complete within twelve minutes; inspect AWS before any further write.');
