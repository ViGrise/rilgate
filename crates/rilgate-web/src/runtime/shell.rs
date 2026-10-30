use crate::templates::context::PublicChallengeInfo;
use anyhow::{Result, ensure};
use serde::Serialize;
use std::io::Write;

pub(super) const BOOTSTRAP_SLOT: &str = "<script data-rilgate-bootstrap></script>";

pub(super) struct ChallengeShell {
  pub(super) hashes: crate::csp::InlineHashes,
  before: String,
  after: String,
}

impl ChallengeShell {
  pub(super) fn new(html: String) -> Result<Self> {
    let (before, after) = html
      .split_once(BOOTSTRAP_SLOT)
      .ok_or_else(|| anyhow::anyhow!("Challenge bootstrap slot is missing"))?;
    ensure!(
      !after.contains(BOOTSTRAP_SLOT),
      "Duplicate challenge bootstrap slot"
    );
    Ok(Self {
      hashes: crate::csp::inline_hashes(&html.replace(BOOTSTRAP_SLOT, "")),
      before: before.into(),
      after: after.into(),
    })
  }

  pub(super) fn render(&self, prefix: &str, challenge: &PublicChallengeInfo) -> Result<String> {
    let mut html = Vec::with_capacity(
      self.before.len() + self.after.len() + bootstrap_capacity(prefix, challenge),
    );
    html.extend_from_slice(self.before.as_bytes());
    write_bootstrap(&mut html, prefix, challenge)?;
    html.extend_from_slice(self.after.as_bytes());
    Ok(String::from_utf8(html)?)
  }
}

struct ScriptFormatter;
impl serde_json::ser::Formatter for ScriptFormatter {
  fn write_string_fragment<W: ?Sized + Write>(
    &mut self,
    writer: &mut W,
    fragment: &str,
  ) -> std::io::Result<()> {
    // Escape HTML delimiters while serializing so script contents cannot close their element.
    let mut start = 0;
    for (offset, character) in fragment.char_indices() {
      let escaped = match character {
        '&' => b"\\u0026",
        '<' => b"\\u003c",
        '>' => b"\\u003e",
        '\u{2028}' => b"\\u2028",
        '\u{2029}' => b"\\u2029",
        _ => continue,
      };
      writer.write_all(&fragment.as_bytes()[start..offset])?;
      writer.write_all(escaped)?;
      start = offset + character.len_utf8();
    }
    writer.write_all(&fragment.as_bytes()[start..])
  }
}

fn write_script_json(writer: &mut Vec<u8>, value: &impl Serialize) -> Result<()> {
  value.serialize(&mut serde_json::Serializer::with_formatter(
    writer,
    ScriptFormatter,
  ))?;
  Ok(())
}

pub(super) fn script_json(value: &impl Serialize) -> Result<String> {
  let mut json = Vec::new();
  write_script_json(&mut json, value)?;
  Ok(String::from_utf8(json)?)
}

fn bootstrap_capacity(prefix: &str, challenge: &PublicChallengeInfo) -> usize {
  256
    + prefix.len()
    + challenge.solver_module_url.len()
    + challenge.token.len()
    + challenge.algorithm.len()
}

fn write_bootstrap(
  output: &mut Vec<u8>,
  prefix: &str,
  challenge: &PublicChallengeInfo,
) -> Result<()> {
  output.extend_from_slice(b"<script type=\"application/json\" id=\"pow-bootstrap\">");
  write_script_json(
    output,
    &serde_json::json!({
      "internal_prefix": prefix,
      "solver_module_url": challenge.solver_module_url,
      "presentation_token": challenge.token,
      "algorithm": challenge.algorithm,
      "embedded": challenge.embedded,
    }),
  )?;
  output.extend_from_slice(b"</script>");
  Ok(())
}

pub(super) fn bootstrap(prefix: &str, challenge: &PublicChallengeInfo) -> Result<String> {
  let mut output = Vec::with_capacity(bootstrap_capacity(prefix, challenge));
  write_bootstrap(&mut output, prefix, challenge)?;
  Ok(String::from_utf8(output)?)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn script_json_preserves_json_values_without_html_delimiters() {
    let value = serde_json::json!({
      "</script>&\u{2028}": ["quotes\" slash\\ newline\n tab\t \u{0000}", "Tiếng Việt 😀 > \u{2029}"]
    });
    let encoded = script_json(&value).unwrap();
    assert!(!encoded.contains(['<', '>', '&', '\u{2028}', '\u{2029}']));
    assert_eq!(
      serde_json::from_str::<serde_json::Value>(&encoded).unwrap(),
      value
    );
    let expected = serde_json::to_string(&value)
      .unwrap()
      .replace('&', "\\u0026")
      .replace('<', "\\u003c")
      .replace('>', "\\u003e")
      .replace('\u{2028}', "\\u2028")
      .replace('\u{2029}', "\\u2029");
    assert_eq!(encoded, expected);
  }
}
