pub mod cache;
pub mod config;
pub mod environments;
pub mod error;
pub mod models;
pub mod routes;
pub mod services;
pub mod state;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use tracing::info;

use crate::config::settings::AppSettings;
use crate::routes::create_router;

const DEFAULT_HOST: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

/// Run the proxy to completion: start polling, load the initial
/// environment data, and serve until the process is stopped.
///
/// Lives in the library so an alternative binary can compose it.
pub async fn run(settings: AppSettings) -> anyhow::Result<()> {
    info!("Starting Edge Proxy server...");
    info!("API URL: {}", settings.api_url);
    info!(
        "Polling frequency: {}s",
        settings.api_poll_frequency_seconds
    );

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
