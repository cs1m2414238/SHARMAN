import assert from 'node:assert/strict';
import test from 'node:test';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { RingPlaygroundClient, RingPlaygroundError } from './ring-playground.js';
import { normalizeRingDeviceStatus, normalizeRingHistoryEvent, parseRingEvent, RingAdapter, RingPayloadError } from './ring.js';
import { SharmanStore } from './store.js';

const history = (eventType = 'motion') => ({
  type: 'history-events', id: 'fixture-playground-event',
  attributes: { event_type: eventType, start: 1_792_548_930_000, end: 1_792_548_940_000 },
  relationships: { source: { data: { type: 'devices', id: 'fixture-playground-device' } } },
});

test('Playground uses only the fixed official API origin, runtime token, read requests and no redirects', async () => {
  const calls: string[] = [];
  const request: typeof fetch = async (url, options) => {
    calls.push(String(url));
    assert.equal(options?.method, 'GET');
    assert.equal(options?.redirect, 'error');
    assert(options?.signal);
    const headers = options?.headers as Record<string, string>;
    assert.equal(headers.Authorization, 'Bearer test-playground-access-token');
    assert.equal(headers.Accept, 'application/json');
    assert.equal(headers['Content-Type'], 'application/json');
    assert.match(headers['User-Agent']!, /^SHARMAN\/[\d.]+$/);
    assert.match(headers.Hardware_Id!, /^[a-f\d]{8}-(?:[a-f\d]{4}-){3}[a-f\d]{12}$/);
    return new Response(JSON.stringify(String(url).endsWith('/v1/devices')
      ? { data: [{ type: 'devices', id: 'fixture-playground-device', attributes: { name: 'Playground camera' } }] }
      : { data: [history()] }), { status: 200 });
  };
  const client = new RingPlaygroundClient('playground', 'test-playground-access-token', request);
  assert.deepEqual(await client.listDevices(), [{ id: 'fixture-playground-device', name: 'Playground camera' }]);
  const event = await client.latestEvent('fixture-playground-device');
  assert.equal(normalizeRingHistoryEvent(event, 'fixture-playground-device').payload?.ingestion_mode, 'ring_developer_playground');
  assert.equal(calls[2], 'https://api.amazonvision.com/v1/history/devices/fixture-playground-device/events?event_types=motion,ding');
  assert(client.status().lastSuccessAt);
  assert(!JSON.stringify(client.status()).includes('test-playground-access-token'));
});

test('missing/expired tokens, network failures, malformed metadata and undiscovered devices fail without a fabricated event', async () => {
  let requests = 0;
  const forbidden: typeof fetch = async () => { requests++; throw new Error('Provider detail including test-playground-access-token'); };
  await assert.rejects(new RingPlaygroundClient('', 'test-playground-access-token', forbidden).listDevices(), RingPlaygroundError);
  assert.equal(requests, 0);
  await assert.rejects(new RingPlaygroundClient('playground', 'test-playground-access-token', forbidden).listDevices(), (error: Error) => !error.message.includes('test-playground-access-token'));
  const unauthorized: typeof fetch = async () => new Response('private-provider-body', { status: 401 });
  await assert.rejects(new RingPlaygroundClient('playground', 'test-playground-access-token', unauthorized).listDevices(), (error: RingPlaygroundError) => error.status === 401 && !error.message.includes('private-provider-body'));
  const malformed: typeof fetch = async () => new Response('{"data":{}}');
  await assert.rejects(new RingPlaygroundClient('playground', 'test-playground-access-token', malformed).listDevices(), RingPlaygroundError);
  const empty: typeof fetch = async () => new Response('{"data":[]}');
  await assert.rejects(new RingPlaygroundClient('playground', 'test-playground-access-token', empty).latestEvent('undiscovered'), RingPlaygroundError);
});

test('history normalization binds device relationships, preserves subtype uncertainty and opaque event IDs', () => {
  const event = normalizeRingHistoryEvent(history(), 'fixture-playground-device');
  assert.equal(event.event_type, 'motion_detected');
  assert.equal(event.source_event_id, 'fixture-playground-event');
  assert.equal(event.subject_id, null);
  assert.equal(event.payload?.detection_type, 'motion');
  assert.equal(event.payload?.account_id, null, 'history contains no account ID to invent');
  assert.equal(normalizeRingHistoryEvent(history('motion.vehicle'), 'fixture-playground-device').payload?.detection_type, 'vehicle');
  assert.equal(normalizeRingHistoryEvent(history('ding'), 'fixture-playground-device').event_type, 'doorbell_ring');
  assert.throws(() => normalizeRingHistoryEvent(history(), 'different-device'), RingPayloadError);
  assert.equal(normalizeRingHistoryEvent(history('on_demand'), 'fixture-playground-device').event_type, 'system_ping');
});

