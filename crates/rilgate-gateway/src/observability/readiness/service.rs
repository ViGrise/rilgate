use super::dto::Readiness;
use crate::{
  runtime::state::AppState,
  transport::response::{Response, json},
};
use http::StatusCode;

pub async fn check(state: &AppState) -> Response {
  let Ok(_permit) = state.probes.try_acquire() else {
    return json(
      StatusCode::SERVICE_UNAVAILABLE,
      &serde_json::json!({"ready": false, "reason": "probe in progress"}),
    );
  };
  let url = url::Url::parse(&state.cfg.upstream.url).expect("validated upstream URL");
  let needs_pool = state.cfg.pow.selects_randomx();
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
      Some(state.pool.readiness(timeout).await)
    } else {
      None
    }
  };
  let remote = async {
    if state.cfg.pow.needs_remote_verifier() {
      Some(state.pow.remote_ready(timeout).await)
    } else {
      None
    }
  };
  let (upstream, pool, remote) = tokio::join!(upstream, pool, remote);
  let pools = pool;
  let pool = pools
    .as_ref()
    .map(|pools| pools.iter().any(|(_, ready)| *ready));
  let ip_sets = state.pow.ip_sets().all_remote_loaded();
  let geoip = state.pow.geoip().all_loaded();
  let ready = upstream
    && pool.unwrap_or(true)
    && remote.unwrap_or(true)
    && ip_sets
    && geoip
    && !state.shutdown.is_cancelled();
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
      pools,
      remote,
      ip_sets,
      geoip,
    },
  )
}
