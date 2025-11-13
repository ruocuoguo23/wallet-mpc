use std::path::Path;
use std::fs;

use anyhow::{Context, Result};
use log::info;
use serde::{Deserialize, Serialize};

use sse::{AppConfig as SseAppConfig, SSEConfig};

#[derive(Debug, Deserialize, Serialize)]
pub struct SignServiceConfig {
    pub server: ServerConfig,
    pub logging: LoggingConfig,
    pub mpc: MpcConfig,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ServerConfig {
    pub sse: SseServerConfig,
    pub participant: ParticipantServerConfig,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SseServerConfig {
    pub host: String,
    pub port: u16,
    pub cors_origins: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ParticipantServerConfig {
    pub host: String,
    pub port: u16,
    pub index: u16,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LoggingConfig {
    pub level: String,
    pub format: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct MpcConfig {
    pub threshold: u16,
    pub total_participants: u16,
    pub key_share_file: String,
}

impl SignServiceConfig {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path.as_ref())
            .with_context(|| format!("Failed to read config file: {}", path.as_ref().display()))?;
        
        serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse YAML config file: {}", path.as_ref().display()))
    }

    pub fn to_sse_config(&self) -> SseAppConfig {
        SseAppConfig {
            sse: SSEConfig {
                host: self.server.sse.host.clone(),
                port: self.server.sse.port,
            },
        }
    }
}

pub fn setup_logging(config: &LoggingConfig) -> Result<()> {
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
