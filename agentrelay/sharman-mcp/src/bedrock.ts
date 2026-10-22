import {
  BedrockRuntimeClient,
  ConverseCommand,
  type ConverseCommandOutput,
} from '@aws-sdk/client-bedrock-runtime';
import type { HomeWorkflow, ProcessingMode } from './store.js';

export type BedrockStatus = {
  configured: boolean;
  connectionState: 'disabled' | 'not_validated' | 'connected' | 'unavailable' | 'validation_failed';
  lastAttemptAt: string | null;
  lastSuccessAt: string | null;
  lastErrorCode: string | null;
  region: string;
  modelId: string;
  credentialSource: 'AWS default provider chain';
  purpose: string;
};

export type CareExplanation = {
  mode: 'bedrock' | 'local_fallback';
  text: string;
  provider: 'Amazon Bedrock' | 'SHARMAN deterministic fallback';
  modelId: string | null;
  region: string | null;
  groundedFields: string[];
  fallbackReason: string | null;
};

type BedrockConfig = {
  enabled: boolean;
  region: string;
  modelId: string;
};

type ConverseSender = {
  send(command: ConverseCommand, options?: { abortSignal?: AbortSignal }): Promise<ConverseCommandOutput>;
};

const groundedFields = [
  'scenario',
  'event.summary',
  'event.expectedMatch',
  'event.occurrenceCount',
  'decision',
  'agent.permissions',
  'agent.forbidden',
];

const configFromEnvironment = (): BedrockConfig => ({
  enabled: process.env.SHARMAN_AWS_BEDROCK_ENABLED === 'true',
  region: process.env.AWS_REGION || 'ap-southeast-2',
  modelId: process.env.SHARMAN_BEDROCK_MODEL_ID || 'global.amazon.nova-2-lite-v1:0',
});

const localExplanation = (workflow: HomeWorkflow): string => {
  if (workflow.scenario === 'ring_event') return 'The configured Ring device reported activity. No scheduled visit has been verified. VisitorAgent used its limited read and suggestion authority; no external action was taken.';
  if (workflow.scenario === 'expected_delivery') {
    return 'A front-door event matches the expected medicine delivery window. SHARMAN can create a ten-minute reminder only if you approve.';
  }
  if (workflow.scenario === 'repeated_unexpected') {
    if (workflow.event.source === 'ring') return `${workflow.event.occurrenceCount} events from the configured Ring device occurred within four minutes. No scheduled visit was verified. A contact message can only be prepared for review; nothing was sent.`;
    return 'Two unscheduled front-door events occurred within four minutes. SHARMAN can prepare—without sending—a trusted-contact alert for your approval.';
  }
  return 'The door-unlock request was blocked because VisitorAgent lacks that permission. No lock action occurred.';
};

const boundedModelText = (response: ConverseCommandOutput, workflow: HomeWorkflow): string | null => {
  const text = response.output?.message?.content
    ?.map((block) => block.text || '')
    .join(' ')
    .replace(/\s+/g, ' ')
    .trim();
  if (!text || text.length > 500) return null;
  const lower = text.toLowerCase();
  if (['criminal', 'intruder', 'call the police', 'unlock the door'].some((phrase) => lower.includes(phrase))) return null;
  if (workflow.decision === 'denied' && !['blocked', 'denied'].some((word) => lower.includes(word))) return null;
  if (workflow.decision === 'approval_required' && !['approval', 'review', 'would you like'].some((phrase) => lower.includes(phrase))) return null;
  return text;
};

export class BedrockCareExplainer {
  private readonly config: BedrockConfig;
  private readonly client: ConverseSender | null;
  private connectionState: BedrockStatus['connectionState'];
  private lastAttemptAt: string | null = null;
  private lastSuccessAt: string | null = null;
  private lastErrorCode: string | null = null;

