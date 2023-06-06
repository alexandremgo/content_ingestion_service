use content_ingestion_service::{
    configuration::get_configuration,
    startup::Application,
    telemetry::{get_tracing_subscriber, init_tracing_subscriber},
};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let tracing_subscriber = get_tracing_subscriber("content_ingestion_service".into(), "info".into(), std::io::stdout);
    init_tracing_subscriber(tracing_subscriber);

    // Panics if we can't read configuration
    let configuration = get_configuration().expect("Failed to read configuration.");

    let application = Application::build(configuration).await?;
    application.run_until_stopped().await?;
    Ok(())
}
