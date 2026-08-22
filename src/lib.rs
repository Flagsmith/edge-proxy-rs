pub mod cache;
pub mod config;
pub mod environments;
pub mod error;
pub mod models;
pub mod routes;
pub mod services;
pub mod state;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use tracing::{info, warn};

use crate::config::settings::AppSettings;
use crate::routes::create_router;

const DEFAULT_HOST: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

/// Run the proxy to completion: start polling, load the initial
/// environment data, and serve until the process is stopped.
///
/// In the library rather than main.rs so a separately distributed binary
/// could compose the proxy without forking main; nothing besides main.rs
/// calls it today.
pub async fn run(settings: AppSettings) -> anyhow::Result<()> {
    info!("Starting Edge Proxy server...");
    info!("API URL: {}", settings.api_url);
    info!(
        "Polling frequency: {}s",
        settings.api_poll_frequency_seconds
    );

    // serde defaults environment_key_pairs, so a typo'd field name parses
    // as an empty set and the proxy would report healthy while rejecting
    // every request — make that state loud.
    if settings.environment_key_pairs.is_empty() {
        warn!(
            "No environments configured: environment_key_pairs is empty or \
             missing, so every request will be rejected with 401"
        );
    }

    let (app, environment_service) = create_router(settings.clone());

    let polling_service = environment_service.clone();
    tokio::spawn(async move {
        polling_service.poll_environments().await;
    });

    info!("Loading initial environment data...");
    environment_service.refresh_environment_caches().await;

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
