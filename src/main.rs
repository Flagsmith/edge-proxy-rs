use edge_proxy::config::logging::setup_logging;
use edge_proxy::config::settings::get_settings;
use edge_proxy::routes::create_router;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tracing::{info, warn};

const DEFAULT_HOST: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let settings = get_settings()?;

    setup_logging(&settings.logging);

    info!("Starting Edge Proxy server...");
    info!("API URL: {}", settings.api_url);
    info!(
        "Polling frequency: {}s",
        settings.api_poll_frequency_seconds
    );

    // serde defaults these fields, so a typo'd field name parses as an
    // empty set and the proxy would report healthy while rejecting every
    // request — make that state loud.
    if settings.environment_key_pairs.is_empty() && settings.proxy_key.is_none() {
        warn!(
            "No environments configured: environment_key_pairs and proxy_key \
             are both empty or missing, so every request will be rejected \
             with 401"
        );
    }

    let (app, environment_service) = create_router(settings.clone());

    // Refreshes must never overlap: a delayed older poll finishing after a
    // newer one could restore removed environments or rotated keys. The
    // poll loop is serial, so it just has to start after the initial
    // refresh completes.
    info!("Loading initial environment data...");
    environment_service.refresh_environment_caches().await;

    let polling_service = environment_service.clone();
    tokio::spawn(async move {
        polling_service.poll_environments().await;
    });

    let usage_service = environment_service.clone();
    tokio::spawn(async move {
        usage_service.flush_usage_periodically().await;
    });

    let addr = SocketAddr::from((
        settings
            .server
            .host
            .parse::<IpAddr>()
            .unwrap_or(DEFAULT_HOST),
        settings.server.port,
    ));

    info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
