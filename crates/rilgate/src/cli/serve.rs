use anyhow::{Context, Result};
use rilgate_config::Config;
use tokio::net::TcpListener;

pub async fn run(config: Config) -> Result<()> {
  let router = crate::app::router(config.clone()).await?;
  let listener = TcpListener::bind(&config.listen_address)
    .await
    .context("Failed to bind listen address")?;
  tracing::info!("Starting Rilgate on {}", listener.local_addr()?);
  axum::serve(
    listener,
    router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
  )
  .with_graceful_shutdown(crate::shutdown::shutdown_signal())
  .await
  .context("Gateway server failed")
}