test('official-format generic Motion delegates without inventing a person, and retains denied-unlock policy', async () => {
  const directory = await mkdtemp(path.join(tmpdir(), 'sharman-playground-test-'));
  try {
    const store = new SharmanStore(directory);
    const adapter = new RingAdapter((event) => store.ingestRingEvent(event));
    const receipt = await adapter.ingestPlaygroundHistoryEvent(history(), 'fixture-playground-device');
    assert.equal(receipt.event.provenance, 'ring_developer_playground');
    assert.equal(receipt.workflow?.agent.name, 'VisitorAgent');
    assert.match(receipt.workflow?.alexaMessage ?? '', /no person.*identified/i);
    assert.equal(receipt.workflow?.agent.permissionCeiling, 'LOW_RISK');
    const denied = await store.denyRingUnlock(receipt.event.id);
    assert.equal(denied.decision, 'denied');
    assert.equal(denied.agent.denialDetail?.requiredPermission, 'HIGH_RISK');
    assert.equal((await adapter.ingestPlaygroundHistoryEvent(history(), 'fixture-playground-device')).status, 'duplicate_ignored');
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('the owner-captured nine on-demand sessions become neutral HomeEvents and cannot create a VisitorAgent', async () => {
  const capture = JSON.parse(await readFile(new URL('../test-fixtures/ring/event-history-on-demand.json', import.meta.url), 'utf8'));
  assert.equal(capture._fixture, true);
  assert.equal(capture.data.length, 9);
  const deviceId = capture.data[0].relationships.source.data.id;
  for (const event of capture.data) {
    assert.equal(event.attributes.event_type, 'on_demand');
    assert.deepEqual(event.relationships.cv_detections.data, []);
    const normalized = normalizeRingHistoryEvent(event, deviceId);
    assert.equal(normalized.event_type, 'system_ping');
    assert.equal(normalized.source_event_id, event.id);
    assert.equal(normalized.payload?.ended_at, event.attributes.end);
    assert.deepEqual(normalized.payload?.cv_detections, []);
  }
  const directory = await mkdtemp(path.join(tmpdir(), 'sharman-captured-ring-test-'));
  try {
    const store = new SharmanStore(directory);
    const adapter = new RingAdapter((event) => store.ingestRingEvent(event));
    const imported = await adapter.ingestCapturedPlaygroundHistory(capture);
    assert.equal(imported.length, 9);
    assert(imported.every((record) => record.status === 'ingested' && record.workflow === null));
    assert.equal((await store.homeSnapshot()).events.length, 9);
    assert.equal((await store.homeSnapshot()).agents.length, 0);
    assert((await adapter.ingestCapturedPlaygroundHistory(capture)).every((record) => record.status === 'duplicate_ignored'));
  } finally { await rm(directory, { recursive: true, force: true }); }
  assert.throws(() => parseRingEvent('{not-json'), RingPayloadError);
});

test('captured device status preserves report/send times and requires a separately known device ID', async () => {
  const capture = JSON.parse(await readFile(new URL('../test-fixtures/ring/device-status-online.json', import.meta.url), 'utf8'));
  const normalized = normalizeRingDeviceStatus(capture, 'fixture-requested-device');
  assert.equal(normalized.event_type, 'system_ping');
  assert.equal(normalized.occurred_at, '2026-10-21T16:35:31Z');
  assert.equal(normalized.payload?.notification_sent_at, '2026-10-21T16:40:47Z');
  assert.equal(normalized.payload?.source_device_id, 'fixture-requested-device');
  assert.equal(normalized.payload?.ring_status_resource_id, capture.data.id);
  assert.equal(normalized.payload?.online, true);
  assert.equal(normalized.payload?.audio_snooze_active, null);
  assert.equal(normalized.payload?.audio_snooze_until, null);
  assert.throws(() => normalizeRingDeviceStatus(capture, ''), RingPayloadError);
  const directory = await mkdtemp(path.join(tmpdir(), 'sharman-ring-status-'));
  try {
    const store = new SharmanStore(directory);
    const receipt = await store.ingestRingEvent(normalized);
    assert.equal(receipt.workflow, null);
    assert.equal(receipt.event.kind, 'system');
    assert.equal((await store.homeSnapshot()).agents.length, 0);
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('read-only device inspection shows real status and on-demand history without leaking unknown fields', async () => {
  const devices = JSON.parse(await readFile(new URL('../test-fixtures/ring/device-list.json', import.meta.url), 'utf8'));
  const status = JSON.parse(await readFile(new URL('../test-fixtures/ring/device-status-online.json', import.meta.url), 'utf8'));
  const historyCapture = JSON.parse(await readFile(new URL('../test-fixtures/ring/event-history-on-demand.json', import.meta.url), 'utf8'));
  const deviceId = devices.data[0].id;
  const capturedDeviceId = historyCapture.data[0].relationships.source.data.id;
  // IDs are varied deliberately; fixture matching is never the integration.
  for (const event of historyCapture.data) event.relationships.source.data.id = deviceId;
  assert.notEqual(deviceId, capturedDeviceId);
  status.data.attributes.access_token = 'test-provider-private-detail';
  const hardwareIds = new Map<string, string>();
  const request: typeof fetch = async (url, options) => {
    const key = String(url).endsWith('/v1/devices') ? 'discovery' : 'device';
    const id = (options!.headers as Record<string, string>).Hardware_Id!;
    if (hardwareIds.has(key)) assert.equal(id, hardwareIds.get(key));
    hardwareIds.set(key, id);
    return new Response(JSON.stringify(key === 'discovery' ? devices : String(url).endsWith('/status') ? status : historyCapture));
  };
  const client = new RingPlaygroundClient('playground', 'test-playground-access-token', request);
  assert.equal(client.status().connectionState, 'not_checked');
  const snapshot = await client.inspectDevice(deviceId);
  assert.equal(client.status().connectionState, 'connected');
  assert.equal(snapshot.status.online, true);
  assert.equal(snapshot.status.reportedAt, '2026-10-21T16:35:31Z');
  assert.equal(snapshot.status.responseAt, '2026-10-21T16:40:47Z');
  assert.equal(snapshot.status.resourceId, status.data.id);
  assert.equal(snapshot.events.length, 9);
  assert(snapshot.events.every((event) => event.eventType === 'on_demand' && event.cvDetections.length === 0));
  assert(!JSON.stringify(snapshot).includes('test-provider-private-detail'));
  assert(!JSON.stringify(snapshot).includes('test-playground-access-token'));
  assert.equal((await client.getEventHistory(deviceId)).length, 9);
  assert.equal((await client.getDeviceStatus(deviceId) as typeof status).data.attributes.online, true);
  assert.notEqual(hardwareIds.get('device'), hardwareIds.get('discovery'));
  status.data.attributes.online = false;
  delete status.data.attributes.reported_at;
  const offline = await client.inspectDevice(deviceId);
  assert.equal(offline.status.online, false);
  assert.equal(offline.status.reportedAt, null, 'a missing report time is never replaced by the API response time');
});

test('invalid device metadata, cross-device history and provider failures invalidate connection health', async () => {
  let body: unknown = { data: [history()] };
  const request: typeof fetch = async (url) => new Response(JSON.stringify(String(url).endsWith('/v1/devices') ? { data: [{ type: 'devices', id: 'fixture-playground-device' }] } : body));
  const client = new RingPlaygroundClient('playground', 'test-playground-access-token', request);
  await client.listDevices();
  assert.equal(client.status().connectionState, 'connected');
  await assert.rejects(client.inspectDevice('fixture-playground-device'), RingPlaygroundError);
  assert.equal(client.status().connectionState, 'unavailable');
  body = { data: [{ ...history(), relationships: { source: { data: { type: 'devices', id: 'different-device' } } } }] };
  await assert.rejects(client.getEventHistory('fixture-playground-device'), RingPlaygroundError);
  assert.equal(client.status().connectionState, 'unavailable');
});

test('schema-derived Motion, Package and Vehicle fixtures use actual fields and preserve permission boundaries', async () => {
  const directory = await mkdtemp(path.join(tmpdir(), 'sharman-ring-webhook-schemas-'));
  try {
    const store = new SharmanStore(directory);
    const adapter = new RingAdapter((event) => store.ingestRingEvent(event));
    for (const subtype of ['motion', 'package', 'vehicle']) {
      const input = JSON.parse(await readFile(new URL(`../test-fixtures/ring/webhook-${subtype}.json`, import.meta.url), 'utf8'));
      assert.equal(input._captured, false, 'documentation examples are never described as captured deliveries');
      const parsed = parseRingEvent(JSON.stringify(input));
      assert.equal(parsed.data.type, 'motion_detected');
      const receipt = await adapter.ingestTestEvent(input);
      assert.equal(receipt.normalized.source_event_id, parsed.data.id);
      assert.equal(receipt.normalized.payload?.source_device_id, parsed.data.attributes.source);
      if (subtype === 'vehicle') assert.equal(receipt.workflow, null);
      else {
        assert.equal(receipt.workflow?.agent.name, 'VisitorAgent');
        assert.equal(receipt.event.expectedMatch, false);
        assert.equal((await store.denyRingUnlock(receipt.event.id)).decision, 'denied');
      }
    }
  } finally { await rm(directory, { recursive: true, force: true }); }
});
