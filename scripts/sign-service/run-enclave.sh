#!/usr/bin/env bash
set -euo pipefail

EIF_PATH=${EIF_PATH:-target/sign-service.enclave.eif}
ENCLAVE_NAME=${ENCLAVE_NAME:-sign-service-enclave}
CPU_COUNT=${CPU_COUNT:-4}
MEMORY_MIB=${MEMORY_MIB:-1024}
CONSOLE_FILE=${CONSOLE_FILE:-target/sign-service-enclave-console.log}
HOST_GRPC_PORT=${HOST_GRPC_PORT:-50051}
VSOCK_PORT=${VSOCK_PORT:-50051}
SOCAT_BIN=${SOCAT_BIN:-socat}
HOST_EGRESS_ENABLED=${HOST_EGRESS_ENABLED:-1}
HOST_EGRESS_TARGET_HOST=${HOST_EGRESS_TARGET_HOST:-127.0.0.1}
HOST_EGRESS_TARGET_PORT=${HOST_EGRESS_TARGET_PORT:-8080}
HOST_EGRESS_VSOCK_PORT=${HOST_EGRESS_VSOCK_PORT:-4000}

if ! command -v "${SOCAT_BIN}" >/dev/null; then
    echo "socat binary '${SOCAT_BIN}' not found. Install socat on the host or point SOCAT_BIN to an existing binary." >&2
    exit 1
fi

start_vsock_proxy() {
    "${SOCAT_BIN}" TCP-LISTEN:"${HOST_GRPC_PORT}",fork,reuseaddr VSOCK-CONNECT:16:"${VSOCK_PORT}" &
    INGRESS_PID=$!
    echo "Started vsock proxy (${SOCAT_BIN}) on host port ${HOST_GRPC_PORT} (PID ${INGRESS_PID})"
}

start_host_egress_bridge() {
    "${SOCAT_BIN}" VSOCK-LISTEN:"${HOST_EGRESS_VSOCK_PORT}",fork,reuseaddr TCP-CONNECT:"${HOST_EGRESS_TARGET_HOST}":"${HOST_EGRESS_TARGET_PORT}" &
    EGRESS_PID=$!
    echo "Started egress bridge (${SOCAT_BIN}) vsock ${HOST_EGRESS_VSOCK_PORT} -> ${HOST_EGRESS_TARGET_HOST}:${HOST_EGRESS_TARGET_PORT} (PID ${EGRESS_PID})"
}

cleanup_bridges() {
    if [[ -n "${INGRESS_PID:-}" ]]; then
        kill "${INGRESS_PID}" 2>/dev/null || true
        wait "${INGRESS_PID}" 2>/dev/null || true
    fi
    if [[ -n "${EGRESS_PID:-}" ]]; then
        kill "${EGRESS_PID}" 2>/dev/null || true
        wait "${EGRESS_PID}" 2>/dev/null || true
    fi
}

trap cleanup_bridges EXIT

if ! command -v nitro-cli >/dev/null; then
    echo "nitro-cli not found in PATH" >&2
    exit 1
fi

if [[ ! -f "${EIF_PATH}" ]]; then
    echo "EIF file not found at ${EIF_PATH}. Run scripts/sign-service/build-eif.sh first." >&2
    exit 1
fi

if nitro-cli describe-enclaves | grep -q "\"EnclaveName\": \"${ENCLAVE_NAME}\""; then
    echo "Enclave ${ENCLAVE_NAME} already running. Terminate it before starting a new one." >&2
    exit 1
fi

nitro-cli run-enclave \
    --eif-path "${EIF_PATH}" \
    --cpu-count "${CPU_COUNT}" \
    --memory "${MEMORY_MIB}" \
    --enclave-cid 16 \
    --debug-mode \
    --enclave-name "${ENCLAVE_NAME}"

start_vsock_proxy

if [[ "${HOST_EGRESS_ENABLED}" == "1" ]]; then
    start_host_egress_bridge
fi

echo "Streaming enclave console output. Press Ctrl+C to stop."
nitro-cli console --enclave-name "${ENCLAVE_NAME}" | tee "${CONSOLE_FILE}"
