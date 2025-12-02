#!/usr/bin/env bash
set -euo pipefail

ENCLAVE_NAME=${ENCLAVE_NAME:-sign-service-enclave}

if ! command -v nitro-cli >/dev/null; then
    echo "nitro-cli not found in PATH" >&2
    exit 1
fi

if ! nitro-cli describe-enclaves | grep -q "\"EnclaveName\": \"${ENCLAVE_NAME}\""; then
    echo "Enclave ${ENCLAVE_NAME} is not running." >&2
    exit 1
fi

echo "Terminating enclave ${ENCLAVE_NAME}..."
nitro-cli terminate-enclave --enclave-name "${ENCLAVE_NAME}"

echo "Enclave ${ENCLAVE_NAME} terminated successfully."

