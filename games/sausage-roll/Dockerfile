FROM rust:1.96-slim-bookworm AS build

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release --locked \
    --bin sausage-server \
    --bin sausage-verifier

ARG TARGETARCH
RUN mkdir -p "/export/${TARGETARCH}" \
    && cp target/release/sausage-server "/export/${TARGETARCH}/sausage-server" \
    && cp target/release/sausage-verifier "/export/${TARGETARCH}/sausage-verifier"

FROM scratch
COPY --from=build /export/ /
