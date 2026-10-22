import { z } from 'zod';
import { createHmac, timingSafeEqual } from 'node:crypto';
import type { NewHomeEvent } from './household-types.js';

// https://developer.amazon.com/docs/ring/api-documentation.html#motion-detection
// Format validation is separate from signed-webhook authentication.
const nonEmptyString = z.string().refine((value) => value.trim().length > 0);
const ringPayloadSchema = z.object({
  meta: z.object({
    version: z.literal('1.1'),
    time: z.iso.datetime({ offset: true }),
    request_id: nonEmptyString,
    account_id: nonEmptyString,
  }),
  data: z.object({
    id: nonEmptyString,
    type: z.enum(['motion_detected', 'button_press']),
    attributes: z.object({
      source: nonEmptyString,
      source_type: z.literal('devices'),
      // Epoch milliseconds, bounded to a four-digit UTC year for Rust RFC3339.
      timestamp: z.number().int().min(0).max(253_402_300_799_999),
      sub_type: nonEmptyString.optional(),
    }),
  }),
});
const epochMilliseconds = z.union([z.number(), z.string().regex(/^\d+$/).transform(Number)])
  .pipe(z.number().int().min(0).max(253_402_300_799_999));
export const historyEventSchema = z.object({
  type: z.literal('history-events'), id: nonEmptyString,
  attributes: z.object({ event_type: nonEmptyString, start: epochMilliseconds, end: epochMilliseconds.optional(), is_third_party_reviewed: z.boolean().optional() }),
  relationships: z.object({ source: z.object({ data: z.object({ type: z.literal('devices'), id: nonEmptyString }) }), cv_detections: z.object({ data: z.array(z.object({ type: nonEmptyString, id: nonEmptyString })) }).optional() }),
});

/** Translate the documented history resource, never a guessed webhook envelope. */
export function normalizeRingHistoryEvent(payload: unknown, deviceId: string): NewHomeEvent {
  const parsed = historyEventSchema.safeParse(payload);
  if (!parsed.success || parsed.data.relationships.source.data.id !== deviceId) throw new RingPayloadError();
  const { id, attributes } = parsed.data;
  const isMotion = attributes.event_type === 'motion' || attributes.event_type.startsWith('motion.');
  const liveView = attributes.event_type === 'on_demand';
  if (!isMotion && attributes.event_type !== 'ding' && !liveView) throw new RingPayloadError();
  const detectionType = isMotion ? attributes.event_type === 'motion' ? 'motion' : attributes.event_type.slice(7) : 'unknown';
  return {
    source: 'ring', source_event_id: id, occurred_at: new Date(attributes.start).toISOString(),
    event_type: liveView ? 'system_ping' : isMotion ? 'motion_detected' : 'doorbell_ring', severity: !liveView && (!isMotion || detectionType === 'human') ? 'attention' : 'normal',
    zone_id: null, access_point_id: null, subject_id: null,
    payload: { source_device_id: deviceId, detection_type: detectionType, account_id: null,
      ring_event_type: attributes.event_type, ingestion_mode: 'ring_developer_playground', environment: 'developer_playground', api_resource_type: 'history-events',
      started_at: attributes.start, ended_at: attributes.end ?? null, cv_detections: parsed.data.relationships.cv_detections?.data ?? [],
      is_third_party_reviewed: attributes.is_third_party_reviewed ?? null },
  };
}

export const ringDeviceStatusSchema = z.object({
  data: z.object({ type: z.literal('device-status'), id: nonEmptyString, attributes: z.object({
    online: z.boolean(), reported_at: z.iso.datetime({ offset: true }).nullable().optional(),
    audio: z.object({ snooze: z.object({ active: z.boolean().nullable().optional(), until: z.iso.datetime({ offset: true }).nullable().optional() }).optional() }).optional(),
  }) }),
  meta: z.object({ time: z.iso.datetime({ offset: true }) }),
});

