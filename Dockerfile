# syntax=docker/dockerfile:1.7

FROM node:26.5.0-bookworm-slim AS library-builder

WORKDIR /src

RUN npm install --global pnpm@11.17.0

COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY apps/desktop/package.json apps/desktop/package.json
COPY apps/library/package.json apps/library/package.json
COPY apps/web/package.json apps/web/package.json
COPY packages/trace-ui/package.json packages/trace-ui/package.json

RUN --mount=type=cache,id=trace-pnpm,target=/pnpm/store \
    pnpm config set store-dir /pnpm/store && \
    pnpm install --frozen-lockfile

COPY apps/library apps/library
COPY packages/trace-ui packages/trace-ui
COPY crates/trace-server/assets/library crates/trace-server/assets/library

RUN pnpm --filter @trace/library build

FROM rust:1.91.1-bookworm AS server-builder

WORKDIR /src

COPY . .
COPY --from=library-builder /src/crates/trace-server/assets/library \
    crates/trace-server/assets/library

RUN --mount=type=cache,id=trace-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=trace-server-target,target=/src/target \
    cargo build --locked --release -p trace-server --bin trace-server && \
    cp target/release/trace-server /usr/local/bin/trace-server

FROM debian:bookworm-slim AS runtime

LABEL org.opencontainers.image.title="TRACE server" \
    org.opencontainers.image.description="Self-hosted TRACE live telemetry and session library service" \
    org.opencontainers.image.source="https://github.com/keystroke-tools/TRACE" \
    org.opencontainers.image.licenses="MIT"

RUN apt-get update && \
    apt-get install --yes --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd --gid 10001 trace && \
    useradd --uid 10001 --gid trace --no-create-home --shell /usr/sbin/nologin trace && \
    install --directory --owner trace --group trace /var/lib/trace-server

COPY --from=server-builder /usr/local/bin/trace-server /usr/local/bin/trace-server

ENV TRACE_BIND="0.0.0.0:8080" \
    TRACE_DATA_DIR="/var/lib/trace-server" \
    TRACE_PUBLIC_BASE_URL="http://localhost:8080"

VOLUME ["/var/lib/trace-server"]
EXPOSE 8080
USER trace

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl --fail --silent --show-error http://127.0.0.1:8080/health >/dev/null || exit 1

ENTRYPOINT ["/usr/local/bin/trace-server"]
