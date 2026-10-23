use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use tokio::{
  net::{
    TcpStream,
    tcp::{OwnedReadHalf, OwnedWriteHalf},
  },
  time::timeout,
};

use crate::{
  StratumConfig,
  codec::{JsonLineReader, write_json_line},
  job::parse_job,
};
use rilgate_protocol::StratumJob;

#[derive(Debug)]
pub enum StratumEvent {
  Job(StratumJob),
  Response {
    id: u64,
    accepted: bool,
    error: Option<String>,
  },
}

pub struct StratumSession {
  reader: JsonLineReader<OwnedReadHalf>,
  writer: OwnedWriteHalf,
  pub rpc_id: String,
  next_request_id: u64,
  pub current_job: StratumJob,
  submit_timeout: Duration,
}

impl StratumSession {
  pub async fn connect(cfg: &StratumConfig, difficulty: u64) -> Result<Self> {
    if difficulty < 1000 {
      return Err(anyhow!("Stratum pool difficulty must be at least 1000"));
    }
    let stream = timeout(
      cfg.connect_timeout,
      TcpStream::connect((cfg.host.as_str(), cfg.port)),
    )
    .await
    .context("Stratum pool connection timed out")?
    .context("Unable to connect to the Stratum pool")?;

    let (read_half, write_half) = stream.into_split();
    let mut reader = JsonLineReader::new(read_half);
    let mut writer = write_half;

    let login = json!({
      "id": 1,
      "jsonrpc": "2.0",
      "method": "login",
      "params": {
        "login": cfg.user.expose().replace("{difficulty}", &difficulty.to_string()),
        "pass": cfg.password.expose(),
        "agent": cfg.agent.clone(),
      }
    });

    let response = timeout(cfg.connect_timeout, async {
      write_json_line(&mut writer, &login).await?;
      reader.next_json().await
    })
    .await
    .context("Stratum pool login timed out")??;

    if response.get("id").and_then(Value::as_u64) != Some(1) {
      return Err(anyhow!(
        "Stratum pool login response has an unexpected request id"
      ));
    }

    if response.get("error").is_some_and(|value| !value.is_null()) {
      return Err(anyhow!("Stratum pool login rejected"));
    }

    let result = response
      .get("result")
      .ok_or_else(|| anyhow!("Stratum pool login response has no result"))?;

    let rpc_id = result
      .get("id")
      .and_then(Value::as_str)
      .ok_or_else(|| anyhow!("Stratum pool login response has no miner id"))?
      .to_string();

    let job = result
      .get("job")
      .ok_or_else(|| anyhow!("Stratum pool login response has no job"))?;

    let current_job = parse_job(job)?;

    Ok(Self {
      reader,
      writer,
      rpc_id,
      next_request_id: 2,
      current_job,
      submit_timeout: cfg.submit_timeout,
    })
  }

  pub async fn read_event(&mut self) -> Result<StratumEvent> {
    loop {
      let message = self.reader.next_json().await?;

      if message.get("method").and_then(Value::as_str) == Some("job") {
        let params = message
          .get("params")
          .ok_or_else(|| anyhow!("Stratum job notification has no params"))?;
        let job = parse_job(params)?;
        self.current_job = job.clone();
        return Ok(StratumEvent::Job(job));
      }

      if let Some(id) = message.get("id").and_then(Value::as_u64) {
        let error = message
          .get("error")
          .filter(|value| !value.is_null())
          .map(|value| value.to_string());

        let accepted = error.is_none()
          && message
            .get("result")
            .and_then(|result| result.get("status"))
            .and_then(Value::as_str)
            .is_some_and(|status| status.eq_ignore_ascii_case("OK"));

        return Ok(StratumEvent::Response {
          id,
          accepted,
          error,
        });
      }
    }
  }

  pub async fn submit_share(&mut self, job_id: &str, nonce: &str, result: &str) -> Result<u64> {
    if nonce.len() != 8 || hex::decode(nonce).is_err() {
      return Err(anyhow!("Nonce must be exactly 8 hexadecimal characters"));
    }

    if result.len() != 64 || hex::decode(result).is_err() {
      return Err(anyhow!(
        "RandomX result must be exactly 64 hexadecimal characters"
      ));
    }

    if job_id != self.current_job.job_id {
      return Err(anyhow!("Share refers to a stale Stratum pool job"));
    }
    // Stratum encodes the nonce's little-endian bytes, while gateway clients use a numeric hex value.
    let nonce = u32::from_str_radix(nonce, 16).context("Invalid share nonce")?;
    let nonce = hex::encode(nonce.to_le_bytes());
    let id = self.next_request_id;
    self.next_request_id = id
      .checked_add(1)
      .ok_or_else(|| anyhow!("Stratum request id exhausted"))?;

    let submit = json!({
      "id": id,
      "jsonrpc": "2.0",
      "method": "submit",
      "params": {
        "id": self.rpc_id,
        "job_id": job_id,
        "nonce": nonce,
        "result": result,
      }
    });

    timeout(
      self.submit_timeout,
      write_json_line(&mut self.writer, &submit),
    )
    .await
    .context("Stratum pool share write timed out")??;
    Ok(id)
  }
}

pub async fn fetch_job(cfg: &StratumConfig, difficulty: u64) -> Result<StratumJob> {
  Ok(StratumSession::connect(cfg, difficulty).await?.current_job)
}
