use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use log::{info, error};
use tokio::task::JoinHandle;
use uuid::Uuid;
use futures::future::join_all;
use rand::{thread_rng, Rng};

use participant::ParticipantServer;
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
    pub key_share_data: String, // JSON-formatted key share data
}

/// Simplified config structure for direct initialization (no file dependency)
#[derive(Debug, Clone)]
pub struct SignerConfig {
    pub local_participant_host: String,
    pub local_participant_port: u16,
    pub local_participant_index: u16,
    pub key_shares: Vec<KeyShareData>,  // 直接传入key share数据
    pub sign_gateway_host: String,
    pub sign_gateway_port: u16,
    pub sse_host: String,
    pub sse_port: u16,
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


pub struct Signer {
    config: SignerConfig,
    local_participant_server: Option<ParticipantServer>,
    local_participant_handle: Option<JoinHandle<Result<()>>>,
    all_participant_clients: Vec<ParticipantClient<Channel>>,
    /// Instance unique identifier (high 16 bits of tx_id)
    /// Combines timestamp and random number to avoid collision across instances
    instance_id: u16,
    /// Incremental counter within this instance (low 16 bits of tx_id)
    tx_counter: u16,
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

        // Generate instance unique identifier to avoid tx_id collision across instances
        // Strategy: Mix timestamp (milliseconds) with random number
        let instance_id = Self::generate_instance_id();
        info!("Instance ID: 0x{:04X} (for tx_id generation)", instance_id);

        // Connect to remote services
        let mut remote_clients = Vec::new();
        
        let sign_service_uri = format!("http://{}:{}",
                                     config.sign_gateway_host,
                                     config.sign_gateway_port);

        info!("Connecting to sign-gateway at: {}", sign_service_uri);
        let channel = Channel::from_shared(sign_service_uri)?
            .connect()
            .await
            .context("Failed to connect to sign-gateway")?;
        
        remote_clients.push(ParticipantClient::new(channel));
        info!("Connected to sign-gateway participant");

        Ok(Self {
            config,
            local_participant_server: None,
            local_participant_handle: None,
            all_participant_clients: remote_clients,
            instance_id,
            tx_counter: 0,
        })
    }

    /// Generate a unique instance identifier for this Signer instance
    ///
    /// Combines timestamp (milliseconds) and random number to create a 16-bit ID
    /// that is highly unlikely to collide across different instances or restarts.
    ///
    /// Strategy:
    /// - High 8 bits: Mix of timestamp milliseconds (lower bits for variation)
    /// - Low 8 bits: Random number
    ///
    /// This gives us 65536 possible instance IDs with very low collision probability.
    fn generate_instance_id() -> u16 {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Take bits from timestamp that change frequently
        // Use middle bits to avoid both slow-changing high bits and fast-cycling low bits
        let timestamp_part = ((timestamp_ms >> 8) & 0xFF) as u16;

        // Generate random byte for additional entropy
        let random_part = thread_rng().gen::<u8>() as u16;

        // Combine: high byte from timestamp, low byte random
        let instance_id = (timestamp_part << 8) | random_part;

        info!(
            "Generated instance_id: 0x{:04X} (timestamp_part: 0x{:02X}, random_part: 0x{:02X})",
            instance_id, timestamp_part, random_part
        );

        instance_id
    }

    /// Generate next transaction ID for signing request
    ///
    /// TX ID format (32-bit i32):
    /// - High 16 bits: Instance unique identifier (from `instance_id`)
    /// - Low 16 bits: Incremental counter within this instance (0-65535)
    ///
    /// This ensures:
    /// 1. Different Signer instances have different ID spaces (via instance_id)
    /// 2. Same instance generates sequential IDs (via counter)
    /// 3. Very low collision probability even with multiple instances/restarts
    ///
    /// Example:
    /// - Instance 1: 0x1A2B0001, 0x1A2B0002, 0x1A2B0003, ...
    /// - Instance 2: 0x7F3C0001, 0x7F3C0002, 0x7F3C0003, ...
    fn next_tx_id(&mut self) -> i32 {
        // Get current counter and increment for next call
        let counter = self.tx_counter;
        self.tx_counter = self.tx_counter.wrapping_add(1);

        // Combine instance_id (high 16 bits) and counter (low 16 bits)
        let tx_id = ((self.instance_id as i32) << 16) | (counter as i32);

        info!(
            "Generated tx_id: {} (0x{:08X}) [instance: 0x{:04X}, counter: {}]",
            tx_id, tx_id as u32, self.instance_id, counter
        );

        tx_id
    }

