FROM rust:1.96-slim-bookworm AS build

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release --locked \
    --bin sokoban \
    --bin sokoban-server \
    --bin sokoban-verifier

ARG TARGETARCH
RUN mkdir -p "/export/${TARGETARCH}" \
    && cp target/release/sokoban-server "/export/${TARGETARCH}/sokoban-server" \
    && cp target/release/sokoban-verifier "/export/${TARGETARCH}/sokoban-verifier" \
    && cp target/release/sokoban "/export/${TARGETARCH}/sokoban"

FROM scratch
COPY --from=build /export/ /
