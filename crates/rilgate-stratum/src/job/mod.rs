use anyhow::{Context, Result, anyhow};
use rilgate_protocol::StratumJob;
use serde_json::Value;
pub(crate) fn parse_job(value: &Value) -> Result<StratumJob> {
  let string = |name: &str| -> Result<String> {
    value
      .get(name)
      .and_then(Value::as_str)
      .map(ToOwned::to_owned)
      .ok_or_else(|| anyhow!("P2Pool job is missing {name}"))
  };

  let job = StratumJob {
    blob: string("blob")?,
    job_id: string("job_id")?,
    target: string("target")?,
    seed_hash: string("seed_hash")?,
    height: value.get("height").and_then(Value::as_u64),
    algo: value
      .get("algo")
      .and_then(Value::as_str)
      .map(ToOwned::to_owned),
  };
  let blob = hex::decode(&job.blob).context("Invalid P2Pool hashing blob")?;
  let seed = hex::decode(&job.seed_hash).context("Invalid P2Pool seed hash")?;
  let target = hex::decode(&job.target).context("Invalid P2Pool target")?;
  if !(4..=256).contains(&blob.len()) || seed.len() != 32 || !matches!(target.len(), 4 | 8) {
    return Err(anyhow!(
      "P2Pool job has invalid blob, seed, or target lengths"
    ));
  }
  if job.job_id.is_empty() || job.algo.as_deref().is_some_and(|algo| algo != "rx/0") {
    return Err(anyhow!(
      "P2Pool job requires a job id and RandomX v1 algorithm"
    ));
  }
  Ok(job)
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;
  #[test]
  fn unsupported_or_malformed_jobs_are_rejected() {
    let valid = json!({"blob": "00".repeat(64), "job_id": "job", "seed_hash": "ab".repeat(32), "target": "ffffffff", "algo": "rx/0"});
    assert!(parse_job(&valid).is_ok());
    for (field, value) in [
      ("blob", "zz"),
      ("seed_hash", "ab"),
      ("algo", "rx/wow"),
      ("job_id", ""),
      ("target", "ff"),
    ] {
      let mut malformed = valid.clone();
      malformed[field] = Value::String(value.into());
      assert!(parse_job(&malformed).is_err(), "field {field}");
    }
  }
}