    /// Start local participant server using new ParticipantServer::new method
    pub async fn start_local_participant(&mut self) -> Result<()> {
        info!("Starting local participant server...");
        
        info!("Using SSE server: {}:{}", 
              self.config.sse_host, 
              self.config.sse_port);

        // Convert KeyShareData to HashMap<String, KeyShare> format expected by ParticipantServer
        let mut key_shares = HashMap::new();
        
        for key_share_data in &self.config.key_shares {
            // Parse JSON key share data
            let key_share: KeyShare<Secp256k1, SecurityLevel128> = serde_json::from_str(&key_share_data.key_share_data)
                .map_err(|e| anyhow::anyhow!("Failed to parse key share for {}: {}", key_share_data.account_id, e))?;
            
            key_shares.insert(key_share_data.account_id.clone(), key_share);
            info!("✓ Key share loaded for account_id: {}", key_share_data.account_id);
        }
        
        if key_shares.is_empty() {
            return Err(anyhow::anyhow!("No key shares provided"));
        }
        
        info!("✓ Loaded {} key shares", key_shares.len());
        info!("  - Available account_ids: {:?}", key_shares.keys().collect::<Vec<_>>());

        // Create participant server using new interface with pre-loaded key shares
        let sse_url = format!("http://{}:{}", self.config.sse_host, self.config.sse_port);
        let participant_server = ParticipantServer::new(
            &sse_url,
            &self.config.local_participant_host,
            self.config.local_participant_port,
            key_shares,
        ).map_err(|e| anyhow::anyhow!("Failed to create local participant server: {}", e))?;

        info!("Local participant server created - {}:{}", 
              self.config.local_participant_host,
              self.config.local_participant_port);
        info!("Connected to SSE server at {}", sse_url);

        // Clone the participant server for the background task
        let participant_server_clone = participant_server.clone();

        // Start participant server in background
        let handle = tokio::spawn(async move {
            participant_server_clone.start().await
                .map_err(|e| anyhow::anyhow!("Local participant server failed: {}", e))
        });

        // Store the participant server instance for graceful shutdown
        self.local_participant_server = Some(participant_server);
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

        // Generate unique tx_id using instance_id + counter
        let tx_id = self.next_tx_id();

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

        // Send sign requests to all participants (must reach threshold)
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

    /// Stop local participant server gracefully
    pub async fn stop_local_participant(&mut self) -> Result<()> {
        info!("🛑 Initiating graceful shutdown of local participant server...");

        // Step 1: Call shutdown on the ParticipantServer for graceful shutdown
        if let Some(ref participant_server) = self.local_participant_server {
            info!("Calling ParticipantServer::shutdown()...");
            participant_server.shutdown().await
                .map_err(|e| anyhow::anyhow!("Failed to shutdown participant server: {}", e))?;
            info!("✓ ParticipantServer shutdown completed");
        }

        // Step 2: Wait for the server task to complete or abort it
        if let Some(handle) = self.local_participant_handle.take() {
            info!("Waiting for server task to complete...");

            // Give the server a bit of time to finish gracefully
            let timeout = tokio::time::Duration::from_secs(5);
            match tokio::time::timeout(timeout, handle).await {
                Ok(result) => {
                    match result {
                        Ok(Ok(())) => info!("✓ Server task completed successfully"),
                        Ok(Err(e)) => error!("Server task finished with error: {}", e),
                        Err(e) => error!("Server task panicked: {:?}", e),
                    }
                }
                Err(_) => {
                    info!("Server task did not complete within timeout, this is expected");
                    // Note: The handle is already dropped, no need to abort
                }
            }
        }

        // Step 3: Clear the participant server reference
        self.local_participant_server = None;

        info!("✅ Local participant server stopped successfully");
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

        // Use try_init() instead of init() to allow multiple calls
        // This is important for repeated initialization scenarios (e.g., tests)
        match env_logger::Builder::from_default_env()
            .filter_level(log_level)
            .try_init()
        {
            Ok(_) => {
                info!("Logging initialized with level: {}", level);
            }
            Err(_) => {
                // Logger already initialized, this is fine in repeated initialization scenarios
                info!("Logger already initialized, continuing with existing configuration");
            }
        }

        Ok(())
    }
}

impl Drop for Signer {
    fn drop(&mut self) {
        // Attempt graceful shutdown when Signer is dropped
        log::warn!("⚠️ Signer being dropped, attempting graceful shutdown...");

        // We can't await in Drop, so we use a blocking approach
        if self.local_participant_server.is_some() || self.local_participant_handle.is_some() {
            // Create a runtime for blocking cleanup
            if let Ok(rt) = tokio::runtime::Runtime::new() {
                let _ = rt.block_on(async {
                    let _ = self.stop_local_participant().await;
                });
            } else {
                // Fallback to abort if we can't create runtime
                log::error!("Failed to create runtime for graceful shutdown, aborting task");
                if let Some(handle) = self.local_participant_handle.take() {
                    handle.abort();
                }
            }
        }

        log::info!("Signer dropped");
    }
}
