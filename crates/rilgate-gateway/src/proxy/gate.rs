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
use rilgate_challenge::authorization::binding::{BindingInput, BindingMismatch};
use rilgate_config::DecisionLog;
use rilgate_policy::{PolicyAction, PolicyDecision, conditions::Request};

struct DecisionContext<'a> {
  request_id: &'a str,
  request: &'a Request<'a>,
  action: PolicyAction,
}

fn log_decision(level: DecisionLog, decision: &PolicyDecision<'_>, context: DecisionContext<'_>) {
  let enabled = match level {
    DecisionLog::Off => false,
    DecisionLog::Deny => context.action == PolicyAction::Deny,
    DecisionLog::Challenge => {
      matches!(context.action, PolicyAction::Deny | PolicyAction::Challenge)
    }
    DecisionLog::All => true,
  };
  if !enabled {
    return;
  }
  let action = match context.action {
    PolicyAction::Allow => "allow",
    PolicyAction::Deny => "deny",
    PolicyAction::Challenge => "challenge",
    PolicyAction::Score => "score",
  };
  tracing::info!(
    request_id = context.request_id,
    client_ip = %context.request.client_ip.map(|ip| ip.to_string()).unwrap_or_else(|| "unknown".into()),
    method = %context.request.method,
    host = context.request.host,
    path = context.request.path,
    action,
    rule = decision.rule,
    weight = decision.weight,
    algorithm = ?decision.algorithm,
    difficulty = ?decision.difficulty,
    unresolved = decision.unresolved,
    "Policy decision"
  );
}

pub async fn gate(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  method: &Method,
) -> Result<Option<Response>, ApiError> {
  gate_with_client(state, headers, uri, method, None, false, "").await
}

pub async fn gate_with_client(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  method: &Method,
  client_ip: Option<std::net::IpAddr>,
  trusted_peer: bool,
  request_id: &str,
) -> Result<Option<Response>, ApiError> {
  let json = crate::web::negotiation::wants_json(&state.cfg.web.responses, headers, uri.path());
  let host = request_host(headers, uri);
  let request = rilgate_policy::conditions::Request {
    headers,
    host: &host,
    path: uri.path(),
    method,
    client_ip,
  };
  let decision = state.pow.evaluate_context(&request);
  if decision.unresolved {
    state
      .metrics
      .policy_unresolved_decisions
      .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
  }

  let log = |action| {
    log_decision(
      state.cfg.server.decision_log,
      &decision,
      DecisionContext {
        request_id,
        request: &request,
        action,
      },
    )
  };

  match decision.action {
    PolicyAction::Allow => {
      log(PolicyAction::Allow);
      return Ok(None);
    }

    PolicyAction::Deny => {
      log(PolicyAction::Deny);

      let template = if json {
        "pages/denied.json.jinja"
      } else {
        "pages/denied.html.jinja"
      };
      let (html, locale) = render_page(state, template, "", "fast", headers, uri);
      return Ok(Some(formatted_response(
        state.cfg.web.responses.deny_status,
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

  let binding = BindingInput {
    client_ip,
    headers,
    trusted_peer,
  };
  let authenticated = cookie_value(headers, &state.cfg.pow.auth_cookie_name)
    .and_then(
      |token| match state.pow.verify_auth_token(token, &host, &binding) {
        Ok(claims) => Some(claims),
        Err(error) => {
          if error.downcast_ref::<BindingMismatch>().is_some() {
            state
              .metrics
              .auth_binding_mismatches
              .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            tracing::debug!(path = %uri.path(), "Authentication cookie does not match its binding");
          }
          None
        }
      },
    )
    .is_some_and(|claims| {
      claims.enforce == state.pow.enforcement()
        && claims.algorithm == decision.algorithm.expect("challenge algorithm resolved")
        && claims.difficulty >= required_difficulty
    });

  if authenticated {
    log(PolicyAction::Allow);
    return Ok(None);
  }

  if json || method == Method::GET || method == Method::HEAD {
    let presentation = state
      .pow
      .issue_presentation_with_claims(host.clone(), uri.path().to_string(), &decision)
      .map_err(ApiError::Internal)?;

    let challenge_url =
      crate::web::pages::challenge_url(&state.cfg.web.base_path, &presentation.token);
    let open_graph = if !json && method == Method::GET {
      state
        .open_graph
        .lookup(&state.proxy, uri, &request, &state.metrics)
        .await
    } else {
      Vec::new()
    };
    let (html, locale) =
      render_challenge_page(state, presentation, headers, uri, json, false, &open_graph);

    let mut out = formatted_response(
      if json {
        StatusCode::FORBIDDEN
      } else {
        state.cfg.web.responses.challenge_status
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
    log(PolicyAction::Challenge);
    return Ok(Some(out));
  }

  log(PolicyAction::Challenge);
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
