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

An update restarts the process and interrupts active streams. The current service
keeps credentials and sessions in memory; durable state and zero-downtime deployment
remain future work.

## HTTP API

All implemented routes are versioned under `/api/v1`.

| Method   | Route                                 | Authentication | Purpose                                                |
| -------- | ------------------------------------- | -------------- | ------------------------------------------------------ |
| `GET`    | `/health`                             | none           | Process health probe; returns `204`                    |
| `POST`   | `/api/v1/installations`               | none           | Bootstrap an installation ID and publishing token      |
| `POST`   | `/api/v1/live-sessions`               | publisher      | Create an unlisted live session                        |
| `GET`    | `/api/v1/live-sessions/{id}`          | none           | Read public session state and retained sequence bounds |
| `DELETE` | `/api/v1/live-sessions/{id}`          | publisher      | Explicitly end an owned session                        |
| `GET`    | `/api/v1/live-sessions/{id}/publish`  | publisher      | Upgrade to the publisher WebSocket                     |
| `GET`    | `/api/v1/live-sessions/{id}/spectate` | none           | Upgrade to the spectator WebSocket                     |
| `GET`    | `/live/{id}`                          | none           | Open the built-in browser spectator page               |

Publisher requests send both headers:

```text
X-TRACE-Installation-ID: <installation_id>
Authorization: Bearer <publishing_token>
```

The token is returned only by installation bootstrap. The server stores its SHA-256
digest and compares credentials in constant time. Desktop persistence must treat the
plain token as a secret and must not place it in spectator URLs or logs.

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
2,400-message queue—roughly two minutes when the desktop publishes at 20 Hz. A new
spectator first receives that retained snapshot in sequence order, then follows new
messages. Ending a session publishes a terminal `end` envelope to spectators.

The buffer and credentials are intentionally in memory in this first slice. Restarting
the process therefore ends existing sessions and requires installation bootstrap
again. Durable credentials/session records, expiry, rate limits, resume acknowledgements,
and multi-instance fan-out must be added before deploying the public service.

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

Publisher credentials remain memory-only while the server credential store is also
memory-only. A failed broadcast discards the cached credential so a later attempt can
bootstrap against a restarted service.

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

For Assetto Corsa, the publisher reads the selected layout's `ai/fast_lane.ai` and
sends its centreline and road edges once at session start. The pit-wall map therefore
has complete static geometry before a lap is driven, and overlays live world position
without learning spins or off-track excursions as track shape. Simulator adapters can
provide the same canonical geometry for future games; the spectator falls back to a
clearly labelled driven line when no static geometry exists. Session metadata also
includes a human-readable simulator name and compact simulator mark.

## Next slices

1. Persist installation credentials and live-session lifecycle in the service database.
2. Add publisher acknowledgements, reconnect/resume negotiation, expiry, and rate limits.
3. Add explicit publisher acknowledgements and retained-sequence negotiation for multi-instance deployments.
