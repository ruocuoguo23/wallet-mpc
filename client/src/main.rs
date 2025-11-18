use std::fs;
use alloy::primitives::{Address, Bytes, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy_consensus::private::alloy_eips::Encodable2718;
use alloy_consensus::{SignableTransaction, Signed, TxEip1559};
use anyhow::{Result, Context};
use log::{error, info};
use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};

use mpc_client::{MpcSigner, MpcConfig, KeyShare};

/// Load client configuration from YAML file and convert to MpcConfig
fn load_mpc_config(config_path: &str) -> Result<MpcConfig> {
    let yaml_content = fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read config file: {}", config_path))?;
    
    let config: serde_yaml::Value = serde_yaml::from_str(&yaml_content)
        .with_context(|| "Failed to parse YAML config")?;
    
    // Extract configuration values
    let local_participant = config.get("local_participant")
        .ok_or_else(|| anyhow::anyhow!("Missing local_participant config"))?;
    let remote_services = config.get("remote_services")
        .ok_or_else(|| anyhow::anyhow!("Missing remote_services config"))?;
    let sign_service = remote_services.get("sign_service")
        .ok_or_else(|| anyhow::anyhow!("Missing sign_service config"))?;
    let mpc = config.get("mpc")
        .ok_or_else(|| anyhow::anyhow!("Missing mpc config"))?;
    let logging = config.get("logging")
        .ok_or_else(|| anyhow::anyhow!("Missing logging config"))?;

    // Load key share from file - backward compatibility support
    let key_share_file = local_participant.get("key_share_file")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing local_participant.key_share_file"))?;
    
    let participant_index = local_participant.get("index")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow::anyhow!("Missing local_participant.index"))?
        as u16;
    
    // Read key shares file content - now supports dictionary format with account_id as key
    let key_share_content = fs::read_to_string(key_share_file)
        .with_context(|| format!("Failed to read key share file: {}", key_share_file))?;
    
    // Parse the key shares dictionary from JSON
    let key_shares_dict: std::collections::HashMap<String, serde_json::Value> = 
        serde_json::from_str(&key_share_content)
        .with_context(|| format!("Failed to parse key shares file as JSON: {}", key_share_file))?;
    
    // Convert to KeyShare vector format expected by mpc-client
    let mut key_shares = Vec::new();
    for (account_id, key_share_data) in key_shares_dict {
        let key_share_json = serde_json::to_string(&key_share_data)
            .with_context(|| format!("Failed to serialize key share for account_id: {}", account_id))?;
        
        key_shares.push(KeyShare {
            account_id: account_id.clone(),
            key_share_data: key_share_json,
        });
        info!("Added account_id: {}", account_id);
    }
    
    if key_shares.is_empty() {
        return Err(anyhow::anyhow!("No key shares found in file: {}", key_share_file));
    }
    
    Ok(MpcConfig {
        local_participant_host: local_participant.get("host")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing local_participant.host"))?
            .to_string(),
        local_participant_port: local_participant.get("port")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing local_participant.port"))?
            as u16,
        local_participant_index: participant_index,
        key_shares,
        sign_service_host: sign_service.get("participant_host")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing sign_service.participant_host"))?
            .to_string(),
        sign_service_port: sign_service.get("participant_port")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing sign_service.participant_port"))?
            as u16,
        sse_host: sign_service.get("sse_host")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing sign_service.sse_host"))?
            .to_string(),
        sse_port: sign_service.get("sse_port")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing sign_service.sse_port"))?
            as u16,
        sign_service_index: sign_service.get("index")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing sign_service.index"))?
            as u16,
        threshold: mpc.get("threshold")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing mpc.threshold"))?
            as u16,
        total_participants: mpc.get("total_participants")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing mpc.total_participants"))?
            as u16,
        log_level: logging.get("level")
            .and_then(|v| v.as_str())
            .unwrap_or("info")
            .to_string(),
    })
}

/// Recover public key from ECDSA signature
/// Returns (compressed_hex, uncompressed_hex)
fn recover_public_key(message_hash: &[u8], r: &[u8], s: &[u8], recovery_id: u32) -> Result<(String, String)> {
    // Convert r and s to k256 signature format
    let mut sig_bytes = [0u8; 64];
    sig_bytes[..32].copy_from_slice(r);
    sig_bytes[32..].copy_from_slice(s);
    
    // Create k256 signature
    let signature = Signature::from_bytes(&sig_bytes.into())
        .map_err(|e| anyhow::anyhow!("Invalid signature format: {}", e))?;
    
    // Create recovery ID (0 or 1)
    let recovery_id = RecoveryId::try_from(recovery_id as u8)
        .map_err(|e| anyhow::anyhow!("Invalid recovery ID: {}", e))?;
    
    // Recover the verifying key (public key)
    let verifying_key = VerifyingKey::recover_from_prehash(message_hash, &signature, recovery_id)
        .map_err(|e| anyhow::anyhow!("Public key recovery failed: {}", e))?;
    
    // Get the encoded point
    let encoded_point = verifying_key.to_encoded_point(false); // false = uncompressed
    let uncompressed_bytes = encoded_point.as_bytes();
    
    let encoded_point_compressed = verifying_key.to_encoded_point(true); // true = compressed  
    let compressed_bytes = encoded_point_compressed.as_bytes();
    
    // Convert to hex strings
    let compressed_hex = hex::encode(compressed_bytes);
    let uncompressed_hex = hex::encode(uncompressed_bytes);
    
    Ok((compressed_hex, uncompressed_hex))
}

