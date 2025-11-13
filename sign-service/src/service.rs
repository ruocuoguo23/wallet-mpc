use std::fs;
use std::collections::HashMap;

use anyhow::{Context, Result};
use log::{info, error};
use tokio::signal;

use sse::SseServer;
use participant::ParticipantServer;
use cggmp21::KeyShare;
use cggmp21::security_level::SecurityLevel128;
use cggmp21::supported_curves::Secp256k1;

use crate::config::SignServiceConfig;

/// Load key shares from configured file path
pub fn load_key_shares(key_share_file: &str) -> Result<HashMap<String, KeyShare<Secp256k1, SecurityLevel128>>> {
    info!("Loading key shares from file: {}", key_share_file);
    let key_share_json = fs::read_to_string(key_share_file)
        .with_context(|| format!("Failed to read key share file {}", key_share_file))?;

    let key_shares: HashMap<String, KeyShare<Secp256k1, SecurityLevel128>> = serde_json::from_str(&key_share_json)
        .with_context(|| format!("Key shares deserialization failed for {}", key_share_file))?;

    info!("✓ Key shares loaded successfully. Account IDs: {:?}", key_shares.keys().collect::<Vec<_>>());
    
    Ok(key_shares)
}

pub async fn run_services(config: SignServiceConfig) -> Result<()> {
    info!("Initializing services...");
    
    // Load key shares from the configuration file
    let key_shares = load_key_shares(&config.mpc.key_share_file)
        .context("Failed to load key shares")?;
    
    // Log the loaded key shares information
    let key_share_count = key_shares.len();
    let account_ids: Vec<String> = key_shares.keys().cloned().collect();
    
    // Create SSE server
    let sse_config = config.to_sse_config();
    let sse_server = SseServer::new(sse_config);
    info!("SSE Server created - {}:{}", config.server.sse.host, config.server.sse.port);

    // Create Participant server using the new interface
    let sse_url = format!("http://{}:{}", config.server.sse.host, config.server.sse.port);
    let participant_server = ParticipantServer::new(
        &sse_url,
        &config.server.participant.host,
        config.server.participant.port,
        key_shares,
    ).map_err(|e| anyhow::anyhow!("Failed to create participant server: {}", e))?;
    
    info!("Participant Server created - {}:{}", 
          config.server.participant.host, 
          config.server.participant.port);
    info!("Using SSE server: {}", sse_url);
    info!("Loaded {} key shares for account IDs: {:?}", 
          key_share_count,
          account_ids);

    info!("Starting both servers concurrently...");

    // Clone servers for the tasks
    let sse_server_clone = sse_server.clone();
    let participant_server_clone = participant_server.clone();

    // Start SSE server in a separate task
    let sse_task = tokio::spawn(async move {
        sse_server_clone.start().await
            .context("SSE server failed")
    });

    // Start Participant server in a separate task
    let participant_task = tokio::spawn(async move {
        participant_server_clone.start().await
            .map_err(|e| anyhow::anyhow!("Participant server failed: {}", e))
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

    // Gracefully shutdown both servers
    info!("Shutting down SSE server...");
    if let Err(e) = sse_server.shutdown().await {
        error!("Error shutting down SSE server: {}", e);
    }

    info!("Shutting down Participant server...");
    if let Err(e) = participant_server.shutdown().await {
        error!("Error shutting down Participant server: {}", e);
    }

    // Wait for both tasks to complete
    info!("Waiting for server tasks to complete...");
    
    let (sse_result, participant_result) = tokio::join!(sse_task, participant_task);

    match sse_result {
        Ok(Ok(())) => info!("SSE server stopped successfully"),
        Ok(Err(e)) => error!("SSE server error: {}", e),
        Err(e) => error!("SSE server task panicked: {}", e),
    }

    match participant_result {
        Ok(Ok(())) => info!("Participant server stopped successfully"),
        Ok(Err(e)) => error!("Participant server error: {}", e),
        Err(e) => error!("Participant server task panicked: {}", e),
    }

    info!("Both servers have been shut down");
    Ok(())
}
