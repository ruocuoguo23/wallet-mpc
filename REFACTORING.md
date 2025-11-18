# Architecture Refactoring Summary

## Overview

The sign-service has been successfully refactored to separate concerns:
- **sign-service**: Now only runs the Participant Server (server-side MPC participant)
- **sign-gateway**: New standalone service that runs the SSE server for message coordination

## Changes Made

### 1. New Project: sign-gateway

Created a new standalone service in `/sign-gateway/`:

```
sign-gateway/
├── Cargo.toml
├── README.md
└── src/
    ├── main.rs
    └── config.rs
```

**Purpose**: Handles SSE communication and message routing between all participants (both client and server participants).

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

**Endpoints**:
- `GET /rooms/{room_id}/subscribe` - Subscribe to room events
- `POST /rooms/{room_id}/issue_unique_idx` - Get unique participant index
- `POST /rooms/{room_id}/broadcast` - Broadcast message to room

### 2. Refactored: sign-service

**Changes**:
- Removed SSE server initialization and management
- Removed `sse` dependency from Cargo.toml
- Updated config structure to use gateway URL instead of SSE config
- Simplified service startup to only run Participant Server

**Configuration** (`config/sign-service.yaml`):
```yaml
gateway:
  url: "http://127.0.0.1:8080"

server:
  host: "127.0.0.1"
  port: 50051
  index: 0

logging:
  level: "info"
  format: "json"

mpc:
  threshold: 2
  total_participants: 3
  key_share_file: "service_key_shares.json"
```

**Key Changes**:
- `config.rs`: Removed SSE-related structures, added `GatewayConfig`
- `service.rs`: Removed SSE server logic, kept only Participant Server
- `main.rs`: Updated logging messages to reflect new architecture

### 3. Updated: client configuration

**Configuration** (`config/client.yaml`):
```yaml
gateway:
  url: "http://127.0.0.1:8080"

local_participant:
  host: "127.0.0.1"
  port: 50052
  index: 1
  key_share_file: "client_key_shares.json"

remote_services:
  sign_service:
    participant_host: "127.0.0.1"
    participant_port: 50051
    index: 0
```

**Changes**:
- Added `gateway` section with URL to sign-gateway
- Removed `sse_host` and `sse_port` from `remote_services.sign_service`

### 4. Updated: Root workspace

**Cargo.toml**:
- Added `sign-gateway` to workspace members

## Architecture Diagram

```
┌─────────────────┐
│  Sign Gateway   │  Port 8080 (SSE Server)
│   (Standalone)  │  - Message routing
└────────┬────────┘  - Room management
         │
    ┌────┴────────────────┐
    │                     │
┌───▼──────────┐  ┌──────▼──────┐
│    Client    │  │Sign Service │
│ Participant  │  │ Participant │
│              │  │   Server    │
│ Port 50052   │  │ Port 50051  │
│ (Local)      │  │ (Remote)    │
└──────────────┘  └─────────────┘
```

## Running the Services

### 1. Start Sign Gateway (required first)
```bash
cargo run --bin sign-gateway
# or with custom config
cargo run --bin sign-gateway -- config/sign-gateway.yaml
```

### 2. Start Sign Service(s)
```bash
cargo run --bin sign-service
# or with custom config
cargo run --bin sign-service -- config/sign-service.yaml
```

### 3. Run Client
```bash
cargo run --bin client
```

## Benefits of This Architecture

1. **Separation of Concerns**: Gateway handles communication, services handle MPC logic
2. **Scalability**: Can run multiple sign-service instances connecting to same gateway
3. **Flexibility**: Gateway can be deployed separately and scaled independently
4. **Maintainability**: Clearer responsibilities for each service
5. **Testing**: Easier to test each component in isolation

## Migration Notes

- All participants (client and server) must connect to the same sign-gateway
- The gateway must be started before any participants
- Configuration files have been updated to reflect new structure
- No changes required to the MPC signing logic itself

