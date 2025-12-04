#!/usr/bin/env bash
set -euo pipefail

TARGET_TRIPLE=${TARGET_TRIPLE:-x86_64-unknown-linux-musl}
ARTIFACT_DIR=${ARTIFACT_DIR:-target/sign-service-enclave}
BIN_NAME=sign-service
OUTPUT_BIN="${ARTIFACT_DIR}/${BIN_NAME}"

NITRO_SDK_DIR=${NITRO_SDK_DIR:-modules/aws-nitro-enclaves-sdk-c}
KMSTOOL_DIR="${NITRO_SDK_DIR}/bin/kmstool-enclave-cli"
KMSTOOL_BIN_SRC="${KMSTOOL_DIR}/kmstool_enclave_cli"
KMSTOOL_LIB_SRC="${KMSTOOL_DIR}/libnsm.so"
KMSTOOL_BIN_DEST="${ARTIFACT_DIR}/kmstool_enclave_cli"
KMSTOOL_LIB_DEST="${ARTIFACT_DIR}/libnsm.so"

if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo is required but not found in PATH" >&2
    exit 1
fi

if ! command -v protoc >/dev/null 2>&1; then
    echo "protoc is required but not found in PATH" >&2
    exit 1
fi

if ! command -v docker >/dev/null 2>&1; then
    echo "docker is required to build kmstool-enclave-cli" >&2
    exit 1
fi

if ! command -v sha256sum >/dev/null 2>&1; then
    echo "WARNING: sha256sum not found; checksums will be skipped" >&2
fi

if ! rustup target list --installed | grep -q "^${TARGET_TRIPLE}$"; then
    echo "Adding Rust target ${TARGET_TRIPLE}"
    rustup target add "${TARGET_TRIPLE}"
fi

mkdir -p "${ARTIFACT_DIR}"

# Build sign-service
if [ "${SKIP_SIGN_SERVICE_BUILD:-0}" != "1" ]; then
    echo "Building ${BIN_NAME} for ${TARGET_TRIPLE}..."
    cargo build --release --package sign-service --bin sign-service --target "${TARGET_TRIPLE}"
    cp "target/${TARGET_TRIPLE}/release/${BIN_NAME}" "${OUTPUT_BIN}"
    chmod +x "${OUTPUT_BIN}"
    echo "Binary copied to ${OUTPUT_BIN}"
fi

# Build kmstool-enclave-cli
if [ "${SKIP_KMSTOOL_BUILD:-0}" != "1" ]; then
    if [ ! -d "${KMSTOOL_DIR}" ]; then
        echo "kmstool-enclave-cli directory not found at ${KMSTOOL_DIR}" >&2
        exit 1
    fi

    echo "Building kmstool-enclave-cli via Docker..."
    (cd "${KMSTOOL_DIR}" && ./build.sh)

    if [ ! -f "${KMSTOOL_BIN_SRC}" ] || [ ! -f "${KMSTOOL_LIB_SRC}" ]; then
        echo "Failed to build kmstool-enclave-cli or libnsm.so" >&2
        exit 1
    fi

    cp "${KMSTOOL_BIN_SRC}" "${KMSTOOL_BIN_DEST}"
    cp "${KMSTOOL_LIB_SRC}" "${KMSTOOL_LIB_DEST}"
    chmod +x "${KMSTOOL_BIN_DEST}"
    echo "kmstool artifacts copied to ${ARTIFACT_DIR}"
fi

if command -v sha256sum >/dev/null 2>&1; then
    echo "Artifacts checksums:"
    sha256sum "${OUTPUT_BIN}" || true
    [ -f "${KMSTOOL_BIN_DEST}" ] && sha256sum "${KMSTOOL_BIN_DEST}" || true
    [ -f "${KMSTOOL_LIB_DEST}" ] && sha256sum "${KMSTOOL_LIB_DEST}" || true
fi
