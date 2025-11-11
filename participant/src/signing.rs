use crate::client::{Client, Room};
use alloy::signers::k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use alloy::primitives::{keccak256, Address};
use anyhow::Result;
use cggmp21::DataToSign;
use cggmp21::ExecutionId;
use cggmp21::KeyShare;
use generic_ec::{Curve, Point, coords::HasAffineX, NonZero};
use proto::mpc::Chain;

use cggmp21::round_based::MpcParty;
use cggmp21::security_level::SecurityLevel128;
use cggmp21::signing::msg::Msg;
use sha2::Sha256;
use std::error::Error;
use alloy::hex;

/// Parameters per each curve that are needed in tests
pub trait CurveParams: Curve {
    /// Which HD derivation algorithm to use with that curve
    type HdAlgo: cggmp21::hd_wallet::HdWallet<Self>;
    // type ExVerifier: ExternalVerifier<Self>;
}

impl CurveParams for cggmp21::supported_curves::Secp256k1 {
    type HdAlgo = cggmp21::hd_wallet::Slip10;
    // type ExVerifier = external_verifier::Bitcoin;
}

// impl CurveParams for cggmp24::supported_curves::Secp256r1 {
//     type HdAlgo = cggmp24::hd_wallet::Slip10;
//     // type ExVerifier = external_verifier::Noop;
// }
//
// impl CurveParams for cggmp24::supported_curves::Stark {
//     type HdAlgo = cggmp24::hd_wallet::Stark;
//     // type ExVerifier = external_verifier::blockchains::StarkNet;
// }

pub struct Signing {
    room: Room,
}

impl Signing {
    pub fn new(client: &Client, id: i32) -> Self {
        Self {
            room: client.room(format!("signing_{id}").as_str()),
        }
    }

