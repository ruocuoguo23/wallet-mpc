use log::{debug, error};
use serde::Deserialize;
use std::env;
use std::fmt;

#[derive(Debug)]
pub enum ConfigError {
    InvalidEnvVar(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::InvalidEnvVar(msg) => write!(f, "Invalid environment variable: {}", msg),
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub sse: SSEConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SSEConfig {
    pub host: String,
    pub port: u16,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        debug!("Loading SSE configuration from environment variables");

        let sse_host = env::var("SSE_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let sse_port = env::var("SSE_PORT")
            .unwrap_or_else(|_| "8080".to_string())
            .parse()
            .map_err(|_| {
                let err =
                    ConfigError::InvalidEnvVar("Expected SSE_PORT to be a number".to_string());
                error!("Invalid SSE_PORT configuration: {}", err);
                err
            })?;

        let config = AppConfig {
            sse: SSEConfig {
                host: sse_host,
                port: sse_port,
            },
        };

        Ok(config)
    }
}
