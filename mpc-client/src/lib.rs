use std::sync::Arc;
use tokio::sync::Mutex;

mod signer;
pub use signer::{Signer, SignatureResult as InternalSignatureResult, SignerConfig, KeyShareData};

// UniFFI exports
uniffi::include_scaffolding!("mpc_client");

/// Error types for UniFFI
#[derive(Debug, thiserror::Error)]
pub enum MpcError {
    #[error("Configuration error: {msg}")]
    ConfigError { msg: String },
    #[error("Network error: {msg}")]
    NetworkError { msg: String },
    #[error("Signing error: {msg}")]
    SigningError { msg: String },
    #[error("Initialization error: {msg}")]
    InitializationError { msg: String },
}

/// Signature result for UniFFI
#[derive(Debug, Clone)]
pub struct SignatureResult {
    pub r: Vec<u8>,
    pub s: Vec<u8>,
    pub v: u32,
}

impl From<InternalSignatureResult> for SignatureResult {
    fn from(internal: InternalSignatureResult) -> Self {
        Self {
            r: internal.r,
            s: internal.s,
            v: internal.v,
        }
    }
}

/// Key share data for UniFFI
#[derive(Debug, Clone)]
pub struct KeyShare {
    pub account_id: String,
    pub key_share_data: String,
}

impl From<KeyShare> for KeyShareData {
    fn from(ks: KeyShare) -> Self {
        KeyShareData {
            account_id: ks.account_id,
            key_share_data: ks.key_share_data,
        }
    }
}

/// MPC configuration for UniFFI
#[derive(Debug, Clone)]
pub struct MpcConfig {
    pub local_participant_host: String,
    pub local_participant_port: u16,
    pub local_participant_index: u16,
    pub key_shares: Vec<KeyShare>,  // 使用key_shares替代key_share_file
    pub sign_service_host: String,
    pub sign_service_port: u16,
    pub sse_host: String,
    pub sse_port: u16,
    pub sign_service_index: u16,
    pub threshold: u16,
    pub total_participants: u16,
    pub log_level: String,
}

impl From<MpcConfig> for SignerConfig {
    fn from(config: MpcConfig) -> Self {
        SignerConfig {
            local_participant_host: config.local_participant_host,
            local_participant_port: config.local_participant_port,
            local_participant_index: config.local_participant_index,
            key_shares: config.key_shares.into_iter().map(|ks| ks.into()).collect(),
            sign_service_host: config.sign_service_host,
            sign_service_port: config.sign_service_port,
            sse_host: config.sse_host,
            sse_port: config.sse_port,
            sign_service_index: config.sign_service_index,
            threshold: config.threshold,
            total_participants: config.total_participants,
            log_level: config.log_level,
        }
    }
}

/// MPC Signer for UniFFI
pub struct MpcSigner {
    signer: Arc<Mutex<Option<Signer>>>,
    config: MpcConfig,
}

impl MpcSigner {
    /// Create a new MPC signer with configuration
    pub fn new(config: MpcConfig) -> Result<Self, MpcError> {
        Ok(Self {
            signer: Arc::new(Mutex::new(None)),
            config,
        })
    }

    /// Initialize the MPC signer
    pub fn initialize(&self) -> Result<(), MpcError> {
        let config = self.config.clone();
        let signer_mutex = self.signer.clone();

        // Use spawn_blocking to run async code
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                // Convert MpcConfig to SignerConfig
                let signer_config: SignerConfig = config.into();

                // Create signer with direct config (no file loading)
                let signer = Signer::new(signer_config)
                    .await
                    .map_err(|e| MpcError::InitializationError {
                        msg: format!("Failed to create signer: {}", e)
                    })?;

                // Store the signer
                let mut signer_guard = signer_mutex.lock().await;
                *signer_guard = Some(signer);

                Ok::<_, MpcError>(())
            })
        })?;

        // Start local participant
        let signer_mutex = self.signer.clone();

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                let mut signer_guard = signer_mutex.lock().await;
                if let Some(ref mut signer) = *signer_guard {
                    signer.start_local_participant()
                        .await
                        .map_err(|e| MpcError::InitializationError {
                            msg: format!("Failed to start local participant: {}", e)
                        })
                } else {
                    Err(MpcError::InitializationError {
                        msg: "Signer not initialized".to_string()
                    })
                }
            })
        })?;

        Ok(())
    }

    /// Sign data using MPC with account_id
    pub fn sign_data(&self, data: Vec<u8>, account_id: String) -> Result<SignatureResult, MpcError> {
        let signer_mutex = self.signer.clone();
        
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                let mut signer_guard = signer_mutex.lock().await;
                if let Some(ref mut signer) = *signer_guard {
                    let result = signer.sign(data, account_id)
                        .await
                        .map_err(|e| MpcError::SigningError {
                            msg: format!("Signing failed: {}", e)
                        })?;
                    Ok(result.into())
                } else {
                    Err(MpcError::InitializationError {
                        msg: "Signer not initialized".to_string()
                    })
                }
            })
        })
    }

    /// Shutdown the MPC signer
    pub fn shutdown(&self) {
        let signer_mutex = self.signer.clone();
        
        let _ = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                let mut signer_guard = signer_mutex.lock().await;
                if let Some(ref mut signer) = *signer_guard {
                    let _ = signer.stop_local_participant().await;
                }
                *signer_guard = None;
            })
        });
    }
}
