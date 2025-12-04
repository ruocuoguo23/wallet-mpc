#!/bin/sh
set -eu

# Ensure required directories exist (may be missing in minimal enclave filesystem)
mkdir -p /tmp /dev/shm
chmod 1777 /tmp /dev/shm 2>/dev/null || true

PORT=${PORT:-50051}
VSOCK_PORT=${VSOCK_PORT:-$PORT}
APP_BIN=${APP_BIN:-/usr/local/bin/sign-service}
ENCLAVE_EGRESS_ENABLED=${ENCLAVE_EGRESS_ENABLED:-1}
ENCLAVE_EGRESS_PORT=${ENCLAVE_EGRESS_PORT:-8080}
HOST_EGRESS_VSOCK_PORT=${HOST_EGRESS_VSOCK_PORT:-8080}
HOST_PARENT_CID=${HOST_PARENT_CID:-3}

KMS_PROXY_PORT=${KMS_PROXY_PORT:-5000}
AWS_REGION=${AWS_REGION:-us-east-1}
KMSTOOL_BIN=${KMSTOOL_BIN:-/app/kmstool_enclave_cli}
KMSTOOL_FALLBACKS="/usr/local/bin/kmstool_enclave_cli /opt/kmstool/kmstool_enclave_cli"
AGE_KEY_VSOCK_PORT=${AGE_KEY_VSOCK_PORT:-7101}
KEY_SHARES_VSOCK_PORT=${KEY_SHARES_VSOCK_PORT:-7102}
AWS_CREDS_VSOCK_PORT=${AWS_CREDS_VSOCK_PORT:-7100}
AGE_KEY_CIPHERTEXT_PATH=${AGE_KEY_CIPHERTEXT_PATH:-/tmp/age-private.key.enc}
KEY_SHARES_ENCRYPTED_PATH=${KEY_SHARES_ENCRYPTED_PATH:-/tmp/service_key_shares.json.age}
AGE_KEY_PLAINTEXT_PATH=${AGE_KEY_PLAINTEXT_PATH:-/dev/shm/age-private.key}
KEY_SHARES_PLAINTEXT_PATH=${KEY_SHARES_PLAINTEXT_PATH:-/dev/shm/service_key_shares.json}
AWS_CREDS_PATH=${AWS_CREDS_PATH:-/dev/shm/aws-credentials.json}
SIGN_SERVICE_KEY_SHARE_FILE_ENV=${SIGN_SERVICE_KEY_SHARE_FILE_ENV:-SIGN_SERVICE_KEY_SHARE_FILE}

if [ "$#" -eq 0 ]; then
    set -- "$APP_BIN" "config/sign-service.yaml"
fi

log() {
    echo "[enclave-entrypoint] $*"
}

secure_delete() {
    local path=$1
    if [ -f "$path" ]; then
        if command -v shred >/dev/null 2>&1; then
            shred -u "$path" 2>/dev/null || rm -f "$path"
        else
            rm -f "$path"
        fi
    fi
}

cleanup() {
    if [ -n "${SOCAT_INGRESS_PID:-}" ] && kill -0 "$SOCAT_INGRESS_PID" >/dev/null 2>&1; then
        kill "$SOCAT_INGRESS_PID" >/dev/null 2>&1 || true
        wait "$SOCAT_INGRESS_PID" 2>/dev/null || true
    fi
    if [ -n "${SOCAT_EGRESS_PID:-}" ] && kill -0 "$SOCAT_EGRESS_PID" >/dev/null 2>&1; then
        kill "$SOCAT_EGRESS_PID" >/dev/null 2>&1 || true
        wait "$SOCAT_EGRESS_PID" 2>/dev/null || true
    fi
    if [ -n "${AGE_KEY_SOCAT_PID:-}" ] && kill -0 "$AGE_KEY_SOCAT_PID" >/dev/null 2>&1; then
        kill "$AGE_KEY_SOCAT_PID" >/dev/null 2>&1 || true
        wait "$AGE_KEY_SOCAT_PID" 2>/dev/null || true
    fi
    if [ -n "${KEY_SHARES_SOCAT_PID:-}" ] && kill -0 "$KEY_SHARES_SOCAT_PID" >/dev/null 2>&1; then
        kill "$KEY_SHARES_SOCAT_PID" >/dev/null 2>&1 || true
        wait "$KEY_SHARES_SOCAT_PID" 2>/dev/null || true
    fi
    if [ -n "${AWS_CREDS_SOCAT_PID:-}" ] && kill -0 "$AWS_CREDS_SOCAT_PID" >/dev/null 2>&1; then
        kill "$AWS_CREDS_SOCAT_PID" >/dev/null 2>&1 || true
        wait "$AWS_CREDS_SOCAT_PID" 2>/dev/null || true
    fi
    secure_delete "$AGE_KEY_PLAINTEXT_PATH"
    secure_delete "$AWS_CREDS_PATH"
}

