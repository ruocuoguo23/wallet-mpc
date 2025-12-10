#!/usr/bin/env bash
set -euo pipefail

EIF_PATH=${EIF_PATH:-target/sign-service.enclave.eif}
ENCLAVE_NAME=${ENCLAVE_NAME:-sign-service-enclave}
CPU_COUNT=${CPU_COUNT:-2}
MEMORY_MIB=${MEMORY_MIB:-512}
# Debug mode: set to 1 to enable debug mode (PCR0 will be all zeros, console available)
# WARNING: Never use debug mode in production! KMS attestation requires real PCR0 values.
DEBUG_MODE=${DEBUG_MODE:-0}
CONSOLE_FILE=${CONSOLE_FILE:-target/sign-service-enclave-console.log}
# Console streaming only works in debug mode
CONSOLE_STREAM_ENABLED=${CONSOLE_STREAM_ENABLED:-${DEBUG_MODE}}
HOST_GRPC_PORT=${HOST_GRPC_PORT:-50051}
VSOCK_PORT=${VSOCK_PORT:-50051}
SOCAT_BIN=${SOCAT_BIN:-socat}
HOST_EGRESS_ENABLED=${HOST_EGRESS_ENABLED:-1}
HOST_EGRESS_TARGET_HOST=${HOST_EGRESS_TARGET_HOST:-127.0.0.1}
HOST_EGRESS_TARGET_PORT=${HOST_EGRESS_TARGET_PORT:-8080}
HOST_EGRESS_VSOCK_PORT=${HOST_EGRESS_VSOCK_PORT:-8080}
ENCLAVE_CID=${ENCLAVE_CID:-16}

AGE_KEY_S3_URI=${AGE_KEY_S3_URI:-}
KEY_SHARES_S3_URI=${KEY_SHARES_S3_URI:-}
SECRETS_TMPDIR=${SECRETS_TMPDIR:-}
AGE_KEY_VSOCK_PORT=${AGE_KEY_VSOCK_PORT:-7101}
KEY_SHARES_VSOCK_PORT=${KEY_SHARES_VSOCK_PORT:-7102}
AWS_CREDS_VSOCK_PORT=${AWS_CREDS_VSOCK_PORT:-7100}
VSOC_TRANSFER_RETRIES=${VSOC_TRANSFER_RETRIES:-20}
READINESS_SLEEP_SECONDS=${READINESS_SLEEP_SECONDS:-5}

KMS_PROXY_PORT=${KMS_PROXY_PORT:-5000}
AWS_REGION=${AWS_REGION:-us-east-1}
KMS_PROXY_DEST_HOST=${KMS_PROXY_DEST_HOST:-kms.${AWS_REGION}.amazonaws.com}
KMS_PROXY_DEST_PORT=${KMS_PROXY_DEST_PORT:-443}
KMS_PROXY_ENABLE=${KMS_PROXY_ENABLE:-1}

# EC2 Instance Metadata Service endpoints
IMDS_TOKEN_URL="http://169.254.169.254/latest/api/token"
IMDS_ROLE_URL="http://169.254.169.254/latest/meta-data/iam/security-credentials/"
IMDS_TOKEN_TTL=${IMDS_TOKEN_TTL:-21600}

REQUIRED_TOOLS=(aws "${SOCAT_BIN}" nitro-cli curl jq)

require_tools() {
    for tool in "$@"; do
        if ! command -v "$tool" >/dev/null 2>&1; then
            echo "Required tool '$tool' not found in PATH" >&2
            exit 1
        fi
    done
}

require_secrets_config() {
    if [[ -z "${AGE_KEY_S3_URI}" || -z "${KEY_SHARES_S3_URI}" ]]; then
        cat <<'EOF' >&2
AGE_KEY_S3_URI and KEY_SHARES_S3_URI must be set, e.g.:
  export AGE_KEY_S3_URI=s3://bucket/enclave/age-private.key.enc
  export KEY_SHARES_S3_URI=s3://bucket/enclave/service_key_shares.json.age
EOF
        exit 1
    fi
}

create_secrets_dir() {
    if [[ -n "${SECRETS_TMPDIR}" ]]; then
        mkdir -p "${SECRETS_TMPDIR}"
        SECRETS_DIR="${SECRETS_TMPDIR}"
    else
        SECRETS_DIR=$(mktemp -d /dev/shm/sign-service-secrets.XXXX)
    fi
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
    if [[ -n "${KMS_PROXY_PID:-}" ]]; then
        sudo kill "${KMS_PROXY_PID}" 2>/dev/null || true
        wait "${KMS_PROXY_PID}" 2>/dev/null || true
    fi
    if [[ -n "${CONSOLE_PID:-}" ]]; then
        kill "${CONSOLE_PID}" 2>/dev/null || true
        wait "${CONSOLE_PID}" 2>/dev/null || true
    fi
}

