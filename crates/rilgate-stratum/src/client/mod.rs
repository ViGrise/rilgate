use std::{collections::VecDeque, time::Duration};

use crate::transport::{Stream, connect_via};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use tokio::{
  io::{ReadHalf, WriteHalf},
  time::timeout,
};

use crate::{
  StratumConfig,
  codec::{JsonLineReader, write_json_line},
  job::parse_job,
};
use rilgate_protocol::StratumJob;

// Shares for the last jobs go to the pool, which decides whether each job is still valid.
const RECENT_JOBS: usize = 3;

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
  reader: JsonLineReader<ReadHalf<Stream>>,
  writer: WriteHalf<Stream>,
  pub rpc_id: String,
  next_request_id: u64,
  pub current_job: StratumJob,
  recent_job_ids: VecDeque<String>,
  submit_timeout: Duration,
  pool_name: String,
  requested_difficulty: u64,
  mismatch_reported: bool,
  mismatch_counter: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl StratumSession {
  pub async fn connect(cfg: &StratumConfig, difficulty: u64) -> Result<Self> {
    Self::connect_via(cfg, None, difficulty).await
  }

  pub async fn connect_via(
    cfg: &StratumConfig,
    proxy: Option<&crate::config::Proxy>,
    difficulty: u64,
  ) -> Result<Self> {
    if difficulty < 1000 {
      return Err(anyhow!("Stratum pool difficulty must be at least 1000"));
    }
    let stream = timeout(cfg.connect_timeout, connect_via(cfg, proxy))
      .await
      .context("Stratum pool connection timed out")??;
    Self::login(cfg, stream, difficulty).await
  }

  pub async fn login(cfg: &StratumConfig, stream: Stream, difficulty: u64) -> Result<Self> {
    if difficulty < 1000 {
      return Err(anyhow!("Stratum pool difficulty must be at least 1000"));
    }
    let (read_half, write_half) = tokio::io::split(stream);
    let mut reader = JsonLineReader::new(read_half);
    let mut writer = write_half;

    let user = cfg
      .user
      .expose()
      .replace("{difficulty}", &difficulty.to_string());
    anyhow::ensure!(
      (1..=256).contains(&user.len()),
      "Stratum pool login must be 1 to 256 bytes"
    );
    let login = json!({
      "id": 1,
      "jsonrpc": "2.0",
      "method": "login",
      "params": {
        "login": user,
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

    let mut session = Self {
      reader,
      writer,
      rpc_id,
      next_request_id: 2,
      recent_job_ids: VecDeque::from([current_job.job_id.clone()]),
      current_job,
      submit_timeout: cfg.submit_timeout,
      pool_name: cfg.name.clone(),
      requested_difficulty: difficulty,
      mismatch_reported: false,
      mismatch_counter: cfg.difficulty_mismatches.clone(),
    };
    session.observe_difficulty();
    Ok(session)
  }

  fn observe_difficulty(&mut self) {
    if self.mismatch_reported {
      return;
    }
    let bytes = hex::decode(&self.current_job.target).expect("validated target");
    let (target, maximum) = if bytes.len() == 4 {
      (
        u64::from(u32::from_le_bytes(bytes.try_into().expect("4 byte target"))),
        u64::from(u32::MAX),
      )
    } else {
      (
        u64::from_le_bytes(bytes.try_into().expect("8 byte target")),
        u64::MAX,
      )
    };
    if target != maximum / self.requested_difficulty {
      let received = maximum.checked_div(target).unwrap_or(u64::MAX);
      self.mismatch_reported = true;
      self
        .mismatch_counter
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
      tracing::warn!(pool = %self.pool_name, requested = self.requested_difficulty, received, "Stratum pool difficulty differs from the request");
    }
  }

  pub async fn read_event(&mut self) -> Result<StratumEvent> {
    loop {
      let message = self.reader.next_json().await?;

      if message.get("method").and_then(Value::as_str) == Some("job") {
        let params = message
          .get("params")
          .ok_or_else(|| anyhow!("Stratum job notification has no params"))?;
        let job = parse_job(params)?;
        self.recent_job_ids.push_back(job.job_id.clone());
        if self.recent_job_ids.len() > RECENT_JOBS {
          self.recent_job_ids.pop_front();
        }
        self.current_job = job.clone();
        self.observe_difficulty();
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

  pub fn is_recent_job(&self, job_id: &str) -> bool {
    self.recent_job_ids.iter().any(|recent| recent == job_id)
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

    if !self.is_recent_job(job_id) {
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