export function normalizeRingDeviceStatus(payload: unknown, sourceDeviceId: string): NewHomeEvent {
  const parsed = ringDeviceStatusSchema.safeParse(payload);
  if (!parsed.success || !sourceDeviceId.trim()) throw new RingPayloadError();
  const { data, meta } = parsed.data;
  return { source: 'ring', source_event_id: `status:${JSON.stringify([data.id, data.attributes.reported_at ?? meta.time])}`,
    occurred_at: data.attributes.reported_at ?? meta.time, event_type: 'system_ping', severity: data.attributes.online ? 'normal' : 'attention',
    zone_id: null, access_point_id: null, subject_id: null,
    payload: { source_device_id: sourceDeviceId, account_id: null, detection_type: 'unknown', ring_event_type: 'device-status',
      ring_status_resource_id: data.id, online: data.attributes.online, reported_at: data.attributes.reported_at ?? null, notification_sent_at: meta.time,
      audio_snooze_active: data.attributes.audio?.snooze?.active ?? null, audio_snooze_until: data.attributes.audio?.snooze?.until ?? null,
      ingestion_mode: 'ring_developer_playground', environment: 'developer_playground' } };
}

export class RingPayloadError extends Error {
  constructor() {
    super('Invalid Ring v1.1 motion or button-press payload.');
    this.name = 'RingPayloadError';
  }
}

export function parseRingEvent(payload: unknown): z.infer<typeof ringPayloadSchema> {
  let input = payload;
  if (typeof input === 'string') {
    try { input = JSON.parse(input); } catch { throw new RingPayloadError(); }
  }
  const parsed = ringPayloadSchema.safeParse(input);
  if (!parsed.success) throw new RingPayloadError();
  return parsed.data;
}

export type RingDeviceLocation = Pick<NewHomeEvent, 'zone_id' | 'access_point_id'>;
export type RingDeviceResolver = (deviceId: string, accountId: string) => RingDeviceLocation | undefined;
export type RingEventSink<Result> = (event: NewHomeEvent) => Result | Promise<Result>;
export type RingWebhookConfig = { signingKey: string; accountId: string; deviceId: string };

export class RingAuthenticationError extends Error {
  constructor() {
    super('Ring webhook authentication failed.');
    this.name = 'RingAuthenticationError';
  }
}

/**
 * Normalize a locally supplied test payload. This validates its format, not its
 * origin. Device placement comes only from trusted configuration. No request ID,
 * timestamp or device relationship is used to infer a person's identity.
 */
export function normalizeRingEvent(payload: unknown, resolveDevice?: RingDeviceResolver): NewHomeEvent {
  const { meta, data } = parseRingEvent(payload);
  const attributes = data.attributes;
  const isMotion = data.type === 'motion_detected';
  // Missing classification is unknown; new Ring classifications pass through.
  // In particular, the report's incorrect data.subType cannot establish "human".
  const detectionType = isMotion ? attributes.sub_type ?? 'unknown' : 'unknown';
  const location = resolveDevice?.(attributes.source, meta.account_id);

  return {
    source: 'ring',
    source_event_id: data.id,
    occurred_at: new Date(attributes.timestamp).toISOString(),
    event_type: isMotion ? 'motion_detected' : 'doorbell_ring',
    zone_id: location?.zone_id ?? null,
    access_point_id: location?.access_point_id ?? null,
    subject_id: null,
    severity: !isMotion || detectionType === 'human' ? 'attention' : 'normal',
    payload: {
      source_device_id: attributes.source,
      detection_type: detectionType,
      account_id: meta.account_id,
      request_id: meta.request_id,
      notification_sent_at: meta.time,
      ring_event_type: data.type,
      ring_payload_version: meta.version,
      ingestion_mode: 'local_test',
    },
  };
}

/**
 * Supply the existing household ingestion function as the sink. It owns schema
 * version, local received_at, sequence numbers and durable deduplication using
 * source + source_event_id. The adapter neither caches deliveries nor retries
 * writes, so the sink's duplicate/rejection results and failures pass through.
 */
