use crate::{runtime::state::AppState, transport::context::request_host};
use http::{HeaderMap, Uri};
pub fn render_challenge_page(
  state: &AppState,
  issued: rilgate_challenge::IssuedToken<rilgate_protocol::claims::PresentationClaims>,
  headers: &HeaderMap,
  uri: &Uri,
  json: bool,
  embedded: bool,
  open_graph: &[rilgate_web::templates::context::OpenGraphTag],
) -> (String, String) {
  use rilgate_web::templates::context::PublicChallengeInfo;
  let snapshot = state.web.current();
  let locale = negotiate_locale(&snapshot, headers, uri);
  let now = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap_or_default()
    .as_secs() as i64;
  let solver_module_path =
    solver_module_path(state, issued.claims.algorithm).expect("Registered challenge algorithm");
  let challenge = PublicChallengeInfo {
    url: challenge_url(&snapshot.config.base_path, &issued.token),
    embedded,
    token: issued.token,
    algorithm: match issued.claims.algorithm {
      rilgate_protocol::PowAlgorithm::Fast => "fast",
      rilgate_protocol::PowAlgorithm::Hashwx => "hashwx",
      rilgate_protocol::PowAlgorithm::RandomX => "randomx",
      rilgate_protocol::PowAlgorithm::Refresh => "refresh",
    }
    .into(),
    difficulty: Some(issued.claims.difficulty),
    expires_at: Some(issued.claims.expires_at),
    expires_in: Some(issued.claims.expires_at.saturating_sub(now).max(0) as u64),
    solver_module_url: resolve_gateway_asset(&snapshot.config.base_path, &solver_module_path),
    honeypot_url: honeypot_url(state, &snapshot.config.base_path),
    ..Default::default()
  };
  let render = |snapshot: &rilgate_web::runtime::snapshot::WebSnapshot| {
    if json {
      snapshot.render_challenge_json(&challenge, &locale)
    } else {
      snapshot.render_challenge_with_open_graph(&challenge, &locale, open_graph)
    }
  };
  let html = render(&snapshot).unwrap_or_else(|error| {
    tracing::error!(error = %format!("{error:#}"), "Web page rendering failed");
    render(&state.web.fallback()).expect("Validated embedded challenge template")
  });
  (html, locale)
}

/// Returns a hidden link target that is unique to one page, or an empty string when
/// the honeypot is off.
fn honeypot_url(state: &AppState, prefix: &str) -> String {
  if state.policy_services.load().honeypot.is_none() {
    return String::new();
  }
  let mut segment = [0u8; 16];
  getrandom::fill(&mut segment).expect("Operating system randomness");
  format!("{prefix}/trap/{}", hex::encode(segment))
}

pub(crate) fn challenge_url(prefix: &str, token: &str) -> String {
  let query = url::form_urlencoded::Serializer::new(String::new())
    .append_pair("presentation_token", token)
    .finish();
  format!("{prefix}/challenge?{query}")
}

pub(crate) fn challenge_page(
  state: &AppState,
  headers: &HeaderMap,
  uri: &Uri,
  head: bool,
) -> Result<crate::transport::response::Response, crate::transport::errors::ApiError> {
  use crate::transport::errors::ApiError;
  let token = uri
    .query()
    .and_then(|query| {
      url::form_urlencoded::parse(query.as_bytes())
        .find(|(key, _)| key == "presentation_token")
        .map(|(_, value)| value.into_owned())
    })
    .filter(|token| token.len() <= 16384)
    .ok_or_else(|| ApiError::InvalidRequest("Missing or oversized presentation_token".into()))?;
  let claims = state
    .pow
    .verify_presentation(&token, &request_host(headers, uri))
    .map_err(ApiError::proof)?;
  let issued = rilgate_challenge::IssuedToken { token, claims };
  let (html, locale) = render_challenge_page(state, issued, headers, uri, false, true, &[]);
  let mut out = crate::proxy::gate::page_response(http::StatusCode::OK, html, locale, head);
  out.headers_mut().insert(
    "content-security-policy",
    "frame-ancestors 'self'".parse().unwrap(),
  );
  out
    .headers_mut()
    .insert("x-frame-options", "SAMEORIGIN".parse().unwrap());
  out
    .headers_mut()
    .insert("referrer-policy", "no-referrer".parse().unwrap());
  Ok(out)
}

fn negotiate_locale(
  snapshot: &rilgate_web::runtime::snapshot::WebSnapshot,
  headers: &HeaderMap,
  uri: &Uri,
) -> String {
  let explicit = uri.query().and_then(query_language);
  let cookie =
    crate::auth::cookies::cookie_value(headers, &snapshot.config.i18n.language_cookie_name);
  snapshot.supported.negotiate(
    explicit.as_deref(),
    cookie,
    headers
      .get("accept-language")
      .and_then(|value| value.to_str().ok()),
  )
}

