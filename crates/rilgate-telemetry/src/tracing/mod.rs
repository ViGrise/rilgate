use tower_http::{
  classify::{ServerErrorsAsFailures, SharedClassifier},
  trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::Level;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub fn setup_tracing() {
  // Per-request diagnostics live at DEBUG, so the default stays at INFO. A
  // default of rilgate=debug re-enables them and measured a 61% throughput loss
  // on the challenge path; raise it with RUST_LOG when they are wanted.
  let env_filter_layer =
    EnvFilter::try_from_default_env().unwrap_or_else(|_| "rilgate=info,tower_http=info".into());
  let formatting_layer = fmt::layer().compact();

  tracing_subscriber::registry()
    .with(env_filter_layer)
    .with(formatting_layer)
    .init();
}

/// Per-request HTTP tracing.
///
/// These are per-request diagnostics rather than lifecycle events, so they are
/// DEBUG: at INFO the layer emits a span plus two events for every request,
/// which measured a 61% throughput loss on the challenge path and roughly
/// 666MB of log output per 15 seconds under load. Set `RUST_LOG=tower_http=debug`
/// to turn them back on.
pub fn trace_layer() -> TraceLayer<SharedClassifier<ServerErrorsAsFailures>> {
  TraceLayer::new_for_http()
    .make_span_with(DefaultMakeSpan::new().level(Level::DEBUG))
    .on_request(DefaultOnRequest::new().level(Level::DEBUG))
    .on_response(DefaultOnResponse::new().level(Level::DEBUG))
}
