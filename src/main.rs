use edge_proxy::config::logging::setup_logging;
use edge_proxy::config::settings::get_settings;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let settings = get_settings()?;

    setup_logging(&settings.logging);

    edge_proxy::run(settings).await
}
