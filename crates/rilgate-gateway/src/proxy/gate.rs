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
  gate_with_client(state, headers, uri, method, None)
}

pub fn gate_with_client(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  method: &Method,
  client_ip: Option<std::net::IpAddr>,
) -> Result<Option<Response>, ApiError> {
  let json = crate::web::negotiation::wants_json(&state.cfg.web.responses, headers, uri.path());
  let decision = state
    .pow
    .evaluate_context(&rilgate_policy::conditions::Request {
      headers,
      path: uri.path(),
      method,
      client_ip,
    });
  if decision.unresolved {
    state
      .metrics
      .policy_unresolved_decisions
      .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
  }

  match decision.action {
    PolicyAction::Allow => {
      return Ok(None);
    }

    PolicyAction::Deny => {
      tracing::debug!(
        rule = %decision.rule,
        weight = decision.weight,
        unresolved = decision.unresolved,
        path = %uri.path(),
        "Request denied by PoW policy"
      );

      let template = if json {
        "pages/denied.json.jinja"
      } else {
        "pages/denied.html.jinja"
      };
      let (html, locale) = render_page(state, template, "", "fast", headers, uri);
      return Ok(Some(formatted_response(
        StatusCode::FORBIDDEN,
        html,
        locale,
        method == Method::HEAD,
        json,
      )));
    }

    PolicyAction::Challenge => {}

    PolicyAction::Score => {
      return Err(ApiError::Internal(anyhow::anyhow!(
        "score cannot be a final policy decision"
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
    .and_then(|token| state.pow.verify_auth_token(token, &host).ok())
    .is_some_and(|claims| {
      claims.enforce == state.pow.enforcement()
        && claims.algorithm == decision.algorithm.expect("challenge algorithm resolved")
        && claims.difficulty >= required_difficulty
    });

  if authenticated {
    return Ok(None);
  }

  if json || method == Method::GET || method == Method::HEAD {
    let presentation = state
      .pow
      .issue_presentation_with_claims(host, uri.path().to_string(), &decision)
      .map_err(ApiError::Internal)?;

    tracing::debug!(
      rule = %decision.rule,
      weight = decision.weight,
      algorithm = ?decision.algorithm,
      difficulty = required_difficulty,
      unresolved = decision.unresolved,
      path = %uri.path(),
      "Presenting proof-of-work challenge"
    );

    let challenge_url =
      crate::web::pages::challenge_url(&state.cfg.web.base_path, &presentation.token);
    let (html, locale) = render_challenge_page(state, presentation, headers, uri, json, false);

    let mut out = formatted_response(
      if json {
        StatusCode::FORBIDDEN
      } else {
        StatusCode::OK
      },
      html,
      locale,
      method == Method::HEAD,
      json,
    );
    if json {
      out
        .headers_mut()
        .insert("rilgate-challenge", "required".parse().unwrap());
      out.headers_mut().insert(
        "rilgate-challenge-url",
        challenge_url
          .parse()
          .map_err(|e| ApiError::Internal(anyhow::Error::new(e)))?,
      );
    }
    return Ok(Some(out));
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
  formatted_response(status, html, locale, head, false)
}

fn formatted_response(
  status: StatusCode,
  body: String,
  locale: String,
  head: bool,
  json: bool,
) -> Response {
  let mut out = response(status, if head { String::new() } else { body });
  let headers = out.headers_mut();
  headers.insert(
    CONTENT_TYPE,
    if json {
      "application/json"
    } else {
      "text/html; charset=utf-8"
    }
    .parse()
    .unwrap(),
  );
  headers.insert(CACHE_CONTROL, "no-store".parse().unwrap());
  headers.insert(
    http::header::VARY,
    "Accept, Content-Type, X-Requested-With, Accept-Language, Cookie"
      .parse()
      .unwrap(),
  );
  headers.insert(
    http::header::CONTENT_LANGUAGE,
    locale.parse().expect("Validated locale"),
  );
  out
}
