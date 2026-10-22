import { z } from 'zod';
import { randomUUID } from 'node:crypto';
import { historyEventSchema, ringDeviceStatusSchema } from './ring.js';

const deviceSchema = z.object({ type: z.literal('devices'), id: z.string().min(1), attributes: z.object({ name: z.string().optional() }).optional() });
const devicesSchema = z.object({ data: z.array(deviceSchema).max(1000) });
const eventsSchema = z.object({ data: z.array(historyEventSchema).max(1000), links: z.object({ next: z.string().nullable().optional() }).optional() });
export type PlaygroundDevice = { id: string; name: string };

export class RingPlaygroundError extends Error {
  constructor(readonly status: number, message: string) { super(message); this.name = 'RingPlaygroundError'; }
}

/** Runtime token stays server-side and is sent only to the fixed Ring API origin. */
export class RingPlaygroundClient {
  private readonly token: string;
  readonly configured: boolean;
  private lastSuccessAt: string | null = null;
  private connectionState: 'not_checked' | 'connected' | 'unavailable' = 'not_checked';
  private readonly hardwareIds = new Map<string, string>();
  constructor(
    environment = process.env.RING_ENVIRONMENT,
    token = process.env.RING_ACCESS_TOKEN,
    private readonly request: typeof fetch = fetch,
  ) {
    this.token = token?.trim() ?? '';
    this.configured = environment === 'playground' && this.token.length > 0;
  }

  status() { return { configured: this.configured, connectionState: this.configured ? this.connectionState : 'unconfigured', lastSuccessAt: this.lastSuccessAt, source: 'ring_developer_playground', apiOrigin: 'https://api.amazonvision.com' }; }

  private success() { this.lastSuccessAt = new Date().toISOString(); this.connectionState = 'connected'; }

  private invalidMetadata(message: string): never { this.connectionState = 'unavailable'; throw new RingPlaygroundError(502, message); }

  private async read(apiPath: string, deviceId = 'discovery'): Promise<unknown> {
    if (!this.configured) throw new RingPlaygroundError(503, 'Configure RING_ACCESS_TOKEN and RING_ENVIRONMENT=playground in the server environment.');
    try {
      if (!this.hardwareIds.has(deviceId)) this.hardwareIds.set(deviceId, randomUUID());
      const response = await this.request(`https://api.amazonvision.com${apiPath}`, {
        method: 'GET', headers: { Authorization: `Bearer ${this.token}`, Accept: 'application/json', 'Content-Type': 'application/json', 'User-Agent': 'SHARMAN/0.23.0', Hardware_Id: this.hardwareIds.get(deviceId)! },
        redirect: 'error', signal: AbortSignal.timeout(10_000),
      });
      if (!response.ok) throw new RingPlaygroundError(response.status === 401 || response.status === 403 ? 401 : 502,
        response.status === 401 || response.status === 403 ? 'Ring token is expired or this device is not authorized. Generate a fresh Playground token locally.' : 'Ring API could not complete the request.');
      const text = await response.text();
      if (text.length > 1_048_576) throw new RingPlaygroundError(502, 'Ring API response exceeded the metadata limit.');
      return JSON.parse(text) as unknown;
    } catch (error) {
      this.connectionState = 'unavailable';
      if (error instanceof RingPlaygroundError) throw error;
      throw new RingPlaygroundError(502, 'Ring API is unavailable or returned invalid metadata. No token or provider response was recorded.');
    }
  }

  async listDevices(): Promise<PlaygroundDevice[]> {
    const parsed = devicesSchema.safeParse(await this.read('/v1/devices'));
    if (!parsed.success) this.invalidMetadata('Ring returned an unsupported device-list structure.');
    this.success();
    return parsed.data.data.map((device) => ({ id: device.id, name: device.attributes?.name ?? 'Ring Playground device' }));
  }

  async historyEvents(deviceId: string, includeLiveView = false): Promise<z.infer<typeof eventsSchema>['data']> {
    const devices = await this.listDevices();
    if (!devices.some((device) => device.id === deviceId)) throw new RingPlaygroundError(400, 'Choose a device returned by Ring device discovery.');
    return (await this.readHistory(deviceId, includeLiveView)).data;
  }

  private async readHistory(deviceId: string, includeLiveView: boolean) {
    const parsed = eventsSchema.safeParse(await this.read(`/v1/history/devices/${encodeURIComponent(deviceId)}/events?event_types=${includeLiveView ? 'motion,ding,on_demand' : 'motion,ding'}`, deviceId));
    if (!parsed.success || parsed.data.data.some((event) => event.relationships.source.data.id !== deviceId)) this.invalidMetadata('Ring returned unsupported or mismatched event-history metadata.');
    this.success();
    parsed.data.data.sort((left, right) => right.attributes.start - left.attributes.start);
    return parsed.data;
  }

  /** Includes on-demand sessions without converting them to detections. */
  async getEventHistory(deviceId: string) { return this.historyEvents(deviceId, true); }

  async latestEvent(deviceId: string): Promise<unknown | null> {
    return (await this.historyEvents(deviceId)).find((event) => event.attributes.event_type === 'ding' || event.attributes.event_type === 'motion' || event.attributes.event_type.startsWith('motion.')) ?? null;
  }

  async deviceStatus(deviceId: string): Promise<unknown> {
    if (!(await this.listDevices()).some((device) => device.id === deviceId)) throw new RingPlaygroundError(400, 'Choose a discovered Ring device.');
    return this.readStatus(deviceId);
  }

  async getDeviceStatus(deviceId: string) { return this.deviceStatus(deviceId); }

  private async readStatus(deviceId: string) {
    const parsed = ringDeviceStatusSchema.safeParse(await this.read(`/v1/devices/${encodeURIComponent(deviceId)}/status`, deviceId));
    if (!parsed.success) this.invalidMetadata('Ring returned an unsupported device-status structure.');
    this.success();
    return parsed.data;
  }

  /** Read-only Home panel: never invokes a workflow or imports a fixture. */
  async inspectDevice(deviceId: string) {
    const device = (await this.listDevices()).find((entry) => entry.id === deviceId);
    if (!device) throw new RingPlaygroundError(400, 'Choose a discovered Ring device.');
    // Sequential reads keep the final connection state consistent if one fails.
    const status = await this.readStatus(deviceId);
    const history = await this.readHistory(deviceId, true);
    return {
      source: 'ring', environment: 'developer_playground', delivery: 'api_response', fetchedAt: this.lastSuccessAt,
      device,
      status: { resourceId: status.data.id, online: status.data.attributes.online,
        reportedAt: status.data.attributes.reported_at ?? null, responseAt: status.meta.time,
        audioSnooze: { active: status.data.attributes.audio?.snooze?.active ?? null, until: status.data.attributes.audio?.snooze?.until ?? null } },
      events: history.data.map((event) => ({ id: event.id, deviceId: event.relationships.source.data.id,
        eventType: event.attributes.event_type, startedAt: new Date(event.attributes.start).toISOString(),
        endedAt: event.attributes.end === undefined ? null : new Date(event.attributes.end).toISOString(),
        cvDetections: event.relationships.cv_detections?.data ?? [], reviewed: event.attributes.is_third_party_reviewed ?? null })),
      hasMoreHistory: Boolean(history.links?.next),
    };
  }
}
