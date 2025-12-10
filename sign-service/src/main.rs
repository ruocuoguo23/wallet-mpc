mod config;
mod service;

use anyhow::{Context, Result};
use log::info;

use crate::config::{SignServiceConfig, setup_logging};
use crate::service::run_services;

#[tokio::main]
async fn main() -> Result<()> {
    // Get the configuration file path, default to config/sign-service.yaml
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config/sign-service.yaml".to_string());

    // Load the configuration file
    let config = SignServiceConfig::load_from_file(&config_path)
        .context("Failed to load configuration")?;

    // Set up logging
    setup_logging(&config.logging)
        .context("Failed to setup logging")?;

    info!("Sign Service starting up...");
    info!("Configuration loaded from: {}", config_path);
    info!("Gateway URL: {}", config.gateway.url);
    info!("Participant Server will start on: {}:{}", config.server.host, config.server.port);
    info!("Participant index: {}", config.server.index);
    info!("MPC configuration: threshold={}, total_participants={}", config.mpc.threshold, config.mpc.total_participants);

    // Run services (including signal handling and graceful shutdown)
    run_services(config).await?;

    info!("Sign Service shut down successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::config::SignServiceConfig;

    #[test]
    fn test_config_loading() {
        let yaml_content = r#"
gateway:
  url: "http://127.0.0.1:8080"
server:
  host: "127.0.0.1"
  port: 50051
  index: 0
logging:
  level: "info"
  format: "json"
mpc:
  threshold: 2
  total_participants: 2
  key_share_file: "participant/key_share_1.json"
"#;

        let config: SignServiceConfig = serde_yaml::from_str(yaml_content).unwrap();
        assert_eq!(config.gateway.url, "http://127.0.0.1:8080");
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 50051);
        assert_eq!(config.server.index, 0);
        assert_eq!(config.mpc.threshold, 2);
    }

    #[test]
    fn test_config_conversion() {
        let yaml_content = r#"
gateway:
  url: "http://127.0.0.1:8080"
server:
  host: "127.0.0.1"
  port: 50051
  index: 1
logging:
  level: "info"
  format: "json"
mpc:
  threshold: 2
  total_participants: 2
  key_share_file: "participant/key_share_1.json"
"#;

        let config: SignServiceConfig = serde_yaml::from_str(yaml_content).unwrap();

        assert_eq!(config.gateway.url, "http://127.0.0.1:8080");
        assert_eq!(config.server.index, 1);
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 50051);
    }
}
