use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ContentSecurityPolicy {
  pub enabled: bool,
  pub extra_sources: BTreeMap<String, Vec<String>>,
}

impl Default for ContentSecurityPolicy {
  fn default() -> Self {
    Self {
      enabled: true,
      extra_sources: BTreeMap::new(),
    }
  }
}

impl ContentSecurityPolicy {
  pub fn validate(&self) -> Result<()> {
    for (directive, sources) in &self.extra_sources {
      let path = format!("site.content_security_policy.extra_sources.{directive}");
      ensure!(
        [
          "script-src",
          "style-src",
          "img-src",
          "font-src",
          "connect-src"
        ]
        .contains(&directive.as_str()),
        "{path}: unsupported directive"
      );
      ensure!(sources.len() <= 16, "{path}: expected at most 16 sources");
      for (index, source) in sources.iter().enumerate() {
        ensure!(
          !source.is_empty()
            && !source
              .chars()
              .any(|c| c.is_whitespace() || c.is_control() || matches!(c, ';' | ',')),
          "{path}[{index}]: expected a nonempty source without whitespace, control characters, semicolons or commas"
        );
      }
    }
    Ok(())
  }

  pub fn header(&self, hashes: &InlineHashes, stylesheet: &str) -> Option<String> {
    if !self.enabled {
      return None;
    }
    let mut script_sources = vec!["'self'".into(), "'wasm-unsafe-eval'".into()];
    script_sources.extend(hashes.scripts.iter().cloned());
    let mut style_sources = vec!["'self'".into()];
    style_sources.extend(hashes.styles.iter().cloned());
    if let Ok(url) = url::Url::parse(stylesheet)
      && matches!(url.scheme(), "http" | "https")
    {
      style_sources.push(url.origin().ascii_serialization());
    }
    let mut directives = vec![
      ("default-src", vec!["'none'".into()]),
      ("script-src", script_sources),
      ("style-src", style_sources),
      ("img-src", vec!["'self'".into(), "data:".into()]),
      ("font-src", vec!["'self'".into()]),
      ("connect-src", vec!["'self'".into()]),
      ("worker-src", vec!["'self'".into()]),
      ("manifest-src", vec!["'self'".into()]),
      ("base-uri", vec!["'none'".into()]),
      ("form-action", vec!["'self'".into()]),
      ("frame-ancestors", vec!["'self'".into()]),
    ];
    for (name, sources) in &mut directives {
      if let Some(extra) = self.extra_sources.get(*name) {
        sources.extend(extra.iter().cloned());
      }
    }
    Some(
      directives
        .into_iter()
        .map(|(name, sources)| format!("{name} {}", sources.join(" ")))
        .collect::<Vec<_>>()
        .join("; "),
    )
  }
}

#[derive(Debug, Default, Clone)]
pub struct InlineHashes {
  pub scripts: Vec<String>,
  pub styles: Vec<String>,
}

pub fn inline_hashes(html: &str) -> InlineHashes {
  let lower = html.to_ascii_lowercase();
  let mut hashes = InlineHashes::default();
  let mut offset = 0;
  while let Some(start) = lower[offset..].find('<').map(|i| i + offset) {
    if lower[start..].starts_with("<!--") {
      offset = lower[start + 4..]
        .find("-->")
        .map_or(html.len(), |i| start + 4 + i + 3);
      continue;
    }
    let mut end = start + 1;
    let mut quote = None;
    for byte in html.as_bytes()[end..].iter().copied() {
      match (quote, byte) {
        (Some(q), b) if q == b => quote = None,
        (None, b'\'' | b'"') => quote = Some(byte),
        (None, b'>') => break,
        _ => {}
      }
      end += 1;
    }
    if end == html.len() {
      break;
    }
    offset = end + 1;
    let tag = &lower[start + 1..end];
    let name_end = tag
      .find(|c: char| c.is_ascii_whitespace() || c == '/')
      .unwrap_or(tag.len());
    let name = &tag[..name_end];
    if !matches!(name, "script" | "style") {
      continue;
    }
    let closing = format!("</{name}");
    let mut search = offset;
    let close = loop {
      let Some(candidate) = lower[search..].find(&closing).map(|i| i + search) else {
        break None;
      };
      if lower
        .as_bytes()
        .get(candidate + closing.len())
        .is_some_and(|b| b.is_ascii_whitespace() || *b == b'>')
      {
        break Some(candidate);
      }
      search = candidate + closing.len();
    };
    let Some(close) = close else {
      break;
    };
    let attrs = attributes(&tag[name_end..]);
    let executable = !attrs.contains_key("src")
      && attrs.get("type").is_none_or(|t| {
        matches!(
          t.trim(),
          "text/javascript" | "application/javascript" | "module"
        )
      });
    if name == "style" || executable {
      let hash = format!(
        "'sha256-{}'",
        STANDARD.encode(Sha256::digest(&html.as_bytes()[offset..close]))
      );
      let list = if name == "style" {
        &mut hashes.styles
      } else {
        &mut hashes.scripts
      };
      if !list.contains(&hash) {
        list.push(hash);
      }
    }
    offset = close + closing.len();
  }
  hashes
}

