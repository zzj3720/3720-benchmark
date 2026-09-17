FROM rust:1.96-slim-bookworm AS build

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release --locked \
    --bin minesweeper \
    --bin minesweeper-server \
    --bin minesweeper-verifier

ARG TARGETARCH
RUN mkdir -p "/export/${TARGETARCH}" \
    && cp target/release/minesweeper-server "/export/${TARGETARCH}/minesweeper-server" \
    && cp target/release/minesweeper-verifier "/export/${TARGETARCH}/minesweeper-verifier" \
    && cp target/release/minesweeper "/export/${TARGETARCH}/minesweeper"

FROM scratch
COPY --from=build /export/ /