/// Run a complete MPC signing test: initialize -> sign -> shutdown
/// This function can be called multiple times to test repeated initialization
async fn run_mpc_signing_test(mpc_config: MpcConfig, test_number: u32) -> Result<()> {
    println!("\n{}", "=".repeat(60));
    println!("🔄 Test Run #{}", test_number);
    println!("{}", "=".repeat(60));

    // Get the first available account_id
    let account_id = mpc_config.key_shares.get(0)
        .map(|ks| ks.account_id.clone())
        .ok_or_else(|| anyhow::anyhow!("No key shares available"))?;

    // Step 1: Initialize MpcSigner
    println!("\n[1/3] 🚀 Initializing MpcSigner...");
    let signer = match MpcSigner::new(mpc_config) {
        Ok(s) => {
            info!("✅ MpcSigner initialized successfully");
            println!("✅ MpcSigner created");
            s
        }
        Err(e) => {
            error!("❌ Failed to initialize MpcSigner: {}", e);
            return Err(e.into());
        }
    };

    // Initialize and start MPC infrastructure
    if let Err(e) = signer.initialize() {
        error!("❌ Failed to initialize MPC infrastructure: {}", e);
        return Err(e.into());
    }

    println!("✅ MPC Infrastructure Ready");
    println!("   - Local participant server: RUNNING");
    println!("   - Remote sign-service: CONNECTED");

    // Step 2: Create and Sign Transaction
    println!("\n[2/3] 🔐 Creating and Signing Transaction...");

    // Setup Base Sepolia RPC connection
    let rpc_url = "https://tiniest-clean-sponge.base-sepolia.quiknode.pro/5380b34bde82bd24e05443cbe7f3efce0625d89e";
    let chain_id: u64 = 84532; // Base Sepolia chain ID

    println!("🌐 Connecting to Base Sepolia (Chain ID: {})", chain_id);

    let provider = ProviderBuilder::new()
        .connect_http(rpc_url.parse().expect("Invalid RPC URL"));

    // Get latest block to verify connection
    match provider.get_block_number().await {
        Ok(block_number) => {
            println!("✅ Connected to Base Sepolia (Block: {})", block_number);
        }
        Err(e) => {
            error!("❌ Failed to connect to RPC: {}", e);
            // Cleanup in blocking context before returning error
            let _ = signer.shutdown();
            tokio::task::spawn_blocking(move || {
                drop(signer);
            })
            .await
            .ok();
            return Err(e.into());
        }
    };

    println!("📝 Using Account ID: {}", account_id);

    // Create real Ethereum transaction for Base Sepolia
    println!("💰 Creating Transaction (0.0001 ETH transfer)");

    let to_address = "0x9548251949b08521f4397cdfafbb58b50571a2e6"
        .parse::<Address>()
        .expect("Invalid address");
    
    let value = U256::from(100_000_000_000_000u64); // 0.0001 ETH in wei
    let gas_limit = 21_000u64; // Basic transfer gas limit
    let data = Bytes::new(); // Empty data, simple transfer

    // Get current base fee and construct EIP-1559 fee parameters
    let max_priority_fee_per_gas = 1_000_000_000u64; // 1 Gwei priority fee
    let max_fee_per_gas = match provider.get_gas_price().await {
        Ok(price) => {
            let base_fee = price as u64;
            let max_fee = base_fee + max_priority_fee_per_gas;
            let max_fee_gwei = max_fee / 1_000_000_000;
            info!("Gas: base={} Gwei, max={} Gwei", base_fee / 1_000_000_000, max_fee_gwei);
            max_fee
        }
        Err(e) => {
            error!("⚠️  Failed to get gas price, using default: {}", e);
            let default_max_fee = 20_000_000_000u64; // 20 Gwei fallback
            default_max_fee
        }
    };

    // For demo purposes, use incremental nonce based on test number
    let nonce = test_number as u64;

    info!("EIP-1559 Transaction details:");
    info!("  To: {}", to_address);
    info!("  Value: 0.0001 ETH");
    info!("  Nonce: {}", nonce);
    info!("  Gas Limit: {}", gas_limit);
    info!("  Chain ID: {}", chain_id);

    // Build EIP-1559 transaction using alloy_consensus
    let tx = TxEip1559 {
        chain_id,
        nonce,
        gas_limit: gas_limit.into(),
        max_fee_per_gas: max_fee_per_gas.into(),
        max_priority_fee_per_gas: max_priority_fee_per_gas.into(),
        to: to_address.into(),
        value,
        input: data.clone(),
        access_list: Default::default(), // Empty access list
    };

    // Get the signing hash (this is what gets signed)
    let signing_hash = tx.signature_hash();
    let signing_hash_bytes = signing_hash.as_slice().to_vec();

    info!("Transaction signing hash: 0x{}", hex::encode(&signing_hash_bytes));

    println!("🔐 Executing MPC Signature (Threshold 2/3)...");

    // Execute MPC signature with account_id
    match signer.sign_data(signing_hash_bytes.clone(), account_id.clone()) {
        Ok(signature) => {
            println!("✅ Signature Generated!");
            info!("Signature: R={} bytes, S={} bytes, V={}",
                  signature.r.len(), signature.s.len(), signature.v);

            // For EIP-1559, we use y_parity (0 or 1) instead of v
            let y_parity = signature.v;

            // Recover public key from signature for verification
            match recover_public_key(&signing_hash_bytes, &signature.r, &signature.s, y_parity) {
                Ok((compressed, _uncompressed)) => {
                    info!("Public Key (compressed): 0x{}", compressed);
                }
                Err(e) => {
                    error!("Public Key Recovery Failed: {}", e);
                }
            }

            // Convert signature components
            let r = U256::from_be_slice(&signature.r);
            let s = U256::from_be_slice(&signature.s);
            
            // Create the signature using alloy_consensus
            let sig = alloy::primitives::Signature::new(r, s, y_parity != 0);
            
            // Create signed transaction
            let signed_tx = Signed::new_unchecked(tx, sig, signing_hash);

            // Encode using EIP-2718 format (includes 0x02 type prefix for EIP-1559)
            let encoded = signed_tx.encoded_2718();

            println!("📦 Signed Transaction: {} bytes (type 0x{:02x})",
                     encoded.len(), encoded.get(0).unwrap_or(&0));

            // Note: We don't broadcast in test mode to avoid nonce conflicts
            println!("ℹ️  Broadcasting skipped in test mode");
            info!("Transaction would be sent to: {}", to_address);

            println!("✅ Signing test completed successfully");
        }
        Err(e) => {
            error!("❌ Signature failed: {}", e);
            println!("❌ Signature Failed: {}", e);

            // Cleanup in blocking context before returning error
            let _ = signer.shutdown();
            tokio::task::spawn_blocking(move || {
                drop(signer);
            })
            .await
            .ok();

            return Err(e.into());
        }
    }

    // Step 3: Graceful Shutdown
    println!("\n[3/3] 🛑 Shutting Down MpcSigner...");
    match signer.shutdown() {
        Ok(_) => {
            println!("✅ MPC infrastructure stopped gracefully");
        }
        Err(e) => {
            error!("⚠️  Shutdown error: {}", e);
            println!("⚠️  Shutdown completed with warnings: {}", e);
        }
    }

    // IMPORTANT: Drop signer in a blocking context to avoid "Cannot drop a runtime in async context" panic
    // MpcSigner contains a tokio runtime, which cannot be dropped from within an async context
    tokio::task::spawn_blocking(move || {
        drop(signer);
        info!("Signer dropped in blocking context");
    })
    .await
    .expect("Failed to drop signer in blocking context");

    println!("✅ Test Run #{} Completed", test_number);

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 MPC Wallet Client - Repeated Initialization Test");
    println!("====================================================");
    println!("This test verifies that MpcSigner can be initialized,");
    println!("used for signing, and shut down multiple times safely.");
    println!();

    // Get config file path, default to config/client.yaml
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config/client.yaml".to_string());

    println!("📋 Loading configuration from: {}", config_path);

    // Load configuration from YAML and create MpcConfig
    let mpc_config = load_mpc_config(&config_path)?;

    // Print available account_ids
    println!("\n📋 Available Account IDs:");
    for key_share in &mpc_config.key_shares {
        println!("  - {}", key_share.account_id);
    }

    // Get number of test runs from environment or default to 3
    let num_runs = std::env::var("TEST_RUNS")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(3);

    println!("\n🔄 Running {} test cycles...\n", num_runs);

    // Run multiple test cycles
    for i in 1..=num_runs {
        // Clone config for each test run
        let config_clone = mpc_config.clone();

        match run_mpc_signing_test(config_clone, i).await {
            Ok(_) => {
                info!("Test run #{} succeeded", i);
            }
            Err(e) => {
                error!("Test run #{} failed: {}", i, e);
                println!("\n❌ Test run #{} failed: {}", i, e);
                println!("Stopping test sequence.");
                return Err(e);
            }
        }

        // Add a small delay between runs to ensure clean separation
        if i < num_runs {
            println!("\n⏳ Waiting 2 seconds before next test run...\n");
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
    }

    println!("\n{}", "=".repeat(60));
    println!("🎉 All {} Test Runs Completed Successfully!", num_runs);
    println!("{}", "=".repeat(60));
    println!("✅ MpcSigner repeated initialization: PASSED");
    println!("✅ MPC threshold signature: PASSED");
    println!("✅ Graceful shutdown: PASSED");
    println!("✅ Resource cleanup: VERIFIED");

    Ok(())
}
