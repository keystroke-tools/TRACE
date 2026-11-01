# Container deployment

TRACE provides container images for its two deployable services:

- `trace-server` is the API, live publisher and spectator service, account system,
  private session library, and embedded browser UI. These intentionally share one
  binary and one SQLite data directory.
- `trace-site` serves the static `simtrace.run` product, download, documentation, and
  FAQ pages. It is optional for self-hosted live/library instances.

The desktop recorder is a native Windows application and is not containerised because
it reads simulator shared memory and owns desktop windows, tray integration, overlays,
and local session storage.

## Start with Compose

Build and start both services from the repository root:

```sh
docker compose up --build -d
```

The defaults expose the TRACE server at `http://localhost:8080` and the product site at
`http://localhost:8081`. Server state is retained in the named
`trace-server-data` volume. Recreating or updating the container does not delete
accounts, live-session credentials, shared packages, or private libraries.

For a public installation, set at least the externally visible URL:

```sh
TRACE_PUBLIC_BASE_URL=https://live.example.com docker compose up --build -d
```

Compose accepts these deployment variables:

| Variable                       | Default                 | Purpose                                      |
| ------------------------------ | ----------------------- | -------------------------------------------- |
| `TRACE_SERVER_PORT`            | `8080`                  | Host port mapped to the server               |
| `TRACE_SITE_PORT`              | `8081`                  | Host port mapped to the optional static site |
| `TRACE_PUBLIC_BASE_URL`        | `http://localhost:8080` | Public origin used in generated links        |
| `TRACE_INSTANCE_NAME`          | `TRACE`                 | Human-readable self-hosted instance name     |
| `TRACE_REGISTRATION_POLICY`    | `disabled`              | Initial account registration policy          |
| `TRACE_SHARE_TTL_DAYS`         | `7`                     | Temporary `.trace` share lifetime            |
| `TRACE_LIVE_RETENTION_MINUTES` | `10`                    | Live replay-buffer duration                  |
| `TRACE_TRUST_PROXY_HEADERS`    | `false`                 | Trust a directly connected reverse proxy     |

All other server rate-limit variables documented in [Go Live](go-live.md) can be added
to the `server.environment` section or supplied with a Compose override.

The server container listens on plain HTTP. Terminate TLS in a reverse proxy and proxy
both ordinary HTTP and WebSocket upgrades to port 8080. Set
`TRACE_TRUST_PROXY_HEADERS=true` only when that proxy overwrites untrusted forwarding
headers and is the container's only direct network peer.

## Build individual images

Build the self-contained server image:

```sh
docker build --tag trace-server:local .
```

Build the optional product site:

```sh
docker build --file apps/web/Dockerfile --tag trace-site:local .
```

Run only the server:

```sh
docker run --detach \
  --name trace-server \
  --publish 8080:8080 \
  --env TRACE_PUBLIC_BASE_URL=http://localhost:8080 \
  --volume trace-server-data:/var/lib/trace-server \
  trace-server:local
```

The runtime images use unprivileged users, expose health checks, and keep their root
filesystems compatible with read-only operation. The server's named volume is its only
durable writable path.