trap cleanup EXIT

log "configuring loopback"
ip addr add 127.0.0.1/32 dev lo >/dev/null 2>&1 || true
ip link set dev lo up

start_ingress_bridge() {
    log "starting vsock ingress on port ${VSOCK_PORT} -> 127.0.0.1:${PORT}"
    socat VSOCK-LISTEN:${VSOCK_PORT},fork,reuseaddr TCP:127.0.0.1:${PORT} &
    SOCAT_INGRESS_PID=$!
}

start_egress_bridge() {
    if [ "${ENCLAVE_EGRESS_ENABLED}" = "1" ]; then
        log "starting egress tunnel 127.0.0.1:${ENCLAVE_EGRESS_PORT} -> vsock ${HOST_PARENT_CID}:${HOST_EGRESS_VSOCK_PORT}"
        socat TCP-LISTEN:${ENCLAVE_EGRESS_PORT},fork,reuseaddr VSOCK-CONNECT:${HOST_PARENT_CID}:${HOST_EGRESS_VSOCK_PORT} &
        SOCAT_EGRESS_PID=$!
    fi
}

parse_kmstool_output() {
    awk -F': ' '/PLAINTEXT/ {print $2}' | tr -d '\r\n'
}

find_kmstool() {
    if [ -x "${KMSTOOL_BIN}" ]; then
        echo "${KMSTOOL_BIN}"
        return 0
    fi
    for candidate in ${KMSTOOL_FALLBACKS}; do
        if [ -x "${candidate}" ]; then
            echo "${candidate}"
            return 0
        fi
    done
    return 1
}

load_aws_credentials() {
    if [ ! -f "$AWS_CREDS_PATH" ]; then
        log "AWS credentials file not found at ${AWS_CREDS_PATH}"
        return 1
    fi

    if ! command -v jq >/dev/null 2>&1; then
        log "jq not found, cannot parse AWS credentials"
        return 1
    fi

    AWS_ACCESS_KEY_ID=$(jq -r '.AccessKeyId // empty' "$AWS_CREDS_PATH")
    AWS_SECRET_ACCESS_KEY=$(jq -r '.SecretAccessKey // empty' "$AWS_CREDS_PATH")
    AWS_SESSION_TOKEN=$(jq -r '.Token // .SessionToken // empty' "$AWS_CREDS_PATH")

    if [ -z "$AWS_ACCESS_KEY_ID" ] || [ -z "$AWS_SECRET_ACCESS_KEY" ]; then
        log "AWS credentials incomplete in ${AWS_CREDS_PATH}"
        return 1
    fi

    log "AWS credentials loaded successfully"
    return 0
}

decrypt_age_key() {
    log "decrypting age private key via kmstool-enclave-cli"
    if [ ! -f "$AGE_KEY_CIPHERTEXT_PATH" ]; then
        log "age key ciphertext not found at ${AGE_KEY_CIPHERTEXT_PATH}"
        return 1
    fi

    KMSTOOL_PATH=$(find_kmstool) || {
        log "kmstool_enclave_cli binary not found"
        return 1
    }
    log "using kmstool at ${KMSTOOL_PATH}"

    # Load AWS credentials from file
    if ! load_aws_credentials; then
        log "failed to load AWS credentials"
        return 1
    fi

    # Read ciphertext and base64 encode it
    CIPHERTEXT_B64=$(base64 -w0 "${AGE_KEY_CIPHERTEXT_PATH}")

    log "calling kmstool decrypt with region=${AWS_REGION}, proxy-port=${KMS_PROXY_PORT}"
    KMSTOOL_OUTPUT=$("${KMSTOOL_PATH}" decrypt \
        --region "${AWS_REGION}" \
        --proxy-port "${KMS_PROXY_PORT}" \
        --aws-access-key-id "${AWS_ACCESS_KEY_ID}" \
        --aws-secret-access-key "${AWS_SECRET_ACCESS_KEY}" \
        --aws-session-token "${AWS_SESSION_TOKEN}" \
        --ciphertext "${CIPHERTEXT_B64}" \
        ${KMS_KEY_ID:+--key-id "${KMS_KEY_ID}"} 2>&1) || {
        log "kmstool decrypt failed: ${KMSTOOL_OUTPUT}"
        return 1
    }

    PLAINTEXT_B64=$(echo "${KMSTOOL_OUTPUT}" | parse_kmstool_output)

    if [ -z "${PLAINTEXT_B64}" ]; then
        log "kmstool output missing PLAINTEXT: ${KMSTOOL_OUTPUT}"
        return 1
    fi

    echo "${PLAINTEXT_B64}" | base64 -d > "${AGE_KEY_PLAINTEXT_PATH}"
    chmod 600 "${AGE_KEY_PLAINTEXT_PATH}"
    log "age private key decrypted"

    # Clear credentials from memory
    unset AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY AWS_SESSION_TOKEN
    secure_delete "$AWS_CREDS_PATH"
}

