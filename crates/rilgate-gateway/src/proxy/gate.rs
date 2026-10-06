use axum::{
  body::Body,
  extract::State,
  http::{
    Method, Request, StatusCode,
    header::{CACHE_CONTROL, CONTENT_TYPE},
  },
  response::{IntoResponse, Response},
};

use crate::{
  auth::cookies::cookie_value,
  runtime::state::AppState,
  transport::{context::request_host, errors::ApiError},
  web::pages::{render_challenge_page, render_page},
};
use rilgate_policy::PolicyAction;
use rilgate_protocol::PowAlgorithm;

pub async fn gate(
  State(state): State<AppState>,
  request: Request<Body>,
) -> Result<Response, ApiError> {
  if is_gateway_internal_path(request.uri().path(), &state.cfg.web.base_path) {
    return Ok(StatusCode::NOT_FOUND.into_response());
  }

  let decision = state
    .pow
    .evaluate_request(request.headers(), request.uri().path());

  match decision.action {
    PolicyAction::Allow => {
      return forward(&state, request).await;
    }

    PolicyAction::Deny => {
      tracing::debug!(
        rule = %decision.rule,
        weight = decision.weight,
        path = %request.uri().path(),
        "Request denied by PoW policy"
      );

      let (html, locale) = render_page(
        &state,
        "pages/denied.html",
        "",
        "fast",
        request.headers(),
        request.uri(),
      );
      return Ok(page_response(
        StatusCode::FORBIDDEN,
        html,
        locale,
        request.method() == Method::HEAD,
      ));
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

  let host = request_host(request.headers(), request.uri());
  let authenticated = cookie_value(request.headers(), &state.cfg.pow.auth_cookie_name)
    .and_then(|token| state.pow.verify_auth_token(&token, &host).ok())
    .is_some_and(|claims| {
      claims.enforce == state.pow.enforcement()
        && claims.algorithm == decision.algorithm.expect("challenge algorithm resolved")
        && claims.difficulty >= required_difficulty
    });

  if authenticated {
    return forward(&state, request).await;
  }

  if request.method() == Method::GET || request.method() == Method::HEAD {
    let presentation = state
      .pow
      .issue_presentation(host, request.uri().path().to_string(), &decision)
      .map_err(ApiError::Internal)?;

    tracing::debug!(
      rule = %decision.rule,
      weight = decision.weight,
      algorithm = ?decision.algorithm,
      difficulty = required_difficulty,
      path = %request.uri().path(),
      "Presenting proof-of-work challenge"
    );

    let algorithm = match decision.algorithm {
      Some(PowAlgorithm::Fast) => "fast",
      Some(PowAlgorithm::RandomX) => "randomx",
      None => "fast",
    };

    let (html, locale) = render_challenge_page(
      &state,
      &presentation,
      algorithm,
      request.headers(),
      request.uri(),
    );

    return Ok(page_response(
      StatusCode::OK,
      html,
      locale,
      request.method() == Method::HEAD,
    ));
  }

  Ok(
    (
      StatusCode::UNAUTHORIZED,
      "Proof of work is required before sending this request.",
    )
      .into_response(),
  )
}

fn is_gateway_internal_path(path: &str, prefix: &str) -> bool {
  path == prefix
    || path
      .strip_prefix(prefix)
      .is_some_and(|rest| rest.starts_with('/'))
}

fn page_response(status: StatusCode, html: String, locale: String, head: bool) -> Response {
  let mut response = (
    status,
    [
      (CONTENT_TYPE, "text/html; charset=utf-8"),
      (CACHE_CONTROL, "no-store"),
      (axum::http::header::VARY, "Accept-Language, Cookie"),
    ],
    if head { String::new() } else { html },
  )
    .into_response();
  response.headers_mut().insert(
    axum::http::header::CONTENT_LANGUAGE,
    locale.parse().expect("Validated locale"),
  );
  response
}

async fn forward(state: &AppState, request: Request<Body>) -> Result<Response, ApiError> {
  let html_request = matches!(*request.method(), Method::GET | Method::HEAD);
  let head = request.method() == Method::HEAD;
  let headers = request.headers().clone();
  let uri = request.uri().clone();
  match state.proxy.forward(request).await {
    Ok(response) => Ok(response),
    Err(error) if html_request => {
      let status = error.into_response().status();
      let (html, locale) = render_page(state, "pages/error.html", "", "fast", &headers, &uri);
      Ok(page_response(status, html, locale, head))
    }
    Err(error) => Err(error),
  }
}
