# Sign Gateway

Sign Gateway is a standalone SSE (Server-Sent Events) service that facilitates communication between client participants and server participants in the MPC signing process.

## Overview

The Sign Gateway acts as a message broker using SSE technology, enabling real-time bidirectional communication for multi-party computation signing operations.

## Features

- **SSE Communication**: Provides Server-Sent Events endpoints for real-time message delivery
- **Room Management**: Manages isolated communication rooms for different signing sessions
- **Message Broadcasting**: Broadcasts messages to all participants in a room
- **CORS Support**: Configurable CORS origins for web client access

## Configuration

Configure the gateway using `config/sign-gateway.yaml`:

```yaml
server:
  host: "127.0.0.1"
  port: 8080
  cors_origins: ["http://localhost:3000"]

logging:
  level: "info"
  format: "json"
```

## Running

```bash
# Build
cargo build --release --bin sign-gateway

# Run with default config
./target/release/sign-gateway

# Run with custom config
./target/release/sign-gateway config/sign-gateway.yaml
```

## API Endpoints

- `GET /rooms/{room_id}/subscribe` - Subscribe to room events
- `POST /rooms/{room_id}/issue_unique_idx` - Get unique participant index
- `POST /rooms/{room_id}/broadcast` - Broadcast message to room

## Architecture

The Sign Gateway works with:
- **Client Participants**: Connect via SSE to participate in signing
- **Server Participants**: Connect via SSE to coordinate the signing process

All parties communicate through the gateway, which maintains message ordering and delivery guarantees.