pub fn render_page(
  state: &AppState,
  template: &str,
  token: &str,
  algorithm: &str,
  headers: &HeaderMap,
  uri: &Uri,
) -> (String, String) {
  use rilgate_web::templates::context::{PageInfo, PublicChallengeInfo, TemplateContext};
  let snapshot = state.web.current();
  let locale = negotiate_locale(&snapshot, headers, uri);
  let title_key = match template {
    "pages/denied.html.jinja" | "pages/denied.json.jinja" => "denied.title",
    "pages/error.html.jinja" => "error.title",
    _ => "challenge.title",
  };
  let title = snapshot
    .translations
    .translate(&locale, title_key, &Default::default())
    .unwrap_or_else(|_| title_key.into());
  let presentation = (!token.is_empty())
    .then(|| {
      state
        .pow
        .verify_presentation(token, &request_host(headers, uri))
        .ok()
    })
    .flatten();
  let description_key = match template {
    "pages/denied.html.jinja" | "pages/denied.json.jinja" => "denied.description",
    "pages/error.html.jinja" => "error.description",
    _ => "challenge.description",
  };
  let description = snapshot
    .translations
    .translate(&locale, description_key, &Default::default())
    .unwrap_or_else(|_| description_key.into());
  let solver_module_path = algorithm
    .parse()
    .ok()
    .and_then(|algorithm| solver_module_path(state, algorithm).ok())
    .unwrap_or_default();
  let public = TemplateContext {
    rilgate: snapshot.public.clone(),
    page: PageInfo {
      title,
      description,
      open_graph: Vec::new(),
    },
    challenge: PublicChallengeInfo {
      token: token.into(),
      algorithm: algorithm.into(),
      difficulty: presentation.as_ref().map(|claims| claims.difficulty),
      expires_at: presentation.as_ref().map(|claims| claims.expires_at),
      expires_in: presentation.as_ref().map(|claims| {
        let now = std::time::SystemTime::now()
          .duration_since(std::time::UNIX_EPOCH)
          .unwrap_or_default()
          .as_secs() as i64;
        claims.expires_at.saturating_sub(now).max(0) as u64
      }),
      solver_module_url: resolve_gateway_asset(&snapshot.config.base_path, &solver_module_path),
      ..Default::default()
    },
  };
  let html = snapshot
    .render(template, &public, &locale)
    .unwrap_or_else(|error| {
      tracing::error!(error = %format!("{error:#}"), "Web page rendering failed");
      state
        .web
        .fallback()
        .render(template, &public, &locale)
        .expect("Validated embedded page template")
    });
  (html, locale)
}

/// Renders the script-free page whose meta refresh opens the refresh URL after `seconds`.
pub fn render_refresh_page(
  state: &AppState,
  token: &str,
  seconds: u64,
  headers: &HeaderMap,
  uri: &Uri,
) -> (String, String) {
  use rilgate_web::templates::context::{PageInfo, PublicChallengeInfo, TemplateContext};
  let snapshot = state.web.current();
  let locale = negotiate_locale(&snapshot, headers, uri);
  let translate = |key: &str| {
    snapshot
      .translations
      .translate(&locale, key, &Default::default())
      .unwrap_or_else(|_| key.into())
  };
  let query = url::form_urlencoded::Serializer::new(String::new())
    .append_pair("presentation_token", token)
    .finish();
  let public = TemplateContext {
    rilgate: snapshot.public.clone(),
    page: PageInfo {
      title: translate("refresh.title"),
      description: translate("refresh.description"),
      open_graph: Vec::new(),
    },
    challenge: PublicChallengeInfo {
      url: format!("{}/api/refresh?{query}", snapshot.config.base_path),
      algorithm: "refresh".into(),
      difficulty: Some(seconds),
      honeypot_url: honeypot_url(state, &snapshot.config.base_path),
      ..Default::default()
    },
  };
  let template = "pages/refresh.html.jinja";
  let html = snapshot
    .render(template, &public, &locale)
    .unwrap_or_else(|error| {
      tracing::error!(error = %format!("{error:#}"), "Web page rendering failed");
      state
        .web
        .fallback()
        .render(template, &public, &locale)
        .expect("Validated embedded page template")
    });
  (html, locale)
}

pub(crate) fn resolve_gateway_asset(prefix: &str, configured: &str) -> String {
  format!(
    "{}{}",
    prefix.trim_end_matches('/'),
    if configured.starts_with('/') {
      configured.to_string()
    } else {
      format!("/{configured}")
    }
  )
}

/// Reads the `lang` query parameter from a request URI.
///
/// The query is decoded as a form rather than by parsing the URI, because
/// rendering a page needs this one parameter and not a URL.
fn query_language(query: &str) -> Option<String> {
  url::form_urlencoded::parse(query.as_bytes())
    .find(|(name, _)| name == "lang")
    .map(|(_, value)| value.into_owned())
    .filter(|value| !value.is_empty())
}

fn solver_module_path(
  state: &AppState,
  algorithm: rilgate_protocol::PowAlgorithm,
) -> anyhow::Result<String> {
  let descriptor = state.pow.algorithm_descriptor(algorithm)?;
  Ok(descriptor.solver_module_path)
}
