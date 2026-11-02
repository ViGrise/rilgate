use http::HeaderMap;
pub fn request_host(headers: &HeaderMap, uri: &http::Uri) -> String {
  headers
    .get(http::header::HOST)
    .and_then(|value| value.to_str().ok())
    .or_else(|| uri.authority().map(|authority| authority.as_str()))
    .unwrap_or_default()
    .to_ascii_lowercase()
}

/// Whether a request is a browser opening a URL as a page. The challenge scripts call the
/// API with fetch or XMLHttpRequest, which never send `Sec-Fetch-Mode: navigate` or ask for
/// HTML first.
pub fn is_navigation(method: &http::Method, headers: &HeaderMap) -> bool {
  if method != http::Method::GET && method != http::Method::HEAD {
    return false;
  }
  let header = |name| headers.get(name).and_then(|value| value.to_str().ok());
  match header("sec-fetch-mode") {
    Some(mode) => mode.eq_ignore_ascii_case("navigate"),
    None => {
      header(http::header::ACCEPT.as_str()).is_some_and(|accept| accept.contains("text/html"))
    }
  }
}

/// The same-host page that led to a navigation, from `Referer`, or `/`. Gateway URLs below
/// `base_path` are never returned, so the redirect target is always an application page.
pub fn navigation_return_path(headers: &HeaderMap, host: &str, base_path: &str) -> String {
  headers
    .get(http::header::REFERER)
    .and_then(|value| value.to_str().ok())
    .and_then(|referer| referer.parse::<http::Uri>().ok())
    .filter(|referer| {
      referer
        .authority()
        .is_some_and(|authority| authority.as_str().eq_ignore_ascii_case(host))
    })
    .and_then(|referer| {
      referer
        .path_and_query()
        .map(|path| path.as_str().to_owned())
    })
    .filter(|path| {
      path.starts_with('/')
        && !path.starts_with("//")
        && !path.contains('\\')
        && path != base_path
        && !path.starts_with(&format!("{base_path}/"))
    })
    .unwrap_or_else(|| "/".into())
}
