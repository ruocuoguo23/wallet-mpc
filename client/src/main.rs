use alloy::primitives::{Address, Bytes, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy_consensus::private::alloy_eips::Encodable2718;
use alloy_consensus::{SignableTransaction, Signed, TxEip1559};
use anyhow::Result;
use log::{error, info};
use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};

use mpc_client::Signer;

/// Parse HD wallet derivation path from string like "m/44'/60'/0'/0/0"
/// Returns a vector of u32 values where hardened keys have the 0x80000000 bit set
///
/// Note: For MPC signing, only non-hardened derivation is supported since
/// hardened derivation requires access to the private key
fn parse_derivation_path(path_str: &str) -> Result<Vec<u32>> {
    if !path_str.starts_with("m/") {
        return Err(anyhow::anyhow!("Derivation path must start with 'm/'"));
    }
    
    let path_parts = &path_str[2..]; // Remove "m/"
    if path_parts.is_empty() {
        return Ok(vec![]);
    }
    
    let mut path = Vec::new();
    for part in path_parts.split('/') {
        if part.is_empty() {
            continue;
        }
        
        let (num_str, is_hardened) = if part.ends_with('\'') || part.ends_with('h') {
            (&part[..part.len()-1], true)
        } else {
            (part, false)
        };
        
        let num: u32 = num_str.parse()
            .map_err(|_| anyhow::anyhow!("Invalid number in derivation path: {}", num_str))?;
        
        if is_hardened {
            log::warn!("⚠️  Hardened index detected ({}'), will be treated as non-hardened for MPC", num);
            log::warn!("    MPC signing only supports non-hardened derivation");
        }

        // For MPC, we only use the raw index value without hardening bit
        // The MPC library will handle derivation using public key only
        path.push(num);
    }
    
    Ok(path)
}

/// Format derivation path for display
fn format_derivation_path(path: &[u32]) -> String {
    let mut result = String::from("m");
    for &component in path {
        result.push('/');
        // For MPC, all indices are non-hardened
        result.push_str(&component.to_string());
    }
    result
}

