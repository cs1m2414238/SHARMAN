# Ring fixtures

`device-list.json` is a sanitized documentation-shaped example with synthetic
IDs and `_captured:false`. The owner supplied a collapsed Device List fragment;
its missing attributes and relationships cannot be reconstructed as a capture.

`event-history-on-demand.json` contains all nine records supplied by the owner
from the Ring Developer Playground, with fixture metadata added. IDs, original
resource/event types, timestamps and empty CV relationships are preserved. Each
record becomes a neutral `system_ping` HomeEvent; it cannot create a visitor.

`device-status-online.json` is the owner's status response. Its `data.id` is a
status-resource ID. A caller must provide the source device ID from discovery or
the requested endpoint. Reported time and response metadata time stay separate.

The `webhook-*.json` files follow Amazon's published motion notification schema.
Their identifiers are synthetic and `_captured:false`; they do not establish
live delivery. Motion, package and vehicle all use `motion_detected` with a
classification in `data.attributes.sub_type`.

No captured Motion, Package or Vehicle delivery is claimed. No credential fields
were present in the owner-supplied history/status captures. Never add tokens,
Authorization headers, OAuth secrets, HMAC keys or signed media URLs to fixtures.
