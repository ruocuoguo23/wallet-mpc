# Sign Service Nitro Enclave Packaging

These scripts package the `sign-service` binary into a Nitro Enclave-friendly Docker image, build an EIF, and run it with socat bridges for ingress (port 50051) and egress (host port 8080).

## Contents
- `build_server.sh`: Cross-compiles `sign-service` on the build host (e.g., EC2) and writes the binary to `target/sign-service-enclave/`.
- `build-docker.sh`: Builds the runtime image using the precompiled binary.
- `build-eif.sh`: Produces `target/sign-service.enclave.eif` via `nitro-cli`.
- `run-enclave.sh`: Launches the EIF and sets up socat bridges.
- `stop-enclave.sh`: Terminates the enclave.

## Repository Setup

To make sure `kmstool-enclave-cli` sources are available, initialize submodules after cloning the repo:

```bash
# from repo root
git submodule update --init --recursive
```

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              EC2 Parent Host                                 │
│                                                                              │
│  ┌─────────────┐    ┌─────────────┐    ┌──────────────────────────────────┐ │
│  │  S3 Bucket  │───▶│ run-enclave │───▶│         Nitro Enclave            │ │
│  │  - age.enc  │    │    .sh      │    │                                  │ │
│  │  - shares   │    │             │    │  ┌────────────────────────────┐ │ │
│  └─────────────┘    │ 1. Fetch    │    │  │  enclave-entrypoint.sh     │ │ │
│                     │    IMDS     │    │  │                            │ │ │
│  ┌─────────────┐    │    creds    │    │  │  1. Receive AWS creds      │ │ │
│  │    IMDS     │───▶│             │    │  │  2. Receive age.key.enc    │ │ │
│  │ (169.254.   │    │ 2. Download │    │  │  3. Receive shares.age     │ │ │
│  │  169.254)   │    │    from S3  │    │  │  4. kmstool decrypt        │ │ │
│  └─────────────┘    │             │    │  │  5. age decrypt shares     │ │ │
│                     │ 3. Start    │    │  │  6. Launch sign-service    │ │ │
│  ┌─────────────┐    │    enclave  │    │  └────────────────────────────┘ │ │
│  │ vsock-proxy │◀──▶│             │    │                                  │ │
│  │  port 5000  │    │ 4. Send     │    │  ┌────────────────────────────┐ │ │
│  │  → KMS      │    │    secrets  │    │  │     kmstool-enclave-cli    │ │ │
│  └─────────────┘    │    via      │    │  │     (vsock → KMS proxy)    │ │ │
│                     │    vsock    │    │  └────────────────────────────┘ │ │
│                     └─────────────┘    └──────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────┘

Vsock Ports:
  - 7100: AWS credentials (JSON)
  - 7101: KMS-encrypted age private key
  - 7102: age-encrypted key shares
  - 5000: KMS proxy (vsock-proxy → kms.<region>.amazonaws.com:443)
  - 50051: gRPC ingress (host TCP → enclave)
  - 8080: Egress bridge (enclave → host TCP)
```

## Secret Injection Workflow

### Prerequisites

1. **KMS Key**: Create a KMS key for encrypting the age private key
2. **S3 Bucket**: Store encrypted artifacts
3. **IAM Role**: EC2 instance role with permissions for:
   - `kms:Decrypt` on the KMS key
   - `s3:GetObject` on the S3 bucket

### Prepare Encrypted Artifacts

```bash
# 1. Generate age keypair (one-time setup)
age-keygen -o age-private.key
# Output: public key: age1xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx

# 2. Encrypt the age private key with KMS
aws kms encrypt \
  --key-id arn:aws:kms:us-east-1:ACCOUNT:key/KEY_ID \
  --plaintext fileb://age-private.key \
  --output text \
  --query CiphertextBlob | base64 --decode > age-private.key.enc

# 3. Encrypt key shares with age public key
age -r age1xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx \
  -o service_key_shares.json.age \
  service_key_shares.json

# 4. Upload to S3
aws s3 cp age-private.key.enc s3://your-bucket/enclave/age-private.key.enc
aws s3 cp service_key_shares.json.age s3://your-bucket/enclave/service_key_shares.json.age
```

### Run the Enclave

```bash
export AGE_KEY_S3_URI=s3://your-bucket/enclave/age-private.key.enc
export KEY_SHARES_S3_URI=s3://your-bucket/enclave/service_key_shares.json.age
export AWS_REGION=us-east-1

# Example with pre-uploaded artifacts:
# export AGE_KEY_S3_URI=s3://sign-service-enclave-artifacts/enclave/age-private.key.enc
# export KEY_SHARES_S3_URI=s3://sign-service-enclave-artifacts/enclave/service_key_shares.json.age

