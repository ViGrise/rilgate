use crate::transport::response::{Response, response};
use crate::{
  auth::cookies::cookie_value,
  runtime::state::AppState,
  transport::{context::request_host, errors::ApiError, headers::NO_STORE, path::policy_path},
  web::pages::{render_challenge_page, render_page, render_refresh_page},
};
use http::{
  HeaderMap, HeaderValue, Method, StatusCode, Uri,
  header::{CACHE_CONTROL, CONTENT_TYPE},
};
use rilgate_challenge::authorization::binding::{BindingInput, BindingMismatch};
use rilgate_challenge::authorization::steps::first_unsatisfied;
use rilgate_config::DecisionLog;
use rilgate_policy::{Lookups, PolicyAction, PolicyDecision, conditions::Request};
use rilgate_protocol::{ChallengeStep, PowAlgorithm, PowStep};

struct DecisionContext<'a> {
  request_id: &'a str,
  path: &'a str,
  request: &'a Request<'a>,
  action: PolicyAction,
  fallback: bool,
  required: &'a [ChallengeStep],
  presented: Option<usize>,
  effective: &'a [ChallengeStep],
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
  let selection = context
    .presented
    .and_then(|i| context.effective.get(i))
    .or_else(|| context.required.first());
  let step = selection.map(|ChallengeStep::Pow(step)| step);
  let summaries = (!context.required.is_empty()).then(|| {
    context
      .required
      .iter()
      .map(ChallengeStep::summary)
      .collect::<Vec<_>>()
      .join(",")
  });
  tracing::info!(
    request_id = context.request_id,
    client_ip = %context.request.client_ip.map(|ip| ip.to_string()).unwrap_or_else(|| "unknown".into()),
    method = %context.request.method,
    host = context.request.host,
    path = context.path,
    action,
    rule = &*decision.rule,
    weight = decision.weight,
    algorithm = step.map(|step| match step.algorithm {
      PowAlgorithm::Fast => "fast", PowAlgorithm::Hashwx => "hashwx",
      PowAlgorithm::RandomX => "randomx", PowAlgorithm::Refresh => "refresh",
    }),
    difficulty = step.map(|step| step.difficulty),
    step = context.presented.map(|i| i + 1),
    steps = summaries.as_deref(),
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
  gate_with_client(
    state,
    GateRequest {
      headers,
      uri,
      policy_path: &canonical,
      method,
      client_ip: None,
      trusted_peer: false,
      request_id: "",
    },
  )
  .await
}

/// The request facts that the policy gate evaluates.
pub struct GateRequest<'a> {
  pub headers: &'a HeaderMap,
  pub uri: &'a Uri,
  pub policy_path: &'a str,
  pub method: &'a Method,
  pub client_ip: Option<std::net::IpAddr>,
  pub trusted_peer: bool,
  pub request_id: &'a str,
}

pub async fn gate_with_client(
  state: &AppState,
  request: GateRequest<'_>,
) -> Result<Option<Response>, ApiError> {
  let GateRequest {
    headers,
    uri,
    policy_path,
    method,
    client_ip,
    trusted_peer,
    request_id,
  } = request;
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

  let required = state
    .pow
    .resolve_steps(&decision)
    .map_err(ApiError::Internal)?;
  let mut effective = required.clone();
  if let Some(fallback) = &state.cfg.pow.randomx_fallback
    && !state.pool.work_available()
  {
    for ChallengeStep::Pow(step) in &mut effective {
      if step.algorithm == PowAlgorithm::RandomX {
        *step = PowStep {
          algorithm: fallback.algorithm,
          difficulty: fallback.difficulty,
          verification: state.pow.verification(fallback.algorithm),
        };
      }
    }
  }
  let fallback = effective != required;
  let log = |action, presented| {
    log_decision(
      state.cfg.server.decision_log,
      &decision,
      DecisionContext {
        request_id,
        path: uri.path(),
        request: &request,
        action,
        fallback,
        required: &required,
        effective: &effective,
        presented,
      },
    )
  };

  match decision.action {
    PolicyAction::Allow => {
      log(PolicyAction::Allow, None);
      return Ok(None);
    }

    PolicyAction::Deny => {
      log(PolicyAction::Deny, None);

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

  let binding = BindingInput {
    client_ip,
    headers,
    trusted_peer,
  };
  let grants = cookie_value(headers, &state.cfg.pow.auth_cookie_name)
    .and_then(
      |token| match state.pow.verify_auth_token(token, &host, &binding) {
        Ok(access) => Some(access.grants),
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
    .unwrap_or_default();
  let Some(step) = first_unsatisfied(&required, &effective, &grants) else {
    log(PolicyAction::Allow, None);
    return Ok(None);
  };
  let ChallengeStep::Pow(presented) = &effective[step];
  let refresh = presented.algorithm == PowAlgorithm::Refresh;
  let presentation = state
    .pow
    .issue_presentation(
      host.clone(),
      uri.path().to_string(),
      decision.rule.to_string(),
      decision.weight,
      effective.clone(),
      step,
    )
    .map_err(ApiError::Internal)?;
  let challenge_url: HeaderValue =
    crate::web::pages::challenge_url(&state.cfg.web.base_path, &presentation.token)
      .parse()
      .map_err(|e| ApiError::Internal(anyhow::Error::new(e)))?;

  if json || method == Method::GET || method == Method::HEAD {
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
        presentation.claims.presented().difficulty,
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
    insert_challenge_notification(state, &mut out, challenge_url);
    if required
      .iter()
      .any(|ChallengeStep::Pow(step)| step.algorithm == PowAlgorithm::RandomX)
    {
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
    log(PolicyAction::Challenge, Some(step));
    return Ok(Some(out));
  }

  log(PolicyAction::Challenge, Some(step));
  let mut out = response(
    StatusCode::UNAUTHORIZED,
    if method == Method::HEAD {
      ""
    } else {
      "Proof of work is required before sending this request."
    },
  );
  out.headers_mut().insert(
    CONTENT_TYPE,
    HeaderValue::from_static("text/plain; charset=utf-8"),
  );
  out.headers_mut().insert(CACHE_CONTROL, NO_STORE);
  insert_challenge_notification(state, &mut out, challenge_url);
  Ok(Some(out))
}

/// Marks a challenge response so that any client, whatever the response body, can detect the
/// challenge and open its solver.
fn insert_challenge_notification(state: &AppState, out: &mut Response, challenge_url: HeaderValue) {
  let headers = out.headers_mut();
  headers.insert(
    state.challenge_header.clone(),
    HeaderValue::from_static("required"),
  );
  headers.insert(state.challenge_url_header.clone(), challenge_url);
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
    HeaderValue::from_static(if json {
      "application/json"
    } else {
      "text/html; charset=utf-8"
    }),
  );
  if !json && let Some(policy) = policy {
    headers.insert(
      "content-security-policy",
      policy.parse().expect("Validated page policy"),
    );
  }
  headers.insert(CACHE_CONTROL, NO_STORE);
  headers.insert(
    http::header::VARY,
    HeaderValue::from_static("Accept, Content-Type, X-Requested-With, Accept-Language, Cookie"),
  );
  headers.insert(
    http::header::CONTENT_LANGUAGE,
    locale.parse().expect("Validated locale"),
  );
  out
}
