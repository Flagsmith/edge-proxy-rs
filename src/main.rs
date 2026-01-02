use edge_proxy::config::logging::setup_logging;
use edge_proxy::config::settings::get_settings;
use edge_proxy::routes::create_router;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tracing::info;

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
