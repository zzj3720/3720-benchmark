FROM rust:1.96-slim-bookworm AS build

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release --locked

ARG TARGETARCH
RUN mkdir -p "/export/${TARGETARCH}" \
    && cp target/release/benchmark-observer-relay "/export/${TARGETARCH}/observer-relay"

FROM scratch
COPY --from=build /export/ /
