import { Radio } from 'lucide-react';
import type { RingPlaygroundDevice, RingPlaygroundSnapshot, RingStatus } from './types';

const utc = (value: string | null) => value ? `${new Date(value).toISOString().replace('T', ' ').replace('.000Z', '').replace('Z', '')} UTC` : 'Not reported';

export default function RingPlaygroundPanel({ status, devices, deviceId, snapshot, error, disabled, busy, bridgeAvailable, onSelect, onLoad, onRefresh, onMotion }: {
  status: RingStatus | null;
  devices: RingPlaygroundDevice[];
  deviceId: string;
  snapshot: RingPlaygroundSnapshot | null;
  error: string | null;
  disabled: boolean;
  busy: boolean;
  bridgeAvailable: boolean;
  onSelect: (id: string) => void;
  onLoad: () => void;
  onRefresh: () => void;
  onMotion: () => void;
}) {
  const connection = error || !bridgeAvailable || status?.playground.connectionState === 'unavailable' ? 'Unavailable' : status?.playground.connectionState === 'connected' ? 'Connected' : status?.playgroundConfigured ? 'Ready to connect' : 'Not connected';
  const latest = snapshot?.events[0];
  const visibleEvents = snapshot?.events.slice(0, 20) ?? [];
  return <article className="ring-platform" aria-labelledby="ring-platform-title" aria-busy={busy}>
    <div className="ring-platform-heading"><div><small>HOME DEVICES</small><h2 id="ring-platform-title">Ring devices</h2></div><span className={`ring-connection ${connection === 'Connected' ? 'connected' : ''}`} role="status"><Radio />{busy ? 'Loading…' : connection}</span></div>
    <p>Camera status and recent activity.</p>
    {status && !status.playgroundConfigured && <div className="ring-empty"><Radio aria-hidden="true" /><div><strong>No Ring devices connected</strong><p>Device status and recent activity will appear here when connected.</p></div></div>}
    {status?.playgroundConfigured && <div className="ring-controls">
      <button disabled={disabled || !status?.playgroundConfigured} onClick={onLoad}>Refresh Ring data</button>
      {devices.length > 0 && <>
        <select aria-label="Ring Playground device" value={deviceId} disabled={disabled} onChange={(event) => onSelect(event.target.value)}><option value="">Choose a Ring device</option>{devices.map((device) => <option value={device.id} key={device.id}>{device.name}</option>)}</select>
        <button disabled={disabled || !deviceId} onClick={onRefresh}>Refresh device status and history</button>
      </>}
    </div>}
    {error && <p className="ring-error" role="alert">{error}{snapshot ? ' Showing the last successful response below.' : ''}</p>}
    {snapshot && <>
      <div className="ring-device-heading"><h3>{snapshot.device.name}</h3><strong className={snapshot.status.online ? 'device-online' : 'device-offline'}>Device — {snapshot.status.online ? 'Online' : 'Offline'}</strong></div>
      <dl className="ring-device-facts">
        <div><dt>Last reported</dt><dd>{utc(snapshot.status.reportedAt)}</dd></div>
        <div><dt>API response time</dt><dd>{utc(snapshot.status.responseAt)}</dd></div>
        <div><dt>Latest Ring event type</dt><dd>{latest?.eventType ?? 'No events returned'}</dd></div>
        <div><dt>Audio snooze</dt><dd>{snapshot.status.audioSnooze.active === null ? 'Not reported' : snapshot.status.audioSnooze.active ? 'Active' : 'Inactive'}{snapshot.status.audioSnooze.until && ` · until ${utc(snapshot.status.audioSnooze.until)}`}</dd></div>
        <div><dt>Source</dt><dd>Official Ring API · Developer Playground</dd></div>
        <div><dt>Fetched</dt><dd>{utc(snapshot.fetchedAt)}</dd></div>
      </dl>
      <details className="ring-identifiers"><summary>Device and status identifiers</summary><dl><dt>Device ID</dt><dd>{snapshot.device.id}</dd><dt>Status resource ID</dt><dd>{snapshot.status.resourceId}</dd></dl></details>
      <h3>Ring event history</h3>
      <p className="ring-history-note"><code>on_demand</code> records a live-view session. Device status and live-view sessions do not indicate motion or visitors.</p>
      {visibleEvents.length ? <div className="ring-history-scroll" role="region" aria-label="Ring event history" tabIndex={0}><table><thead><tr><th scope="col">Event type</th><th scope="col">Started (UTC)</th><th scope="col">Ended (UTC)</th></tr></thead><tbody>{visibleEvents.map((event) => <tr key={event.id}><td><code>{event.eventType}</code><details><summary>Event details</summary><dl><dt>Event ID</dt><dd>{event.id}</dd><dt>Source device</dt><dd>{event.deviceId}</dd><dt>CV detections</dt><dd>{event.cvDetections.length ? event.cvDetections.map((detection) => `${detection.type}: ${detection.id}`).join(', ') : 'None returned'}</dd></dl></details></td><td>{utc(event.startedAt)}</td><td>{utc(event.endedAt)}</td></tr>)}</tbody></table></div> : <p>No Ring events were returned for this device.</p>}
      <p className="ring-history-note">Showing {visibleEvents.length} of {snapshot.events.length} records on the fetched page.{snapshot.hasMoreHistory && ' More history is available from Ring.'} Reading this panel does not start a VisitorAgent.</p>
    </>}
    {deviceId && <div className="ring-controls"><button disabled={disabled} onClick={onMotion}>Import Playground Motion event</button><span>Runs the existing workflow only when Ring returns a motion or doorbell event.</span></div>}
  </article>;
}
