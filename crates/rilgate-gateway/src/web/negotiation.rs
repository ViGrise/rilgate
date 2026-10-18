use http::{HeaderMap, header::ACCEPT};
use rilgate_web::config::{ResponseConfig, ResponseFormat};

pub fn wants_json(config: &ResponseConfig, headers: &HeaderMap, path: &str) -> bool {
  match config.format {
    ResponseFormat::Json => return true,
    ResponseFormat::Html => return false,
    ResponseFormat::Auto => {}
  }
  if config.json_paths.iter().any(|prefix| {
    let prefix = prefix.trim_end_matches('/');
    path == prefix
      || path
        .strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with('/'))
  }) {
    return true;
  }
  let mut json = None;
  let mut html = None;
  for entry in headers
    .get_all(ACCEPT)
    .iter()
    .filter_map(|v| v.to_str().ok())
    .flat_map(|value| value.split(','))
    .take(64)
  {
    let mut parts = entry.split(';');
    let media = parts.next().unwrap_or_default().trim();
    let mut quality = 1000;
    for parameter in parts {
      if let Some((name, value)) = parameter.trim().split_once('=')
        && name.eq_ignore_ascii_case("q")
      {
        quality = parse_quality(value.trim()).unwrap_or(0);
      }
    }
    for (target, wildcard, destination) in [
      ("application/json", "application/*", &mut json),
      ("text/html", "text/*", &mut html),
    ] {
      let specificity = if media.eq_ignore_ascii_case(target) {
        2
      } else if media.eq_ignore_ascii_case(wildcard) {
        1
      } else if media == "*/*" {
        0
      } else {
        continue;
      };
      let candidate = (specificity, quality);
      if destination.is_none_or(|current| candidate > current) {
        *destination = Some(candidate);
      }
    }
  }
  let json_quality = json.map_or(0, |(_, q)| q);
  let html_quality = html.map_or(0, |(_, q)| q);
  if json_quality != html_quality
    || json.is_some_and(|(specificity, _)| specificity == 2)
    || html.is_some_and(|(specificity, _)| specificity == 2)
  {
    return json_quality > html_quality;
  }
  // Wildcard Accept is common in fetch clients and does not identify a page navigation.
  let media = headers
    .get(http::header::CONTENT_TYPE)
    .and_then(|v| v.to_str().ok())
    .unwrap_or_default()
    .split(';')
    .next()
    .unwrap_or_default()
    .trim()
    .to_ascii_lowercase();
  media == "application/json"
    || (media.starts_with("application/") && media.ends_with("+json"))
    || headers
      .get("x-requested-with")
      .is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"XMLHttpRequest"))
}

fn parse_quality(value: &str) -> Option<u16> {
  let (integer, fraction) = value.split_once('.').unwrap_or((value, ""));
  if !matches!(integer, "0" | "1")
    || fraction.len() > 3
    || !fraction.bytes().all(|b| b.is_ascii_digit())
  {
    return None;
  }
  let fraction = format!("{fraction:0<3}").parse::<u16>().ok()?;
  match integer {
    "0" => Some(fraction),
    "1" if fraction == 0 => Some(1000),
    _ => None,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn accept_quality_and_specificity_choose_the_representation() {
    for (accept, expected) in [
      ("application/json", true),
      ("text/html,application/json;q=0.9", false),
      ("application/json;q=0,text/html;q=0.5,*/*;q=1", false),
      ("application/json;q=0.8,text/html;q=0.1", true),
      ("application/*,text/html;q=0", true),
      ("*/*", false),
      ("application/json;q=NaN", false),
      ("application/json;q=1.001", false),
      ("application/json;q=0.001,text/html;q=0", true),
      ("text/html,application/json", false),
    ] {
      let mut headers = HeaderMap::new();
      headers.insert(ACCEPT, accept.parse().unwrap());
      assert_eq!(
        wants_json(&ResponseConfig::default(), &headers, "/"),
        expected,
        "{accept}"
      );
    }
  }

  #[test]
  fn path_selection_respects_segment_boundaries() {
    let config = ResponseConfig {
      json_paths: vec!["/api".into()],
      ..Default::default()
    };
    for (path, expected) in [
      ("/api", true),
      ("/api/users", true),
      ("/apiary", false),
      ("/", false),
    ] {
      assert_eq!(wants_json(&config, &HeaderMap::new(), path), expected);
    }
  }
}
