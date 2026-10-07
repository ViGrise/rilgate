use crate::transport::response::{Response, response};
use crate::{
  auth::cookies::cookie_value,
  runtime::state::AppState,
  transport::{context::request_host, errors::ApiError},
  web::pages::{render_challenge_page, render_page},
};
use http::{
  HeaderMap, Method, StatusCode, Uri,
  header::{CACHE_CONTROL, CONTENT_TYPE},
};
use rilgate_policy::PolicyAction;

pub fn gate(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  method: &Method,
) -> Result<Option<Response>, ApiError> {
  let decision = state.pow.evaluate_request(headers, uri.path());

  match decision.action {
    PolicyAction::Allow => {
      return Ok(None);
    }

    PolicyAction::Deny => {
      tracing::debug!(
        rule = %decision.rule,
        weight = decision.weight,
        path = %uri.path(),
        "Request denied by PoW policy"
      );

      let (html, locale) = render_page(state, "pages/denied.html", "", "fast", headers, uri);
      return Ok(Some(page_response(
        StatusCode::FORBIDDEN,
        html,
        locale,
        method == Method::HEAD,
      )));
    }

    PolicyAction::Challenge => {}

    // WEIGH rules are accumulated by PolicyEngine and should never escape as a
    // final decision. Treat it as a configuration failure instead of silently
    // challenging or allowing.
    PolicyAction::Weigh => {
      return Err(ApiError::Internal(anyhow::anyhow!(
        "WEIGH cannot be a final policy decision"
      )));
    }
  }

  let required_difficulty = decision.difficulty.ok_or_else(|| {
    ApiError::Internal(anyhow::anyhow!(
      "CHALLENGE policy has no resolved difficulty"
    ))
  })?;

  let host = request_host(headers, uri);
  let authenticated = cookie_value(headers, &state.cfg.pow.auth_cookie_name)
    .and_then(|token| state.pow.verify_auth_token(&token, &host).ok())
    .is_some_and(|claims| {
      claims.enforce == state.pow.enforcement()
        && claims.algorithm == decision.algorithm.expect("challenge algorithm resolved")
        && claims.difficulty >= required_difficulty
    });

  if authenticated {
    return Ok(None);
  }

  if method == Method::GET || method == Method::HEAD {
    let presentation = state
      .pow
      .issue_presentation_with_claims(host, uri.path().to_string(), &decision)
      .map_err(ApiError::Internal)?;

    tracing::debug!(
      rule = %decision.rule,
      weight = decision.weight,
      algorithm = ?decision.algorithm,
      difficulty = required_difficulty,
      path = %uri.path(),
      "Presenting proof-of-work challenge"
    );

    let (html, locale) = render_challenge_page(state, presentation, headers, uri);

    return Ok(Some(page_response(
      StatusCode::OK,
      html,
      locale,
      method == Method::HEAD,
    )));
  }

  Ok(Some(response(
    StatusCode::UNAUTHORIZED,
    "Proof of work is required before sending this request.",
  )))
}

pub(crate) fn page_response(
  status: StatusCode,
  html: String,
  locale: String,
  head: bool,
) -> Response {
  let mut out = response(status, if head { String::new() } else { html });
  let headers = out.headers_mut();
  headers.insert(CONTENT_TYPE, "text/html; charset=utf-8".parse().unwrap());
  headers.insert(CACHE_CONTROL, "no-store".parse().unwrap());
  headers.insert(
    http::header::VARY,
    "Accept-Language, Cookie".parse().unwrap(),
  );
  headers.insert(
    http::header::CONTENT_LANGUAGE,
    locale.parse().expect("Validated locale"),
  );
  out
}
