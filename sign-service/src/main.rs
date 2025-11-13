mod config;
mod service;

use anyhow::{Context, Result};
use log::{info, error};

use crate::config::{SignServiceConfig, setup_logging};
use crate::service::run_services;

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
    use crate::config::SignServiceConfig;

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
