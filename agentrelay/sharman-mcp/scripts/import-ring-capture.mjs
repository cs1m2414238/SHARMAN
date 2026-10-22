import { readFile, stat } from 'node:fs/promises';
import path from 'node:path';

try {
  const filename = process.argv[2];
  if (!filename || path.extname(filename).toLowerCase() !== '.json' || (await stat(filename)).size > 1_048_576) throw new Error('INVALID_CAPTURE_FILE');
  const payload = JSON.parse(await readFile(filename, 'utf8'));
  const endpoint = new URL(process.env.SHARMAN_RING_IMPORT_URL || 'http://127.0.0.1:4179');
  if (endpoint.protocol !== 'http:' || !['127.0.0.1', 'localhost', '[::1]'].includes(endpoint.hostname) || endpoint.username || endpoint.password) throw new Error('LOOPBACK_REQUIRED');
  endpoint.pathname = '/api/ring/playground/import'; endpoint.search = '';
  if (process.argv[3]) endpoint.searchParams.set('deviceId', process.argv[3]);
  const body = JSON.stringify(payload, (key, value) => {
    if (/authorization|bearer|token|secret|password|credential|signing[_ -]?key|hmac[_ -]?key|api[_ -]?key/i.test(key)) return undefined;
    if (typeof value === 'string' && /Bearer\s+\S+|[?&](?:access_token|token|x-amz-signature|x-amz-credential)=/i.test(value)) return '[REDACTED]';
    return value;
  });
  const response = await fetch(endpoint, { method: 'POST', headers: { 'content-type': 'application/json' }, body, signal: AbortSignal.timeout(20_000) });
  if (!response.ok) throw new Error('CAPTURE_REJECTED');
  const result = await response.json();
  console.log(JSON.stringify({ status: result.status, records: result.records?.length ?? 1, source: 'Ring Developer Playground user-supplied capture', liveDeliveryVerified: false }));
} catch {
  console.error('Capture import failed. Supply a sanitized JSON file and a running loopback bridge with RING_ENVIRONMENT=playground and SHARMAN_RING_TEST_EVENTS_ENABLED=true. Device-status imports also require the source device ID as the second argument.');
  process.exitCode = 1;
}
