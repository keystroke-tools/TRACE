# TRACE Go Live service

Status: local and hosted publishing support both recorded sessions and active captures;
the browser spectator page is embedded in the same server binary.

`trace-server` is the single service behind the configured TRACE endpoint. It owns
the HTTP API, publisher ingestion, spectator fan-out, and browser spectator page.
TRACE does not split these across separate `api` and `live` hosts.
The hosted service is expected at `https://live.simtrace.run`; self-hosters can run
the same crate at another base URL.

The server receives only versioned canonical messages from `trace-protocol`. It does
not know about Assetto Corsa shared memory, replay files, Arrow storage, or desktop
implementation types.

## Run locally

Install the Mise-managed toolchains and dependencies, then start the service:

```sh
mise install
mise run install
mise run dev-live-service
```

The defaults are:

```text
TRACE_BIND=127.0.0.1:8080
TRACE_PUBLIC_BASE_URL=http://127.0.0.1:8080
```

Set both variables when the bind address and externally visible URL differ. The
public URL is used to generate spectator and publisher WebSocket links; HTTPS is
automatically translated to WSS.

## Install on a Linux server

Tagged releases contain native x86-64 and ARM64 Linux binaries. On a systemd-based
VM, install the latest release with:

```sh
curl -fsSL https://simtrace.run/install-server | sudo sh
```

On first install, the script opens a terminal wizard for any unset listen-address
and public-URL options. Publisher credentials are generated automatically during
desktop bootstrap, so the current server does not require a static operator secret.
Existing configuration skips the wizard.

The installer verifies the release checksum, installs the binary at
`/usr/local/bin/trace-server`, creates `/etc/trace-server.env`, and enables the
`trace-server.service` systemd unit. Running the same command again updates the
binary while preserving the existing environment file. A failed service start
restores the previous binary.

The initial defaults are `TRACE_BIND=127.0.0.1:8080` and
`TRACE_PUBLIC_BASE_URL=https://live.simtrace.run`. Override them on first install:

```sh
curl -fsSL https://simtrace.run/install-server | \
  sudo env TRACE_BIND=127.0.0.1:9000 TRACE_PUBLIC_BASE_URL=https://live.example.com sh
```

After installation, edit `/etc/trace-server.env` and run
`sudo systemctl restart trace-server` to change them. Pin a particular release by
setting `TRACE_SERVER_VERSION` to its exact tag. The public URL should terminate TLS
at a reverse proxy that forwards HTTP and WebSocket upgrade requests to `TRACE_BIND`.
For unattended provisioning, set `TRACE_INSTALL_NONINTERACTIVE=1`; missing options
then use their documented defaults instead of opening the wizard.

Completed `.trace` shares are stored beneath `/var/lib/trace-server` and expire after
seven days by default. Set `TRACE_SHARE_TTL_DAYS` in `/etc/trace-server.env` to retain
them for 1–365 days. Uploads are validated before publication and limited to 128 MiB.

Live publisher credentials, session state, and the rolling playback window are stored
in `/var/lib/trace-server/live.sqlite`. `TRACE_LIVE_RETENTION_MINUTES` controls the
retained window from 1–10 minutes and defaults to 10. Older envelopes are pruned as
new telemetry arrives. A process restart briefly interrupts connected sockets, but
the publisher can authenticate again against the same session and spectators receive
the retained window after reconnecting. Zero-downtime and multi-instance deployment
remain future work.

The service applies in-memory fixed-window limits with conservative defaults. A
limited HTTP request receives `429 Too Many Requests` and `Retry-After: 60`; a
publisher that exceeds its message or byte budget is closed with WebSocket policy
code `1008`. Self-hosters can tune the limits in `/etc/trace-server.env`:

| Variable                               | Default     |
| -------------------------------------- | ----------- |
| `TRACE_BOOTSTRAP_PER_MINUTE`           | `30`        |
| `TRACE_SESSION_CREATIONS_PER_MINUTE`   | `20`        |
| `TRACE_PUBLISHER_UPGRADES_PER_MINUTE`  | `60`        |
| `TRACE_PUBLISH_MESSAGES_PER_SECOND`    | `120`       |
| `TRACE_PUBLISH_BYTES_PER_SECOND`       | `8388608`   |
| `TRACE_MAX_LIVE_SESSIONS`              | `4` per installation |
| `TRACE_MAX_SPECTATORS_PER_SESSION`     | `64`        |
| `TRACE_MAX_SPECTATORS_PER_ADDRESS`     | `16`        |
| `TRACE_MAX_INSTALLATION_RECORDS`       | `100000`    |
| `TRACE_MAX_LIVE_SESSION_RECORDS`       | `10000`     |

