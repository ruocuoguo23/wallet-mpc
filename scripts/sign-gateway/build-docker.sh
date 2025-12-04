#!/usr/bin/env bash
set -euo pipefail

IMAGE_NAME=${IMAGE_NAME:-sign-gateway}
IMAGE_TAG=${IMAGE_TAG:-local}
DOCKERFILE_PATH=${DOCKERFILE_PATH:-scripts/sign-gateway/Dockerfile}
PLATFORM=${PLATFORM:-linux/amd64}

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "${SCRIPT_DIR}/../../" && pwd)

cd "${REPO_ROOT}"

docker build \
    --platform "${PLATFORM}" \
    -f "${DOCKERFILE_PATH}" \
    -t "${IMAGE_NAME}:${IMAGE_TAG}" \
    .

echo "Built Docker image ${IMAGE_NAME}:${IMAGE_TAG}"