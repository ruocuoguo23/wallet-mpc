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

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 Starting MPC Wallet Client with Account ID Architecture");
    println!("=======================================================");

    // Get config file path, default to config/client.yaml
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config/client.yaml".to_string());

    // Load configuration from YAML and create MpcConfig
    let mpc_config = load_mpc_config(&config_path)?;
    
    // Print available account_ids
    println!("\n📋 Available Account IDs:");
    for key_share in &mpc_config.key_shares {
        println!("  - {}", key_share.account_id);
    }

    // Get the first available account_id before moving mpc_config
    let account_id = mpc_config.key_shares.get(0)
        .map(|ks| ks.account_id.clone())
        .ok_or_else(|| anyhow::anyhow!("No key shares available"))?;

    // Initialize MpcSigner with MpcConfig
    let signer = match MpcSigner::new(mpc_config) {
        Ok(s) => {
            info!("✅ MpcSigner initialized successfully");
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

    println!("\n📡 MPC Infrastructure Ready");
    println!("- Local participant server: RUNNING");
    println!("- Remote sign-service: CONNECTED");

    // Setup Base Sepolia RPC connection
    let rpc_url = "https://tiniest-clean-sponge.base-sepolia.quiknode.pro/5380b34bde82bd24e05443cbe7f3efce0625d89e";
    let chain_id: u64 = 84532; // Base Sepolia chain ID

    println!("\n🌐 Connecting to Base Sepolia Network");
    println!("RPC URL: {}", rpc_url);
    println!("Chain ID: {}", chain_id);

    let provider = ProviderBuilder::new()
        .connect_http(rpc_url.parse().expect("Invalid RPC URL"));

    // Get latest block to verify connection
    match provider.get_block_number().await {
        Ok(block_number) => {
            println!("✅ Connected to Base Sepolia");
            println!("Latest block: {}", block_number);
        }
        Err(e) => {
            error!("❌ Failed to connect to RPC: {}", e);
            return Err(e.into());
        }
    }

    println!("\n🗝️  Using Account ID Architecture");
    println!("Account ID: {} (from loaded key shares)", account_id);
    println!("📝 Note: Each account_id represents a pre-derived HD wallet key_share");
    println!("   No runtime derivation needed - key_shares are pre-generated for each path");

    // Create real Ethereum transaction for Base Sepolia
    println!("\n💰 Creating Real Ethereum Transaction (0.0001 ETH)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

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
            println!("✅ Estimated base fee: {} Gwei", base_fee / 1_000_000_000);
            println!("✅ Max fee per gas: {} Gwei (base + priority)", max_fee_gwei);
            max_fee
        }
        Err(e) => {
            error!("⚠️  Failed to get gas price, using default: {}", e);
            let default_max_fee = 20_000_000_000u64; // 20 Gwei fallback
            println!("⚠️  Using default max fee: {} Gwei", default_max_fee / 1_000_000_000);
            default_max_fee
        }
    };

    // For demo purposes, we'll use nonce 1 (in real usage, you'd get this from the account)
    let nonce = 1u64;

    info!("EIP-1559 Transaction details:");
    info!("  To: {}", to_address);
    info!("  Value: 0.0001 ETH");
    info!("  Nonce: {}", nonce);
    info!("  Max Priority Fee: {} Gwei", max_priority_fee_per_gas / 1_000_000_000);
    info!("  Max Fee Per Gas: {} Gwei", max_fee_per_gas / 1_000_000_000);
    info!("  Gas Limit: {}", gas_limit);
    info!("  Data: {} bytes", data.len());
    info!("  Network: Base Sepolia (EIP-1559)");
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
    info!("Signing hash size: {} bytes", signing_hash_bytes.len());

    println!("\n🔐 Starting MPC Signature Process with Account ID (EIP-1559)");
    println!("- Transaction Type: EIP-1559 (Type 2)");
    println!("- Threshold: 2 out of 3 participants");
    println!("- Participants: Local + Sign-service");
    println!("- Account ID: {} (pre-derived key_share)", account_id);
    println!("- Architecture: Account ID -> Key Share Mapping (no runtime derivation)");

    // Execute MPC signature with account_id
    match signer.sign_data(signing_hash_bytes.clone(), account_id.clone()) {
        Ok(signature) => {
            println!("\n✅ Account ID Signature Generated Successfully!");
            println!("📝 Signature components:");
            println!("   Account ID: {}", account_id);
            println!("   R: {} bytes", signature.r.len());
            println!("   S: {} bytes", signature.s.len());
            println!("   Recovery ID: {} (raw)", signature.v);

            // For EIP-1559, we use y_parity (0 or 1) instead of v
            let y_parity = signature.v; // The recovery ID is already 0 or 1
            println!("   Y Parity (EIP-1559): {} (recovery_id)", y_parity);

            // Recover public key from signature
            println!("\n🔑 Recovering Public Key from MPC Signature:");
            match recover_public_key(&signing_hash_bytes, &signature.r, &signature.s, y_parity) {
                Ok((compressed, uncompressed)) => {
                    println!("   ✅ Public Key Recovery Successful!");
                    println!("   Compressed: 0x{}", compressed);
                    println!("   Uncompressed: 0x{}", uncompressed);
                    
                    // Note: Compare this with the public key displayed during MPC signing process
                    println!("   💡 Compare above with the account key shown in participant logs");
                }
                Err(e) => {
                    println!("   ❌ Public Key Recovery Failed: {}", e);
                    println!("   This might indicate signature issues");
                }
            }

            // Convert signature components
            let r = U256::from_be_slice(&signature.r);
            let s = U256::from_be_slice(&signature.s);
            
            // Create the signature using alloy_consensus
            // For EIP-1559, parity is the recovery ID directly (0 or 1)
            let sig = alloy::primitives::Signature::new(r, s, y_parity != 0);
            
            // Create signed transaction
            let signed_tx = Signed::new_unchecked(tx, sig, signing_hash);

            // Encode using EIP-2718 format (includes 0x02 type prefix for EIP-1559)
            let encoded = signed_tx.encoded_2718();

            println!("\n📦 Signed Transaction (EIP-1559):");
            println!("   Size: {} bytes", encoded.len());
            println!("   Hex: 0x{}", hex::encode(&encoded));
            
            // Verify the transaction starts with 0x02 (EIP-1559 type)
            if !encoded.is_empty() && encoded[0] == 0x02 {
                println!("   ✅ Correct EIP-1559 type prefix (0x02)");
            } else {
                println!("   ❌ Missing EIP-1559 type prefix! Found: 0x{:02x}", encoded.get(0).unwrap_or(&0));
                println!("   This will likely fail when broadcasting");
            }

            println!("\n🚀 Broadcasting Transaction to Base Sepolia");
            
            // Send the raw transaction to the network
            match provider.send_raw_transaction(&Bytes::from(encoded)).await {
                Ok(pending_tx) => {
                    let tx_hash = *pending_tx.tx_hash();
                    println!("✅ Transaction Broadcasted Successfully!");
                    println!("🔍 Transaction Hash: {:#x}", tx_hash);
                    println!("🌐 Explorer URL: https://sepolia.basescan.org/tx/{:#x}", tx_hash);
                    
                    println!("\n⏳ Waiting for transaction confirmation...");
                    match pending_tx.get_receipt().await {
                        Ok(receipt) => {
                            println!("✅ Transaction Confirmed!");
                            println!("   Block: {}", receipt.block_number.unwrap_or_default());
                            println!("   Gas Used: {}", receipt.gas_used);
                            println!("   Effective Gas Price: {} Gwei",
                                     receipt.effective_gas_price / 1_000_000_000);
                            println!("   Status: {}", if receipt.status() { "Success" } else { "Failed" });
                            
                            if !receipt.status() {
                                error!("❌ Transaction failed on-chain");
                            } else {
                                println!("🎉 Transaction executed successfully on Base Sepolia!");
                            }
                        }
                        Err(e) => {
                            error!("⚠️  Failed to get transaction receipt: {}", e);
                            println!("Transaction was broadcasted but receipt retrieval failed");
                        }
                    }
                }
                Err(e) => {
                    error!("❌ Failed to broadcast transaction: {}", e);
                    println!("💥 Transaction Broadcast Failed!");
                    println!("Error: {}", e);
                    println!("This might be due to:");
                    println!("- Insufficient balance for gas");
                    println!("- Invalid nonce (expected: see explorer for address)");
                    println!("- Invalid signature (check y_parity, r, s values)");
                    println!("- Network issues");
                    println!("- Transaction encoding issues");
                    println!("\n🔍 Signature Debug Info:");
                    println!("   Recovery ID: {}", signature.v);
                    println!("   Y Parity (EIP-1559): {} (0x{:x})", y_parity, y_parity);
                    println!("   Chain ID: {} (0x{:x})", chain_id, chain_id);
                    println!("   R (hex): 0x{}", hex::encode(&signature.r));
                    println!("   S (hex): 0x{}", hex::encode(&signature.s));
                    println!("   Signing Hash: 0x{}", hex::encode(&signing_hash_bytes));
                }
            }
            
            info!("Account ID transaction process completed");
        }
        Err(e) => {
            error!("❌ Account ID signature failed: {}", e);
            println!("\n💥 Account ID Signature Failed!");
            println!("Error: {}", e);
            println!("\n🔍 Debugging Information:");
            println!("- Account ID: {}", account_id);
            println!("- Check that the account_id exists in the key_shares mapping");

            // Try to cleanup resources
            signer.shutdown();
            
            return Err(e.into());
        }
    }

    // Graceful shutdown
    println!("\n🛑 Shutting Down");
    signer.shutdown();
    println!("✅ MPC infrastructure stopped");

    println!("\n🎯 Account ID MPC Client Demo Completed");
    println!("=========================================");
    println!("✅ Account ID architecture: SUCCESS");
    println!("✅ MPC threshold signature: SUCCESS");
    println!("✅ Real blockchain transaction: ATTEMPTED");
    println!("✅ Using alloy_consensus standard EIP-1559");
    println!("📋 Architecture: Pre-derived key_shares for each account");
    println!("🔍 Check the explorer link above for transaction status");
    
    Ok(())
}
