FROM rust:1.96-slim-bookworm AS build

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release --locked \
    --bin kitchen \
    --bin kitchen-oracle \
    --bin kitchen-server \
    --bin kitchen-verifier

ARG TARGETARCH
RUN mkdir -p "/export/${TARGETARCH}" \
    && cp target/release/kitchen-server "/export/${TARGETARCH}/kitchen-server" \
    && cp target/release/kitchen-verifier "/export/${TARGETARCH}/kitchen-verifier" \
    && cp target/release/kitchen-oracle "/export/${TARGETARCH}/kitchen-oracle" \
    && cp target/release/kitchen "/export/${TARGETARCH}/kitchen"

FROM scratch
COPY --from=build /export/ /
