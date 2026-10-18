use super::dto::*;
use crate::{
  runtime::state::AppState,
  transport::{
    context::request_host,
    errors::ApiError,
    response::{Response, json},
  },
};
use http::{HeaderMap, StatusCode, Uri, header::SET_COOKIE};
use rilgate_challenge::authorization::binding::BindingInput;
use rilgate_protocol::PowEnforcement;

pub fn client_config(state: &AppState) -> Response {
  let snapshot = state.web.current();
  let prefix = &snapshot.config.base_path;
  json(
    StatusCode::OK,
    &ClientConfigResponse {
      algorithms: state.pow.algorithm_descriptors(),
      enforce: state.pow.enforcement(),
      internal_prefix: prefix.clone(),
      solver_module_url: crate::web::pages::resolve_gateway_asset(
        prefix,
        &state.cfg.pow.browser.solver_module_path,
      ),
      websocket_url: format!("{prefix}/api/ws"),
    },
  )
}
pub async fn challenge(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  request: ChallengeRequest,
) -> Result<Response, ApiError> {
  let issued = state
    .pow
    .issue_challenge(&request.presentation_token, &request_host(headers, uri))
    .await
    .map_err(ApiError::proof)?;
  Ok(json(StatusCode::OK, &issued))
}
pub async fn verify(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  request: VerifyRequest,
) -> Result<Response, ApiError> {
  let receipt = state
    .pow
    .verify_challenge(
      &request.challenge_token,
      &request.nonce,
      &request_host(headers, uri),
    )
    .await
    .map_err(ApiError::proof)?;
  Ok(json(StatusCode::OK, &VerifyResponse { receipt }))
}
pub fn exchange(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  request: ExchangeRequest,
  binding: &BindingInput<'_>,
) -> Result<Response, ApiError> {
  let issued = state
    .pow
    .exchange_receipt_with_claims(&request.receipt, &request_host(headers, uri), binding)
    .map_err(ApiError::proof)?;
  let cookie =
    crate::auth::cookies::issued_cookie_header(&state.cfg, &issued.token, issued.claims.expires_at)
      .map_err(ApiError::Internal)?;
  let mut out = json(StatusCode::OK, &ExchangeResponse { status: "ok" });
  out.headers_mut().insert(SET_COOKIE, cookie);
  Ok(out)
}
pub fn websocket_admission(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
) -> Result<rilgate_protocol::claims::PresentationClaims, ApiError> {
  if state.pow.enforcement() != PowEnforcement::P2poolAccepted {
    return Err(ApiError::InvalidRequest(
      "WebSocket P2Pool enforcement is disabled".into(),
    ));
  }
  let token = url::form_urlencoded::parse(uri.query().unwrap_or_default().as_bytes())
    .find(|(key, _)| key == "presentation_token")
    .map(|(_, value)| value.into_owned())
    .ok_or_else(|| ApiError::InvalidRequest("Missing presentation_token".into()))?;
  let presentation = state
    .pow
    .verify_presentation(&token, &request_host(headers, uri))
    .map_err(ApiError::proof)?;
  if presentation.algorithm != rilgate_pow::PowAlgorithm::RandomX {
    return Err(ApiError::InvalidRequest(
      "WebSocket enforcement is only valid for randomx challenges".into(),
    ));
  }
  Ok(presentation)
}
