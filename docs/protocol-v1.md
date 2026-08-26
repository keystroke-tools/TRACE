# TRACE live protocol v1

Status: implemented data model, validation, and initial JSON/WebSocket transport.

The protocol carries canonical TRACE data. It never exposes Assetto Corsa shared
memory, Rust struct layouts, storage records, or desktop-internal models.

## Envelope

Every message has this logical structure:

```text
Envelope
  protocol_version
  message_id
  session_id
  sequence
  sent_at_unix_ms
  payload
```

`protocol_version` must equal `1`. A server must reject unsupported versions rather
than guessing compatibility.

`message_id` is 1–64 ASCII alphanumeric, hyphen, or underscore characters.
`session_id` is 16–64 characters from the same alphabet. This validation supports
unguessable identifiers but does not itself generate them; generation must use a
cryptographically secure random source.

`sequence` orders publisher messages and will support duplicate detection and
reconnect/resume behavior. `sent_at_unix_ms` is presentation/diagnostic wall time,
not the telemetry clock. Telemetry uses monotonic elapsed nanoseconds.

## Payloads

The v1 payload discriminator uses snake-case names:

| Payload           | Purpose                                                     |
| ----------------- | ----------------------------------------------------------- |
| `hello`           | Publisher version and canonical source identity             |
| `session_state`   | Driver, simulator, car, track/layout, type, and live status |
| `track_geometry`  | Bounded world-space centreline and left/right road edges    |
| `telemetry_batch` | Bounded columnar live samples                               |
| `lap_event`       | Completed/invalid/unknown lap timing update                 |
| `heartbeat`       | Keeps an otherwise idle live session current                |
| `end`             | Explicit terminal state and reason                          |

Live status is one of `live`, `paused`, `reconnecting`, or `ended`. Spectators must
show these states rather than silently freezing on stale data.

Text fields are non-empty, reject control characters, and default to at most 256
UTF-8 bytes. Implementations may negotiate or configure stricter limits, but cannot
accept values beyond server resource policy.

Session state carries a stable simulator key plus optional human-readable name and
compact mark. Track geometry contains 3–4,096 aligned finite points per line; it is
published once and uses the same world-space metres as `motion.position.x/z`.

## Telemetry batches

Telemetry is columnar to avoid repeating channel names and units for every sample:

```text
TelemetryBatch
  base_elapsed_ns
  offsets_ns[]
  channels[]

ChannelColumn
  id
  unit
  values[]          nullable
```

Each timestamp is `base_elapsed_ns + offsets_ns[index]`. Offsets must be strictly
increasing. Every channel column has exactly the same length as the offset array.
`None` represents unavailable data at that sample and is distinct from zero.

The default validation limits are:

| Limit              |     Value |
| ------------------ | --------: |
| Samples per batch  |       512 |
| Channels per batch |        64 |
| Text field size    | 256 bytes |
| Channel ID size    | 128 bytes |

At the intended live rate of approximately 20 Hz, these bounds leave ample batching
headroom while constraining malformed inputs. Transport-level encoded byte limits
must also be enforced once an encoding is selected.

Channel IDs contain lowercase ASCII letters, digits, `.`, `_`, and `-`. Duplicate
IDs in one batch are rejected. Present v1 units are:

```text
ratio
metre
metres_per_second
radian
revolutions_per_minute
pascal
degree_celsius
litre
second
unitless
```

TRACE's current live projection publishes driver inputs, speed, RPM, gear, fuel,
lap time/progress/sector/completed-lap count, world position, temperatures, and the
explicit simulator pit flags when available. The pit channels are
`session.in_pit` and `session.in_pit_lane`; each uses unitless `0`/`1` values rather
than inferring pit state from speed or track position.

All present numeric values must be finite; NaN and infinities are invalid wire data.
The wire vocabulary is intentionally smaller than the domain vocabulary and expands
only through an explicit protocol versioning decision.

## Lap events

A lap event contains a zero-based lap index, optional duration in seconds, and
validity (`valid`, `invalid`, or `unknown`). A supplied duration must be finite and
non-negative. Unknown validity is preserved rather than treated as valid.

## Validation order

Receivers validate before publishing or retaining a message:

1. supported envelope version
2. message and session identifiers
3. text/resource limits
4. batch dimensions and timestamp ordering
5. channel identifier uniqueness and column alignment
6. numeric finiteness and lap timing validity
7. transport encoded size, authentication, and sequence policy

Protocol validation does not itself authenticate a publisher. The Go Live service
wraps it with installation credentials, publisher ownership, a 512 KiB WebSocket
message limit, and strictly increasing per-session sequences. Rate limiting,
acknowledgements, durable resume state, and binary encoding remain future work.

## Service boundaries

TRACE uses one configurable service boundary for live-session creation, publisher
ingestion, spectator fan-out, session metadata, and live-session links. The hosted
base URL is `https://live.simtrace.run`. Secure WebSocket URLs are derived from the
configured HTTPS base rather than stored separately. A self-hosted deployment may
replace that base URL, but must implement the same versioned protocol and validation
boundaries.

## Spectator time shifting

The spectator page needs a bounded playback buffer rather than a latest-frame-only
view. Its seek bar spans the telemetry still retained by the service and ends with a
`LIVE ●` control at the current broadcast edge.

- Dragging or clicking the seek bar moves only that spectator through retained data;
  it does not pause the publisher or affect other spectators.
- Leaving the live edge puts the viewer in an explicit behind-live state and stops
  automatic cursor following.
- Selecting `LIVE ●` jumps to the newest available timestamp and resumes following.
- If retention evicts the requested timestamp, the viewer is clamped to the oldest
  available point and told that earlier data is no longer buffered.
- Transport messages must expose the retained time/sequence range and allow bounded
  backfill or resume from a requested sequence or timestamp.

An already-recorded TRACE session can also act as a publisher for development and
testing. It uses the same live encoder and server path, preserves the original sample
spacing, and rebases its monotonic clock to the broadcast start. For such a broadcast,
`LIVE ●` means the current replay broadcast position—not the end of the stored session.
The recorded session remains immutable.

## Compatibility rules

- Never serialize internal Rust or simulator structs directly as wire messages.
- Additive optional fields still require compatibility review and fixtures.
- Changed meaning, unit, required fields, or enum behavior requires a new version.
- Unknown protocol versions are rejected explicitly.
- A v1 receiver must preserve missing values and must not invent absent channels.
- Local full-resolution recording remains independent of live encoding and failure.
