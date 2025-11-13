use participant::{ParticipantServer, AppConfig};
use cggmp21::KeyShare;
use cggmp21::security_level::SecurityLevel128;
use cggmp21::supported_curves::Secp256k1;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use log::info;

/// Load key shares from files for demo/testing purposes
fn load_key_shares_from_files() -> Result<HashMap<String, KeyShare<Secp256k1, SecurityLevel128>>, Box<dyn std::error::Error>> {
    let mut key_shares = HashMap::new();
    
    // Scan all key_share_*.json files in the current directory
    for i in 1..=10 {  // Assume up to 10 key_share files are supported
        let filename = format!("key_share_{}.json", i);
        if Path::new(&filename).exists() {
            info!("   Loading key share from file: {}", filename);
            
            let key_share_json = fs::read_to_string(&filename)
                .map_err(|e| format!("Failed to read key share file {}: {}", filename, e))?;

            let key_share: KeyShare<Secp256k1, SecurityLevel128> = serde_json::from_str(&key_share_json)
                .map_err(|e| format!("Key share deserialization failed for {}: {}", filename, e))?;

            // Use participant_index as the default account_id
            let participant_index = key_share.core.i;
            let default_account_id = format!("account_{}", participant_index);
            
            key_shares.insert(default_account_id.clone(), key_share);
            
            info!("   ✓ Key share loaded successfully from {} with account_id: {}", filename, default_account_id);
        }
    }
    
    if key_shares.is_empty() {
        return Err("No key share files found. Please ensure the key share files are properly distributed.".into());
    }
    
    info!("✓ Loaded {} key shares in total", key_shares.len());
    Ok(key_shares)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();

    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    log::info!("Starting MPC participant service");

    // Load configuration from environment
    let config = AppConfig::from_env()
        .map_err(|e| format!("Failed to load configuration: {}", e))?;

    // Load key shares from files (for backward compatibility in this demo)
    let key_shares = load_key_shares_from_files()?;
    
    info!("Configuration loaded:");
    info!("  - SSE URL: {}", config.sse_url());
    info!("  - Participant Address: {}", config.participant_addr());
    info!("  - Available account_ids: {:?}", key_shares.keys().collect::<Vec<_>>());

    // Initialize and start the participant server using the new interface
    let server = ParticipantServer::new(
        &config.sse_url(),
        &config.participant.host,
        config.participant.port,
        key_shares,
    )?;
    
    server.start().await?;

    Ok(())
}
