FROM rust:1.96-slim-bookworm AS build

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release --locked \
    --bin sausage \
    --bin sausage-solve \
    --bin sausage-server \
    --bin sausage-verifier

ARG TARGETARCH
RUN mkdir -p "/export/${TARGETARCH}" \
    && cp target/release/sausage-server "/export/${TARGETARCH}/sausage-server" \
    && cp target/release/sausage-verifier "/export/${TARGETARCH}/sausage-verifier" \
    && cp target/release/sausage "/export/${TARGETARCH}/sausage" \
    && cp target/release/sausage-solve "/export/${TARGETARCH}/sausage-solve"

FROM scratch
COPY --from=build /export/ /
