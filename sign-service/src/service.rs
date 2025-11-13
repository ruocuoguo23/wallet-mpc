use std::fs;
use std::collections::HashMap;

use anyhow::{Context, Result};
use log::{info, error};
use tokio::try_join;

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
    
    // 从配置文件中加载key shares
    let key_shares = load_key_shares(&config.mpc.key_share_file)
        .context("Failed to load key shares")?;
    
    // 记录加载的key shares信息
    let key_share_count = key_shares.len();
    let account_ids: Vec<String> = key_shares.keys().cloned().collect();
    
    // 创建SSE server
    let sse_config = config.to_sse_config();
    let sse_server = SseServer::new(sse_config);
    info!("SSE Server created - {}:{}", config.server.sse.host, config.server.sse.port);

    // 创建Participant server 使用新的接口
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

    // 并发运行两个服务器
    let result: Result<((), ()), anyhow::Error> = try_join!(
        async {
            sse_server.start().await
                .context("SSE server failed")
        },
        async {
            participant_server.start().await
                .map_err(|e| anyhow::anyhow!("Participant server failed: {}", e))
        }
    );

    match result {
        Ok(_) => {
            info!("Both servers have stopped successfully");
            Ok(())
        }
        Err(e) => {
            error!("One or both servers failed: {}", e);
            Err(e)
        }
    }
}