    pub async fn sign_tx<T>(
        self,
        index: u16,
        execution_id: &[u8],
        tx: &[u8],
        key_share: KeyShare<T, SecurityLevel128>,
        chain: Chain,
        derivation_path: Option<Vec<u32>>,
    ) -> Result<(Vec<u8>, Vec<u8>, u32)>
    where
        T: Curve + CurveParams + cggmp21::hd_wallet::slip10::SupportedCurve,
        Point<T>: HasAffineX<T>,
    {
        let eid = ExecutionId::new(execution_id);

        let (_, incoming, outgoing) = self.room.join_room::<Msg<T, Sha256>>(index).await?;

        let party = MpcParty::connected((incoming, outgoing));

        // tx parameter should already be the signing hash, not the raw transaction
        // So we create DataToSign from the hash directly, not digest it again
        let data = DataToSign::from_scalar(generic_ec::Scalar::from_be_bytes_mod_order(tx));

        // TODO: Harcoded parties_indexes_at_keygen. Participants has a harcoded index.
        // Indexes must be issued on room creation and stored in DB.
        let signing = cggmp21::signing(eid, index, &[0, 1], &key_share);
        
        // Apply HD wallet derivation path if provided
        let signing = if let Some(derivation_path) = derivation_path.as_ref() {
            log::info!("Using HD wallet with derivation path: {:?}", derivation_path);
            signing
                // .set_derivation_path_with_algo::<T::HdAlgo, _>(derivation_path.iter().cloned())
                .set_derivation_path(derivation_path.iter().cloned())
                .map_err(|err| {
                    log::error!("Failed to set derivation path: {err}");
                    anyhow::anyhow!("HD wallet derivation path error: {}", err)
                })?
        } else {
            log::info!("Using standard signing (no HD wallet derivation)");
            signing
        };
        
        let signature = signing
            .sign(&mut rand::rngs::OsRng, party, data)
            .await
            .map_err(|err| {
                log::error!("Signing phase failed: {err}");
                if let Some(source) = err.source() {
                    log::error!("Caused by: {}", source);
                }
                err
            })?;

        let r = signature.r.into_inner().to_be_bytes();
        let r_bytes = r.as_bytes();
        let s = signature.s.into_inner().to_be_bytes();
        let s_bytes = s.as_bytes();

        // Compute recovery ID (0 or 1) for signature verification
        // Upper layers can convert this to chain-specific format (e.g., EIP-155 for Ethereum)
        let recovery_id = match chain {
            Chain::Ethereum => {
                // Use derived public key for HD wallet, otherwise use shared public key
                // Following the BIP-32 standard implementation
                let public_key = if let Some(derivation_path) = derivation_path.as_ref() {
                    log::info!("Computing public key from derivation path: {:?}", derivation_path);

                    // Derive child public key following BIP-32 standard
                    let child_key = key_share
                        .derive_child_public_key::<T::HdAlgo, _>(derivation_path.iter().cloned())
                        .map_err(|err| {
                            log::error!("Failed to derive child public key: {err}");
                            anyhow::anyhow!("HD wallet child key derivation error: {}", err)
                        })?;

                    // Wrap the derived public key with NonZero (important for proper key handling)
                    let derived_public_key = NonZero::from_point(child_key.public_key)
                        .ok_or_else(|| {
                            log::error!("Derived public key is zero point (invalid)");
                            anyhow::anyhow!("HD wallet derived public key is invalid (zero point)")
                        })?;

                    // Print detailed public key information
                    log::info!("📊 Derived Public Key Details:");
                    let pub_key_compressed = derived_public_key.to_bytes(true);
                    let pub_key_uncompressed = derived_public_key.to_bytes(false);
                    log::info!("  Compressed (33 bytes): 0x{}", hex::encode(&pub_key_compressed));
                    log::info!("  Uncompressed (65 bytes): 0x{}", hex::encode(&pub_key_uncompressed));
                    log::info!("  First byte (should be 0x04): 0x{:02x}", pub_key_uncompressed.get(0).unwrap_or(&0));
                    log::info!("  Length: {} bytes", pub_key_uncompressed.len());

                    println!("\n📊 Derived Public Key Details:");
                    println!("  Compressed: 0x{}", hex::encode(&pub_key_compressed));
                    println!("  Uncompressed: 0x{}", hex::encode(&pub_key_uncompressed));

                    // Verify signature with derived public key
                    if let Err(e) = signature.verify(&derived_public_key, &data) {
                        log::error!("❌ Signature verification failed with derived public key: {:?}", e);
                        return Err(anyhow::anyhow!("Signature verification failed: {:?}", e));
                    }
                    log::info!("✅ Signature verified successfully with derived public key");

                    // if let Err(e) = T::ExVerifier::verify(&derived_public_key, &signature, tx) {
                    //     log::error!("❌ External signature verification failed: {:?}", e);
                    //     return Err(anyhow::anyhow!("External signature verification failed: {:?}", e));
                    // }
                    // log::info!("✅ External signature verified successfully");

                    // Calculate and print Ethereum address for derived public key
                    if pub_key_uncompressed.len() == 65 && pub_key_uncompressed[0] == 0x04 {
                        // Remove the 0x04 prefix for keccak256 hashing
                        let pub_key_data = &pub_key_uncompressed[1..];
                        let hash = keccak256(pub_key_data);
                        let address = Address::from_slice(&hash[12..]);

                        log::info!("🔐 Ethereum Address Derivation:");
                        log::info!("  Public key (64 bytes, no prefix): 0x{}", hex::encode(pub_key_data));
                        log::info!("  Keccak256 hash: 0x{}", hex::encode(&hash));
                        log::info!("  Address (last 20 bytes): {:#x}", address);

                        println!("🏦 HD Wallet Derived Ethereum Address: {:#x}", address);
                    } else {
                        log::error!("❌ Invalid public key format!");
                        log::error!("   Expected: 65 bytes starting with 0x04");
                        log::error!("   Actual: {} bytes starting with 0x{:02x}",
                                   pub_key_uncompressed.len(),
                                   pub_key_uncompressed.get(0).unwrap_or(&0));
                        return Err(anyhow::anyhow!("Invalid public key format for Ethereum address calculation"));
                    }

                    derived_public_key
                } else {
                    // Use shared public key (no HD derivation)
                    let shared_public_key = key_share.shared_public_key;

                    // Print shared public key information
                    log::info!("📊 Shared Public Key Details (no HD derivation):");
                    let pub_key_compressed = shared_public_key.to_bytes(true);
                    let pub_key_uncompressed = shared_public_key.to_bytes(false);
                    log::info!("  Compressed (33 bytes): 0x{}", hex::encode(&pub_key_compressed));
                    log::info!("  Uncompressed (65 bytes): 0x{}", hex::encode(&pub_key_uncompressed));

                    // Calculate and print Ethereum address for shared public key
                    if pub_key_uncompressed.len() == 65 && pub_key_uncompressed[0] == 0x04 {
                        // Remove the 0x04 prefix for keccak256 hashing
                        let pub_key_data = &pub_key_uncompressed[1..];
                        let hash = keccak256(pub_key_data);
                        let address = Address::from_slice(&hash[12..]);

                        log::info!("🔐 Ethereum Address Derivation:");
                        log::info!("  Public key (64 bytes, no prefix): 0x{}", hex::encode(pub_key_data));
                        log::info!("  Keccak256 hash: 0x{}", hex::encode(&hash));
                        log::info!("  Address (last 20 bytes): {}", address.to_checksum(None));

                        println!("🏦 Standard MPC Ethereum Address: {}", address.to_checksum(None));
                    } else {
                        log::warn!("Invalid public key format for Ethereum address calculation");
                    }

                    shared_public_key
                };

                // Compute recovery ID using k256 library
                let pub_key = public_key.to_bytes(false);
                let v_key = VerifyingKey::from_sec1_bytes(&pub_key).map_err(|err| {
                    log::error!("Verifying key failed: {err}");
                    if let Some(source) = err.source() {
                        log::error!("Caused by: {}", source);
                    }
                    err
                })?;
                let sig = Signature::from_slice(&[r_bytes, s_bytes].concat()).map_err(|err| {
                    log::error!("Signature failed: {err}");
                    if let Some(source) = err.source() {
                        log::error!("Caused by: {}", source);
                    }
                    err
                })?;

                // Try to recover the recovery ID from the signature
                let recovery_id = RecoveryId::trial_recovery_from_msg(
                    &v_key,
                    &data.to_scalar().to_be_bytes(),
                    &sig,
                );

                match recovery_id {
                    Ok(id) => {
                        log::info!("Recovery ID computed: {}", id.to_byte());
                        id.to_byte() as u32
                    }
                    Err(_) => {
                        log::warn!("⚠️ Primary recovery ID calculation failed, trying manual recovery");

                        // 手动尝试 recovery ID 0 和 1
                        for test_id in [0u8, 1u8] {
                            if let Ok(recovery_id) = RecoveryId::try_from(test_id) {
                                if let Ok(recovered_key) = VerifyingKey::recover_from_prehash(
                                    &data.to_scalar().to_be_bytes(),
                                    &sig,
                                    recovery_id,
                                ) {
                                    // 检查恢复的公钥是否匹配
                                    let recovered_bytes = recovered_key.to_encoded_point(false);
                                    let expected_bytes = public_key.to_bytes(false);

                                    // 正确比较两个字节数组
                                    if recovered_bytes.as_bytes() == expected_bytes.as_ref() {
                                        log::info!("✅ Correct recovery ID found through manual testing: {}", test_id);
                                        return Ok((r_bytes.to_vec(), s_bytes.to_vec(), test_id as u32));
                                    }
                                }
                            }
                        }

                        log::error!("❌ Failed to determine correct recovery ID");
                        return Err(anyhow::anyhow!("Cannot determine recovery ID"));
                    }
                }
            }
            Chain::Bitcoin => {
                // Bitcoin doesn't use recovery ID in the same way
                // Return 0 as placeholder
                0
            }
        };

        log::info!("Signature generated - r: {} bytes, s: {} bytes, recovery_id: {}",
                   r_bytes.len(), s_bytes.len(), recovery_id);

        Ok((r_bytes.to_vec(), s_bytes.to_vec(), recovery_id))
    }
}
