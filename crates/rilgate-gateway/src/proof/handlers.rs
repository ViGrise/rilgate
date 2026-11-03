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
  header::{LOCATION, RETRY_AFTER, SET_COOKIE},
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
    Some(exchange_cookie(
      state,
      headers,
      uri,
      &receipt,
      binding,
      request.presentation_token.as_deref(),
    )?)
  } else {
    None
  };
  let mut out = json(
    StatusCode::OK,
    &VerifyResponse {
      next: cookie.as_ref().and_then(|(_, next)| next.clone()),
      receipt,
      cookie_set: cookie.as_ref().map(|_| true),
    },
  );
  if let Some((cookie, _)) = cookie {
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
  let (cookie, next) = exchange_cookie(
    state,
    headers,
    uri,
    &request.receipt,
    binding,
    request.presentation_token.as_deref(),
  )?;
  let mut out = json(StatusCode::OK, &ExchangeResponse { status: "ok", next });
  out.headers_mut().insert(SET_COOKIE, cookie);
  Ok(out)
}
fn exchange_cookie(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  receipt: &str,
  binding: &BindingInput<'_>,
  presentation_token: Option<&str>,
) -> Result<(HeaderValue, Option<NextStep>), ApiError> {
  let presentation = presentation_token.and_then(|token| {
    state
      .pow
      .verify_presentation(token, &request_host(headers, uri))
      .ok()
  });
  let fallback_for = presentation
    .as_ref()
    .and_then(|p| p.presented_pow().ok())
    .and_then(|p| p.fallback_for.clone());
  let issued = state
    .pow
    .exchange_receipt_with_claims(
      receipt,
      &request_host(headers, uri),
      binding,
      cookie_value(headers, &state.cfg.pow.auth_cookie_name),
      fallback_for,
    )
    .map_err(ApiError::proof)?;
  if issued.network_merged {
    state
      .metrics
      .auth_network_merges
      .fetch_add(1, Ordering::Relaxed);
  }
  let cookie =
    crate::auth::cookies::issued_cookie_header(&state.cfg, &issued.token, issued.expires_at)
      .map_err(ApiError::Internal)?;
  let next = if let Some(p) = presentation {
    state.metrics.challenge_steps_completed[p.step][0].fetch_add(1, Ordering::Relaxed);
    next_step(state, headers, uri, &p, &issued.grants)?
  } else {
    None
  };
  Ok((cookie, next))
}

pub(crate) fn refresh_token(uri: &Uri) -> Option<String> {
  url::form_urlencoded::parse(uri.query().unwrap_or_default().as_bytes())
    .find(|(key, _)| key == "presentation_token")
    .map(|(_, value)| value.into_owned())
    .filter(|token| token.len() <= 16384)
}

/// Answers a rate-limited refresh URL with a refresh page that opens the same URL after
/// the quota delay, so a browser without JavaScript continues on its own.
pub fn refresh_rate_limited(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  seconds: u64,
  head: bool,
) -> Option<Response> {
  let token = refresh_token(uri)?;
  let (html, locale, policy) =
    crate::web::pages::render_refresh_page(state, &token, seconds, headers, uri);
  let mut out =
    crate::proxy::gate::page_response(StatusCode::TOO_MANY_REQUESTS, html, locale, head, policy);
  out
    .headers_mut()
    .insert(RETRY_AFTER, HeaderValue::from(seconds));
  Some(out)
}

pub fn refresh(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  binding: &BindingInput<'_>,
  head: bool,
) -> Result<Response, ApiError> {
  let host = request_host(headers, uri);
  let Some(token) = refresh_token(uri) else {
    return Ok(restart_refresh(state, headers, None, &host));
  };
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
    .map_err(ApiError::proof)
  {
    Err(ApiError::Unauthorized(_)) => Ok(restart_refresh(state, headers, Some(&token), &host)),
    Err(error) => Err(error),
    Ok(redemption) => refresh_redemption(
      state, headers, uri, binding, head, &token, &host, redemption,
    ),
  }
}

/// Sends a browser that opened a refresh URL it cannot redeem back to the protected page, where
/// the gateway serves a new challenge. Browsers reach this URL only by navigation.
fn restart_refresh(
  state: &AppState,
  headers: &HeaderMap,
  token: Option<&str>,
  host: &str,
) -> Response {
  let path = token
    .and_then(|token| state.pow.refresh_return_path(token, host))
    .unwrap_or_else(|| {
      crate::transport::context::navigation_return_path(headers, host, &state.cfg.web.base_path)
    });
  crate::transport::response::see_other(&path)
}

#[allow(clippy::too_many_arguments)]
fn refresh_redemption(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  binding: &BindingInput<'_>,
  head: bool,
  token: &str,
  host: &str,
  redemption: RefreshRedemption,
) -> Result<Response, ApiError> {
  match redemption {
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
        host,
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
        crate::web::pages::render_refresh_page(state, token, seconds, headers, uri);
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
) -> Result<rilgate_challenge::challenge::Presentation, ApiError> {
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
  if presentation
    .presented_pow()
    .map_err(ApiError::proof)?
    .algorithm
    != rilgate_pow::PowAlgorithm::RandomX
  {
    return Err(ApiError::InvalidRequest(
      "WebSocket verification is only valid for randomx challenges".into(),
    ));
  }
  Ok(presentation)
}

pub(crate) fn next_step(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  p: &rilgate_challenge::challenge::Presentation,
  grants: &[rilgate_protocol::Grant],
) -> Result<Option<NextStep>, ApiError> {
  let effective = crate::proxy::gate::effective_steps(state, &p.steps);
  let Some(i) = rilgate_challenge::authorization::steps::first_unsatisfied(
    &p.steps,
    &effective,
    grants,
    &state.until_expiry,
  ) else {
    return Ok(None);
  };
  let presentation = state
    .pow
    .issue_presentation(
      p.host.clone(),
      p.path.clone(),
      p.rule.clone(),
      p.weight,
      effective,
      i,
    )
    .map_err(ApiError::Internal)?;
  Ok(Some(next_presentation(state, headers, uri, presentation)?))
}
pub(crate) fn next_presentation(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  presentation: rilgate_challenge::IssuedToken<rilgate_challenge::challenge::Presentation>,
) -> Result<NextStep, ApiError> {
  let mut next = NextStep {
    presentation_token: presentation.token.clone(),
    step: presentation.claims.step + 1,
    steps: presentation.claims.steps.len(),
    method: "pow",
    algorithm: None,
    difficulty: None,
    solver_module_url: None,
    hook: None,
  };
  match presentation.claims.presented() {
    rilgate_protocol::ChallengeStep::Pow(step) => {
      next.algorithm = Some(step.algorithm);
      next.difficulty = Some(step.difficulty);
      next.solver_module_url = Some(
        state.web.current().asset_url(
          &crate::web::pages::solver_module_path(state, step.algorithm)
            .map_err(ApiError::Internal)?,
        ),
      );
    }
    rilgate_protocol::ChallengeStep::Hook(_) => {
      next.method = "hook";
      let locale = crate::web::pages::negotiate_locale(&state.web.current(), headers, uri);
      next.hook = Some(
        crate::web::pages::public_hook(state, &presentation.claims, &presentation.token, &locale)
          .map_err(ApiError::Internal)?,
      );
    }
  }
  Ok(next)
}
