# Wallet MPC
A Multi-Party Computation (MPC) wallet solution based on the CGGMP21 threshold signature scheme, designed for secure, decentralized cryptocurrency signing operations.
## Overview
Wallet MPC implements a 3-of-2 threshold signature scheme, where any 2 out of 3 participants can collaboratively generate valid signatures without any single party having access to the complete private key. This architecture provides enhanced security for cryptocurrency wallets by eliminating single points of failure.
### Key Features
- **Threshold Signatures**: 3-of-2 MPC signing scheme using CGGMP21 protocol
- **No Single Point of Failure**: Private keys are split across multiple parties
- **Cross-Platform Client**: UniFFI-based library supporting iOS, Android, and more
- **Real-time Communication**: SSE (Server-Sent Events) based message coordination
- **Secp256k1 Support**: Compatible with Bitcoin, Ethereum, and other major blockchains
- **HD Wallet Support**: Hierarchical Deterministic wallet implementation (SLIP-10)
## Architecture
```
┌───────────────���─────────────────────────────────────────────┐
│                     Sign Gateway                             │
│                  (SSE Message Broker)                        │
│                    Port: 8080                                │
└────────────────┬────────────────────────────────────────────┘
                 │
    ┌────────────┴────────────────┐
    │                             │
┌───▼─────────────┐    ┌──────────▼──────────┐
│  MPC Client     │    │  Sign Service        │
│  (Mobile/Web)   │    │  (Server Participant)│
│                 │    │                      │
│ • Local         │    │ • Remote Participant │
│   Participant   │    │ • gRPC Server        │
│ • UniFFI Lib    │    │ • Key Share Storage  │
│ • Port: 50052   │    │ • Port: 50051        │
└─────────────────┘    └─────────────────────┘
         Any 2 participants can sign ✓
```
## Core Components
### 1. mpc-client
**Location**: `mpc-client/`
A cross-platform cryptographic library built with Rust and exposed via UniFFI. This component encapsulates the local participant functionality for MPC signing operations.
**Features**:
- UniFFI-based cross-platform bindings (iOS, Android, Python, Kotlin, Swift)
- Local participant server management
- Signing API for transactions
- Key share management
- Automatic participant coordination
**Key APIs**:
```rust
// Initialize MPC client with configuration
async fn new_mpc_client(config: MpcConfig) -> Result<MpcSigner>
// Sign a transaction hash
async fn sign_tx(tx_hash: String, account_id: String) -> Result<SignatureResult>
// Get public key for an account
fn get_public_key(account_id: String) -> Result<String>
```
**Platforms Supported**:
- iOS (via Swift bindings)
- Android (via Kotlin bindings)
- Python
- Any platform supporting Rust
**Build**:
```bash
cargo build -p mpc-client
# Generate UniFFI bindings
cargo run --bin uniffi-bindgen generate src/mpc_client.udl --language swift
```
### 2. sign-service
**Location**: `sign-service/`
A server-side MPC participant that runs as a standalone service. It maintains key shares and participates in the MPC signing protocol.
**Features**:
- Server-side participant implementation
- Connects to sign-gateway for message coordination
- Manages multiple account key shares
- gRPC API for signing requests
- Automatic reconnection and error recovery
**Configuration** (`config/sign-service.yaml`):
```yaml
gateway:
  url: "http://127.0.0.1:8080"
server:
  host: "127.0.0.1"
  port: 50051
  index: 0  # Participant index (0, 1, or 2)
mpc:
  threshold: 2
  total_participants: 3
  key_share_file: "service_key_shares.json"
```
**Run**:
```bash
cargo run --bin sign-service
# or with custom config
cargo run --bin sign-service -- config/sign-service.yaml
```
### 3. sign-gateway
**Location**: `sign-gateway/`
A standalone SSE (Server-Sent Events) server that acts as a message broker for all MPC participants. It facilitates real-time communication and message routing during the signing protocol.
**Features**:
- SSE-based real-time messaging
- Room-based message isolation
- CORS support for web clients
- Message broadcasting to all participants
- Participant index management
**Configuration** (`config/sign-gateway.yaml`):
```yaml
server:
  host: "127.0.0.1"
  port: 8080
  cors_origins: ["http://localhost:3000"]
logging:
  level: "info"
  format: "json"
```
**API Endpoints**:
- `GET /rooms/{room_id}/subscribe` - Subscribe to room events (SSE)
- `POST /rooms/{room_id}/issue_unique_idx` - Get unique participant index
- `POST /rooms/{room_id}/broadcast` - Broadcast message to all participants
**Run**:
```bash
cargo run --bin sign-gateway
```
### 4. client
**Location**: `client/`
A command-line test client for testing the MPC signing workflow. Demonstrates how to use the participant library.
**Run**:
```bash
cargo run --bin client
```
### 5. key-gen
**Location**: `key-gen/`
Key generation utility for creating distributed key shares among participants.
**Features**:
- Generate 3-of-2 threshold key shares
- Support for multiple accounts
- Secp256k1 curve
- HD wallet key derivation
**Run**:
```bash
cargo run --bin key-gen
```
### 6. participant
**Location**: `participant/`
Core library implementing the MPC participant logic, including the signing protocol and message handling.
**Features**:
- CGGMP21 protocol implementation
- gRPC server for signing requests
- SSE client for message coordination
- Key share management
### 7. proto
**Location**: `proto/`
Protocol Buffers definitions for gRPC communication between participants.
### 8. sse
**Location**: `sse/`
SSE server library providing the real-time messaging infrastructure used by sign-gateway.
## MPC Protocol: CGGMP21
This project uses the **CGGMP21** (Canetti-Gennaro-Goldfeder-Makriyannis-Peled 2021) threshold signature scheme, which is:
- **UC-secure**: Universally Composable security guarantees
- **Non-interactive**: No need for all parties to be online during key generation
- **Identifiable abort**: Can detect which party caused a protocol failure
- **Efficient**: Optimized for practical use
**Academic Reference**: [UC Non-Interactive, Proactive, Threshold ECDSA with Identifiable Aborts](https://eprint.iacr.org/2021/060)
## Setup and Installation
### Prerequisites
- Rust 1.70+ (specified in `rust-toolchain.toml`)
- GMP and MPFR libraries (included in `vendor/`)
- Protocol Buffers compiler (for gRPC)
### Build All Components
```bash
# Build everything
cargo build --release
# Build specific components
cargo build --release --bin sign-gateway
cargo build --release --bin sign-service
cargo build --release --bin client
cargo build --release --bin key-gen
```
### Quick Start
1. **Generate Key Shares**:
```bash
cargo run --bin key-gen
# Generates: client_key_shares.json, service_key_shares.json
```
2. **Start Sign Gateway**:
```bash
cargo run --bin sign-gateway
# Listening on http://127.0.0.1:8080
```
3. **Start Sign Service** (in another terminal):
```bash
cargo run --bin sign-service
# gRPC server on 127.0.0.1:50051
# Connected to gateway at http://127.0.0.1:8080
```
4. **Run Client** (in another terminal):
```bash
cargo run --bin client
# Interactive signing session
```
## Configuration
### Directory Structure
```
wallet-mpc/
├── config/
│   ├── sign-gateway.yaml    # Gateway configuration
│   ├── sign-service.yaml    # Server participant config
│   └── client.yaml          # Client participant config
├── client_key_shares.json   # Client key shares
├── service_key_shares.json  # Server key shares
└── ...
```
### Key Share Format
```json
{
  "account_id_1": {
    "key_share": "...",
    "public_key": "...",
    // ... other fields
  },
  "account_id_2": {
    // ...
  }
}
```
## Security Considerations
### Key Share Distribution
- Each participant holds ONE key share
- Minimum 2 participants needed to sign (threshold)
- Total 3 key shares generated
- No single participant can sign alone
- No single point of compromise
### Best Practices
1. **Key Storage**: Store key shares in secure, encrypted storage
2. **Transport Security**: Use TLS for all network communication in production
3. **Access Control**: Implement proper authentication for sign-service
4. **Monitoring**: Log all signing operations for audit
5. **Backup**: Securely backup key shares in separate locations
6. **Network Isolation**: Run sign-gateway in a trusted network environment
## Development
### Project Structure
```
wallet-mpc/
├── mpc-client/       # UniFFI-based cross-platform library
├── sign-service/     # Server-side MPC participant
├── sign-gateway/     # SSE message broker
├── client/           # CLI test client
├── participant/      # Core MPC participant logic
├── key-gen/          # Key generation utility
├── proto/            # Protocol Buffers definitions
├── sse/              # SSE server library
└── vendor/           # Vendored dependencies (GMP/MPFR)
```
### Running Tests
```bash
# Run all tests
cargo test
# Run tests for specific component
cargo test -p mpc-client
cargo test -p sign-service
cargo test -p participant
```
### Code Style
This project follows standard Rust conventions:
```bash
# Format code
cargo fmt
# Lint code
cargo clippy
```
## Deployment
### Production Checklist
- [ ] Enable TLS for sign-gateway
- [ ] Implement authentication for sign-service
- [ ] Set up secure key share storage (HSM recommended)
- [ ] Configure proper CORS origins
- [ ] Set up monitoring and alerting
- [ ] Implement rate limiting
- [ ] Regular security audits
- [ ] Backup key shares securely
- [ ] Document disaster recovery procedures
### Docker Deployment (Optional)
```bash
# Build sign-gateway
docker build -t sign-gateway -f Dockerfile.gateway .
# Build sign-service
docker build -t sign-service -f Dockerfile.service .
# Run with docker-compose
docker-compose up -d
```
## Troubleshooting
### Common Issues
**1. "Connection refused" errors**
- Ensure sign-gateway is running first
- Check firewall settings
- Verify ports are not in use
**2. "Key share not found"**
- Run key-gen to generate key shares
- Check file paths in configuration
- Verify JSON format is correct
**3. "Threshold not met"**
- Ensure at least 2 participants are online
- Check participant indices are unique
- Verify all participants connect to same gateway
**4. Build errors with GMP/MPFR**
- Check vendor/gmp-mpfr-sys directory exists
- Ensure Rust toolchain matches rust-toolchain.toml
- Try: `cargo clean && cargo build`
## Use Cases
### Mobile Wallet Integration
Integrate mpc-client into your mobile wallet app:
**iOS (Swift)**:
```swift
import mpc_client
let config = MpcConfig(...)
let signer = try await newMpcClient(config: config)
let signature = try await signer.signTx(txHash: "0x...", accountId: "account1")
```
**Android (Kotlin)**:
```kotlin
import mpc_client.*
val config = MpcConfig(...)
val signer = newMpcClient(config)
val signature = signer.signTx("0x...", "account1")
```
### Exchange Hot Wallet
Deploy sign-service instances across multiple secure servers for exchange hot wallet security.
### Hardware Wallet Supplement
Use as an additional signing party alongside hardware wallets for enhanced security.
## Roadmap
- [ ] Support for more curves (Ed25519, BLS)
- [ ] Dynamic threshold adjustment
- [ ] Key rotation support
- [ ] Enhanced monitoring and metrics
- [ ] Web-based admin interface
- [ ] Docker images and Kubernetes manifests
- [ ] Formal security audit
## Contributing
Contributions are welcome! Please:
1. Fork the repository
2. Create a feature branch
3. Make your changes with tests
4. Run `cargo fmt` and `cargo clippy`
5. Submit a pull request
## License
[Add your license here]
## References
- [CGGMP21 Paper](https://eprint.iacr.org/2021/060)
- [cggmp21 Rust Implementation](https://github.com/ZenGo-X/multi-party-ecdsa)
- [UniFFI Documentation](https://mozilla.github.io/uniffi-rs/)
- [Threshold Signatures Overview](https://en.wikipedia.org/wiki/Threshold_cryptosystem)
## Support
For questions and support:
- Contact: [jeff.wu@cmexpro.com]
---
**⚠️ Security Notice**: This software handles cryptographic key material. Always conduct thorough security reviews and audits before using in production environments.
