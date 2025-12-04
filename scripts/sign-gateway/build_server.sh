#!/usr/bin/env bash
set -euo pipefail

TARGET_TRIPLE=${TARGET_TRIPLE:-x86_64-unknown-linux-musl}
ARTIFACT_DIR=${ARTIFACT_DIR:-target/sign-gateway}
BIN_NAME=sign-gateway
OUTPUT_BIN="${ARTIFACT_DIR}/${BIN_NAME}"

if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo is required but not found in PATH" >&2
    exit 1
fi

if ! rustup target list --installed | grep -q "^${TARGET_TRIPLE}$"; then
    rustup target add "${TARGET_TRIPLE}"
fi

echo "Building ${BIN_NAME} for ${TARGET_TRIPLE}..."
cargo build --release --package sign-gateway --bin sign-gateway --target "${TARGET_TRIPLE}"

mkdir -p "${ARTIFACT_DIR}"
cp "target/${TARGET_TRIPLE}/release/${BIN_NAME}" "${OUTPUT_BIN}"
chmod +x "${OUTPUT_BIN}"

echo "Binary copied to ${OUTPUT_BIN}"