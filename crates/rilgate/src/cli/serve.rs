use anyhow::{Context, Result};
use rilgate_config::Config;
use tokio::net::TcpListener;

pub async fn run(config: Config) -> Result<()> {
  let gateway = crate::app::build(config.clone()).await?;
  let listener = TcpListener::bind(&config.listen_address)
    .await
    .context("Failed to bind listen address")?;
  tracing::info!("Starting Rilgate on {}", listener.local_addr()?);
  gateway
    .serve(listener, crate::shutdown::shutdown_signal())
    .await
    .context("Gateway server failed")
}
