use sse::SseServer;
use tokio::signal;

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    // Load environment variables
    dotenv::dotenv().ok();

    // Initialize logger
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    // Create SSE Server
    let server = SseServer::with_default_config()?;
    
    // Start server in a separate task
    let server_task = tokio::spawn({
        let server_clone = server.clone();
        async move {
            server_clone.start().await
        }
    });

    // Wait for shutdown signal
    match signal::ctrl_c().await {
        Ok(()) => {
            log::info!("Received shutdown signal (Ctrl+C)");
            // Gracefully shutdown the server
            server.shutdown().await?;
        }
        Err(err) => {
            log::error!("Unable to listen for shutdown signal: {}", err);
        }
    }

    // Wait for server task to complete
    match server_task.await {
        Ok(Ok(())) => log::info!("Server shut down successfully"),
        Ok(Err(e)) => log::error!("Server error: {}", e),
        Err(e) => log::error!("Server task panicked: {}", e),
    }

    Ok(())
}
