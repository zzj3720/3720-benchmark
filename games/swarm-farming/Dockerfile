FROM haskell:9.10.3-slim-bookworm AS build

ARG SWARM_COMMIT=da708abbe5a3d3b339f27e2090d92fe9f4f97ea3

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        git \
        libffi-dev \
        libgmp-dev \
        libtinfo-dev \
        pkg-config \
        zlib1g-dev \
    && rm -rf /var/lib/apt/lists/*

RUN git clone --filter=blob:none https://github.com/swarm-game/swarm.git /opt/swarm \
    && git -C /opt/swarm checkout --detach "${SWARM_COMMIT}" \
    && test "$(git -C /opt/swarm rev-parse HEAD)" = "${SWARM_COMMIT}"

WORKDIR /opt/adapter
COPY games/swarm-farming/swarm-harbor.cabal games/swarm-farming/cabal.project games/swarm-farming/LICENSE ./

# Hackage's signed mirror list still names the retired dream.io mirror. Fetch
# the index directly over TLS; the Swarm source is pinned and verified above,
# and the resulting task artifacts are frozen after the acceptance tests.
RUN cabal user-config init --force \
    && sed -i \
        -e 's|url: http://hackage.haskell.org/|url: https://hackage.haskell.org/|' \
        -e 's|  -- secure: True|  secure: False|' \
        /root/.config/cabal/config \
    && cabal update \
    && cabal build --only-dependencies exe:swarm-harbor

COPY games/swarm-farming/app ./app

RUN cabal build exe:swarm-harbor \
    && mkdir -p /out \
    && cp "$(cabal list-bin exe:swarm-harbor)" /out/swarm-harbor \
    && strip /out/swarm-harbor

FROM rust:1.96-slim-bookworm AS client

WORKDIR /build
COPY games/swarm-farming/client/Cargo.toml games/swarm-farming/client/Cargo.lock ./
COPY games/swarm-farming/client/src/ src/
RUN cargo build --release --locked --bin swarm

FROM scratch AS package

COPY --from=build /out/swarm-harbor /swarm-harbor
COPY --from=client /build/target/release/swarm /swarm
COPY --from=build /opt/swarm/LICENSE /licenses/swarm-LICENSE
COPY games/swarm-farming/LICENSE /licenses/swarm-harbor-LICENSE
COPY --from=build /opt/swarm/data/*.txt /data/
COPY --from=build /opt/swarm/data/*.yaml /data/
COPY --from=build /opt/swarm/data/worlds /data/worlds

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        libffi8 \
        libgmp10 \
        libtinfo6 \
        zlib1g \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /out/swarm-harbor /usr/local/bin/swarm-harbor
COPY --from=build /opt/swarm/LICENSE /usr/share/licenses/swarm/LICENSE
COPY games/swarm-farming/LICENSE /usr/share/licenses/swarm-harbor/LICENSE
COPY --from=build /opt/swarm/data/*.txt /root/.local/share/swarm/data/
COPY --from=build /opt/swarm/data/*.yaml /root/.local/share/swarm/data/
COPY --from=build /opt/swarm/data/worlds /root/.local/share/swarm/data/worlds
COPY games/swarm-farming/data/scenario/farming.yaml /opt/swarm-task/farming.yaml

RUN mkdir -p /var/lib/swarm

EXPOSE 3720

ENTRYPOINT ["swarm-harbor"]
CMD ["serve", "/opt/swarm-task/farming.yaml", "/var/lib/swarm/audit.jsonl", "3720", "1000000"]
