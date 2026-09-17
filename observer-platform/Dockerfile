FROM rust:1.96-bookworm AS runtime-build
WORKDIR /build
COPY tools/observer/runtime/Cargo.toml tools/observer/runtime/Cargo.lock ./
COPY tools/observer/runtime/src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked --bins && \
    mkdir /binaries && cp target/release/live-gateway target/release/run-archive /binaries/

FROM node:24-bookworm-slim AS web-build
WORKDIR /workspace/observer-platform
COPY observer-platform/package.json observer-platform/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci --no-audit --no-fund
COPY observer-platform ./
COPY games /workspace/games
RUN npm run typecheck && npm run build

FROM debian:bookworm-slim AS gateway
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/* && \
    useradd --uid 10001 --no-create-home observer && mkdir -p /cache /data && chown observer:observer /cache
COPY --from=runtime-build /binaries/ /usr/local/bin/
COPY games/parabox-intro/data/campaign/index.tsv /data/games/parabox-intro/data/campaign/index.tsv
WORKDIR /data
USER observer
ENV LIVE_GATEWAY_HOST=0.0.0.0 LIVE_GATEWAY_PORT=3740 LIVE_CACHE_DIR=/cache LIVE_LEASE_MODE=heartbeat LIVE_DETAIL_CACHE_MIB=32
EXPOSE 3740
CMD ["live-gateway", "--root", "/data"]

FROM node:24-bookworm-slim AS web
WORKDIR /app
COPY --from=web-build --chown=node:node /workspace/observer-platform/dist/standalone/ ./
RUN mv /app/dist/client/assets /app/immutable-assets && mkdir /app/dist/client/assets && chown node:node /app/dist/client/assets
COPY --chmod=755 observer-platform/scripts/container-start.sh /usr/local/bin/container-start
USER node
ENV NODE_ENV=production HOST=0.0.0.0 PORT=3000 NODE_OPTIONS=--max-old-space-size=160 LIVE_GATEWAY_ORIGIN=http://gateway:3740
EXPOSE 3000
CMD ["container-start"]
