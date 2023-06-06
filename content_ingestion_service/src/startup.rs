use actix_web::{
    dev::Server,
    web::{self, Data},
    App, HttpServer,
};
use std::net::TcpListener;
use tracing_actix_web::TracingLogger;

use crate::{
    configuration::Settings,
    // routes::health_check,
};

/// Holds the newly built server and its port
pub struct Application {
    port: u16,
    server: Server,
}

impl Application {
    pub async fn build(configuration: Settings) -> Result<Self, std::io::Error> {
        let address = format!(
            "{}:{}",
            configuration.application.host, configuration.application.port
        );
        let listener = TcpListener::bind(address)?;
        let port = listener.local_addr().unwrap().port();
        let server = run(
            listener,
        )?;

        Ok(Self { port, server })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// This function only returns when the application is stopped
    pub async fn run_until_stopped(self) -> Result<(), std::io::Error> {
        self.server.await
    }
}

/// listener: the consumer binds their own port
///
/// TracingLogger middleware: helps collecting telemetry data.
/// It generates a unique identifier for each incoming request: `request_id`.
pub fn run(
    listener: TcpListener,
) -> Result<Server, std::io::Error> {
    // `move` to capture `connection` from the surrounding environment
    let server = HttpServer::new(move || {
        App::new()
            .wrap(TracingLogger::default())
            // .route("/health_check", web::get().to(health_check))
            // .route("/ingest_document", web::post().to(publish_newsletter))
    })
    .listen(listener)?
    .run();

    // No await
    Ok(server)
}
