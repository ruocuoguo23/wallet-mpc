# Repository Guidelines for Agents

## Scope

This file applies to the entire repository. More specific `AGENTS.md` files may
override it for their own subtrees if they are added later.

Wallet MPC is a Rust workspace implementing a CGGMP21 threshold-ECDSA wallet on
secp256k1. The production path currently assumes two fixed parties and a 2-of-2
signature: one share is held by the client-side participant and the other by
`sign-service`, optionally inside an AWS Nitro Enclave.

Read [ARCHITECTURE.md](ARCHITECTURE.md) before changing protocol flow,
networking, key-share handling, or deployment scripts.

## Sources of Truth

Use this order when repository documentation disagrees:

1. `proto/proto/mpc.proto` for the public gRPC contract.
2. The Rust implementation under each crate's `src/` directory.
3. `config/*.yaml` and `scripts/` for deployment wiring.
4. Root and crate README files for background and usage examples.

Some README examples describe older 3-party or direct-to-service layouts. Do
not preserve those assumptions in new code. The current signing implementation
uses party indexes `[0, 1]`, routes remote gRPC requests through
`sign-gateway`, and uses `account_id` to select a pre-derived key share.

## Workspace Map

| Path | Responsibility |
| --- | --- |
| `client/` | CLI/demo that constructs an Ethereum prehash and exercises `mpc-client`. |
| `mpc-client/` | Rust/UniFFI client SDK; owns the local participant lifecycle and coordinates gRPC requests. |
| `participant/` | Shared participant gRPC server, key-share selection, CGGMP21 signing, and SSE transport adapter. |
| `sign-service/` | Remote participant process; loads server-side shares and hosts `Participant.SignTx`. |
| `sign-gateway/` | Public edge process; hosts the SSE broker and proxies gRPC signing calls to `sign-service`. |
| `sse/` | In-memory, room-based Actix SSE broker used for MPC round messages. |
| `proto/` | Tonic/prost code generation and the shared protobuf definition. |
| `key-gen/` | Trusted-dealer CLI for generating account-specific key-share files, optionally encrypted with age. |
| `config/` | Sample/deployment YAML; values are not all suitable for localhost without overrides. |
| `scripts/` | Docker, server, and Nitro Enclave build/run scripts. |
| `vendor/` | Vendored cryptographic/native dependencies; avoid editing unless the task explicitly targets them. |

## Toolchain and Commands

The pinned Rust toolchain is 1.88.0 with `rustfmt`, `clippy`, and Apple targets.
Run commands from the repository root.

```bash
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features
```

Prefer focused checks while iterating, then expand verification in proportion
to the change:

```bash
cargo test -p sse
cargo test -p sign-service
cargo check -p participant -p mpc-client
cargo check -p sign-gateway
```

Native GMP/MPFR builds can be slow, especially after a clean build or for a new
target. Do not delete `target/` merely to troubleshoot an unrelated failure.

For protobuf changes, edit `proto/proto/mpc.proto`; `proto/build.rs` generates
the Rust bindings during Cargo builds. Keep field numbers stable and make
wire-compatible additions unless a breaking change is explicitly requested.

For a local end-to-end run, start the upstream `sign-service` before
`sign-gateway`, because the gateway connects to it during startup. The checked-in
YAML mixes container/remote values (`/app/...`, `host.docker.internal`, and
public IPs), so use explicit local overrides or temporary local config instead
of silently rewriting deployment configuration.

## Implementation Invariants

- A signing input is already a 32-byte/prehashed scalar input. Do not hash it a
  second time inside `participant`.
- `account_id` selects an account-specific, pre-derived `KeyShare`; in-protocol
  HD derivation is not part of the current signing path.
- Both participants must receive the same `tx_id`, `execution_id`, chain, data,
  and `account_id`.
- The SSE room name is derived from `tx_id` as `signing_{tx_id}`. The
  `execution_id` is the CGGMP21 execution identifier; these values are related
  but not interchangeable.
- Party indexes come from the serialized key shares. The current CGGMP21 call
  explicitly uses parties `[0, 1]`; generic threshold/participant configuration
  does not make higher-party signing work by itself.
- `sign-gateway` has two separate responsibilities: HTTP/SSE message brokering
  and gRPC request proxying. Trace both paths when diagnosing a signing hang.
- `MpcSigner` exposes a synchronous UniFFI API around a Tokio runtime. Preserve
  its thread/runtime boundaries and graceful participant shutdown behavior.
- Subscribe to an SSE room before relying on round-message delivery. Preserve
  sender/receiver filtering for broadcast and point-to-point MPC messages.

## Change Guidelines

- Make the smallest coherent change and keep crate responsibilities intact.
- Reuse shared workspace dependencies where practical; do not introduce a new
  framework for a narrow fix.
- Keep transport/protocol types in `proto`, MPC mechanics in `participant`, SDK
  lifecycle/orchestration in `mpc-client`, and deployment wiring in the service
  crates or `scripts/`.
- Return contextual errors at I/O and service boundaries. Avoid new `unwrap`,
  `expect`, or process panics on request paths.
- Do not log private scalars, key-share JSON, decrypted material, credentials,
  KMS plaintext, or full secret-bearing configuration.
- Update `ARCHITECTURE.md` when changing component ownership, ports, protocols,
  trust boundaries, key lifecycle, or the signing sequence.
- If behavior intentionally diverges from an existing README, update that README
  in the same change or clearly report the remaining inconsistency.

## Security and Sensitive Files

Treat `*_key_shares.json`, `*.age`, age identities, KMS ciphertext/plaintext,
AWS credentials, and enclave artifacts as sensitive even when a sample is
already tracked.

- Never paste their contents into logs, tests, documentation, commits, or task
  summaries.
- Do not regenerate, decrypt, overwrite, or redistribute key material unless the
  user explicitly asks for that exact operation.
- Use synthetic fixtures in a temporary directory for tests that need key-share
  shaped data.
- Do not weaken KMS attestation/PCR policy or move enclave plaintext onto the
  parent host as a convenience fix.
- Default HTTP/gRPC configuration is plaintext and unauthenticated. Do not
  describe it as production-secure without an authenticated TLS boundary.

## Verification Expectations

At minimum, format-check every Rust change and run checks/tests for every crate
whose behavior changed. Protocol or signing changes should cover both
participants and the gateway paths. Deployment-script changes should receive a
shell syntax check and a review of host/container/enclave path and port
mappings. Documentation-only changes should still be checked for valid paths,
commands, Mermaid syntax, and consistency with the source.

When a full workspace check is impractical because of native builds, platform
targets, credentials, or external services, run the strongest focused check
available and state exactly what was and was not verified.
