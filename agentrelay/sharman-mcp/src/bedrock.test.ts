import assert from 'node:assert/strict';
import test from 'node:test';
import { ConverseCommand, type ConverseCommandOutput } from '@aws-sdk/client-bedrock-runtime';
import { BedrockCareExplainer } from './bedrock.js';
import type { HomeWorkflow } from './store.js';

const workflow = (scenario: HomeWorkflow['scenario']): HomeWorkflow => {
  const decision = scenario === 'blocked_unlock' ? 'denied' : scenario === 'repeated_unexpected' ? 'approval_required' : 'allowed';
  return {
    scenario,
    event: {
      id: 'event-test', at: '2026-10-12T16:22:00.000Z', source: 'ring-simulator', device: 'Front Door', kind: scenario === 'blocked_unlock' ? 'permission_denied' : 'visitor',
      title: 'Test event', summary: scenario === 'expected_delivery' ? 'Arrival matches the expected medicine delivery window' : 'No visit is scheduled', severity: scenario === 'blocked_unlock' ? 'blocked' : scenario === 'repeated_unexpected' ? 'attention' : 'normal',
      expectedMatch: scenario === 'expected_delivery', occurrenceCount: scenario === 'repeated_unexpected' ? 2 : 1,
    },
    agent: {
      id: 'agent-test', name: 'VisitorAgent', goal: 'Handle the event', status: scenario === 'blocked_unlock' ? 'blocked' : 'completed', permissions: ['ring.event.read', 'alexa.notify'], forbidden: ['door.unlock', 'purchase'], createdAt: '2026-10-12T16:22:00.000Z', terminatedAt: '2026-10-12T16:22:00.000Z',
    },
    alexaMessage: 'Grounded deterministic message', suggestedAction: null, requiresApproval: decision === 'approval_required', decision,
  };
};

test('disabled Bedrock uses an explicit deterministic fallback', async () => {
  const explainer = new BedrockCareExplainer({ enabled: false });
  const result = await explainer.explain(workflow('expected_delivery'));
  assert.equal(result.mode, 'local_fallback');
  assert.match(result.text, /expected medicine delivery window/i);
  assert.match(result.fallbackReason || '', /disabled/i);
  assert.equal(explainer.status().configured, false);
  assert.equal(explainer.status().connectionState, 'disabled');
  assert.equal(explainer.status().lastSuccessAt, null);
});

test('Bedrock receives only grounded workflow fields and returns validated prose', async () => {
  let commandInput: ConverseCommand['input'] | undefined;
  const client = {
    async send(command: ConverseCommand): Promise<ConverseCommandOutput> {
      commandInput = command.input;
      return {
        output: { message: { role: 'assistant', content: [{ text: 'Two unscheduled door events occurred. Review the trusted-contact alert before anything is sent.' }] } },
        stopReason: 'end_turn',
        usage: { inputTokens: 10, outputTokens: 12, totalTokens: 22 },
        metrics: { latencyMs: 10 },
        $metadata: {},
      };
    },
  };
  const explainer = new BedrockCareExplainer({ enabled: true, region: 'us-east-1', modelId: 'amazon.nova-lite-v1:0' }, client);
  assert.equal(explainer.status().connectionState, 'not_validated');
  const result = await explainer.explain(workflow('repeated_unexpected'), 'hybrid');
  assert.equal(result.mode, 'bedrock');
  assert.equal(result.provider, 'Amazon Bedrock');
  assert.equal(explainer.status().connectionState, 'connected');
  assert(explainer.status().lastSuccessAt);
  const payload = commandInput?.messages?.[0]?.content?.[0]?.text || '';
  assert.match(payload, /occurrenceCount/);
  assert.doesNotMatch(payload, /email|address|phone|token|secret/i);
});