decrypt_key_shares() {
    log "decrypting key shares"
    if ! age --decrypt -i "${AGE_KEY_PLAINTEXT_PATH}" "${KEY_SHARES_ENCRYPTED_PATH}" > "${KEY_SHARES_PLAINTEXT_PATH}"; then
        log "age decryption failed"
        return 1
    fi

    if command -v jq >/dev/null 2>&1; then
        if ! jq empty "${KEY_SHARES_PLAINTEXT_PATH}" >/dev/null 2>&1; then
            log "decrypted key shares are not valid JSON"
            return 1
        fi
    fi

    if command -v sha256sum >/dev/null 2>&1; then
        KEY_SHARE_HASH=$(sha256sum "${KEY_SHARES_PLAINTEXT_PATH}" | cut -d' ' -f1)
        log "key shares SHA256: ${KEY_SHARE_HASH}"
    fi

    secure_delete "${AGE_KEY_PLAINTEXT_PATH}"
    log "age private key wiped from disk"
}

start_secret_listener() {
    local port=$1
    local dest=$2
    local pid_var=$3
    local label=$4
    local dest_dir
    dest_dir=$(dirname "$dest")
    mkdir -p "$dest_dir"
    rm -f "$dest"
    socat -u VSOCK-LISTEN:${port},reuseaddr "FILE:${dest},creat,truncate" &
    local pid=$!
    eval "$pid_var=$pid"
    sleep 1
    if ! kill -0 "$pid" >/dev/null 2>&1; then
        log "failed to start listener for ${label} on vsock port ${port}"
        exit 1
    fi
    log "listening for ${label} on vsock port ${port}"
}

wait_for_secret() {
    local pid_var=$1
    local dest=$2
    local label=$3
    eval "local pid=\${$pid_var}"
    if [ -z "${pid}" ]; then
        log "no listener pid recorded for ${label}"
        exit 1
    fi
    if wait "$pid"; then
        log "received ${label} -> ${dest}"
    else
        log "listener for ${label} exited with an error"
        exit 1
    fi
}

start_ingress_bridge
start_egress_bridge

# Start listeners for all secrets
start_secret_listener "${AWS_CREDS_VSOCK_PORT}" "${AWS_CREDS_PATH}" AWS_CREDS_SOCAT_PID "AWS credentials"
start_secret_listener "${AGE_KEY_VSOCK_PORT}" "${AGE_KEY_CIPHERTEXT_PATH}" AGE_KEY_SOCAT_PID "age key ciphertext"
start_secret_listener "${KEY_SHARES_VSOCK_PORT}" "${KEY_SHARES_ENCRYPTED_PATH}" KEY_SHARES_SOCAT_PID "key shares ciphertext"

# Wait for all secrets to arrive
wait_for_secret AWS_CREDS_SOCAT_PID "${AWS_CREDS_PATH}" "AWS credentials"
wait_for_secret AGE_KEY_SOCAT_PID "${AGE_KEY_CIPHERTEXT_PATH}" "age key ciphertext"
wait_for_secret KEY_SHARES_SOCAT_PID "${KEY_SHARES_ENCRYPTED_PATH}" "key shares ciphertext"

decrypt_age_key
decrypt_key_shares

export "${SIGN_SERVICE_KEY_SHARE_FILE_ENV}=${KEY_SHARES_PLAINTEXT_PATH}"
log "exported ${SIGN_SERVICE_KEY_SHARE_FILE_ENV}=${KEY_SHARES_PLAINTEXT_PATH}"

log "launching application: $*"
exec "$@"
