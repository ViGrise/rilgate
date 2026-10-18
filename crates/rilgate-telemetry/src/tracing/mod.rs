use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Installs the global subscriber with an optional filter directive and JSON or
/// compact text output.
pub fn setup_tracing(filter: Option<&str>, json: bool) {
  // Request diagnostics are opt-in; lifecycle and aggregate warnings remain visible at INFO.
  let env_filter_layer = filter
    .and_then(|filter| EnvFilter::try_new(filter).ok())
    .unwrap_or_else(|| "rilgate=info,pingora=error".into());
  if json {
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