./scripts/sign-service/run-enclave.sh
```

The script will:
1. Fetch temporary AWS credentials from EC2 Instance Metadata Service (IMDS)
2. Download encrypted artifacts from S3
3. Start the KMS vsock proxy
4. Launch the enclave
5. Inject credentials and encrypted files via vsock
6. Inside enclave: decrypt age key via KMS, then decrypt key shares

## AWS Credentials Flow

Since Nitro Enclaves are isolated from the host network, AWS credentials cannot be obtained directly inside the enclave. The solution:

1. **Parent host** fetches temporary credentials from IMDS (IMDSv2):
   ```
   GET http://169.254.169.254/latest/api/token
   GET http://169.254.169.254/latest/meta-data/iam/security-credentials/<role-name>
   ```

2. **Credentials JSON** is sent to enclave via vsock port 7100:
   ```json
   {
     "AccessKeyId": "ASIA...",
     "SecretAccessKey": "...",
     "Token": "...",
     "Expiration": "2025-12-09T12:00:00Z"
   }
   ```

3. **Enclave** uses these credentials with `kmstool_enclave_cli`:
   ```bash
   kmstool_enclave_cli decrypt \
     --region us-east-1 \
     --proxy-port 5000 \
     --aws-access-key-id "$ACCESS_KEY_ID" \
     --aws-secret-access-key "$SECRET_ACCESS_KEY" \
     --aws-session-token "$SESSION_TOKEN" \
     --ciphertext "$CIPHERTEXT_B64"
   ```

4. **Credentials are wiped** from enclave memory after use

## KMS Proxy

The enclave communicates with AWS KMS through `vsock-proxy` running on the parent host:

```bash
# Started automatically by run-enclave.sh, or manually:
vsock-proxy 5000 kms.us-east-1.amazonaws.com 443 &
```

Environment variables:
- `KMS_PROXY_PORT`: vsock port (default: 5000)
- `KMS_PROXY_DEST_HOST`: KMS endpoint (default: `kms.${AWS_REGION}.amazonaws.com`)
- `KMS_PROXY_DEST_PORT`: KMS port (default: 443)
- `KMS_PROXY_ENABLE`: Enable/disable proxy (default: 1)

## Build Prerequisites (build host)

Ensure the EC2 builder has:
- Rust toolchain (`rustup`, `cargo`) with target `x86_64-unknown-linux-musl` installed
- `protoc` (protobuf compiler)
- Docker (required to build `kmstool-enclave-cli`)
- Build essentials: `clang`/`gcc`, `musl-tools`, `pkg-config`, `openssl`/`openssl-devel`, `m4`
- `nitro-cli` and `vsock-proxy` (for running enclaves)

## Usage

```bash
# On the builder (produces target/sign-service-enclave/ artifacts)
./scripts/sign-service/build_server.sh

# Then package the enclave image using the prebuilt binaries
./scripts/sign-service/build-docker.sh
./scripts/sign-service/build-eif.sh

# On the EC2 parent host (with env vars exported as above)
./scripts/sign-service/run-enclave.sh
```

Inside the enclave, the gRPC server listens on `127.0.0.1:50051`. Host ingress is proxied via vsock port `50051` to the enclave, while outbound HTTP/gRPC calls from the enclave can reach the host `8080` service through the egress tunnel.

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `AGE_KEY_S3_URI` | (required) | S3 URI for KMS-encrypted age private key |
| `KEY_SHARES_S3_URI` | (required) | S3 URI for age-encrypted key shares |
| `AWS_REGION` | `us-east-1` | AWS region for KMS |
| `ENCLAVE_CID` | `16` | Enclave context ID |
| `CPU_COUNT` | `2` | vCPUs for enclave |
| `MEMORY_MIB` | `512` | Memory for enclave |
| `KMS_PROXY_PORT` | `5000` | KMS vsock proxy port |
| `AWS_CREDS_VSOCK_PORT` | `7100` | Vsock port for AWS credentials |
| `AGE_KEY_VSOCK_PORT` | `7101` | Vsock port for age key ciphertext |
| `KEY_SHARES_VSOCK_PORT` | `7102` | Vsock port for key shares |

## Troubleshooting

### Check enclave logs
```bash
nitro-cli console --enclave-name sign-service-enclave
# Or check saved logs:
cat target/sign-service-enclave-console.log
```

### Common errors

1. **"kmstool_enclave_cli: not found"**
   - Ensure Docker image was rebuilt with glibc runtime
   - Check `LD_LIBRARY_PATH` includes `/app`

2. **"--aws-access-key-id must be set"**
   - AWS credentials not received via vsock
   - Check IMDS is accessible from parent host
   - Verify IAM role is attached to EC2 instance

3. **KMS decrypt fails**
   - Verify IAM role has `kms:Decrypt` permission
   - Check KMS key policy allows the role
   - Ensure vsock-proxy is running on port 5000

4. **Vsock connection refused**
   - Increase `READINESS_SLEEP_SECONDS`
   - Check enclave started successfully
