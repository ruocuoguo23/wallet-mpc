use sse::SseServer;

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    // Load environment variables
    dotenv::dotenv().ok();

    // Initialize logger
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    // Create and start SSE Server
    let server = SseServer::with_default_config()?;
    server.start().await
}
