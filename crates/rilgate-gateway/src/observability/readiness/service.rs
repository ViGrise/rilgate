use super::dto::Readiness;
use crate::{
  runtime::state::AppState,
  transport::response::{Response, json},
};
use http::StatusCode;
use rilgate_config::{PolicyAction, PowAlgorithm};
use std::time::Duration;

pub async fn check(state: &AppState) -> Response {
  let Ok(_permit) = state.probes.try_acquire() else {
    return json(
      StatusCode::SERVICE_UNAVAILABLE,
      &serde_json::json!({"ready": false, "reason": "probe in progress"}),
    );
  };
  let url = url::Url::parse(&state.cfg.upstream.url).expect("validated upstream URL");
  let needs_pool = state
    .cfg
    .pow
    .bots
    .iter()
    .map(|rule| (rule.action, &rule.challenge))
    .chain(
      state
        .cfg
        .pow
        .thresholds
        .iter()
        .map(|rule| (rule.action, &rule.challenge)),
    )
    .any(|(action, challenge)| {
      action == PolicyAction::Challenge
        && challenge
          .algorithm
          .unwrap_or(state.cfg.pow.default_algorithm)
          == PowAlgorithm::RandomX
    });
  let timeout = Duration::from_secs(state.cfg.server.readiness_timeout_seconds);
  let connect = |host: String, port| async move {
    tokio::time::timeout(
      timeout,
      tokio::net::TcpStream::connect((host.as_str(), port)),
    )
    .await
    .is_ok_and(|result| result.is_ok())
  };
  let upstream = connect(
    url.host_str().unwrap().trim_matches(['[', ']']).to_owned(),
    url.port_or_known_default().unwrap(),
  );
  let pool = async {
    if needs_pool {
      Some(connect(state.cfg.pow.p2pool.host.clone(), state.cfg.pow.p2pool.port).await)
    } else {
      None
    }
  };
  let (upstream, pool) = tokio::join!(upstream, pool);
  let ready = upstream && pool.unwrap_or(true) && !state.shutdown.is_cancelled();
  json(
    if ready {
      StatusCode::OK
    } else {
      StatusCode::SERVICE_UNAVAILABLE
    },
    &Readiness {
      ready,
      upstream,
      pool,
    },
  )
}
