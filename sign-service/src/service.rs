use std::fs;
use std::collections::HashMap;

use anyhow::{Context, Result};
use log::{info, error};
use tokio::signal;

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
    info!("Initializing Participant Server...");

    // Load key shares from the configuration file
    let key_shares = load_key_shares(&config.mpc.key_share_file)
        .context("Failed to load key shares")?;
    
    // Log the loaded key shares information
    let key_share_count = key_shares.len();
    let account_ids: Vec<String> = key_shares.keys().cloned().collect();
    
    // Create Participant server
    let participant_server = ParticipantServer::new(
        &config.gateway.url,
        &config.server.host,
        config.server.port,
        key_shares,
    ).map_err(|e| anyhow::anyhow!("Failed to create participant server: {}", e))?;
    
    info!("Participant Server created - {}:{}", 
          config.server.host,
          config.server.port);
    info!("Connected to gateway: {}", config.gateway.url);
    info!("Participant index: {}", config.server.index);
    info!("Loaded {} key shares for account IDs: {:?}",
          key_share_count,
          account_ids);

    info!("Starting Participant Server...");

    // Clone server for the task
    let participant_server_clone = participant_server.clone();

    // Start Participant server in a separate task
    let _ = tokio::spawn(async move {
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

    // Gracefully shutdown the server
    info!("Shutting down Participant server...");
    if let Err(e) = participant_server.shutdown().await {
        error!("Error shutting down Participant server: {}", e);
    }

    info!("Sign Service has been shut down");
    Ok(())
}
