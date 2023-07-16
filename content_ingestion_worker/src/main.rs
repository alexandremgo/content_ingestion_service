use content_ingestion_worker::{
    configuration::get_configuration,
    startup::Application,
    telemetry::{get_tracing_subscriber, init_tracing_subscriber},
};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let tracing_subscriber = get_tracing_subscriber(
        "content_ingestion_worker".into(),
        "info".into(),
        std::io::stdout,
    );
    init_tracing_subscriber(tracing_subscriber);

    // Panics if the configuration can't be read
    let configuration = get_configuration().expect("Failed to read configuration.");

    let application = match Application::build(configuration).await {
        Ok(application) => application,
        Err(error) => panic!("Failed to build application: {:?}", error),
    };

    // To force the shutdown of the application running as an infinite loop
    // TODO: use it with shutdown signal ? Or put it as optional.
    let cancel_token = CancellationToken::new();
    let cloned_cancel_token = cancel_token.clone();

    application
        .run_until_stopped(cloned_cancel_token)
        .await
        .unwrap();

    Ok(())
}