cleanup() {
    cleanup_bridges
    if [[ -n "${SECRETS_DIR:-}" && -d "${SECRETS_DIR}" ]]; then
        rm -rf "${SECRETS_DIR}"
    fi
}

trap cleanup EXIT

start_vsock_proxy() {
    "${SOCAT_BIN}" TCP-LISTEN:"${HOST_GRPC_PORT}",fork,reuseaddr VSOCK-CONNECT:${ENCLAVE_CID}:"${VSOCK_PORT}" &
    INGRESS_PID=$!
    echo "Started vsock proxy (${SOCAT_BIN}) on host port ${HOST_GRPC_PORT} (PID ${INGRESS_PID})"
}

start_host_egress_bridge() {
    "${SOCAT_BIN}" VSOCK-LISTEN:"${HOST_EGRESS_VSOCK_PORT}",fork,reuseaddr TCP-CONNECT:"${HOST_EGRESS_TARGET_HOST}":"${HOST_EGRESS_TARGET_PORT}" &
    EGRESS_PID=$!
    echo "Started egress bridge (${SOCAT_BIN}) vsock ${HOST_EGRESS_VSOCK_PORT} -> ${HOST_EGRESS_TARGET_HOST}:${HOST_EGRESS_TARGET_PORT} (PID ${EGRESS_PID})"
}

start_kms_proxy() {
    if [[ "${KMS_PROXY_ENABLE}" != "1" ]]; then
        echo "KMS proxy disabled (KMS_PROXY_ENABLE=${KMS_PROXY_ENABLE})."
        return 0
    fi
    if ! command -v vsock-proxy >/dev/null 2>&1; then
        echo "vsock-proxy not found; cannot start KMS proxy." >&2
        return 1
    fi
    echo "Starting KMS vsock proxy on port ${KMS_PROXY_PORT} -> ${KMS_PROXY_DEST_HOST}:${KMS_PROXY_DEST_PORT}"
    vsock-proxy "${KMS_PROXY_PORT}" "${KMS_PROXY_DEST_HOST}" "${KMS_PROXY_DEST_PORT}" &
    KMS_PROXY_PID=$!
    sleep 1
    if ! kill -0 "${KMS_PROXY_PID}" >/dev/null 2>&1; then
        echo "Failed to launch vsock-proxy for KMS." >&2
        return 1
    fi
    echo "KMS proxy started (PID ${KMS_PROXY_PID})"
}

fetch_aws_credentials() {
    local creds_file=$1
    echo "Fetching AWS credentials from EC2 Instance Metadata Service..."

    # Get IMDSv2 token
    local token
    token=$(curl -s -X PUT "${IMDS_TOKEN_URL}" -H "X-aws-ec2-metadata-token-ttl-seconds: ${IMDS_TOKEN_TTL}")
    if [[ -z "${token}" ]]; then
        echo "Failed to get IMDSv2 token" >&2
        return 1
    fi

    # Get IAM role name
    local role_name
    role_name=$(curl -s -H "X-aws-ec2-metadata-token: ${token}" "${IMDS_ROLE_URL}")
    if [[ -z "${role_name}" ]]; then
        echo "Failed to get IAM role name from IMDS" >&2
        return 1
    fi
    echo "Using IAM role: ${role_name}"

    # Get credentials
    local creds_json
    creds_json=$(curl -s -H "X-aws-ec2-metadata-token: ${token}" "${IMDS_ROLE_URL}${role_name}")
    if [[ -z "${creds_json}" ]]; then
        echo "Failed to get credentials from IMDS" >&2
        return 1
    fi

    # Validate credentials JSON
    if ! echo "${creds_json}" | jq -e '.AccessKeyId and .SecretAccessKey and .Token' >/dev/null 2>&1; then
        echo "Invalid credentials JSON from IMDS" >&2
        return 1
    fi

    # Write credentials to file
    echo "${creds_json}" > "${creds_file}"
    chmod 600 "${creds_file}"
    echo "AWS credentials saved to ${creds_file}"
}

