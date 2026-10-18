use anyhow::{Context, Result, ensure};

pub(crate) fn validate(name: &str, body: &str) -> Result<()> {
  if !matches!(
    name,
    "pages/challenge.json" | "pages/denied.json"
  ) {
    return Ok(());
  }
  let value: serde_json::Value = serde_json::from_str(body)
    .with_context(|| format!("{name}: expected valid JSON; escape template values with tojson"))?;
  ensure!(value.is_object(), "{name}: expected a JSON object");
  Ok(())
}
