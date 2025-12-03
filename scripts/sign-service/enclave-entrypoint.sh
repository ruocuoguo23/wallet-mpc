#!/bin/sh
set -eu

PORT=${PORT:-50051}
VSOCK_PORT=${VSOCK_PORT:-$PORT}
APP_BIN=${APP_BIN:-/usr/local/bin/sign-service}
ENCLAVE_EGRESS_ENABLED=${ENCLAVE_EGRESS_ENABLED:-1}
ENCLAVE_EGRESS_PORT=${ENCLAVE_EGRESS_PORT:-8080}
HOST_EGRESS_VSOCK_PORT=${HOST_EGRESS_VSOCK_PORT:-8080}
HOST_PARENT_CID=${HOST_PARENT_CID:-3}

if [ "$#" -eq 0 ]; then
    set -- "$APP_BIN" "config/sign-service.yaml"
fi

log() {
    echo "[enclave-entrypoint] $*"
}

log "configuring loopback"
ip addr add 127.0.0.1/32 dev lo >/dev/null 2>&1 || true
ip link set dev lo up

log "starting vsock ingress on port ${VSOCK_PORT} -> 127.0.0.1:${PORT}"
socat VSOCK-LISTEN:${VSOCK_PORT},fork,reuseaddr TCP:127.0.0.1:${PORT} &
SOCAT_INGRESS_PID=$!

if [ "${ENCLAVE_EGRESS_ENABLED}" = "1" ]; then
    log "starting egress tunnel 127.0.0.1:${ENCLAVE_EGRESS_PORT} -> vsock ${HOST_PARENT_CID}:${HOST_EGRESS_VSOCK_PORT}"
    socat TCP-LISTEN:${ENCLAVE_EGRESS_PORT},fork,reuseaddr VSOCK-CONNECT:${HOST_PARENT_CID}:${HOST_EGRESS_VSOCK_PORT} &
    SOCAT_EGRESS_PID=$!
fi

cleanup() {
    if kill -0 "$SOCAT_INGRESS_PID" >/dev/null 2>&1; then
        kill "$SOCAT_INGRESS_PID" >/dev/null 2>&1 || true
        wait "$SOCAT_INGRESS_PID" 2>/dev/null || true
    fi
    if [ -n "${SOCAT_EGRESS_PID:-}" ] && kill -0 "$SOCAT_EGRESS_PID" >/dev/null 2>&1; then
        kill "$SOCAT_EGRESS_PID" >/dev/null 2>&1 || true
        wait "$SOCAT_EGRESS_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT

log "launching application: $*"
exec "$@"
