# Sign Service

Sign Service is a server-side MPC participant that handles multi-party computation signing operations. It connects to the Sign Gateway for message coordination and provides gRPC endpoints for signing requests.

## Features

- **Participant Server**: Handles MPC signing operations as a server-side participant
- **YAML Configuration**: Easy configuration management
- **Key Share Management**: Automatically loads key shares based on participant index
- **Comprehensive Logging**: Supports multiple log levels and formats
- **Gateway Integration**: Connects to Sign Gateway for message coordination

## Configuration

Configuration file located at `config/sign-service.yaml`:

```yaml
# Gateway configuration
gateway:
  url: "http://127.0.0.1:8080"

# Server configuration
server:
  host: "127.0.0.1"
  port: 50051
  index: 0  # Which participant this service represents (0, 1, or 2)

# Logging configuration
logging:
  level: "info"
  format: "json"  # or "text"

# MPC configuration
mpc:
  threshold: 2
  total_participants: 3
  key_share_file: "service_key_shares.json"
```

## Usage

### Build

```bash
cargo build --release --bin sign-service
```

### Run

With default config:
```bash
./target/release/sign-service
```

With custom config:
```bash
./target/release/sign-service config/sign-service.yaml
```

### Test

```bash
cargo test -p sign-service
```

## Service Endpoints

### Participant Server (default port 50051)
- gRPC service providing `sign_tx` method for transaction signing

## Key Share Files

The service loads key shares based on the participant index from the configured file path.

## Logging

Supported log levels:
- `error` - Error messages
- `warn` - Warning messages
- `info` - General information (default)
- `debug` - Debug information
- `trace` - Detailed trace information

## Architecture

Sign Service works as part of the MPC signing infrastructure:

1. **Gateway Integration**: Connects to Sign Gateway for message coordination
2. **Participant Server**: Processes MPC signing requests via gRPC
3. **Key Management**: Securely manages cryptographic key shares
4. **Configuration**: Centralized configuration management

## Related Components

- **Sign Gateway**: Handles SSE communication and message routing between participants
- **Client**: Client-side participant for initiating signing requests

这为后续与客户端的集成和以太坊交易的多方签名奠定了坚实的基础。