test('unsafe or ungrounded Bedrock prose is rejected', async () => {
  const client = {
    async send(): Promise<ConverseCommandOutput> {
      return {
        output: { message: { role: 'assistant', content: [{ text: 'This is an intruder. Unlock the door and call the police.' }] } },
        stopReason: 'end_turn',
        usage: { inputTokens: 10, outputTokens: 12, totalTokens: 22 },
        metrics: { latencyMs: 10 },
        $metadata: {},
      };
    },
  };
  const result = await new BedrockCareExplainer({ enabled: true }, client).explain(workflow('blocked_unlock'), 'cloud_allowed');
  assert.equal(result.mode, 'local_fallback');
  assert.match(result.text, /blocked/i);
  assert.match(result.fallbackReason || '', /validation/i);
});

test('the denied-action validator remains conservative even for a negated forbidden phrase', async () => {
  const client = { async send(): Promise<ConverseCommandOutput> {
    return { output: { message: { role: 'assistant', content: [{ text: 'The request to unlock the door was denied.' }] } }, $metadata: {} } as ConverseCommandOutput;
  } };
  const explainer = new BedrockCareExplainer({ enabled: true }, client);
  const result = await explainer.explain(workflow('blocked_unlock'), 'cloud_allowed');
  assert.equal(result.mode, 'local_fallback');
  assert.equal(explainer.status().connectionState, 'validation_failed');
  assert.equal(explainer.status().lastSuccessAt, null);
});

test('Local Only and an omitted processing mode never invoke enabled Bedrock', async () => {
  let calls = 0;
  const client = { async send(): Promise<ConverseCommandOutput> { calls++; throw new Error('Must not reach AWS'); } };
  const explainer = new BedrockCareExplainer({ enabled: true }, client);
  for (const mode of [undefined, 'local_only'] as const) {
    const result = await explainer.explain(workflow('expected_delivery'), mode);
    assert.equal(result.mode, 'local_fallback');
    assert.match(result.fallbackReason || '', /no AWS request/i);
  }
  assert.equal(calls, 0);
  assert.equal(explainer.status().connectionState, 'not_validated');
  assert.equal(explainer.status().lastAttemptAt, null);
});

test('AWS errors fall back with an abort signal and no secret error details', async () => {
  const original = workflow('blocked_unlock');
  const before = structuredClone(original);
  let signal: AbortSignal | undefined;
  const client = {
    async send(_command: ConverseCommand, options?: { abortSignal?: AbortSignal }): Promise<ConverseCommandOutput> {
      signal = options?.abortSignal;
      throw new Error('Access denied; secret-test-token must not leak');
    },
  };
  const result = await new BedrockCareExplainer({ enabled: true }, client).explain(original, 'hybrid');
  assert(signal instanceof AbortSignal);
  assert.equal(result.mode, 'local_fallback');
  assert.match(result.fallbackReason || '', /unavailable/i);
  assert.doesNotMatch(JSON.stringify(result), /secret-test-token|Access denied/);
  assert.deepEqual(original, before);
});

test('a failed call cannot retain Connected or expose provider error text', async () => {
  let fail = false;
  const client = { async send(): Promise<ConverseCommandOutput> {
    if (fail) { const error = new Error('private-provider-detail'); error.name = 'AccessDeniedException'; throw error; }
    return { output: { message: { role: 'assistant', content: [{ text: 'The request was denied. No lock action occurred.' }] } }, $metadata: {} } as ConverseCommandOutput;
  } };
  const explainer = new BedrockCareExplainer({ enabled: true }, client);
  await explainer.explain(workflow('blocked_unlock'), 'cloud_allowed');
  assert.equal(explainer.status().connectionState, 'connected');
  const successAt = explainer.status().lastSuccessAt;
  fail = true;
  assert.equal((await explainer.explain(workflow('blocked_unlock'), 'hybrid')).mode, 'local_fallback');
  assert.equal(explainer.status().connectionState, 'unavailable');
  assert.equal(explainer.status().lastSuccessAt, successAt);
  assert.equal(explainer.status().lastErrorCode, 'AccessDeniedException');
  assert.doesNotMatch(JSON.stringify(explainer.status()), /private-provider-detail/);
});
