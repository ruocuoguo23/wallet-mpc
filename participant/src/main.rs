use participant::ParticipantServer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();

    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    log::info!("Starting MPC participant service");

    // Initialize and start the participant server using the library
    let server = ParticipantServer::with_default_config()?;
    server.start().await?;

    Ok(())
}
