# syntax=docker/dockerfile:1.7

FROM node:26.5.0-bookworm-slim AS site-builder

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

COPY apps/web apps/web
COPY packages/trace-ui packages/trace-ui

RUN pnpm --filter @trace/web build

FROM nginx:1.29-alpine AS runtime

LABEL org.opencontainers.image.title="TRACE website" \
    org.opencontainers.image.description="Static simtrace.run product and documentation site" \
    org.opencontainers.image.source="https://github.com/keystroke-tools/TRACE" \
    org.opencontainers.image.licenses="MIT"

RUN rm /etc/nginx/conf.d/default.conf

COPY apps/web/nginx.conf /etc/nginx/nginx.conf
COPY --from=site-builder /src/apps/web/dist /usr/share/nginx/html

EXPOSE 8080
USER nginx

HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD wget --quiet --spider http://127.0.0.1:8080/ || exit 1
