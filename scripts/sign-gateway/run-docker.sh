#!/usr/bin/env bash
set -euo pipefail

IMAGE_NAME=${IMAGE_NAME:-sign-gateway}
IMAGE_TAG=${IMAGE_TAG:-local}
CONTAINER_NAME=${CONTAINER_NAME:-sign-gateway}
HOST_PORT=${HOST_PORT:-8080}
HOST_GRPC_PORT=${HOST_GRPC_PORT:-50050}
HOST_CONFIG_PATH=${HOST_CONFIG_PATH:-config/sign-gateway.yaml}

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "${SCRIPT_DIR}/../../" && pwd)

cd "${REPO_ROOT}"

if docker ps -a --format '{{.Names}}' | grep -q "^${CONTAINER_NAME}$"; then
    echo "Container ${CONTAINER_NAME} already exists. Removing..."
    docker rm -f "${CONTAINER_NAME}" >/dev/null 2>&1
    echo "Container ${CONTAINER_NAME} removed."
fi

if [[ ! -f "${HOST_CONFIG_PATH}" ]]; then
    echo "Config file ${HOST_CONFIG_PATH} not found." >&2
    exit 1
fi

CONFIG_ABS=$(readlink -f "${HOST_CONFIG_PATH}" 2>/dev/null || realpath "${HOST_CONFIG_PATH}")

docker run -d \
    --name "${CONTAINER_NAME}" \
    -p "${HOST_PORT}:8080" \
    -p "${HOST_GRPC_PORT}:50050" \
    -v "${CONFIG_ABS}:/app/config/sign-gateway.yaml:ro" \
    --add-host host.docker.internal:host-gateway \
    "${IMAGE_NAME}:${IMAGE_TAG}" \
    /usr/local/bin/sign-gateway /app/config/sign-gateway.yaml

echo "Container ${CONTAINER_NAME} running: HTTP ${HOST_PORT}, gRPC ${HOST_GRPC_PORT}"