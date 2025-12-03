# syntax=docker/dockerfile:1

FROM alpine:3.20 AS builder

RUN apk add --no-cache build-base curl git musl-dev openssl-dev pkgconfig m4 protobuf-dev protobuf
ENV PATH="/root/.cargo/bin:${PATH}"
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal && \
    rustup target add x86_64-unknown-linux-musl
WORKDIR /app
COPY sign-service/rust-toolchain.toml ./rust-toolchain.toml
COPY sign-service/Cargo.local.toml ./Cargo.toml
COPY sign-service ./sign-service
COPY participant ./participant
COPY proto ./proto
COPY vendor ./vendor
RUN cargo build --manifest-path ./Cargo.toml --package sign-service --bin sign-service --target x86_64-unknown-linux-musl --release

FROM alpine:3.20
RUN apk add --no-cache ca-certificates socat iproute2 dumb-init
WORKDIR /app
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/sign-service /usr/local/bin/sign-service
COPY config/sign-service.yaml /app/config/sign-service.yaml
COPY service_key_shares.json /app/service_key_shares.json
COPY scripts/sign-service/enclave-entrypoint.sh /usr/local/bin/enclave-entrypoint.sh
RUN chmod +x /usr/local/bin/enclave-entrypoint.sh /usr/local/bin/sign-service
ENV PORT=50051 \
    VSOCK_PORT=50051 \
    APP_BIN=/usr/local/bin/sign-service \
    ENCLAVE_EGRESS_ENABLED=1 \
    ENCLAVE_EGRESS_PORT=4000 \
    HOST_EGRESS_VSOCK_PORT=4000 \
    HOST_PARENT_CID=3
ENTRYPOINT ["/usr/bin/dumb-init", "--"]
CMD ["/usr/local/bin/enclave-entrypoint.sh", "/usr/local/bin/sign-service", "config/sign-service.yaml"]
