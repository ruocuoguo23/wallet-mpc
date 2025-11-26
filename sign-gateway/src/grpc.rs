use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use log::{error, info};
use proto::mpc::participant_client::ParticipantClient;
use proto::mpc::participant_server::{Participant, ParticipantServer};
use proto::mpc::sign_gateway_server::{SignGateway, SignGatewayServer};
use proto::mpc::{SignMessage, SignatureMessage};
use thiserror::Error;
use tokio::sync::Mutex;
use tonic::transport::{Channel, Server};
use tonic::{Request, Response, Status};

#[derive(Clone)]
pub struct SignGatewayGrpc {
    upstream: Arc<Mutex<ParticipantClient<Channel>>>,
}

impl SignGatewayGrpc {
    pub async fn new(upstream_endpoint: &str) -> Result<Self, GatewayError> {
        let client = ParticipantClient::connect(upstream_endpoint.to_string())
            .await
            .context("failed to connect to sign-service")?;
        Ok(Self {
            upstream: Arc::new(Mutex::new(client)),
        })
    }

    pub async fn serve<F>(self, addr: &str, shutdown: F) -> Result<(), GatewayError>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let addr: SocketAddr = addr
            .parse()
            .context("invalid gRPC bind address")?;
        info!("Starting SignGateway gRPC server on {}", addr);
        info!("  - Exposing Participant service (for client compatibility)");
        info!("  - Exposing SignGateway service");

        Server::builder()
            .add_service(ParticipantServer::new(self.clone()))
            .add_service(SignGatewayServer::new(self))
            .serve_with_shutdown(addr, shutdown)
            .await
            .context("gRPC server failed")?;
        Ok(())
    }
}

#[tonic::async_trait]
impl Participant for SignGatewayGrpc {
    async fn sign_tx(
        &self,
        request: Request<SignMessage>,
    ) -> Result<Response<SignatureMessage>, Status> {
        let payload = request.into_inner();
        info!(
            "[Participant] Proxying SignTx - tx_id: {} account_id: {}",
            payload.tx_id, payload.account_id
        );
        let mut client = self.upstream.lock().await;
        client
            .sign_tx(Request::new(payload))
            .await
            .map_err(|status| {
                error!("Upstream SignTx failed: {}", status.message());
                Status::unavailable("upstream sign-service unavailable")
            })
    }
}

#[tonic::async_trait]
impl SignGateway for SignGatewayGrpc {
    async fn sign_tx(
        &self,
        request: Request<SignMessage>,
    ) -> Result<Response<SignatureMessage>, Status> {
        let payload = request.into_inner();
        info!(
            "[SignGateway] Proxying SignTx - tx_id: {} account_id: {}",
            payload.tx_id, payload.account_id
        );
        let mut client = self.upstream.lock().await;
        client
            .sign_tx(Request::new(payload))
            .await
            .map_err(|status| {
                error!("Upstream SignTx failed: {}", status.message());
                Status::unavailable("upstream sign-service unavailable")
            })
    }
}

#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("{0}")]
    Anyhow(#[from] anyhow::Error),
}
