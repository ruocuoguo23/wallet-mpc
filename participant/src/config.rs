use anyhow::Result;
use log::{debug, error, info};
use serde::Deserialize;
use std::env;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Missing environment variable: {0}")]
    MissingEnvVar(String),
    #[error("Invalid environment variable: {0}")]
    InvalidEnvVar(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub sse: SSEConfig,
    pub participant: ParticipantConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SSEConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ParticipantConfig {
    pub host: String,
    pub port: u16,
    pub index: u16,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        debug!("Loading configuration from environment variables");

        let sse_host = env::var("SSE_HOST").unwrap_or_else(|_| "localhost".to_string());
        let sse_port = env::var("SSE_PORT")
            .unwrap_or_else(|_| "8080".to_string())
            .parse()
            .map_err(|_| {
                let err =
                    ConfigError::InvalidEnvVar("Expected SSE_PORT to be a number".to_string());
                error!("Invalid SSE_PORT configuration: {}", err);
                err
            })?;

        let participant_host = env::var("PARTICIPANT_HOST").unwrap_or_else(|_| "::1".to_string());
        let participant_port = env::var("PARTICIPANT_PORT")
            .unwrap_or_else(|_| "50051".to_string())
            .parse()
            .map_err(|_| {
                let err = ConfigError::InvalidEnvVar(
                    "Expected PARTICIPANT_PORT to be a number".to_string(),
                );
                error!("Invalid PARTICIPANT_PORT configuration: {}", err);
                err
            })?;
        let participant_index = env::var("PARTICIPANT_INDEX")
            .map_err(|_| {
                let err = ConfigError::MissingEnvVar("PARTICIPANT_INDEX is required".to_string());
                error!("Missing required environment variable: {}", err);
                err
            })?
            .parse()
            .map_err(|_| {
                let err = ConfigError::InvalidEnvVar(
                    "Expected PARTICIPANT_INDEX to be a number".to_string(),
                );
                error!("Invalid PARTICIPANT_INDEX configuration: {}", err);
                err
            })?;

        let config = AppConfig {
            sse: SSEConfig {
                host: sse_host,
                port: sse_port,
            },
            participant: ParticipantConfig {
                host: participant_host,
                port: participant_port,
                index: participant_index,
            },
        };

        info!(
            "Configuration loaded successfully - SSE: {}:{}, Participant: {}:{}",
            config.sse.host, config.sse.port, config.participant.host, config.participant.port
        );

        Ok(config)
    }

    pub fn sse_url(&self) -> String {
        format!("http://{}:{}", self.sse.host, self.sse.port)
    }

    pub fn participant_addr(&self) -> String {
        format!("{}:{}", self.participant.host, self.participant.port)
    }
}
