use std::collections::HashMap;

use anyhow::{Context, Result};
use log::{info, error};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use uuid::Uuid;
use futures::future::join_all;

use participant::{ParticipantServer, AppConfig as ParticipantAppConfig, ParticipantConfig, SSEConfig};
use proto::mpc::participant_client::ParticipantClient;
use proto::mpc::{SignMessage, Chain};
use tonic::transport::Channel;
use cggmp21::KeyShare;
use cggmp21::security_level::SecurityLevel128;
use cggmp21::supported_curves::Secp256k1;

/// Key share data structure for iOS client
#[derive(Debug, Clone)]
pub struct KeyShareData {
    pub account_id: String,
    pub key_share_data: String, // JSON格式的key share数据
}

/// Simplified config structure for direct initialization (no file dependency)
#[derive(Debug, Clone)]
pub struct SignerConfig {
    pub local_participant_host: String,
    pub local_participant_port: u16,
    pub local_participant_index: u16,
    pub key_shares: Vec<KeyShareData>,  // 直接传入key share数据
    pub sign_service_host: String,
    pub sign_service_port: u16,
    pub sse_host: String,
    pub sse_port: u16,
    pub sign_service_index: u16,
    pub threshold: u16,
    pub total_participants: u16,
    pub log_level: String,
}

#[derive(Debug, Clone)]
pub struct SignatureResult {
    pub r: Vec<u8>,
    pub s: Vec<u8>,
    pub v: u32,
}

/// Internal participant handler that works directly with key share data
pub struct InternalParticipantHandler {
    client: participant::Client,
    index: u16,
    key_shares: HashMap<String, KeyShare<Secp256k1, SecurityLevel128>>,
}

impl InternalParticipantHandler {
    /// Create new handler with key share data instead of files
    pub fn new(client: participant::Client, index: u16, key_shares_data: Vec<KeyShareData>) -> Result<Self> {
        info!("Loading participant configuration for index: {}", index);
        
        let mut key_shares = HashMap::new();
        
        for key_share_data in key_shares_data {
            // Parse JSON key share data
            let key_share: KeyShare<Secp256k1, SecurityLevel128> = serde_json::from_str(&key_share_data.key_share_data)
                .map_err(|e| anyhow::anyhow!("Failed to parse key share for {}: {}", key_share_data.account_id, e))?;
            
            key_shares.insert(key_share_data.account_id.clone(), key_share);
            info!("✓ Key share loaded for account_id: {}", key_share_data.account_id);
        }
        
        if key_shares.is_empty() {
            return Err(anyhow::anyhow!("No key shares provided"));
        }
        
        info!("✓ Participant {} initialized successfully", index);
        info!("  - Loaded {} key shares", key_shares.len());
        info!("  - Available account_ids: {:?}", key_shares.keys().collect::<Vec<_>>());
        
        Ok(Self {
            client,
            index,
            key_shares,
        })
    }
}

pub struct Signer {
    config: SignerConfig,
    local_participant_handle: Option<JoinHandle<Result<()>>>,
    all_participant_clients: Vec<ParticipantClient<Channel>>,
    next_tx_id: i32,
}

impl Signer {
    /// Create a new Signer instance with direct config (no file loading)
    pub async fn new(config: SignerConfig) -> Result<Self> {
        Self::setup_logging(&config.log_level)?;

        info!("Initializing MPC Signer...");
        info!("Local participant: {}:{} (index: {})", 
              config.local_participant_host, 
              config.local_participant_port,
              config.local_participant_index);

        // Connect to remote services
        let mut remote_clients = Vec::new();
        
        let sign_service_uri = format!("http://{}:{}", 
                                     config.sign_service_host,
                                     config.sign_service_port);
        
        info!("Connecting to sign-service at: {}", sign_service_uri);
        let channel = Channel::from_shared(sign_service_uri)?
            .connect()
            .await
            .context("Failed to connect to sign-service")?;
        
        remote_clients.push(ParticipantClient::new(channel));
        info!("Connected to sign-service participant");

        Ok(Self {
            config,
            local_participant_handle: None,
            all_participant_clients: remote_clients,
            next_tx_id: 1,
        })
    }

