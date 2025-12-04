#!/usr/bin/env bash
set -euo pipefail

CONTAINER_NAME=${CONTAINER_NAME:-sign-gateway}

if ! docker ps -a --format '{{.Names}}' | grep -q "^${CONTAINER_NAME}$"; then
    echo "Container ${CONTAINER_NAME} does not exist." >&2
    exit 1
fi

echo "Stopping container ${CONTAINER_NAME}..."
docker stop "${CONTAINER_NAME}" >/dev/null

echo "Removing container ${CONTAINER_NAME}..."
docker rm "${CONTAINER_NAME}" >/dev/null

echo "Container ${CONTAINER_NAME} stopped and removed."