use super::dto::*;
use crate::auth::cookies::cookie_value;
use crate::{
  runtime::state::AppState,
  transport::{
    context::request_host,
    errors::ApiError,
    response::{Response, json, response},
  },
};
use http::{
  HeaderMap, HeaderValue, StatusCode, Uri,
  header::{LOCATION, SET_COOKIE},
};
use rilgate_challenge::{authorization::binding::BindingInput, challenge::RefreshRedemption};
use rilgate_protocol::{PowAlgorithm, PowVerification};
use std::sync::atomic::Ordering;

pub fn client_config(state: &AppState) -> Response {
  let snapshot = state.web.current();
  let prefix = &snapshot.config.base_path;
  json(
    StatusCode::OK,
    &ClientConfigResponse {
      algorithms: state
        .pow
        .algorithm_descriptors()
        .into_iter()
        .map(|descriptor| super::dto::ClientAlgorithm {
          verification: state.pow.verification(descriptor.id),
          descriptor,
        })
        .collect(),
      internal_prefix: prefix.clone(),
      solver_module_url: snapshot.asset_url(&state.cfg.pow.browser.solver_module_path),
      websocket_url: format!("{prefix}/api/ws"),
    },
  )
}
pub fn session(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  binding: &BindingInput<'_>,
) -> Response {
  let authenticated = cookie_value(headers, &state.cfg.pow.auth_cookie_name).is_some_and(|token| {
    state
      .pow
      .verify_auth_token(token, &request_host(headers, uri), binding)
      .is_ok()
  });
  json(StatusCode::OK, &SessionResponse { authenticated })
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
  binding: &BindingInput<'_>,
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
  let cookie = if request.set_cookie {
    Some(exchange_cookie(state, headers, uri, &receipt, binding)?)
  } else {
    None
  };
  let mut out = json(
    StatusCode::OK,
    &VerifyResponse {
      receipt,
      cookie_set: cookie.as_ref().map(|_| true),
    },
  );
  if let Some(cookie) = cookie {
    out.headers_mut().insert(SET_COOKIE, cookie);
  }
  Ok(out)
}
pub fn exchange(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  request: ExchangeRequest,
  binding: &BindingInput<'_>,
) -> Result<Response, ApiError> {
  let cookie = exchange_cookie(state, headers, uri, &request.receipt, binding)?;
  let mut out = json(StatusCode::OK, &ExchangeResponse { status: "ok" });
  out.headers_mut().insert(SET_COOKIE, cookie);
  Ok(out)
}
fn exchange_cookie(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  receipt: &str,
  binding: &BindingInput<'_>,
) -> Result<HeaderValue, ApiError> {
  let issued = state
    .pow
    .exchange_receipt_with_claims(
      receipt,
      &request_host(headers, uri),
      binding,
      cookie_value(headers, &state.cfg.pow.auth_cookie_name),
    )
    .map_err(ApiError::proof)?;
  if issued.network_merged {
    state
      .metrics
      .auth_network_merges
      .fetch_add(1, Ordering::Relaxed);
  }
  crate::auth::cookies::issued_cookie_header(&state.cfg, &issued.token, issued.claims.expires_at)
    .map_err(ApiError::Internal)
}
pub fn refresh(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  binding: &BindingInput<'_>,
  head: bool,
) -> Result<Response, ApiError> {
  let token = url::form_urlencoded::parse(uri.query().unwrap_or_default().as_bytes())
    .find(|(key, _)| key == "presentation_token")
    .map(|(_, value)| value.into_owned())
    .filter(|token| token.len() <= 16384)
    .ok_or_else(|| ApiError::InvalidRequest("Missing or oversized presentation_token".into()))?;
  let host = request_host(headers, uri);
  let now = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap_or_default()
    .as_secs() as i64;
  match state
    .pow
    .redeem_refresh(
      &token,
      &host,
      binding,
      now,
      cookie_value(headers, &state.cfg.pow.auth_cookie_name),
    )
    .map_err(ApiError::proof)?
  {
    RefreshRedemption::Granted {
      auth_token,
      path,
      network_merged,
    } => {
      if network_merged {
        state
          .metrics
          .auth_network_merges
          .fetch_add(1, Ordering::Relaxed);
      }
      let cookie = crate::auth::cookies::auth_cookie_header(
        &state.pow,
        &state.cfg,
        &auth_token,
        &host,
        binding,
      )
      .map_err(ApiError::Internal)?;
      let mut out = response(StatusCode::SEE_OTHER, "");
      out.headers_mut().insert(SET_COOKIE, cookie);
      out.headers_mut().insert(
        LOCATION,
        HeaderValue::from_str(&path).map_err(|error| ApiError::Internal(error.into()))?,
      );
      Ok(out)
    }
    RefreshRedemption::Wait { seconds } => {
      let (html, locale, policy) =
        crate::web::pages::render_refresh_page(state, &token, seconds, headers, uri);
      Ok(crate::proxy::gate::page_response(
        state.cfg.web.responses.challenge_status,
        html,
        locale,
        head,
        policy,
      ))
    }
  }
}
pub fn websocket_admission(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
) -> Result<rilgate_protocol::claims::PresentationClaims, ApiError> {
  if state.pow.verification(PowAlgorithm::RandomX) != PowVerification::PoolShare {
    return Err(ApiError::InvalidRequest(
      "WebSocket Stratum pool verification is disabled".into(),
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
      "WebSocket verification is only valid for randomx challenges".into(),
    ));
  }
  Ok(presentation)
}
