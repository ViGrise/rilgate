use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub fn setup_tracing() {
  // Request diagnostics are opt-in; lifecycle and aggregate warnings remain visible at INFO.
  let env_filter_layer =
    EnvFilter::try_from_default_env().unwrap_or_else(|_| "rilgate=info,pingora=error".into());
  if std::env::var("RILGATE_LOG_FORMAT").as_deref() == Ok("json") {
    tracing_subscriber::registry()
      .with(env_filter_layer)
      .with(fmt::layer().json())
      .init();
  } else {
    tracing_subscriber::registry()
      .with(env_filter_layer)
      .with(fmt::layer().compact())
      .init();
  }
}
