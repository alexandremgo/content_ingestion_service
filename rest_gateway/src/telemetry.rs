use tracing::subscriber::set_global_default;
use tracing::Subscriber;
use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
use tracing_log::LogTracer;
use tracing_subscriber::{fmt::MakeWriter, layer::SubscriberExt, EnvFilter, Registry};

/// Composes multiple layers into a `tracing`'s Subscriber.
///
/// The Subscriber trait exposes a variety of methods to manage every stage of the lifecycle of a Span:
/// creation, enter/exit, closure, etc.
///
/// `tracing-subscriber` allows us to combine multiple Layers to obtain the processing pipeline needed for our logs/spans.
/// It provides the Registry struct which collects and stores span data that is exposed to any layer wrapping it.
/// Registry is responsible for storing span metadata, recording relationships between spans,
/// and tracking which spans are active and which are closed.
///
/// # Arguments
/// - `name`: name of the app
/// - `fallback_env_filter`: filter level for traces if RUST_LOG env variable has not been set
/// - `sink`: to what the traces will be outputted
///
/// # Returns
/// Using `impl Subscriber` as return type to avoid having to spell out
/// the actual type of the returned subscriber, which is quite complex.
///
pub fn get_tracing_subscriber<Sink>(
    name: String,
    fallback_env_filter: String,
    sink: Sink,
) -> impl Subscriber + Send + Sync
where
    // Higher-ranked trait bound (HRTB) syntax:
    // the Sink implements the `MakeWriter` trait for all choices of the lifetime parameter `'a`
    // Check out https://doc.rust-lang.org/nomicon/hrtb.html for more details.
    Sink: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    // Falls back to printing all spans at `fallback_env_filter` level
    // if the RUST_LOG environment variable has not been set.
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(fallback_env_filter));

    // Built on top of `JsonStorageLayer` and outputs log records in "bunyan"-compatible JSON format
    let formatting_layer = BunyanFormattingLayer::new(name, sink);

    Registry::default()
        // Discards spans based on their log levels and their origins
        .with(env_filter)
        // Processes spans data and stores the associated metadata in an easy-to-consume JSON format for downstream layers.
        // It also propagates context from parent spans to their children.
        .with(JsonStorageLayer)
        .with(formatting_layer)
}

/// Registers a tracing Subscriber as the global default to process span data.
///
/// It should only be called once
pub fn init_tracing_subscriber(subscriber: impl Subscriber + Send + Sync) {
    // Redirects all `log`'s events to our subscriber
    LogTracer::init().expect("Failed to set logger");

    set_global_default(subscriber).expect("Failed to set subscriber");
}
