use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub fn setup_tracing() {
  // Request diagnostics are opt-in; lifecycle and aggregate warnings remain visible at INFO.
  let env_filter_layer =
    EnvFilter::try_from_default_env().unwrap_or_else(|_| "rilgate=info,pingora=error".into());
  let formatting_layer = fmt::layer().compact();

  tracing_subscriber::registry()
    .with(env_filter_layer)
    .with(formatting_layer)
    .init();
}
