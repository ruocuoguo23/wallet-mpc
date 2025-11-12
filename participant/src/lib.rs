mod client;
mod config;
mod signing;

use std::error::Error;
use std::collections::HashMap;
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
    key_shares: HashMap<String, KeyShare<Secp256k1, SecurityLevel128>>,  // account_id -> key_share映射
}

impl ParticipantHandler {
    /// Create a new participant handler with pre-loaded key shares
    pub fn new(client: Client, key_shares: HashMap<String, KeyShare<Secp256k1, SecurityLevel128>>) -> Result<Self, Box<dyn Error>> {
        if key_shares.is_empty() {
            return Err("Key shares cannot be empty".into());
        }
        
        info!("✓ Participant handler initialized successfully");
        info!("  - Loaded {} key shares", key_shares.len());
        info!("  - Available account_ids: {:?}", key_shares.keys().collect::<Vec<_>>());
        
        Ok(Self {
            client,
            key_shares,
        })
    }
    
    /// Get key share and index for a specific account_id
    fn get_key_share_by_account_id(&self, account_id: &str) -> Result<(&KeyShare<Secp256k1, SecurityLevel128>, u16), Box<dyn Error>> {
        let key_share = self.key_shares.get(account_id)
            .ok_or_else(|| format!("Key share not found for account_id: {}", account_id))?;
        
        let index = key_share.core.i;
        Ok((key_share, index))
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
        let account_id = req.account_id;

        // 验证account_id不能为空
        if account_id.is_empty() {
            return Err(Status::invalid_argument("account_id cannot be empty"));
        }

        info!("Processing sign request - tx_id: {}, chain: {:?}, account_id: {}", 
              tx_id, chain, account_id);

        // 通过account_id获取对应的key_share和index
        let (key_share, signing_index) = self.get_key_share_by_account_id(&account_id)
            .map_err(|e| {
                log::error!("Failed to get key share for account_id {}: {}", account_id, e);
                Status::not_found(format!("Key share not found for account_id: {}", account_id))
            })?;

        let signing = Signing::new(&self.client, tx_id);

        // 使用account_id对应的key_share和index进行签名
        // 注意：现在不再需要derivation_path，因为每个account_id对应的key_share已经是派生后的
        let (r, s, v) = signing
            .sign_tx(signing_index, &execution_id, &tx, key_share.clone(), chain, None)
            .await
            .map_err(|e| {
                log::error!("Transaction signing failed: {}", e);
                Status::internal("Transaction signing failed")
            })?;

        info!("Transaction signed successfully - tx_id: {}, account_id: {}, using index: {}", 
              tx_id, account_id, signing_index);

        Ok(Response::new(SignatureMessage { r, s, v }))
    }
}

impl ParticipantServer {
    /// Create a new ParticipantServer with pre-loaded key shares
    pub fn new(sse_url: &str, participant_host: &str, participant_port: u16, key_shares: HashMap<String, KeyShare<Secp256k1, SecurityLevel128>>) -> Result<Self, Box<dyn Error>> {
        info!("Initializing ParticipantServer");

        let server_url = Url::parse(sse_url)?;
        let client = Client::new(server_url)?;

        let handler = ParticipantHandler::new(client, key_shares)?;
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

    /// Get available account IDs
    pub fn account_ids(&self) -> Vec<String> {
        self.handler.key_shares.keys().cloned().collect()
    }
}
