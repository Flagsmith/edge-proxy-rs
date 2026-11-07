use edge_proxy::config::settings::get_settings;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    edge_proxy::run(get_settings()?).await
}
