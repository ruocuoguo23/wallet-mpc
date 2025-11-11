use std::sync::Arc;
use tokio::sync::Mutex;

mod signer;
pub use signer::{Signer, SignatureResult as InternalSignatureResult};

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

/// MPC configuration for UniFFI
#[derive(Debug, Clone)]
pub struct MpcConfig {
    pub local_participant_host: String,
    pub local_participant_port: u16,
    pub local_participant_index: u16,
    pub key_share_file: String,
    pub sign_service_host: String,
    pub sign_service_port: u16,
    pub sse_host: String,
    pub sse_port: u16,
    pub sign_service_index: u16,
    pub threshold: u16,
    pub total_participants: u16,
    pub log_level: String,
}

/// Tokio runtime wrapper
struct TokioRuntime {
    rt: tokio::runtime::Runtime,
}

impl TokioRuntime {
    fn new() -> Result<Self, MpcError> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| MpcError::InitializationError { 
                msg: format!("Failed to create tokio runtime: {}", e)
            })?;
        Ok(Self { rt })
    }
}

/// MPC Signer for UniFFI
pub struct MpcSigner {
    signer: Arc<Mutex<Option<Signer>>>,
    config: MpcConfig,
    runtime: Arc<TokioRuntime>,
}

impl MpcSigner {
    /// Create a new MPC signer with configuration
    pub fn new(config: MpcConfig) -> Result<Self, MpcError> {
        let runtime = Arc::new(TokioRuntime::new()?);
        
        Ok(Self {
            signer: Arc::new(Mutex::new(None)),
            config,
            runtime,
        })
    }

    /// Initialize the MPC signer
    pub fn initialize(&self) -> Result<(), MpcError> {
        let config = self.config.clone();
        let signer_mutex = self.signer.clone();
        
        self.runtime.rt.block_on(async move {
            // Create YAML config content from MpcConfig
            let yaml_config = format!(
                r#"
local_participant:
  host: "{}"
  port: {}
  index: {}
  key_share_file: "{}"

remote_services:
  sign_service:
    participant_host: "{}"
    participant_port: {}
    sse_host: "{}"
    sse_port: {}
    index: {}

mpc:
  threshold: {}
  total_participants: {}

logging:
  level: "{}"
  format: "text"
"#,
                config.local_participant_host,
                config.local_participant_port,
                config.local_participant_index,
                config.key_share_file,
                config.sign_service_host,
                config.sign_service_port,
                config.sse_host,
                config.sse_port,
                config.sign_service_index,
                config.threshold,
                config.total_participants,
                config.log_level
            );

            // Write temporary config file
            let temp_config_path = "/tmp/mpc_client_config.yaml";
            std::fs::write(temp_config_path, yaml_config)
                .map_err(|e| MpcError::ConfigError { 
                    msg: format!("Failed to write config file: {}", e)
                })?;

            // Create signer
            let signer = Signer::new(temp_config_path)
                .await
                .map_err(|e| MpcError::InitializationError { 
                    msg: format!("Failed to create signer: {}", e)
                })?;

            // Store the signer
            let mut signer_guard = signer_mutex.lock().await;
            *signer_guard = Some(signer);

            Ok::<_, MpcError>(())
        })?;

        // Start local participant
        let signer_mutex = self.signer.clone();
        self.runtime.rt.block_on(async move {
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
        })?;

        Ok(())
    }

    /// Sign data using MPC
    pub fn sign_data(&self, data: Vec<u8>, derivation_path: Option<Vec<u32>>) -> Result<SignatureResult, MpcError> {
        let signer_mutex = self.signer.clone();
        
        self.runtime.rt.block_on(async move {
            let mut signer_guard = signer_mutex.lock().await;
            if let Some(ref mut signer) = *signer_guard {
                let result = signer.sign(data, derivation_path)
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
    }

    /// Shutdown the MPC signer
    pub fn shutdown(&self) {
        let signer_mutex = self.signer.clone();
        
        let _ = self.runtime.rt.block_on(async move {
            let mut signer_guard = signer_mutex.lock().await;
            if let Some(ref mut signer) = *signer_guard {
                let _ = signer.stop_local_participant().await;
            }
            *signer_guard = None;
        });
    }
}

/// Create MPC signer from config path (legacy function for UniFFI namespace)
pub fn create_mpc_signer(config_path: String) -> Result<Arc<MpcSigner>, MpcError> {
    // For backward compatibility, we'll try to parse YAML config
    let yaml_content = std::fs::read_to_string(&config_path)
        .map_err(|e| MpcError::ConfigError { 
            msg: format!("Failed to read config file {}: {}", config_path, e)
        })?;
    
    let config: serde_yaml::Value = serde_yaml::from_str(&yaml_content)
        .map_err(|e| MpcError::ConfigError { 
            msg: format!("Failed to parse YAML config: {}", e)
        })?;
    
    // Extract configuration values
    let local_participant = config.get("local_participant")
        .ok_or_else(|| MpcError::ConfigError { msg: "Missing local_participant config".to_string() })?;
    let remote_services = config.get("remote_services")
        .ok_or_else(|| MpcError::ConfigError { msg: "Missing remote_services config".to_string() })?;
    let sign_service = remote_services.get("sign_service")
        .ok_or_else(|| MpcError::ConfigError { msg: "Missing sign_service config".to_string() })?;
    let mpc = config.get("mpc")
        .ok_or_else(|| MpcError::ConfigError { msg: "Missing mpc config".to_string() })?;
    let logging = config.get("logging")
        .ok_or_else(|| MpcError::ConfigError { msg: "Missing logging config".to_string() })?;

    let mpc_config = MpcConfig {
        local_participant_host: local_participant.get("host")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MpcError::ConfigError { msg: "Missing local_participant.host".to_string() })?
            .to_string(),
        local_participant_port: local_participant.get("port")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| MpcError::ConfigError { msg: "Missing local_participant.port".to_string() })?
            as u16,
        local_participant_index: local_participant.get("index")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| MpcError::ConfigError { msg: "Missing local_participant.index".to_string() })?
            as u16,
        key_share_file: local_participant.get("key_share_file")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MpcError::ConfigError { msg: "Missing local_participant.key_share_file".to_string() })?
            .to_string(),
        sign_service_host: sign_service.get("participant_host")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MpcError::ConfigError { msg: "Missing sign_service.participant_host".to_string() })?
            .to_string(),
        sign_service_port: sign_service.get("participant_port")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| MpcError::ConfigError { msg: "Missing sign_service.participant_port".to_string() })?
            as u16,
        sse_host: sign_service.get("sse_host")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MpcError::ConfigError { msg: "Missing sign_service.sse_host".to_string() })?
            .to_string(),
        sse_port: sign_service.get("sse_port")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| MpcError::ConfigError { msg: "Missing sign_service.sse_port".to_string() })?
            as u16,
        sign_service_index: sign_service.get("index")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| MpcError::ConfigError { msg: "Missing sign_service.index".to_string() })?
            as u16,
        threshold: mpc.get("threshold")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| MpcError::ConfigError { msg: "Missing mpc.threshold".to_string() })?
            as u16,
        total_participants: mpc.get("total_participants")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| MpcError::ConfigError { msg: "Missing mpc.total_participants".to_string() })?
            as u16,
        log_level: logging.get("level")
            .and_then(|v| v.as_str())
            .unwrap_or("info")
            .to_string(),
    };

    let signer = MpcSigner::new(mpc_config)?;
    Ok(Arc::new(signer))
}
