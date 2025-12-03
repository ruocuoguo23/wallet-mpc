#!/usr/bin/env bash
set -euo pipefail

IMAGE_NAME=${IMAGE_NAME:-sign-gateway}
IMAGE_TAG=${IMAGE_TAG:-local}
CONTAINER_NAME=${CONTAINER_NAME:-sign-gateway}
HOST_PORT=${HOST_PORT:-8080}
HOST_CONFIG_PATH=${HOST_CONFIG_PATH:-config/sign-gateway.yaml}

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "${SCRIPT_DIR}/../../" && pwd)

cd "${REPO_ROOT}"

if docker ps -a --format '{{.Names}}' | grep -q "^${CONTAINER_NAME}$"; then
    echo "Container ${CONTAINER_NAME} already exists. Remove it or pick a different CONTAINER_NAME." >&2
    exit 1
fi

if [[ ! -f "${HOST_CONFIG_PATH}" ]]; then
    echo "Config file ${HOST_CONFIG_PATH} not found." >&2
    exit 1
fi

CONFIG_ABS=$(readlink -f "${HOST_CONFIG_PATH}" 2>/dev/null || realpath "${HOST_CONFIG_PATH}")

docker run -d \
    --name "${CONTAINER_NAME}" \
    -p "${HOST_PORT}:8080" \
    -v "${CONFIG_ABS}:/app/config/sign-gateway.yaml:ro" \
    "${IMAGE_NAME}:${IMAGE_TAG}" \
    /usr/local/bin/sign-gateway /app/config/sign-gateway.yaml

echo "Container ${CONTAINER_NAME} running on port ${HOST_PORT}"
