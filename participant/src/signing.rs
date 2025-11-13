use crate::client::{Client, Room};
use alloy::signers::k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use alloy::primitives::{keccak256, Address};
use anyhow::Result;
use cggmp21::DataToSign;
use cggmp21::ExecutionId;
use cggmp21::KeyShare;
use generic_ec::{Curve, Point, coords::HasAffineX};
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
        _derivation_path: Option<Vec<u32>>, // Reserved for compatibility, not used because key_share is pre-derived
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
        
        // No need for HD wallet derivation anymore because key_share is pre-derived
        log::info!("Using pre-derived key share (account-specific)");
        
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
                // Directly use the shared_public_key in the pre-derived key_share
                // This public_key already corresponds to a specific account_id
                let public_key = key_share.shared_public_key;

                // Print public key information
                log::info!("📊 Account-specific Public Key Details:");
                let pub_key_compressed = public_key.to_bytes(true);
                let pub_key_uncompressed = public_key.to_bytes(false);
                log::info!("  Compressed (33 bytes): 0x{}", hex::encode(&pub_key_compressed));
                log::info!("  Uncompressed (65 bytes): 0x{}", hex::encode(&pub_key_uncompressed));

                // Calculate and print Ethereum address for this account
                if pub_key_uncompressed.len() == 65 && pub_key_uncompressed[0] == 0x04 {
                    // Remove the 0x04 prefix for keccak256 hashing
                    let pub_key_data = &pub_key_uncompressed[1..];
                    let hash = keccak256(pub_key_data);
                    let address = Address::from_slice(&hash[12..]);

                    log::info!("🔐 Ethereum Address Derivation:");
                    log::info!("  Public key (64 bytes, no prefix): 0x{}", hex::encode(pub_key_data));
                    log::info!("  Keccak256 hash: 0x{}", hex::encode(&hash));
                    log::info!("  Address (last 20 bytes): {}", address.to_checksum(None));

                    println!("🏦 Account-specific Ethereum Address: {}", address.to_checksum(None));
                } else {
                    log::warn!("Invalid public key format for Ethereum address calculation");
                }

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

                        // Manually attempt recovery ID 0 and 1
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

                                    // Correctly compare two byte arrays
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