export class RingAdapter<Result> {
  constructor(
    private readonly ingestEvent: RingEventSink<Result>,
    private readonly resolveDevice?: RingDeviceResolver,
    private readonly webhook?: RingWebhookConfig,
  ) {}

  async ingestTestEvent(payload: unknown): Promise<Result> {
    return this.ingestEvent(normalizeRingEvent(payload, this.resolveDevice));
  }

  async ingestPlaygroundHistoryEvent(payload: unknown, deviceId: string): Promise<Result> {
    const event = normalizeRingHistoryEvent(payload, deviceId);
    event.payload!.environment = 'developer_playground';
    event.payload!.delivery = 'api_response';
    return this.ingestEvent(event);
  }

  async ingestCapturedPlaygroundEvent(payload: unknown): Promise<Result> {
    const input = z.object({ data: z.unknown() }).safeParse(payload);
    if (!input.success) throw new RingPayloadError();
    let event: NewHomeEvent;
    if (Array.isArray(input.data.data)) {
      const candidates = input.data.data.map((entry) => historyEventSchema.safeParse(entry))
        .filter((entry) => entry.success && (['ding', 'on_demand', 'motion'].includes(entry.data.attributes.event_type) || entry.data.attributes.event_type.startsWith('motion.')))
        .map((entry) => entry.data!).sort((left, right) => right.attributes.start - left.attributes.start);
      const latest = candidates[0];
      if (!latest) throw new RingPayloadError();
      event = normalizeRingHistoryEvent(latest, latest.relationships.source.data.id);
    } else event = normalizeRingEvent(payload);
    event.payload!.environment = 'developer_playground';
    event.payload!.ingestion_mode = 'ring_developer_playground';
    event.payload!.delivery = 'user_supplied_capture';
    return this.ingestEvent(event);
  }

  async ingestCapturedPlaygroundHistory(payload: unknown, delivery = 'user_supplied_capture'): Promise<Result[]> {
    const parsed = z.object({ data: z.array(historyEventSchema).min(1).max(1000) }).safeParse(payload);
    if (!parsed.success) throw new RingPayloadError();
    const events = parsed.data.data.map((entry) => normalizeRingHistoryEvent(entry, entry.relationships.source.data.id));
    events.sort((left, right) => Date.parse(left.occurred_at) - Date.parse(right.occurred_at));
    const results: Result[] = [];
    for (const event of events) {
      event.payload!.delivery = delivery;
      results.push(await this.ingestEvent(event));
    }
    return results;
  }

  async ingestPlaygroundDeviceStatus(payload: unknown, sourceDeviceId: string, delivery = 'api_response'): Promise<Result> {
    const event = normalizeRingDeviceStatus(payload, sourceDeviceId);
    event.payload!.delivery = delivery;
    return this.ingestEvent(event);
  }

  async ingestWebhook(rawBody: Buffer, signature: string | undefined): Promise<Result> {
    const config = this.webhook;
    if (!config?.signingKey || !config.accountId || !config.deviceId
      || !signature || !/^sha256=[a-fA-F0-9]{64}$/.test(signature)) throw new RingAuthenticationError();
    const expected = createHmac('sha256', config.signingKey).update(rawBody).digest();
    const received = Buffer.from(signature.slice(7), 'hex');
    if (!timingSafeEqual(expected, received)) throw new RingAuthenticationError();
    let payload: unknown;
    try { payload = JSON.parse(rawBody.toString('utf8')); } catch { throw new RingPayloadError(); }
    const event = normalizeRingEvent(payload, this.resolveDevice);
    if (event.payload?.account_id !== config.accountId || event.payload?.source_device_id !== config.deviceId) {
      throw new RingAuthenticationError();
    }
    // A valid signature proves possession of the configured key, not hardware
    // origin. Local signed fixtures must still be described as local fixtures.
    event.payload!.ingestion_mode = 'signed_webhook';
    return this.ingestEvent(event);
  }
}