fn attributes(mut text: &str) -> BTreeMap<String, String> {
  let mut attributes = BTreeMap::new();
  while !text.is_empty() {
    text = text.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '/');
    let end = text
      .find(|c: char| c.is_ascii_whitespace() || matches!(c, '=' | '/'))
      .unwrap_or(text.len());
    if end == 0 {
      break;
    }
    let name = text[..end].to_string();
    text = text[end..].trim_start();
    let mut value = "";
    if let Some(rest) = text.strip_prefix('=') {
      text = rest.trim_start();
      if let Some(quote) = text.chars().next().filter(|c| matches!(c, '\'' | '"')) {
        text = &text[1..];
        let end = text.find(quote).unwrap_or(text.len());
        value = &text[..end];
        text = text.get(end + 1..).unwrap_or("");
      } else {
        let end = text
          .find(|c: char| c.is_ascii_whitespace())
          .unwrap_or(text.len());
        value = &text[..end];
        text = &text[end..];
      }
    }
    attributes.entry(name).or_insert_with(|| value.to_string());
  }
  attributes
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn hashes_exact_executable_text_and_noscript_styles_once() {
    let html = r#"<script src="/app.js">ignored</script><script type="application/json">{}</script><script> alert(1);\n</script><script> alert(1);\n</script><noscript><style>p { color: red; }</style></noscript>"#;
    let hashes = inline_hashes(html);
    assert_eq!(
      hashes.scripts,
      [format!(
        "'sha256-{}'",
        STANDARD.encode(Sha256::digest(b" alert(1);\\n"))
      )]
    );
    assert_eq!(
      hashes.styles,
      [format!(
        "'sha256-{}'",
        STANDARD.encode(Sha256::digest(b"p { color: red; }"))
      )]
    );
  }

  #[test]
  fn scans_attributes_case_insensitively_and_respects_raw_text() {
    let hashes = inline_hashes(
      r#"<!-- <script>ignored</script> --><SCRIPT data-src="x > y" TYPE='MODULE'>var s = '<style>text</style>';</SCRIPT><script SRC='/x'>ignored</script><script type=text/javascript>ok</script><script type=application/javascript>ok</script>"#,
    );
    assert_eq!(hashes.scripts.len(), 2);
    assert!(hashes.styles.is_empty());
  }

  #[test]
  fn validates_source_limits_and_setting_paths() {
    for (directive, sources) in [
      ("worker-src", vec!["'self'".into()]),
      ("script-src", vec!["https://example.test".into(); 17]),
      ("style-src", vec!["".into()]),
      ("img-src", vec!["a;b".into()]),
      ("font-src", vec!["a,b".into()]),
      ("connect-src", vec!["a b".into()]),
    ] {
      let config = ContentSecurityPolicy {
        enabled: true,
        extra_sources: [(directive.into(), sources)].into(),
      };
      assert!(
        config
          .validate()
          .unwrap_err()
          .to_string()
          .contains(&format!(
            "site.content_security_policy.extra_sources.{directive}"
          ))
      );
    }
  }
}
