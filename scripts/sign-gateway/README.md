# Sign Gateway Docker Packaging

Scripts in this directory build and run the `sign-gateway` service inside a plain Docker container (no enclave required).

## Contents
- `build_server.sh`: Cross-compiles `sign-gateway` for `x86_64-unknown-linux-musl` on the build host (e.g., AWS Linux EC2) and writes the binary to `target/sign-gateway/`.
- `build-docker.sh`: Produces a minimal Alpine-based image using the prebuilt binary and config files.
- `run-docker.sh`: Launches the Docker container, mapping host port 8080 by default.
- `stop-docker.sh`: Stops and removes the running container.

## Build Host Prerequisites
- Rust toolchain (`rustup`, `cargo`) with target `x86_64-unknown-linux-musl` installed
- `protoc` (if you add protobuf-dependent features later)
- Build essentials (`gcc`/`clang`, `pkg-config`, `openssl` headers, `m4`) if not already present

## Usage

```bash
# 1. Build the binary on the host
./scripts/sign-gateway/build_server.sh

# 2. Build the runtime image (uses the compiled binary)
./scripts/sign-gateway/build-docker.sh

# 3. Run the gateway container (defaults to exposing 8080)
./scripts/sign-gateway/run-docker.sh

# 4. Stop the container when done
./scripts/sign-gateway/stop-docker.sh
```

Adjust `config/sign-gateway.yaml` before building if you need different host/port or CORS settings; the image copies the file verbatim into `/app/config/sign-gateway.yaml`.

