use super::{
  dto::NextStep,
  handlers::{next_presentation, next_step},
};
use crate::{
  auth::cookies::{cookie_value, issued_cookie_header},
  runtime::state::AppState,
  transport::{
    context::request_host,
    errors::ApiError,
    headers::NO_STORE,
    response::{Response, json, response},
  },
};
use http::{
  HeaderMap, HeaderValue, StatusCode, Uri,
  header::{CACHE_CONTROL, LOCATION, RETRY_AFTER, SET_COOKIE},
};
use rilgate_challenge::{
  authorization::{NewGrant, binding::BindingInput},
  challenge::Presentation,
};
use rilgate_challenge_hooks::{
  Outcome, VerifyValues, definition::Presentation as HookPresentation,
};
use rilgate_protocol::{ChallengeStep, PowStep};
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;
#[derive(Deserialize)]
pub struct HookVerifyRequest {
  pub presentation_token: String,
  pub result: String,
}

#[derive(Serialize)]
struct HookVerifyResponse {
  cookie_set: bool,
  #[serde(skip_serializing_if = "Option::is_none")]
  next: Option<NextStep>,
}

pub async fn verify(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  request: HookVerifyRequest,
  binding: &BindingInput<'_>,
) -> Result<Response, ApiError> {
  let presentation = state
    .pow
    .verify_presentation(&request.presentation_token, &request_host(headers, uri))
    .map_err(ApiError::proof)?;
  let name = hook_name(&presentation)?;
  if !(1..=8192).contains(&request.result.len()) {
    return Err(ApiError::InvalidRequest(
      "result must contain 1 to 8192 bytes".into(),
    ));
  }
  match verify_result(
    state,
    headers,
    uri,
    binding,
    &presentation,
    name,
    &request.result,
  )
  .await
  {
    Outcome::Pass => {
      let issued = grant(state, headers, uri, binding, &presentation, name)?;
      let next = next_step(state, headers, uri, &presentation, &issued.grants)?;
      let mut out = json(
        StatusCode::OK,
        &HookVerifyResponse {
          cookie_set: true,
          next,
        },
      );
      out.headers_mut().insert(
        SET_COOKIE,
        issued_cookie_header(&state.cfg, &issued.token, issued.expires_at)
          .map_err(ApiError::Internal)?,
      );
      out.headers_mut().insert(CACHE_CONTROL, NO_STORE);
      Ok(out)
    }
    Outcome::Fail => Err(ApiError::HookRejected),
    Outcome::Unavailable => Err(ApiError::HookUnavailable(fallback(
      state,
      headers,
      uri,
      &presentation,
      name,
    )?)),
  }
}
fn hook_name(p: &Presentation) -> Result<&str, ApiError> {
  match p.presented() {
    ChallengeStep::Hook(hook) => Ok(&hook.name),
    _ => Err(ApiError::Unauthorized(
      "Presentation step is not a challenge hook".into(),
    )),
  }
}
async fn verify_result(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  binding: &BindingInput<'_>,
  presentation: &Presentation,
  name: &str,
  result: &str,
) -> Outcome {
  let locale = crate::web::pages::negotiate_locale(&state.web.current(), headers, uri);
  state
    .hooks
    .verify(
      name,
      &VerifyValues {
        challenge_id: presentation.challenge_id.as_deref().unwrap_or_default(),
        host: &presentation.host,
        locale: &locale,
        client_ip: binding.client_ip,
        user_agent: headers
          .get(http::header::USER_AGENT)
          .and_then(|v| v.to_str().ok())
          .unwrap_or_default(),
        result,
      },
    )
    .await
}
fn grant(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  binding: &BindingInput<'_>,
  presentation: &Presentation,
  name: &str,
) -> Result<rilgate_challenge::authorization::IssuedAuthToken, ApiError> {
  let now = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map_err(|e| ApiError::Internal(e.into()))?
    .as_secs() as i64;
  let issued = state
    .pow
    .issue_access(
      request_host(headers, uri),
      NewGrant::Hook {
        name: name.into(),
        issued_at: now,
      },
      binding,
      cookie_value(headers, &state.cfg.pow.auth_cookie_name),
    )
    .map_err(ApiError::proof)?;
  state.metrics.challenge_steps_completed[presentation.step][1].fetch_add(1, Ordering::Relaxed);
  if issued.network_merged {
    state
      .metrics
      .auth_network_merges
      .fetch_add(1, Ordering::Relaxed);
  }
  Ok(issued)
}
fn fallback(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  p: &Presentation,
  name: &str,
) -> Result<Option<NextStep>, ApiError> {
  let Some((algorithm, difficulty)) = state.cfg.pow.hooks[name].fallback else {
    return Ok(None);
  };
  let mut steps = p.steps.clone();
  steps[p.step] = ChallengeStep::Pow(PowStep {
    algorithm,
    difficulty,
    verification: state.pow.verification(algorithm),
    fallback_for: Some(name.into()),
  });
  let issued = state
    .pow
    .issue_presentation(
      p.host.clone(),
      p.path.clone(),
      p.rule.clone(),
      p.weight,
      steps,
      p.step,
    )
    .map_err(ApiError::Internal)?;
  state.hooks.fallback(name);
  Ok(Some(next_presentation(state, headers, uri, issued)?))
}
pub async fn callback(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  binding: &BindingInput<'_>,
) -> Result<Response, ApiError> {
  let query: Vec<_> =
    url::form_urlencoded::parse(uri.query().unwrap_or_default().as_bytes()).collect();
  let token = query
    .iter()
    .find(|(key, _)| key == "state")
    .map(|(_, v)| v.as_ref())
    .filter(|v| !v.is_empty() && v.len() <= 16384)
    .ok_or_else(|| ApiError::InvalidRequest("Missing or oversized state".into()))?;
  let p = state
    .pow
    .verify_presentation(token, &request_host(headers, uri))
    .map_err(ApiError::proof)?;
  let redirect = match p.presented() {
    ChallengeStep::Hook(hook) => {
      state
        .hooks
        .set
        .hooks
        .get(&hook.name)
        .and_then(|h| match &h.presentation {
          HookPresentation::Redirect { result_param, .. } => {
            Some((hook.name.as_str(), result_param.as_str()))
          }
          _ => None,
        })
    }
    _ => None,
  };
  let (name, param) = redirect
    .ok_or_else(|| ApiError::InvalidRequest("State is not a redirect challenge hook".into()))?;
  let result = query
    .iter()
    .find(|(key, _)| key == param)
    .map(|(_, v)| v.as_ref())
    .filter(|v| (1..=8192).contains(&v.len()))
    .ok_or_else(|| ApiError::InvalidRequest("Missing hook result".into()))?;
  let outcome = verify_result(state, headers, uri, binding, &p, name, result).await;
  let mut out = match outcome {
    Outcome::Unavailable if state.cfg.pow.hooks[name].fallback.is_none() => {
      let (html, locale, policy) =
        crate::web::pages::render_page(state, "pages/error.html.jinja", "", "fast", headers, uri);
      let mut out = crate::proxy::gate::page_response(
        StatusCode::SERVICE_UNAVAILABLE,
        html,
        locale,
        false,
        policy,
      );
      out
        .headers_mut()
        .insert(RETRY_AFTER, HeaderValue::from_static("1"));
      out
    }
    _ => {
      let mut out = response(StatusCode::FOUND, "");
      out.headers_mut().insert(
        LOCATION,
        HeaderValue::from_str(&state.pow.return_path(&p))
          .map_err(|e| ApiError::Internal(e.into()))?,
      );
      if outcome == Outcome::Pass {
        let issued = grant(state, headers, uri, binding, &p, name)?;
        out.headers_mut().insert(
          SET_COOKIE,
          issued_cookie_header(&state.cfg, &issued.token, issued.expires_at)
            .map_err(ApiError::Internal)?,
        );
      }
      out
    }
  };
  out.headers_mut().insert(CACHE_CONTROL, NO_STORE);
  Ok(out)
}
