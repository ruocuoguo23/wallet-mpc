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
    index: u16,  // 保留默认index用于兼容
    key_shares: HashMap<String, KeyShare<Secp256k1, SecurityLevel128>>,  // account_id -> key_share映射
}

impl ParticipantHandler {
    /// Load all available key shares from files in the current directory
    fn load_key_shares_from_files() -> Result<HashMap<String, KeyShare<Secp256k1, SecurityLevel128>>, Box<dyn Error>> {
        use std::fs;
        use std::path::Path;
        
        let mut key_shares = HashMap::new();
        
        // 扫描当前目录中的所有key_share_*.json文件
        for i in 1..=10 {  // 假设最多支持10个key_share文件
            let filename = format!("key_share_{}.json", i);
            if Path::new(&filename).exists() {
                info!("   Loading key share from file: {}", filename);
                
                let key_share_json = fs::read_to_string(&filename)
                    .map_err(|e| format!("Failed to read key share file {}: {}", filename, e))?;

                let key_share: KeyShare<Secp256k1, SecurityLevel128> = serde_json::from_str(&key_share_json)
                    .map_err(|e| format!("Key share deserialization failed for {}: {}", filename, e))?;

                // 使用participant_index作为默认的account_id，同时支持自定义account_id
                let participant_index = key_share.core.i;
                let default_account_id = format!("account_{}", participant_index);
                
                key_shares.insert(default_account_id.clone(), key_share.clone());
                
                // TODO: 在这里可以添加从配置文件或其他地方加载account_id映射的逻辑
                // 例如读取account_mapping.json文件来建立account_id -> key_share的映射关系
                
                info!("   ✓ Key share loaded successfully from {} with account_id: {}", filename, default_account_id);
            }
        }
        
        if key_shares.is_empty() {
            return Err("No key share files found. Please ensure the key share files are properly distributed.".into());
        }
        
        info!("✓ Loaded {} key shares in total", key_shares.len());
        Ok(key_shares)
    }

    /// Create a new participant handler with the given client and index
    pub fn new(client: Client, index: u16) -> Result<Self, Box<dyn Error>> {
        info!("Loading participant configuration for index: {}", index);
        
        // 加载所有可用的key shares
        let key_shares = Self::load_key_shares_from_files()?;
        
        info!("✓ Participant {} initialized successfully", index);
        info!("  - Loaded {} key shares", key_shares.len());
        info!("  - Available account_ids: {:?}", key_shares.keys().collect::<Vec<_>>());
        
        Ok(Self {
            client,
            index,
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
