use std::path::Path;
use std::fs;
use std::collections::HashMap;

use anyhow::{Context, Result};
use log::{info, error};
use serde::{Deserialize, Serialize};
use tokio::try_join;

use sse::{SseServer, AppConfig as SseAppConfig, SSEConfig};
use participant::ParticipantServer;
use cggmp21::KeyShare;
use cggmp21::security_level::SecurityLevel128;
use cggmp21::supported_curves::Secp256k1;

#[derive(Debug, Deserialize, Serialize)]
struct SignServiceConfig {
    server: ServerConfig,
    logging: LoggingConfig,
    mpc: MpcConfig,
}

#[derive(Debug, Deserialize, Serialize)]
struct ServerConfig {
    sse: SseServerConfig,
    participant: ParticipantServerConfig,
}

#[derive(Debug, Deserialize, Serialize)]
struct SseServerConfig {
    host: String,
    port: u16,
    cors_origins: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ParticipantServerConfig {
    host: String,
    port: u16,
    index: u16,
}

#[derive(Debug, Deserialize, Serialize)]
struct LoggingConfig {
    level: String,
    format: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct MpcConfig {
    threshold: u16,
    total_participants: u16,
    key_share_file: String,
}

impl SignServiceConfig {
    fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read config file: {}", path.as_ref().display()))?;
        
        serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse YAML config file: {}", path.as_ref().display()))
    }

    fn to_sse_config(&self) -> SseAppConfig {
        SseAppConfig {
            sse: SSEConfig {
                host: self.server.sse.host.clone(),
                port: self.server.sse.port,
            },
        }
    }

}

/// Load key_share_1.json file with fixed account_id "1"
fn load_key_share() -> Result<HashMap<String, KeyShare<Secp256k1, SecurityLevel128>>> {
    let filename = "key_share_1.json";
    
    info!("Loading key share from file: {}", filename);
    let key_share_json = fs::read_to_string(filename)
        .with_context(|| format!("Failed to read key share file {}", filename))?;

    let key_share: KeyShare<Secp256k1, SecurityLevel128> = serde_json::from_str(&key_share_json)
        .with_context(|| format!("Key share deserialization failed for {}", filename))?;

    let mut key_shares = HashMap::new();
    let account_id = "1".to_string(); // Fixed account_id as requested
    key_shares.insert(account_id.clone(), key_share);
    
    info!("✓ Key share loaded successfully with account_id: {}", account_id);
    
    Ok(key_shares)
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

async fn run_services(config: SignServiceConfig) -> Result<()> {
    info!("Initializing services...");
    
    // 加载key share (固定使用key_share_1.json和account_id="1")
    let key_shares = load_key_share()
        .context("Failed to load key share")?;
    
    // 创建SSE server
    let sse_config = config.to_sse_config();
    let sse_server = SseServer::new(sse_config);
    info!("SSE Server created - {}:{}", config.server.sse.host, config.server.sse.port);

    // 创建Participant server 使用新的接口
    let sse_url = format!("http://{}:{}", config.server.sse.host, config.server.sse.port);
    let participant_server = ParticipantServer::new(
        &sse_url,
        &config.server.participant.host,
        config.server.participant.port,
        key_shares,
    ).map_err(|e| anyhow::anyhow!("Failed to create participant server: {}", e))?;
    
    info!("Participant Server created - {}:{}", 
          config.server.participant.host, 
          config.server.participant.port);
    info!("Using SSE server: {}", sse_url);
    info!("Loaded key share with account_id: 1");

    info!("Starting both servers concurrently...");

    // 并发运行两个服务器
    let result: Result<((), ()), anyhow::Error> = try_join!(
        async {
            sse_server.start().await
                .context("SSE server failed")
        },
        async {
            participant_server.start().await
                .map_err(|e| anyhow::anyhow!("Participant server failed: {}", e))
        }
    );

    match result {
        Ok(_) => {
            info!("Both servers have stopped successfully");
            Ok(())
        }
        Err(e) => {
            error!("One or both servers failed: {}", e);
            Err(e)
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // 获取配置文件路径，默认使用 config/sign-service.yaml
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config/sign-service.yaml".to_string());

    // 加载配置文件
    let config = SignServiceConfig::load_from_file(&config_path)
        .context("Failed to load configuration")?;

    // 设置日志
    setup_logging(&config.logging)
        .context("Failed to setup logging")?;

    info!("Sign Service starting up...");
    info!("Configuration loaded from: {}", config_path);
    info!("SSE Server will start on: {}:{}", config.server.sse.host, config.server.sse.port);
    info!("Participant Server will start on: {}:{}", config.server.participant.host, config.server.participant.port);
    info!("Participant index: {}", config.server.participant.index);
    info!("MPC configuration: threshold={}, total_participants={}", config.mpc.threshold, config.mpc.total_participants);

    // 运行服务
    if let Err(e) = run_services(config).await {
        error!("Service execution failed: {}", e);
        std::process::exit(1);
    }

    info!("Sign Service shutting down");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_loading() {
        let yaml_content = r#"
server:
  sse:
    host: "127.0.0.1"
    port: 8080
    cors_origins: ["http://localhost:3000"]
  participant:
    host: "127.0.0.1"
    port: 50051
    index: 0
logging:
  level: "info"
  format: "json"
mpc:
  threshold: 2
  total_participants: 3
  key_share_file: "participant/key_share_1.json"
"#;

        let config: SignServiceConfig = serde_yaml::from_str(yaml_content).unwrap();
        assert_eq!(config.server.sse.host, "127.0.0.1");
        assert_eq!(config.server.sse.port, 8080);
        assert_eq!(config.server.participant.index, 0);
        assert_eq!(config.mpc.threshold, 2);
    }

    #[test]
    fn test_config_conversion() {
        let yaml_content = r#"
server:
  sse:
    host: "127.0.0.1"
    port: 8080
    cors_origins: ["http://localhost:3000"]
  participant:
    host: "127.0.0.1"
    port: 50051
    index: 1
logging:
  level: "info"
  format: "json"
mpc:
  threshold: 2
  total_participants: 3
  key_share_file: "participant/key_share_1.json"
"#;

        let config: SignServiceConfig = serde_yaml::from_str(yaml_content).unwrap();
        let sse_config = config.to_sse_config();

        assert_eq!(sse_config.sse.host, "127.0.0.1");
        assert_eq!(sse_config.sse.port, 8080);
        assert_eq!(config.server.participant.index, 1);
        assert_eq!(config.server.participant.host, "127.0.0.1");
        assert_eq!(config.server.participant.port, 50051);
    }
}
