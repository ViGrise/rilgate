use tower_http::{
  classify::{ServerErrorsAsFailures, SharedClassifier},
  trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer},
};
use tracing::Level;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub fn setup_tracing() {
  let env_filter_layer =
    EnvFilter::try_from_default_env().unwrap_or_else(|_| "rilgate=debug,tower_http=info".into());
  let formatting_layer = fmt::layer().compact();

  tracing_subscriber::registry()
    .with(env_filter_layer)
    .with(formatting_layer)
    .init();
}

pub fn trace_layer() -> TraceLayer<SharedClassifier<ServerErrorsAsFailures>> {
  TraceLayer::new_for_http()
    .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
    .on_request(DefaultOnRequest::new().level(Level::INFO))
    .on_response(DefaultOnResponse::new().level(Level::INFO))
}
