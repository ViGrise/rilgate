use super::dto::Readiness;
use crate::{
  runtime::state::AppState,
  transport::response::{Response, json},
};
use http::StatusCode;
use rilgate_config::PowAlgorithm;

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
    .policy
    .challenges()
    .expect("validated policy")
    .iter()
    .any(|(_, selection)| {
      selection
        .algorithm
        .unwrap_or(state.cfg.pow.default_algorithm)
        == PowAlgorithm::RandomX
    });
  let timeout = state.cfg.server.readiness_timeout;
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
