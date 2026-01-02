use super::settings::LoggingSettings;
use tracing::Level;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

pub fn setup_logging(settings: &LoggingSettings) {
    let log_level = match settings.log_level.to_uppercase().as_str() {
        "CRITICAL" | "ERROR" => Level::ERROR,
        "WARNING" | "WARN" => Level::WARN,
        "INFO" => Level::INFO,
        "DEBUG" => Level::DEBUG,
        "TRACE" => Level::TRACE,
        _ => Level::INFO,
    };

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(log_level.as_str()));

    let subscriber = tracing_subscriber::registry().with(env_filter);

    match settings.log_format.as_str() {
        "json" => {
            let fmt_layer = fmt::layer().json();
            subscriber.with(fmt_layer).init();
        }
        _ => {
            // "generic" format
            let fmt_layer = fmt::layer()
                .with_ansi(settings.use_colors)
                .with_target(false);
            subscriber.with(fmt_layer).init();
        }
    }
}
