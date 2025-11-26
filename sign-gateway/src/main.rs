mod config;
mod grpc;

use anyhow::{Context, Result};
use log::info;
use tokio::signal;
use tokio::task::JoinHandle;

use sse::SseServer;
use crate::config::{SignGatewayConfig, setup_logging};
use crate::grpc::SignGatewayGrpc;

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

    let grpc_service = SignGatewayGrpc::new(&config.sign_service.url)
        .await
        .context("Failed to initialize gRPC gateway")?;

    let grpc_addr = config.grpc_addr();

    // Shared shutdown trigger
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel::<bool>(false);

    // Start SSE server in a separate task
    let sse_server_clone = sse_server.clone();
    let mut shutdown_rx_sse = shutdown_rx.clone();
    let server_task: JoinHandle<Result<(), anyhow::Error>> = tokio::spawn(async move {
        tokio::select! {
            res = sse_server_clone.start() => res.context("SSE server failed"),
            _ = shutdown_rx_sse.changed() => Ok(()),
        }
    });

    let mut shutdown_rx_grpc = shutdown_rx.clone();
    let grpc_task: JoinHandle<Result<(), anyhow::Error>> = tokio::spawn(async move {
        let shutdown = async move {
            let _ = shutdown_rx_grpc.changed().await;
        };
        grpc_service
            .serve(&grpc_addr, shutdown)
            .await
            .context("gRPC server failed")
    });

    let mut server_task = Some(server_task);
    let mut grpc_task = Some(grpc_task);

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
        _ = async {
            if let Some(task) = &mut server_task {
                let _ = task.await;
            }
        } => {
            info!("SSE server exited");
            server_task = None;
        }
        _ = async {
            if let Some(task) = &mut grpc_task {
                let _ = task.await;
            }
        } => {
            info!("gRPC server exited");
            grpc_task = None;
        }
    }

    // Notify background tasks to stop
    let _ = shutdown_tx.send(true);

    // Gracefully shutdown the server
    info!("Shutting down SSE server...");
    if let Err(e) = sse_server.shutdown().await {
        log::error!("Error shutting down SSE server: {}", e);
    }

    // Wait for server task to complete
    info!("Waiting for server task to complete...");
    if let Some(task) = server_task {
        match task.await {
            Ok(Ok(())) => info!("SSE server stopped successfully"),
            Ok(Err(e)) => log::error!("SSE server error: {}", e),
            Err(e) => log::error!("SSE server task panicked: {}", e),
        }
    }

    if let Some(task) = grpc_task {
        match task.await {
            Ok(Ok(())) => info!("gRPC server stopped successfully"),
            Ok(Err(e)) => log::error!("gRPC server error: {}", e),
            Err(e) => log::error!("gRPC server task panicked: {}", e),
        }
    }

    info!("Sign Gateway has been shut down");
    Ok(())
}
