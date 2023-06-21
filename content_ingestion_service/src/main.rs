use content_ingestion_service::{
    configuration::get_configuration,
    startup::Application,
    telemetry::{get_tracing_subscriber, init_tracing_subscriber},
};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let tracing_subscriber = get_tracing_subscriber(
        "content_ingestion_service".into(),
        "info".into(),
        std::io::stdout,
    );
    init_tracing_subscriber(tracing_subscriber);

    // Panics if the configuration can't be read
    let configuration = get_configuration().expect("Failed to read configuration.");

    let application = match Application::build(configuration, None).await {
        Ok(application) => application,
        Err(error) => panic!("Failed to build application: {:?}", error),
    };

    application.run_until_stopped().await?;
    Ok(())
}
