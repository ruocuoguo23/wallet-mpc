# SSE Server Library

A Server-Sent Events (SSE) library built with Actix-web that provides real-time messaging capabilities.

## Features

- Room-based message broadcasting
- Subscriber management with automatic cleanup
- Unique index generation per room
- Graceful shutdown support
- Configurable via environment variables

## Usage

### Basic Setup

```rust
use sse::SseServer;

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    // Create SSE Server with default config (from environment variables)
    let server = SseServer::with_default_config()?;
    
    // Start the server
    server.start().await
}
```

### With Graceful Shutdown

```rust
use sse::SseServer;
use tokio::signal;

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    // Create SSE Server
    let server = SseServer::with_default_config()?;
    
    // Start server in a separate task
    let server_task = tokio::spawn({
        let server_clone = server.clone();
        async move {
            server_clone.start().await
        }
    });

    // Wait for shutdown signal (Ctrl+C)
    match signal::ctrl_c().await {
        Ok(()) => {
            log::info!("Received shutdown signal");
            // Gracefully shutdown the server
            server.shutdown().await?;
        }
        Err(err) => {
            log::error!("Unable to listen for shutdown signal: {}", err);
        }
    }

    // Wait for server task to complete
    server_task.await??;
    
    Ok(())
}
```

### Custom Shutdown Trigger

```rust
use sse::SseServer;
use tokio::sync::oneshot;

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    let server = SseServer::with_default_config()?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    
    // Start server in background
    let server_task = tokio::spawn({
        let server_clone = server.clone();
        async move {
            server_clone.start().await
        }
    });

    // Wait for custom shutdown signal
    shutdown_rx.await.ok();
    
    // Gracefully shutdown
    server.shutdown().await?;
    server_task.await??;
    
    Ok(())
}
```

## Configuration

Configure the server using environment variables:

- `SSE_HOST`: Server host address (default: "127.0.0.1")
- `SSE_PORT`: Server port (default: 8080)

Example `.env` file:

```env
SSE_HOST=0.0.0.0
SSE_PORT=8080
```

## API Endpoints

### Subscribe to a Room

```
GET /rooms/{room_id}/subscribe
```

Opens an SSE connection to receive messages from the specified room.

**Headers:**
- `Last-Event-ID` (optional): Resume from a specific event ID

### Issue Unique Index

```
POST /rooms/{room_id}/issue_unique_idx
```

Returns a unique index for the room.

**Response:**
```json
{
  "unique_idx": 0
}
```

### Broadcast Message

```
POST /rooms/{room_id}/broadcast
```

Broadcast a message to all subscribers in the room.

**Body:** Raw message content (string)

## License

See project root for license information.

