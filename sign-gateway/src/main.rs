mod config;

use anyhow::{Context, Result};
use log::info;
use tokio::signal;

use sse::SseServer;
use crate::config::{SignGatewayConfig, setup_logging};

#[tokio::main]
async fn main() -> Result<()> {
    // Get the configuration file path, default to config/sign-gateway.yaml
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config/sign-gateway.yaml".to_string());

    // Load the configuration file
    let config = SignGatewayConfig::load_from_file(&config_path)
        .context("Failed to load configuration")?;

    // Set up logging
    setup_logging(&config.logging)
        .context("Failed to setup logging")?;

    info!("Sign Gateway starting up...");
    info!("Configuration loaded from: {}", config_path);
    info!("Server will start on: {}:{}", config.server.host, config.server.port);

    // Create SSE server
    let sse_config = config.to_sse_config();
    let sse_server = SseServer::new(sse_config);
    info!("SSE Server created - {}:{}", config.server.host, config.server.port);

    // Start SSE server in a separate task
    let sse_server_clone = sse_server.clone();
    let server_task = tokio::spawn(async move {
        sse_server_clone.start().await
            .context("SSE server failed")
    });

    // Wait for shutdown signal (Ctrl+C or SIGTERM)
    tokio::select! {
        _ = signal::ctrl_c() => {
            info!("Received Ctrl+C signal, initiating graceful shutdown...");
        }
        _ = async {
            #[cfg(unix)]
            {
                let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
                    .expect("Failed to setup SIGTERM handler");
                sigterm.recv().await
            }
            #[cfg(not(unix))]
            {
                std::future::pending::<()>().await
            }
        } => {
            info!("Received SIGTERM signal, initiating graceful shutdown...");
        }
    }

    // Gracefully shutdown the server
    info!("Shutting down SSE server...");
    if let Err(e) = sse_server.shutdown().await {
        log::error!("Error shutting down SSE server: {}", e);
    }

    // Wait for server task to complete
    info!("Waiting for server task to complete...");
    match server_task.await {
        Ok(Ok(())) => info!("SSE server stopped successfully"),
        Ok(Err(e)) => log::error!("SSE server error: {}", e),
        Err(e) => log::error!("SSE server task panicked: {}", e),
    }

    info!("Sign Gateway has been shut down");
    Ok(())
}