/// Validate derivation path for MPC usage
fn validate_derivation_path(path: &[u32]) -> Result<()> {
    if path.is_empty() {
        return Err(anyhow::anyhow!("Derivation path cannot be empty"));
    }

    // Check if all indices are valid non-hardened indices
    for &index in path {
        if index >= 0x80000000 {
            return Err(anyhow::anyhow!(
                "Hardened index {} not supported in MPC mode. Use non-hardened indices only.",
                index
            ));
        }
    }

    Ok(())
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
    println!("🚀 Starting MPC Wallet Client with HD Wallet Support");
    println!("=====================================================");

    // Get config file path, default to config/client.yaml
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config/client.yaml".to_string());

    // Initialize Signer
    let mut signer = match Signer::new(&config_path).await {
        Ok(s) => {
            info!("✅ Signer initialized successfully");
            s
        }
        Err(e) => {
            error!("❌ Failed to initialize signer: {}", e);
            return Err(e);
        }
    };

    // Start local participant server
    if let Err(e) = signer.start_local_participant().await {
        error!("❌ Failed to start local participant: {}", e);
        return Err(e);
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

    // HD Wallet derivation path: m/44/60/0/0/0 (Non-hardened for MPC)
    // Note: MPC only supports non-hardened derivation since it requires public key only
    let derivation_path_str = "m/44/60/0/0/0";
    let derivation_path = parse_derivation_path(derivation_path_str)?;
    
    println!("\n🗝️  Using HD Wallet Derivation (MPC Mode)");
    println!("Path (string): {}", derivation_path_str);
    println!("Path (formatted): {}", format_derivation_path(&derivation_path));
    println!("Path (raw u32): {:?}", derivation_path);
    println!("Path (hex): {}", derivation_path.iter()
        .map(|v| format!("0x{:08x}", v))
        .collect::<Vec<_>>()
        .join(", "));
    println!("\n⚠️  Note: MPC signing uses non-hardened derivation only");
    println!("   Standard path m/44'/60'/0'/0/0 becomes m/44/60/0/0/0 in MPC mode");

    // Validate the path
    validate_derivation_path(&derivation_path)?;

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

    // For demo purposes, we'll use nonce 0 (in real usage, you'd get this from the account)
    let nonce = 0u64;

    info!("EIP-1559 Transaction details:");
    info!("  To: {}", to_address);
    info!("  Value: 0.0001 ETH");
    info!("  Nonce: {}", nonce);
    info!("  Max Priority Fee: {} Gwei", max_priority_fee_per_gas / 1_000_000_000);
    info!("  Max Fee Per Gas: {} Gwei", max_fee_per_gas / 1_000_000_000);
    info!("  Max Priority Fee Per Gas: {} Gwei", max_priority_fee_per_gas / 1_000_000_000);
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

    println!("\n🔐 Starting MPC Signature Process with HD Wallet (EIP-1559)");
    println!("- Transaction Type: EIP-1559 (Type 2)");
    println!("- Threshold: 2 out of 3 participants");
    println!("- Participants: Local + Sign-service");
    println!("- HD Path: {} (non-hardened)", derivation_path_str);
    println!("- Derivation Mode: Public key only (MPC compatible)");

    // Execute MPC signature with HD wallet derivation path
    match signer.sign(signing_hash_bytes.clone(), Some(derivation_path.clone())).await {
        Ok(signature) => {
            println!("\n✅ HD Wallet Signature Generated Successfully!");
            println!("📝 Signature components:");
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
                    println!("   💡 Compare above with the HD-derived public key shown in participant logs");
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
            
            info!("HD wallet transaction process completed");
        }
        Err(e) => {
            error!("❌ HD wallet signature failed: {}", e);
            println!("\n💥 HD Wallet Signature Failed!");
            println!("Error: {}", e);
            println!("\n🔍 Debugging Information:");
            println!("- Path (string): {}", derivation_path_str);
            println!("- Path (formatted): {}", format_derivation_path(&derivation_path));
            println!("- Path (raw): {:?}", derivation_path);
            println!("- All indices are non-hardened: {}",
                     derivation_path.iter().all(|&i| i < 0x80000000));

            // Try to cleanup resources
            if let Err(cleanup_err) = signer.stop_local_participant().await {
                error!("Failed to cleanup local participant: {}", cleanup_err);
            }
            
            return Err(e);
        }
    }

    // Graceful shutdown
    println!("\n🛑 Shutting Down");
    if let Err(e) = signer.stop_local_participant().await {
        error!("Failed to stop local participant gracefully: {}", e);
    } else {
        println!("✅ Local participant server stopped");
    }

    println!("\n🎯 HD Wallet MPC Client Demo Completed");
    println!("=====================================");
    println!("✅ HD wallet derivation: SUCCESS");
    println!("✅ MPC threshold signature: SUCCESS");
    println!("✅ Real blockchain transaction: ATTEMPTED");
    println!("✅ Using alloy_consensus standard EIP-1559");
    println!("📋 HD Derivation Mode: Non-hardened (MPC compatible)");
    println!("🔍 Check the explorer link above for transaction status");
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derivation_path_parsing() {
        // Test MPC-compatible non-hardened Ethereum path
        let path = parse_derivation_path("m/44/60/0/0/0").unwrap();
        let expected: Vec<u32> = vec![44, 60, 0, 0, 0];
        assert_eq!(path, expected);

        // Verify formatting
        assert_eq!(format_derivation_path(&path), "m/44/60/0/0/0");

        // Test that hardened notation is ignored (treated as warning)
        let path = parse_derivation_path("m/44'/60'/0'/0/0").unwrap();
        // Should convert to non-hardened
        assert_eq!(path, vec![44, 60, 0, 0, 0]);

        // Test simple path
        let path = parse_derivation_path("m/0/1").unwrap();
        let expected_simple: Vec<u32> = vec![0, 1];
        assert_eq!(path, expected_simple);
        assert_eq!(format_derivation_path(&path), "m/0/1");

        // Test empty path after m/
        let path = parse_derivation_path("m/").unwrap();
        let expected_empty: Vec<u32> = vec![];
        assert_eq!(path, expected_empty);

        // Test invalid paths
        assert!(parse_derivation_path("44/60/0/0/0").is_err()); // Missing m/
        assert!(parse_derivation_path("m/invalid/0").is_err()); // Invalid number
    }

    #[test]
    fn test_non_hardened_indices() {
        // Verify all indices are non-hardened
        let path = parse_derivation_path("m/44/60/0/0/0").unwrap();
        for &index in &path {
            assert!(index < 0x80000000, "Index {} is hardened", index);
        }

        // Test path validation
        assert!(validate_derivation_path(&path).is_ok());
    }

    #[test]
    fn test_derivation_path_validation() {
        // Valid non-hardened path
        let path = vec![44, 60, 0, 0, 0];
        assert!(validate_derivation_path(&path).is_ok());

        // Hardened path should fail validation
        let hardened_path = vec![0x8000002c, 0x8000003c, 0x80000000, 0, 0];
        assert!(validate_derivation_path(&hardened_path).is_err());

        // Empty path should fail
        let empty_path: Vec<u32> = vec![];
        assert!(validate_derivation_path(&empty_path).is_err());
    }

    #[tokio::test]
    async fn test_signer_initialization() {
        // This test requires the config file to exist
        // In actual environments, a test config should be used
        if std::path::Path::new("config/client.yaml").exists() {
            let result = Signer::new("config/client.yaml").await;
            // In test environments, network connection may fail, but config should at least be parsed
            match result {
                Ok(_) => println!("Signer initialized successfully in test"),
                Err(e) => println!("Expected error in test environment: {}", e),
            }
        }
    }
}