Limits use the direct TCP peer address by default. Set
`TRACE_TRUST_PROXY_HEADERS=true` only when TRACE is directly behind a trusted reverse
proxy that replaces (rather than appends untrusted input to) `X-Forwarded-For`.
These counters intentionally reset when the single server process restarts.
Durable-record caps do not reset: expired live sessions are pruned before creating a
new one, and unused installation credentials may be evicted at capacity.
An evicted desktop installation transparently bootstraps a replacement credential on
its next live-session request.

## HTTP API

All implemented routes are versioned under `/api/v1`.

| Method   | Route                                 | Authentication | Purpose                                                |
| -------- | ------------------------------------- | -------------- | ------------------------------------------------------ |
| `GET`    | `/health`                             | none           | Process health probe; returns `204` and version header |
| `POST`   | `/api/v1/installations`               | none           | Bootstrap an installation ID and publishing token      |
| `POST`   | `/api/v1/live-sessions`               | installation   | Create an unlisted live session                        |
| `POST`   | `/api/v1/shared-sessions`             | installation   | Validate and publish an expiring `.trace` package      |
| `GET`    | `/api/v1/shared-sessions/{id}`        | none           | Read metadata for a shared package                     |
| `GET`    | `/api/v1/shared-sessions/{id}/download` | none         | Download the self-contained `.trace` package           |
| `GET`    | `/api/v1/live-sessions/{id}`          | none           | Read public session state and retained sequence bounds |
| `DELETE` | `/api/v1/live-sessions/{id}`          | installation   | Explicitly end an owned session                        |
| `POST`   | `/api/v1/live-sessions/{id}/publisher-credential` | installation | Rotate the session publisher token         |
| `DELETE` | `/api/v1/live-sessions/{id}/publisher-credential` | installation | Revoke the session publisher token         |
| `GET`    | `/api/v1/live-sessions/{id}/publish`  | session token  | Upgrade to the publisher WebSocket                     |
| `GET`    | `/api/v1/live-sessions/{id}/spectate` | none           | Upgrade to the spectator WebSocket                     |
| `GET`    | `/api/v1/instance`                    | none           | Discover this deployment's identity, features, and limits |
| `GET`    | `/`                                   | none           | Open the TRACE Go Live service landing page            |
| `GET`    | `/live/{id}`                          | none           | Open the built-in browser spectator page               |
| `GET`    | `/s/{id}`                             | none           | Open the shared-session download page                  |

The public landing, spectator, and missing-session pages are embedded in the server
binary. They share the TRACE favicon and a hosted social card. Spectator documents
derive their title, description, canonical URL, and Open Graph/Twitter metadata from
the validated session state; all simulator-provided values are HTML-escaped before
rendering. Unknown routes and expired spectator links return a branded HTML `404`.

`GET /api/v1/instance` describes the deployment rather than the TRACE project as a
whole. It reports the configured instance name and public URL, the running server
version, currently available features, upload size, share lifetime, and live playback
retention. Clients should use this response instead of assuming that every endpoint is
hosted by `live.simtrace.run`. Set `TRACE_INSTANCE_NAME` to give a self-hosted instance
a recognisable name; it defaults to `TRACE`.

Installation-managed requests, including session creation, ending, and completed-session
sharing, send both headers:

```text
X-TRACE-Installation-ID: <installation_id>
Authorization: Bearer <installation_token>
```

The installation token is returned only by installation bootstrap. Creating a live
session returns a separate 256-bit publishing token scoped to that session. Publisher
WebSocket upgrades send that token as `Authorization: Bearer <session_token>` without
the installation identifier. The server stores only SHA-256 digests and compares both
credential types in constant time. A session token cannot publish to another session,
survives a service restart for reconnects, and is revoked when its session ends or
expires. An authenticated installation can rotate or revoke the publisher token
without changing the session ID or spectator URL; rotation immediately invalidates
the previous token. Plain tokens must never appear in spectator URLs, logs, or page
metadata.

Session IDs use 128 bits of operating-system randomness and are intentionally
unguessable. Spectating is unlisted rather than authenticated: anyone with the share
URL can view the stream, but public discovery is not implemented.

## Publishing and buffering

Publisher WebSockets currently accept JSON-encoded protocol-v1 `Envelope` values.
Each envelope must:

- pass all `trace-protocol` shape and resource validation;
- name the session in the WebSocket route;
- fit within 512 KiB; and
- have a sequence greater than every previously accepted envelope.

