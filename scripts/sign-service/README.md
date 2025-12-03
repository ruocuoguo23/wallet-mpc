# Sign Service Nitro Enclave Packaging

These scripts package the `sign-service` binary into a Nitro Enclave-friendly Docker image, build an EIF, and run it with socat bridges for ingress (port 50051) and egress (host port 8080).

## Contents
- `build_server.sh`: Cross-compiles `sign-service` on the build host (e.g., EC2) and writes the binary to `target/sign-service-enclave/`.
- `build-docker.sh`: Builds the runtime image using the precompiled binary.
- `build-eif.sh`: Produces `target/sign-service.enclave.eif` via `nitro-cli`.
- `run-enclave.sh`: Launches the EIF and sets up socat bridges.
- `stop-enclave.sh`: Terminates the enclave.

## Build Prerequisites (build host)
Ensure the EC2 builder has:
- Rust toolchain (`rustup`, `cargo`) with target `x86_64-unknown-linux-musl` installed.
- `protoc` (protobuf compiler).
- Build essentials: `clang`/`gcc`, `musl-tools`, `pkg-config`, `openssl`/`openssl-devel`, `m4`.

## Usage

```bash
# On the builder (produces target/sign-service-enclave/sign-service)
./scripts/sign-service/build_server.sh

# Then package the enclave image using the prebuilt binary
./scripts/sign-service/build-docker.sh
./scripts/sign-service/build-eif.sh
HOST_EGRESS_TARGET_HOST=127.0.0.1 HOST_EGRESS_TARGET_PORT=8080 ./scripts/sign-service/run-enclave.sh
```

Inside the enclave, the gRPC server listens on `127.0.0.1:50051`. Host ingress is proxied via vsock port `50051` to the enclave, while outbound HTTP/gRPC calls from the enclave can reach the host `8080` service through the egress tunnel bound to vsock `4000`.