parse_s3_uri() {
    local uri=$1
    local prefix="s3://"
    if [[ $uri != ${prefix}* ]]; then
        echo "Invalid S3 URI: ${uri}" >&2
        exit 1
    fi
    local trimmed=${uri#${prefix}}
    BUCKET=${trimmed%%/*}
    KEY=${trimmed#*/}
}

fetch_secret_from_s3() {
    local uri=$1
    local dest=$2
    parse_s3_uri "$uri"
    echo "Downloading ${uri} -> ${dest}"
    aws s3 cp "${uri}" "${dest}"
}

send_file_to_enclave() {
    local file=$1
    local port=$2
    local label=$3
    local attempt=1
    echo "Transferring ${label} to enclave (port ${port})"
    while (( attempt <= VSOC_TRANSFER_RETRIES )); do
        if "${SOCAT_BIN}" -u "FILE:${file}" VSOCK-CONNECT:${ENCLAVE_CID}:${port}; then
            echo "✓ ${label} sent"
            return 0
        fi
        echo "Retry ${attempt}/${VSOC_TRANSFER_RETRIES} for ${label}"
        sleep 1
        ((attempt++))
    done
    echo "Failed to send ${label} to enclave" >&2
    exit 1
}

require_tools "${REQUIRED_TOOLS[@]}"
require_secrets_config
create_secrets_dir

AGE_KEY_LOCAL_PATH="${SECRETS_DIR}/age-private.key.enc"
KEY_SHARES_LOCAL_PATH="${SECRETS_DIR}/service_key_shares.json.age"
AWS_CREDS_LOCAL_PATH="${SECRETS_DIR}/aws-credentials.json"

# Fetch AWS credentials from IMDS
fetch_aws_credentials "${AWS_CREDS_LOCAL_PATH}"

# Fetch encrypted secrets from S3
fetch_secret_from_s3 "${AGE_KEY_S3_URI}" "${AGE_KEY_LOCAL_PATH}"
fetch_secret_from_s3 "${KEY_SHARES_S3_URI}" "${KEY_SHARES_LOCAL_PATH}"

if nitro-cli describe-enclaves | grep -q "\"EnclaveName\": \"${ENCLAVE_NAME}\""; then
    echo "Enclave ${ENCLAVE_NAME} already running. Terminate it before starting a new one." >&2
    exit 1
fi

if [[ ! -f "${EIF_PATH}" ]]; then
    echo "EIF file not found at ${EIF_PATH}. Run scripts/sign-service/build-eif.sh first." >&2
    exit 1
fi

start_kms_proxy

# Build nitro-cli run-enclave command
ENCLAVE_RUN_ARGS=(
    --eif-path "${EIF_PATH}"
    --cpu-count "${CPU_COUNT}"
    --memory "${MEMORY_MIB}"
    --enclave-cid "${ENCLAVE_CID}"
    --enclave-name "${ENCLAVE_NAME}"
)

if [[ "${DEBUG_MODE}" == "1" ]]; then
    echo "⚠️  WARNING: Running in DEBUG MODE. PCR0 will be all zeros!"
    echo "⚠️  Do NOT use debug mode in production - KMS attestation will fail with real PCR0 policy."
    ENCLAVE_RUN_ARGS+=(--debug-mode)
fi

nitro-cli run-enclave "${ENCLAVE_RUN_ARGS[@]}"

start_console_stream() {
    # Console is ONLY available in debug mode
    if [[ "${DEBUG_MODE}" != "1" ]]; then
        echo "Console not available in production mode (requires --debug-mode)."
        return 0
    fi

    if [[ "${CONSOLE_STREAM_ENABLED}" != "1" ]]; then
        echo "Console streaming disabled. Run 'nitro-cli console --enclave-name ${ENCLAVE_NAME}' manually."
        return 0
    fi

    echo "Attaching to enclave console (background)."
    if command -v stdbuf >/dev/null 2>&1; then
        stdbuf -oL -eL nitro-cli console --enclave-name "${ENCLAVE_NAME}" | tee -a "${CONSOLE_FILE}" &
    else
        nitro-cli console --enclave-name "${ENCLAVE_NAME}" | tee -a "${CONSOLE_FILE}" &
    fi
    CONSOLE_PID=$!
}

start_console_stream

sleep "${READINESS_SLEEP_SECONDS}"

touch "${CONSOLE_FILE}"

# Send all secrets to enclave (order matters: credentials first)
send_file_to_enclave "${AWS_CREDS_LOCAL_PATH}" "${AWS_CREDS_VSOCK_PORT}" "AWS credentials"
send_file_to_enclave "${AGE_KEY_LOCAL_PATH}" "${AGE_KEY_VSOCK_PORT}" "age key ciphertext"
send_file_to_enclave "${KEY_SHARES_LOCAL_PATH}" "${KEY_SHARES_VSOCK_PORT}" "key shares ciphertext"

start_vsock_proxy

if [[ "${HOST_EGRESS_ENABLED}" == "1" ]]; then
    start_host_egress_bridge
fi

echo "✅ Enclave '${ENCLAVE_NAME}' is running."
if [[ "${DEBUG_MODE}" == "1" ]]; then
    echo "   Mode: DEBUG (PCR0 = all zeros)"
else
    echo "   Mode: PRODUCTION (KMS attestation enabled)"
fi
echo "   gRPC available at localhost:${HOST_GRPC_PORT}"

if [[ -n "${CONSOLE_PID:-}" ]]; then
    echo "Console streaming active (PID ${CONSOLE_PID}). Press Ctrl+C to stop."
    wait "${CONSOLE_PID}" || true
else
    # In production mode, just wait indefinitely to keep bridges alive
    echo "Press Ctrl+C to stop the enclave and bridges."
    wait "${INGRESS_PID}" || true
fi
