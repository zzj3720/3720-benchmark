FROM rust:1.96-slim-bookworm AS build

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release --locked \
    --bin operator \
    --bin operator-server \
    --bin operator-verifier

ARG TARGETARCH
RUN mkdir -p "/export/${TARGETARCH}" \
    && cp target/release/operator-server "/export/${TARGETARCH}/operator-server" \
    && cp target/release/operator-verifier "/export/${TARGETARCH}/operator-verifier" \
    && cp target/release/operator "/export/${TARGETARCH}/operator"

FROM scratch
COPY --from=build /export/ /
