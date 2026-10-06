use crate::{runtime::state::AppState, transport::context::request_host};
use axum::http::{HeaderMap, Uri};
pub fn render_challenge_page(
  state: &AppState,
  presentation_token: &str,
  algorithm: &str,
  headers: &HeaderMap,
  uri: &Uri,
) -> (String, String) {
  render_page(
    state,
    "pages/challenge.html",
    presentation_token,
    algorithm,
    headers,
    uri,
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
  let explicit = uri.query().and_then(query_language);
  let cookie =
    crate::auth::cookies::cookie_value(headers, &snapshot.config.i18n.language_cookie_name);
  let locale = snapshot.supported.negotiate(
    explicit.as_deref(),
    cookie.as_deref(),
    headers
      .get("accept-language")
      .and_then(|value| value.to_str().ok()),
  );
  let title_key = match template {
    "pages/denied.html" => "denied.title",
    "pages/error.html" => "error.title",
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
    "pages/denied.html" => "denied.description",
    "pages/error.html" => "error.description",
    _ => "challenge.description",
  };
  let description = snapshot
    .translations
    .translate(&locale, description_key, &Default::default())
    .unwrap_or_else(|_| description_key.into());
  let public = TemplateContext {
    rilgate: snapshot.public.clone(),
    page: PageInfo { title, description },
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
      solver_module_url: resolve_gateway_asset(
        &snapshot.config.base_path,
        &state.cfg.pow.browser.solver_module_path,
      ),
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
        .unwrap_or_else(|_| "Unable to continue".into())
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
