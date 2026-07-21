FROM rust:1.96-slim-bookworm AS build

WORKDIR /repo/games/parabox-intro
COPY games/parabox-intro/Cargo.toml games/parabox-intro/Cargo.lock ./
COPY games/parabox-intro/src/ src/
COPY games/parabox-intro/data/ data/
RUN cargo build --release --locked --bins

ARG TARGETARCH
RUN mkdir -p "/export/${TARGETARCH}" \
    && cp target/release/parabox "/export/${TARGETARCH}/parabox" \
    && cp target/release/parabox-server "/export/${TARGETARCH}/parabox-server" \
    && cp target/release/parabox-verifier "/export/${TARGETARCH}/parabox-verifier"

FROM scratch
COPY --from=build /export/ /
