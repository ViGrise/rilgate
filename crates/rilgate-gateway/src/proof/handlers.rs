use super::dto::*;
use axum::{
  Json,
  extract::{Query, Request, State, WebSocketUpgrade},
  http::{
    HeaderMap, StatusCode, Uri,
    header::{CACHE_CONTROL, SET_COOKIE},
  },
  middleware::Next,
  response::{IntoResponse, Response},
};

use crate::{
  proof::websocket,
  runtime::state::AppState,
  transport::{context::request_host, errors::ApiError},
};
use rilgate_protocol::{ChallengeResponse, PowEnforcement};
pub async fn client_config(State(state): State<AppState>) -> Json<ClientConfigResponse> {
  let snapshot = state.web.current();
  let prefix = &snapshot.config.base_path;
  Json(ClientConfigResponse {
    algorithms: state.pow.algorithm_descriptors(),
    enforce: state.pow.enforcement(),
    internal_prefix: prefix.clone(),
    solver_module_url: crate::web::pages::resolve_gateway_asset(
      prefix,
      &state.cfg.pow.browser.solver_module_path,
    ),
    websocket_url: format!("{prefix}/api/ws"),
  })
}

pub async fn challenge(
  State(state): State<AppState>,
  headers: HeaderMap,
  uri: Uri,
  Json(request): Json<ChallengeRequest>,
) -> Result<Json<ChallengeResponse>, ApiError> {
  let host = request_host(&headers, &uri);

  state
    .pow
    .issue_challenge(&request.presentation_token, &host)
    .await
    .map(Json)
    .map_err(ApiError::proof)
}

pub async fn verify(
  State(state): State<AppState>,
  headers: HeaderMap,
  uri: Uri,
  Json(request): Json<VerifyRequest>,
) -> Result<Json<VerifyResponse>, ApiError> {
  let host = request_host(&headers, &uri);

  let receipt = state
    .pow
    .verify_challenge(&request.challenge_token, &request.nonce, &host)
    .await
    .map_err(ApiError::proof)?;

  Ok(Json(VerifyResponse { receipt }))
}

pub async fn exchange(
  State(state): State<AppState>,
  headers: HeaderMap,
  uri: Uri,
  Json(request): Json<ExchangeRequest>,
) -> Result<Response, ApiError> {
  let token = state
    .pow
    .exchange_receipt(&request.receipt, &request_host(&headers, &uri))
    .map_err(ApiError::proof)?;

  let cookie = crate::auth::cookies::auth_cookie_header(
    &state.pow,
    &state.cfg,
    &token,
    &request_host(&headers, &uri),
  )
  .map_err(ApiError::Internal)?;

  Ok(
    (
      StatusCode::OK,
      [(SET_COOKIE, cookie)],
      Json(ExchangeResponse { status: "ok" }),
    )
      .into_response(),
  )
}

pub async fn websocket(
  ws: WebSocketUpgrade,
  State(state): State<AppState>,
  headers: HeaderMap,
  uri: Uri,
  Query(query): Query<PresentationQuery>,
) -> Result<Response, ApiError> {
  if state.pow.enforcement() != PowEnforcement::P2poolAccepted {
    return Err(ApiError::InvalidRequest(
      "WebSocket P2Pool enforcement is disabled".to_string(),
    ));
  }

  let host = request_host(&headers, &uri);
  let presentation = state
    .pow
    .verify_presentation(&query.presentation_token, &host)
    .map_err(ApiError::proof)?;

  if presentation.algorithm != rilgate_pow::PowAlgorithm::RandomX {
    return Err(ApiError::InvalidRequest(
      "WebSocket enforcement is only valid for randomx challenges".to_string(),
    ));
  }

  let slot = state.pool.acquire_slot().map_err(ApiError::proof)?;
  let task_state = state.clone();

  Ok(
    ws.max_message_size(4096)
      .max_frame_size(4096)
      .on_upgrade(move |socket| async move {
        let _slot = slot;
        websocket::handle(
          socket,
          task_state,
          presentation.difficulty,
          presentation.host,
        )
        .await;
      }),
  )
}

pub async fn no_store(request: Request, next: Next) -> Response {
  let mut response = next.run(request).await;
  response.headers_mut().insert(
    CACHE_CONTROL,
    axum::http::HeaderValue::from_static("no-store"),
  );
  response
}