  constructor(config: Partial<BedrockConfig> = {}, client?: ConverseSender) {
    this.config = { ...configFromEnvironment(), ...config };
    this.connectionState = this.config.enabled ? 'not_validated' : 'disabled';
    this.client = this.config.enabled
      ? client ?? new BedrockRuntimeClient({ region: this.config.region, maxAttempts: 2 })
      : null;
  }

  status(): BedrockStatus {
    return {
      configured: this.config.enabled,
      connectionState: this.connectionState,
      lastAttemptAt: this.lastAttemptAt,
      lastSuccessAt: this.lastSuccessAt,
      lastErrorCode: this.lastErrorCode,
      region: this.config.region,
      modelId: this.config.modelId,
      credentialSource: 'AWS default provider chain',
      purpose: 'Rewrite grounded home-event facts into a short accessible explanation; never classify risk or authorize actions.',
    };
  }

  async explain(workflow: HomeWorkflow, processingMode: ProcessingMode = 'local_only'): Promise<CareExplanation> {
    if (!this.client) return this.fallback(workflow, 'Bedrock is disabled; set SHARMAN_AWS_BEDROCK_ENABLED=true to enable the opt-in request.');
    if (processingMode !== 'hybrid' && processingMode !== 'cloud_allowed') return this.fallback(workflow, 'Local Only processing is selected; no AWS request was made.');
    const facts = {
      scenario: workflow.scenario,
      event: {
        summary: workflow.event.summary,
        expectedMatch: workflow.event.expectedMatch,
        occurrenceCount: workflow.event.occurrenceCount,
      },
      decision: workflow.decision,
      allowedCapabilities: workflow.agent.permissions,
      forbiddenCapabilities: workflow.agent.forbidden,
    };
    const command = new ConverseCommand({
      modelId: this.config.modelId,
      system: [{
        text: 'Write one calm accessibility announcement, at most 40 words, using only supplied facts. Never infer identity, intent, crime, or danger. Never recommend unlocking, purchasing, police contact, or automatic messaging. Preserve any approval requirement or denied decision. Describe a denied request as a "lock action" and say it was blocked or denied; do not repeat the phrase "unlock the door", even when describing a denial. Do not list additional capabilities or propose new actions.',
      }],
      messages: [{ role: 'user', content: [{ text: JSON.stringify(facts) }] }],
      inferenceConfig: { maxTokens: 100, temperature: 0, topP: 0.8 },
    });
    this.lastAttemptAt = new Date().toISOString();
    try {
      const response = await this.client.send(command, { abortSignal: AbortSignal.timeout(15_000) });
      const text = boundedModelText(response, workflow);
      if (!text) {
        this.connectionState = 'validation_failed';
        this.lastErrorCode = 'OUTPUT_VALIDATION_FAILED';
        return this.fallback(workflow, 'Bedrock output did not pass SHARMAN validation.');
      }
      this.connectionState = 'connected';
      this.lastSuccessAt = new Date().toISOString();
      this.lastErrorCode = null;
      return {
        mode: 'bedrock',
        text,
        provider: 'Amazon Bedrock',
        modelId: this.config.modelId,
        region: this.config.region,
        groundedFields,
        fallbackReason: null,
      };
    } catch (error) {
      this.connectionState = 'unavailable';
      const name = error instanceof Error ? error.name : '';
      // Only bounded error categories cross the API boundary; never provider text.
      this.lastErrorCode = ['AccessDeniedException', 'ThrottlingException', 'ValidationException', 'CredentialsProviderError', 'TimeoutError', 'AbortError'].includes(name) ? name : 'BEDROCK_UNAVAILABLE';
      return this.fallback(workflow, 'Bedrock was unavailable; no permission decision or workflow state changed.');
    }
  }

  private fallback(workflow: HomeWorkflow, fallbackReason: string): CareExplanation {
    return {
      mode: 'local_fallback',
      text: localExplanation(workflow),
      provider: 'SHARMAN deterministic fallback',
      modelId: null,
      region: null,
      groundedFields,
      fallbackReason,
    };
  }
}
