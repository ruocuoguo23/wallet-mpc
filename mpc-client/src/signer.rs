use std::path::Path;
use std::fs;

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

#[derive(Debug, Deserialize, Serialize)]
pub struct ClientConfig {
    pub local_participant: LocalParticipantConfig,
    pub remote_services: RemoteServicesConfig,
    pub mpc: MpcConfig,
    pub logging: LoggingConfig,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LocalParticipantConfig {
    pub host: String,
    pub port: u16,
    pub index: u16,
    pub key_share_file: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RemoteServicesConfig {
    pub sign_service: RemoteServiceConfig,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RemoteServiceConfig {
    pub participant_host: String,
    pub participant_port: u16,
    pub sse_host: String,
    pub sse_port: u16,
    pub index: u16,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MpcConfig {
    pub threshold: u16,
    pub total_participants: u16,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LoggingConfig {
    pub level: String,
    pub format: String,
}

#[derive(Debug, Clone)]
pub struct SignatureResult {
    pub r: Vec<u8>,
    pub s: Vec<u8>,
    pub v: u32,
}

pub struct Signer {
    config: ClientConfig,
    local_participant_handle: Option<JoinHandle<Result<()>>>,
    all_participant_clients: Vec<ParticipantClient<Channel>>, // 包含所有participants的clients
    next_tx_id: i32, // 递增的交易ID计数器
}

impl ClientConfig {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read config file: {}", path.as_ref().display()))?;
        
        serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse YAML config file: {}", path.as_ref().display()))
    }
}

impl Signer {
    /// Create a new Signer instance
    pub async fn new(config_path: &str) -> Result<Self> {
        let config = ClientConfig::load_from_file(config_path)
            .context("Failed to load client configuration")?;

        Self::setup_logging(&config.logging)?;

        info!("Initializing MPC Signer...");
        info!("Local participant: {}:{} (index: {})", 
              config.local_participant.host, 
              config.local_participant.port,
              config.local_participant.index);

        // Connect to remote services
        let mut remote_clients = Vec::new();
        
        let sign_service_uri = format!("http://{}:{}", 
                                     config.remote_services.sign_service.participant_host,
                                     config.remote_services.sign_service.participant_port);
        
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
            next_tx_id: 1, // 从1开始计数
        })
    }

    /// Start local participant server
    pub async fn start_local_participant(&mut self) -> Result<()> {
        info!("Starting local participant server...");

        // Read SSE server info from config (deployed in sign-service)
        let sse_config = &self.config.remote_services.sign_service;
        
        info!("Using SSE server from sign-service: {}:{}", 
              sse_config.sse_host, 
              sse_config.sse_port);

        let participant_config = ParticipantAppConfig {
            sse: SSEConfig {
                host: sse_config.sse_host.clone(),
                port: sse_config.sse_port,
            },
            participant: ParticipantConfig {
                host: self.config.local_participant.host.clone(),
                port: self.config.local_participant.port,
                index: self.config.local_participant.index,
            },
        };

        let participant_server = ParticipantServer::new(participant_config)
            .map_err(|e| anyhow::anyhow!("Failed to create local participant server: {}", e))?;

        info!("Local participant server created - {}:{} (index: {})",
              self.config.local_participant.host,
              self.config.local_participant.port,
              self.config.local_participant.index);
        info!("Connected to SSE server at {}:{}",
              sse_config.sse_host,
              sse_config.sse_port);

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
                               self.config.local_participant.host,
                               self.config.local_participant.port);
        
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

    /// Sign arbitrary data using MPC threshold signature
    ///
    /// # Arguments
    /// * `data` - Raw bytes to be signed
    /// * `derivation_path` - Optional HD wallet derivation path
    ///
    /// # Returns
    /// * `SignatureResult` - Contains r, s, v components of the signature
    pub async fn sign(&mut self, data: Vec<u8>, derivation_path: Option<Vec<u32>>) -> Result<SignatureResult> {
        info!("Starting MPC signature process...");
        info!("Data size: {} bytes", data.len());

        // Get and increment tx_id for this signature request
        let tx_id = self.next_tx_id;
        self.next_tx_id += 1;
        
        // Generate unique execution ID
        let execution_id = Uuid::new_v4();
        info!("Starting signature request - TX ID: {}, Execution ID: {}", tx_id, execution_id);

        // Prepare sign request
        let sign_message = SignMessage {
            tx_id,
            execution_id: execution_id.as_bytes().to_vec(),
            chain: Chain::Ethereum.into(),
            data,
            derivation_path: derivation_path.unwrap_or_default(),
        };

        // Send sign requests to all participants (需要达到threshold数量)
        info!("Sending sign requests to {} participants...", self.all_participant_clients.len());
        
        let futures = self.all_participant_clients.iter_mut().take(self.config.mpc.threshold as usize).map(|client| {
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

    fn setup_logging(config: &LoggingConfig) -> Result<()> {
        let log_level = match config.level.to_lowercase().as_str() {
            "error" => log::LevelFilter::Error,
            "warn" => log::LevelFilter::Warn,
            "info" => log::LevelFilter::Info,
            "debug" => log::LevelFilter::Debug,
            "trace" => log::LevelFilter::Trace,
            _ => {
                eprintln!("Warning: Unknown log level '{}', using 'info'", config.level);
                log::LevelFilter::Info
            }
        };

        env_logger::Builder::from_default_env()
            .filter_level(log_level)
            .init();

        info!("Logging initialized with level: {}", config.level);
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