Accepted envelopes are broadcast to connected spectators and retained in a bounded
rolling queue—10 minutes by default when the desktop publishes at 20 Hz. The same tail
is committed to SQLite, so it survives service and deployment restarts. A new or
reconnecting spectator first receives that retained snapshot in sequence order, then
follows new messages. Ending a session publishes a terminal `end` envelope to spectators.
Rate limits, explicit resume acknowledgements, and multi-instance fan-out remain future
work.

## Recorded-session publishing

The desktop can stream any finalized recording from its session overview. It reads the
immutable Arrow telemetry projection, rebases its clock to the broadcast start, reduces
the stream to approximately 20 Hz, then sends it through the same protocol and service
routes intended for active capture. The live subset currently includes pedals,
steering, speed, RPM, gear, fuel, lap progress/time/sector, world position, and reported
air/track temperatures.

The session overview shows replay progress and exposes copy-link and stop actions. The
fixed title bar keeps cancellation available after navigation. Stopping closes the
publisher and sends an authenticated session-end request. HTTP and WebSocket setup use
15-second connection bounds; transport errors update the UI but never mutate the source
recording.

### Local screen mode

`LOCAL SCREEN` in the session overview starts the same publisher and spectator service
inside the desktop process. By default it binds to an available loopback port; the
port can be fixed in Settings → Connectivity (use `0` to return to automatic selection).
TRACE then shows a URL such as `http://127.0.0.1:43127/live/<id>`; open that URL in a browser on the same machine,
for example on a second monitor or a full-screen display. The local service remains
available after the replay finishes so the retained telemetry can still be inspected.
Starting another local stream replaces the previous listener. This mode is intentionally
loopback-only; sharing to another device will require an explicit LAN binding and access
policy in a later slice.

The spectator page is compiled into the `trace-server` binary, so hosted and local
spectating use the same implementation and do not require a separate frontend process.
The browser reconnects automatically after a transient WebSocket interruption; a terminal
session end stops retries and leaves the retained replay visible.

The desktop caches publisher credentials for its current process. Hosted services persist
the authoritative credential hash in SQLite, while the intentionally temporary local-screen
server keeps it in memory. An authentication rejection discards the desktop cache so a later
attempt can bootstrap cleanly.

## Active-capture publishing

While a simulator session is recording, the Live Capture page can publish it through
the hosted service or an internal local-screen server. Accepted canonical frames enter
a bounded in-process fan-out queue and are projected to the same 20 Hz protocol stream
used by recorded replays. The capture thread never waits for the publisher: slow
spectators, a full queue, or a transport failure may drop live-delivery frames but do
not delay or cancel Arrow persistence.

Stopping Go Live or ending/changing the simulator session publishes a terminal message.
A publisher transport failure moves Go Live into a reconnecting state while local
capture keeps running. TRACE retries with exponential backoff capped at ten seconds,
retains the same unlisted spectator URL, and resumes ordered publishing when the service
returns. The server accepts an exact retransmission of the last envelope idempotently,
which covers connections lost while delivery confirmation is ambiguous. Frames may be
dropped from the bounded live queue during a long outage; the local recording remains
complete and independent.

### Automatic streaming

Settings → Connectivity can enable automatic Go Live publishing. The destination is
the same global `LOCAL` or `ONLINE` selection used by the title bar and manual controls;
local mode also uses the configured spectator port. Automation is disabled by default.

Filters are simulator-specific. Assetto Corsa currently exposes Practice, Qualifying,
Race, Hotlap, Time Attack, Drift, and Drag, and each can be enabled independently.
Sessions with an unknown type are not published automatically. Future simulator adapters
can add their own filter sets without treating Assetto Corsa's values as universal.

When an eligible capture starts, TRACE launches the normal active publisher asynchronously.
Manual broadcasts take priority and are never replaced. If AC changes sessions before the
previous automatic publisher finishes its closing handshake, TRACE briefly waits for that
publisher and then starts the next stream. Local listeners are restarted cleanly so a fixed
port can be reused. Authentication, connection, and server failures update Go Live status
and produce a desktop notification, but the capture worker continues writing local telemetry.

For Assetto Corsa, the publisher reads the selected layout's `ai/fast_lane.ai` and
sends its centreline and road edges once at session start. The pit-wall map therefore
has complete static geometry before a lap is driven, and overlays live world position
without learning spins or off-track excursions as track shape. Simulator adapters can
provide the same canonical geometry for future games; the spectator falls back to a
clearly labelled driven line when no static geometry exists. Session metadata also
includes a human-readable simulator name and compact simulator mark.

## Next slices

1. Add publisher acknowledgements, explicit resume negotiation, and rate limits.
2. Add retained-sequence negotiation and shared fan-out for multi-instance deployments.
3. Extend the spectator protocol with multi-car timing and privacy controls.
