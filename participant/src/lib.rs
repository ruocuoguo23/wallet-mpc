mod client;
mod config;
mod signing;

use std::error::Error;
use log::info;

use cggmp21::KeyShare;
use cggmp21::security_level::SecurityLevel128;
use cggmp21::supported_curves::Secp256k1;
use proto::mpc::participant_server::{Participant, ParticipantServer as GrpcParticipantServer};
use proto::mpc::{Chain, SignMessage, SignatureMessage};
use tonic::{Request, Response, Status, transport::Server};
use reqwest::Url;

pub use client::Client;
pub use config::{AppConfig, ParticipantConfig, SSEConfig};
pub use signing::Signing;

/// Main participant server structure that can be used as a library
pub struct ParticipantServer {
    server_address: String,
    handler: ParticipantHandler,
}

/// Internal participant handler
pub struct ParticipantHandler {
    client: Client,
    index: u16,
    key_share: KeyShare<Secp256k1, SecurityLevel128>,
}

impl ParticipantHandler {
    /// Load single key share from file based on participant index
    fn load_key_share_from_file(index: u16) -> Result<KeyShare<Secp256k1, SecurityLevel128>, Box<dyn Error>> {
        use std::fs;
        
        // Construct the filename for the specific participant
        // index 0 -> key_share_1.json, index 1 -> key_share_2.json, etc.
        let filename = format!("key_share_{}.json", index + 1);
        
        // 1. Check if the specific key share file exists
        if !std::path::Path::new(&filename).exists() {
            return Err(format!("{} file does not exist. Please ensure the key share files are properly distributed.", filename).into());
        }
        
        // 2. Load the single key share
        info!("   Loading key share from file: {}", filename);
        let key_share_json = fs::read_to_string(&filename)
            .map_err(|e| format!("Failed to read key share file {}: {}", filename, e))?;

        let key_share: KeyShare<Secp256k1, SecurityLevel128> = serde_json::from_str(&key_share_json)
            .map_err(|e| format!("Key share deserialization failed for {}: {}", filename, e))?;

        // 3. Verify that the loaded key share matches the expected index
        if key_share.core.i != index {
            return Err(format!("Key share index mismatch: file {} contains share for index {}, but expected index {}", 
                              filename, key_share.core.i, index).into());
        }

        info!("   ✓ Key share loaded successfully from {}", filename);

        Ok(key_share)
    }

    /// Create a new participant handler with the given client and index
    pub fn new(client: Client, index: u16) -> Result<Self, Box<dyn Error>> {
        info!("Loading participant configuration for index: {}", index);
        
        // Load the specific key share for this participant
        let key_share = Self::load_key_share_from_file(index)?;
        
        info!("✓ Participant {} initialized successfully", index);
        info!("  - Key share loaded for participant index: {}", index);
        info!("  - Using file: key_share_{}.json", index + 1);
        
        Ok(Self {
            client,
            index,
            key_share,
        })
    }
}

#[tonic::async_trait]
impl Participant for ParticipantHandler {
    async fn sign_tx(
        &self,
        request: Request<SignMessage>,
    ) -> Result<Response<SignatureMessage>, Status> {
        let req = request.into_inner();

        let tx_id = req.tx_id;
        let execution_id = req.execution_id;
        let chain = Chain::try_from(req.chain).map_err(|_| Status::internal("Invalid chain"))?;
        let tx = req.data;
        let derivation_path = if req.derivation_path.is_empty() {
            None
        } else {
            Some(req.derivation_path)
        };

        info!("Processing sign request - tx_id: {}, chain: {:?}, derivation_path: {:?}", 
              tx_id, chain, derivation_path);

        let signing = Signing::new(&self.client, tx_id);

        let (r, s, v) = signing
            .sign_tx(self.index, &execution_id, &tx, self.key_share.clone(), chain, derivation_path)
            .await
            .map_err(|e| {
                log::error!("Transaction signing failed: {}", e);
                Status::internal("Transaction signing failed")
            })?;

        info!("Transaction signed successfully - tx_id: {}", tx_id);

        Ok(Response::new(SignatureMessage { r, s, v }))
    }
}

impl ParticipantServer {
    /// Create a new ParticipantServer with configuration
    pub fn new(config: AppConfig) -> Result<Self, Box<dyn Error>> {
        info!("Initializing ParticipantServer");

        let server_url = Url::parse(&config.sse_url())?;
        let client = Client::new(server_url)?;

        let handler = ParticipantHandler::new(client, config.participant.index)?;
        let server_address = config.participant_addr();

        Ok(Self {
            server_address,
            handler,
        })
    }

    /// Create a new ParticipantServer with environment-based configuration
    pub fn with_default_config() -> Result<Self, Box<dyn Error>> {
        let config = AppConfig::from_env()
            .map_err(|e| format!("Failed to load configuration: {}", e))?;
        Self::new(config)
    }

    /// Create a new ParticipantServer with custom parameters
    pub fn with_params(sse_url: &str, participant_host: &str, participant_port: u16, index: u16) -> Result<Self, Box<dyn Error>> {
        info!("Initializing ParticipantServer with custom parameters");

        let server_url = Url::parse(sse_url)?;
        let client = Client::new(server_url)?;

        let handler = ParticipantHandler::new(client, index)?;
        let server_address = format!("{}:{}", participant_host, participant_port);

        Ok(Self {
            server_address,
            handler,
        })
    }

    /// Start the participant server
    pub async fn start(self) -> Result<(), Box<dyn Error>> {
        let addr = self.server_address.parse()
            .map_err(|e| format!("Invalid server address '{}': {}", self.server_address, e))?;

        info!("Starting gRPC server on address: {}", addr);

        Server::builder()
            .add_service(GrpcParticipantServer::new(self.handler))
            .serve(addr)
            .await
            .map_err(|e| format!("Server failed: {}", e))?;

        info!("MPC participant service stopped");

        Ok(())
    }

    /// Get the server address
    pub fn address(&self) -> &str {
        &self.server_address
    }

    /// Get the participant index
    pub fn index(&self) -> u16 {
        self.handler.index
    }
}
