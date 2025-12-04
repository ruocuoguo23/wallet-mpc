# syntax=docker/dockerfile:1

FROM alpine:3.20
RUN apk add --no-cache ca-certificates socat iproute2 dumb-init bash
WORKDIR /app
COPY config/sign-service.yaml /app/config/sign-service.yaml
COPY service_key_shares.json /app/service_key_shares.json
COPY target/sign-service-enclave/sign-service /usr/local/bin/sign-service
COPY target/sign-service-enclave/kmstool_enclave_cli /usr/local/bin/kmstool_enclave_cli
COPY target/sign-service-enclave/libnsm.so /usr/lib/libnsm.so
COPY scripts/sign-service/enclave-entrypoint.sh /usr/local/bin/enclave-entrypoint.sh
RUN chmod +x /usr/local/bin/enclave-entrypoint.sh /usr/local/bin/sign-service /usr/local/bin/kmstool_enclave_cli
ENV PORT=50051 \
    VSOCK_PORT=50051 \
    APP_BIN=/usr/local/bin/sign-service \
    ENCLAVE_EGRESS_ENABLED=1 \
    ENCLAVE_EGRESS_PORT=8080 \
    HOST_EGRESS_VSOCK_PORT=8080 \
    HOST_PARENT_CID=3 \
    LD_LIBRARY_PATH="/usr/lib:${LD_LIBRARY_PATH}"
ENTRYPOINT ["/usr/bin/dumb-init", "--"]
CMD ["/usr/local/bin/enclave-entrypoint.sh", "/usr/local/bin/sign-service", "/app/config/sign-service.yaml"]
