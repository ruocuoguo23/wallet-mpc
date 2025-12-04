# syntax=docker/dockerfile:1

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates socat iproute2 dumb-init bash jq coreutils \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /tmp /dev/shm \
    && chmod 1777 /tmp /dev/shm

# Install age from GitHub releases (not available in Debian repos)
ARG AGE_VERSION=1.2.0
RUN apt-get update && apt-get install -y --no-install-recommends wget \
    && wget -q -O /tmp/age.tar.gz https://github.com/FiloSottile/age/releases/download/v${AGE_VERSION}/age-v${AGE_VERSION}-linux-amd64.tar.gz \
    && tar -xzf /tmp/age.tar.gz -C /tmp \
    && mv /tmp/age/age /usr/local/bin/age \
    && mv /tmp/age/age-keygen /usr/local/bin/age-keygen \
    && chmod +x /usr/local/bin/age /usr/local/bin/age-keygen \
    && rm -rf /tmp/age /tmp/age.tar.gz \
    && apt-get remove -y wget && apt-get autoremove -y \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY config/sign-service.yaml /app/config/sign-service.yaml
COPY service_key_shares.json /app/service_key_shares.json
COPY target/sign-service-enclave/sign-service /usr/local/bin/sign-service
COPY target/sign-service-enclave/kmstool_enclave_cli /app/kmstool_enclave_cli
COPY target/sign-service-enclave/libnsm.so /app/libnsm.so
COPY scripts/sign-service/enclave-entrypoint.sh /usr/local/bin/enclave-entrypoint.sh
RUN chmod +x /usr/local/bin/enclave-entrypoint.sh /usr/local/bin/sign-service /app/kmstool_enclave_cli
ENV PORT=50051 \
    VSOCK_PORT=50051 \
    APP_BIN=/usr/local/bin/sign-service \
    ENCLAVE_EGRESS_ENABLED=1 \
    ENCLAVE_EGRESS_PORT=8080 \
    HOST_EGRESS_VSOCK_PORT=8080 \
    HOST_PARENT_CID=3 \
    KMSTOOL_BIN=/app/kmstool_enclave_cli \
    LD_LIBRARY_PATH="/usr/lib:/lib:/app"
ENTRYPOINT ["/usr/bin/dumb-init", "--"]
CMD ["/usr/local/bin/enclave-entrypoint.sh", "/usr/local/bin/sign-service", "/app/config/sign-service.yaml"]
