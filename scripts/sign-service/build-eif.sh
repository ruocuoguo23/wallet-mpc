#!/usr/bin/env bash
set -euo pipefail

IMAGE_NAME=${IMAGE_NAME:-sign-service-enclave}
IMAGE_TAG=${IMAGE_TAG:-local}
EIF_OUTPUT=${EIF_OUTPUT:-target/sign-service.enclave.eif}

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "${SCRIPT_DIR}/../../" && pwd)

FULL_IMAGE="${IMAGE_NAME}:${IMAGE_TAG}"

if ! command -v nitro-cli >/dev/null; then
    echo "nitro-cli not found in PATH" >&2
    exit 1
fi

cd "${REPO_ROOT}"
mkdir -p "$(dirname "${EIF_OUTPUT}")"

nitro-cli build-enclave \
    --docker-uri "${FULL_IMAGE}" \
    --output-file "${EIF_OUTPUT}"

echo "EIF artifact generated at ${EIF_OUTPUT}"
