use common::telemetry::{get_tracing_subscriber, init_tracing_subscriber};
use once_cell::sync::Lazy;
use tracing::error;

// Ensures that the `tracing` stack is only initialized once using `once_cell`
static TRACING: Lazy<()> = Lazy::new(|| {
    let default_filter_level = "info".to_string();
    let subscriber_name = "test".to_string();

    // We cannot assign the output of `get_tracing_subscriber` to a variable based on the value of `TEST_LOG`
    // because the sink is part of the type returned by `get_tracing_subscriber`, therefore they are not the
    // same type. The easiest is to have 2 code branches: one with `stdout`, and one `sink`.
    if std::env::var("TEST_LOG").is_ok() {
        let subscriber =
            get_tracing_subscriber(subscriber_name, default_filter_level, std::io::stdout);
        init_tracing_subscriber(subscriber);
    } else {
        let subscriber =
            get_tracing_subscriber(subscriber_name, default_filter_level, std::io::sink);
        init_tracing_subscriber(subscriber);
    };
});

/// Initializes the tracing system for the integration tests
pub fn init_test() {
    // The first time `initialize` is invoked the code in `TRACING` is executed.
    // All other invocations will instead skip execution.
    Lazy::force(&TRACING);

    // Custom panic to catch, display and exit on panics from a different thread
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        error!("panic: {}", info);
        default_panic(info);
        std::process::exit(1);
    }));
}