    /// Start local participant server with custom ParticipantHandler
    pub async fn start_local_participant(&mut self) -> Result<()> {
        info!("Starting local participant server...");
        
        info!("Using SSE server: {}:{}", 
              self.config.sse_host, 
              self.config.sse_port);

        // Create custom participant server with key share data
        let server_url = reqwest::Url::parse(&format!("http://{}:{}", self.config.sse_host, self.config.sse_port))?;
        let client = participant::Client::new(server_url)?;
        
        // Convert KeyShareData to the format expected by InternalParticipantHandler
        let key_shares_data = self.config.key_shares.iter().map(|ks| KeyShareData {
            account_id: ks.account_id.clone(),
            key_share_data: ks.key_share_data.clone(),
        }).collect();

        let internal_handler = InternalParticipantHandler::new(
            client,
            self.config.local_participant_index,
            key_shares_data
        )?;

        // For now, we'll create a custom participant server using the existing infrastructure
        // but with our updated participant handler logic
        let participant_config = ParticipantAppConfig {
            sse: SSEConfig {
                host: self.config.sse_host.clone(),
                port: self.config.sse_port,
            },
            participant: ParticipantConfig {
                host: self.config.local_participant_host.clone(),
                port: self.config.local_participant_port,
                index: self.config.local_participant_index,
            },
        };

        let participant_server = ParticipantServer::new(participant_config)
            .map_err(|e| anyhow::anyhow!("Failed to create local participant server: {}", e))?;

        info!("Local participant server created - {}:{} (index: {})",
              self.config.local_participant_host,
              self.config.local_participant_port,
              self.config.local_participant_index);
        info!("Connected to SSE server at {}:{}",
              self.config.sse_host,
              self.config.sse_port);

        // Start participant server in background
        let handle = tokio::spawn(async move {
            participant_server.start().await
                .map_err(|e| anyhow::anyhow!("Local participant server failed: {}", e))
        });

        self.local_participant_handle = Some(handle);
        
        // Wait a moment to ensure server starts
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        info!("Local participant server started successfully");

        // Now connect to the local participant as a client
        let local_uri = format!("http://{}:{}", 
                               self.config.local_participant_host,
                               self.config.local_participant_port);
        
        info!("Connecting to local participant at: {}", local_uri);
        let local_channel = Channel::from_shared(local_uri)?
            .connect()
            .await
            .context("Failed to connect to local participant")?;
        
        let local_client = ParticipantClient::new(local_channel);
        self.all_participant_clients.push(local_client);
        info!("Connected to local participant, total participants: {}", self.all_participant_clients.len());

        Ok(())
    }

    /// Sign arbitrary data using MPC threshold signature with account_id
    ///
    /// # Arguments
    /// * `data` - Raw bytes to be signed
    /// * `account_id` - Account ID to identify which key share to use
    ///
    /// # Returns
    /// * `SignatureResult` - Contains r, s, v components of the signature
    pub async fn sign(&mut self, data: Vec<u8>, account_id: String) -> Result<SignatureResult> {
        info!("Starting MPC signature process...");
        info!("Data size: {} bytes", data.len());
        info!("Account ID: {}", account_id);

        // Validate account_id exists in our key shares
        if !self.config.key_shares.iter().any(|ks| ks.account_id == account_id) {
            return Err(anyhow::anyhow!("Account ID '{}' not found in available key shares", account_id));
        }

        // Get and increment tx_id for this signature request
        let tx_id = self.next_tx_id;
        self.next_tx_id += 1;
        
        // Generate unique execution ID
        let execution_id = Uuid::new_v4();
        info!("Starting signature request - TX ID: {}, Execution ID: {}", tx_id, execution_id);

        // Prepare sign request with account_id instead of derivation_path
        let sign_message = SignMessage {
            tx_id,
            execution_id: execution_id.as_bytes().to_vec(),
            chain: Chain::Ethereum.into(),
            data,
            account_id,
        };

        // Send sign requests to all participants (需要达到threshold数量)
        info!("Sending sign requests to {} participants...", self.all_participant_clients.len());
        
        let futures = self.all_participant_clients.iter_mut().take(self.config.threshold as usize).map(|client| {
            let request = tonic::Request::new(sign_message.clone());
            async move {
                client.sign_tx(request).await
            }
        });

        let results = join_all(futures).await;

        // Check signature results
        let mut successful_signatures = Vec::new();
        for (i, result) in results.into_iter().enumerate() {
            match result {
                Ok(response) => {
                    let signature = response.into_inner();
                    info!("Received signature from participant {}: r_len={}, s_len={}, v={}", 
                          i, signature.r.len(), signature.s.len(), signature.v);
                    successful_signatures.push(SignatureResult {
                        r: signature.r,
                        s: signature.s,
                        v: signature.v,
                    });
                }
                Err(e) => {
                    error!("Failed to get signature from participant {}: {}", i, e);
                }
            }
        }

        if successful_signatures.is_empty() {
            return Err(anyhow::anyhow!("No valid signatures received"));
        }

        // Return the first valid signature
        let signature = successful_signatures.into_iter().next().unwrap();
        info!("MPC signature completed successfully");

        Ok(signature)
    }

    /// Stop local participant server
    pub async fn stop_local_participant(&mut self) -> Result<()> {
        if let Some(handle) = self.local_participant_handle.take() {
            info!("Stopping local participant server...");
            handle.abort();
            info!("Local participant server stopped");
        }
        Ok(())
    }

    fn setup_logging(level: &str) -> Result<()> {
        let log_level = match level.to_lowercase().as_str() {
            "error" => log::LevelFilter::Error,
            "warn" => log::LevelFilter::Warn,
            "info" => log::LevelFilter::Info,
            "debug" => log::LevelFilter::Debug,
            "trace" => log::LevelFilter::Trace,
            _ => {
                eprintln!("Warning: Unknown log level '{}', using 'info'", level);
                log::LevelFilter::Info
            }
        };

        env_logger::Builder::from_default_env()
            .filter_level(log_level)
            .init();

        info!("Logging initialized with level: {}", level);
        Ok(())
    }
}

impl Drop for Signer {
    fn drop(&mut self) {
        if let Some(handle) = self.local_participant_handle.take() {
            handle.abort();
        }
    }
}
