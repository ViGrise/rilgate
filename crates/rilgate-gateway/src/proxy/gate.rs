use crate::transport::response::{Response, response};
use crate::{
  auth::cookies::cookie_value,
  runtime::state::AppState,
  transport::{context::request_host, errors::ApiError, path::policy_path},
  web::pages::{render_challenge_page, render_page, render_refresh_page},
};
use http::{
  HeaderMap, Method, StatusCode, Uri,
  header::{CACHE_CONTROL, CONTENT_TYPE},
};
use rilgate_challenge::authorization::binding::{BindingInput, BindingMismatch};
use rilgate_config::DecisionLog;
use rilgate_policy::{Lookups, PolicyAction, PolicyDecision, conditions::Request};
use rilgate_protocol::PowAlgorithm;

struct DecisionContext<'a> {
  request_id: &'a str,
  path: &'a str,
  request: &'a Request<'a>,
  action: PolicyAction,
  fallback: bool,
}

fn log_decision(level: DecisionLog, decision: &PolicyDecision, context: DecisionContext<'_>) {
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
    path = context.path,
    action,
    rule = &*decision.rule,
    weight = decision.weight,
    algorithm = ?decision.algorithm,
    difficulty = ?decision.difficulty,
    unresolved = decision.unresolved,
    fallback = context.fallback,
    "Policy decision"
  );
}

pub async fn gate(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  method: &Method,
) -> Result<Option<Response>, ApiError> {
  let canonical = policy_path(uri.path()).unwrap_or_else(|_| uri.path().to_string());
  gate_with_client(state, headers, uri, &canonical, method, None, false, "").await
}

#[allow(clippy::too_many_arguments)]
pub async fn gate_with_client(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  policy_path: &str,
  method: &Method,
  client_ip: Option<std::net::IpAddr>,
  trusted_peer: bool,
  request_id: &str,
) -> Result<Option<Response>, ApiError> {
  let json = crate::web::negotiation::wants_json(&state.cfg.web.responses, headers, uri.path());
  let host = request_host(headers, uri);
  let services = state.policy_services.load_full();
  let mut known = Lookups::NONE;
  if let (Some(honeypot), Some(ip)) = (&services.honeypot, client_ip) {
    known.honeypot = Some(honeypot.is_flagged(ip));
  }
  known.rates = state.pow.counter_values(
    &Request {
      trusted_peer,
      headers,
      host: &host,
      path: policy_path,
      method,
      client_ip,
      lookups: &known,
    },
    |name, key, selected| {
      services.counters.get(name).map(|counter| {
        if selected {
          counter.record(key)
        } else {
          counter.peek(key)
        }
      })
    },
  );
  let mut request = Request {
    trusted_peer,
    headers,
    host: &host,
    path: policy_path,
    method,
    client_ip,
    lookups: &known,
  };
  let mut decision = state.pow.evaluate_context(&request);
  let resolved;
  if decision.needs_lookups
    && let (Some(dns), Some(ip)) = (&services.dns, client_ip)
  {
    let mut filled = known.clone();
    dns.resolve(ip, &state.pow.lookup_plan(), &mut filled).await;
    resolved = filled;
    request = Request {
      lookups: &resolved,
      ..request
    };
    decision = state.pow.evaluate_context(&request);
  }
  if decision.unresolved {
    state
      .metrics
      .policy_unresolved_decisions
      .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
  }

  let mut effective = decision.clone();
  if decision.algorithm == Some(PowAlgorithm::RandomX)
    && let Some(fallback) = &state.cfg.pow.randomx_fallback
    && !state.pool.work_available()
  {
    effective.algorithm = Some(fallback.algorithm);
    effective.difficulty = Some(fallback.difficulty);
  }
  let fallback =
    effective.algorithm != decision.algorithm || effective.difficulty != decision.difficulty;
  let log = |action| {
    log_decision(
      state.cfg.server.decision_log,
      &decision,
      DecisionContext {
        request_id,
        path: uri.path(),
        request: &request,
        action,
        fallback,
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
      let (html, locale, policy) = render_page(state, template, "", "fast", headers, uri);
      return Ok(Some(formatted_response(
        state.cfg.web.responses.deny_status,
        html,
        locale,
        method == Method::HEAD,
        json,
        policy,
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
      if fallback
        && claims.enforce == state.pow.enforcement()
        && Some(claims.algorithm) == effective.algorithm
        && claims.difficulty >= effective.difficulty.expect("challenge difficulty resolved")
      {
        return true;
      }
      let required = decision.algorithm.expect("challenge algorithm resolved");
      claims.enforce == state.pow.enforcement()
        && match (required, claims.algorithm) {
          // Any proof of work outranks a wait, so only a refresh cookie needs the difficulty check.
          (PowAlgorithm::Refresh, algorithm) if algorithm != PowAlgorithm::Refresh => true,
          (required, algorithm) => {
            algorithm == required && claims.difficulty >= required_difficulty
          }
        }
    });

  if authenticated {
    log(PolicyAction::Allow);
    return Ok(None);
  }

  if json || method == Method::GET || method == Method::HEAD {
    let presentation = state
      .pow
      .issue_presentation_with_claims(host.clone(), uri.path().to_string(), &effective)
      .map_err(ApiError::Internal)?;

    let challenge_url =
      crate::web::pages::challenge_url(&state.cfg.web.base_path, &presentation.token);
    let refresh = decision.algorithm == Some(PowAlgorithm::Refresh);
    let open_graph = if !json && !refresh && method == Method::GET {
      state
        .open_graph
        .lookup(&state.proxy, uri, &request, &state.metrics)
        .await
    } else {
      Vec::new()
    };
    let (html, locale, policy) = if refresh && !json {
      render_refresh_page(
        state,
        &presentation.token,
        presentation.claims.difficulty,
        headers,
        uri,
      )
    } else {
      render_challenge_page(state, presentation, headers, uri, json, false, &open_graph)
    };

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
      policy,
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
    if decision.algorithm == Some(PowAlgorithm::RandomX) {
      use std::sync::atomic::Ordering;
      let previous = state
        .randomx_fallback_active
        .swap(fallback, Ordering::Relaxed);
      if fallback {
        state.metrics.pow_fallback.fetch_add(1, Ordering::Relaxed);
        if !previous {
          tracing::warn!("RandomX work is unavailable; serving the fallback challenge");
        }
      } else if previous {
        tracing::info!("RandomX work is available again");
      }
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
  policy: Option<String>,
) -> Response {
  formatted_response(status, html, locale, head, false, policy)
}

fn formatted_response(
  status: StatusCode,
  body: String,
  locale: String,
  head: bool,
  json: bool,
  policy: Option<String>,
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
  if !json && let Some(policy) = policy {
    headers.insert(
      "content-security-policy",
      policy.parse().expect("Validated page policy"),
    );
  }
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
