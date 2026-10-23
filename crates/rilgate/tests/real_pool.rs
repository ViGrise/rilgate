use anyhow::{Context, Result, ensure};
use futures_util::{SinkExt, StreamExt};
use rilgate_pow_randomx::randomx::verification::{hash_meets_difficulty, write_nonce};
use serde_json::{Value, json};
use std::{
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
  time::Duration,
};

fn solve(job: &Value, difficulty: u64, offset: usize) -> Result<(String, String)> {
  solve_cancellable(job, difficulty, offset, &AtomicBool::new(false))?.context("solver cancelled")
}

fn solve_cancellable(
  job: &Value,
  difficulty: u64,
  offset: usize,
  cancelled: &AtomicBool,
) -> Result<Option<(String, String)>> {
  if cancelled.load(Ordering::Relaxed) {
    return Ok(None);
  }
  let seed = hex::decode(job["seed_hash"].as_str().context("seed_hash")?)?;
  let mut blob = hex::decode(job["blob"].as_str().context("blob")?)?;
  let mut vm = randomx_core::RandomX::new(&seed)?;
  for value in 0..u32::MAX {
    if cancelled.load(Ordering::Relaxed) {
      return Ok(None);
    }
    let nonce = format!("{value:08x}");
    write_nonce(&mut blob, offset, &nonce)?;
    let hash = vm.hash(&blob)?;
    if hash_meets_difficulty(&hash, difficulty) {
      return Ok(Some((nonce, hex::encode(hash))));
    }
  }
  anyhow::bail!("nonce space exhausted")
}

/// Exercises a running gateway with real pool work and a native client solver.
#[tokio::test]
#[ignore = "requires RILGATE_E2E_GATEWAY_URL, a live pool and a 256 MiB client cache"]
async fn real_pool_challenge_and_upstream() -> Result<()> {
  let base = std::env::var("RILGATE_E2E_GATEWAY_URL")?;
  let prefix =
    std::env::var("RILGATE_E2E_PREFIX").unwrap_or_else(|_| "/.well-known/rilgate".into());
  let path = std::env::var("RILGATE_E2E_PATH").unwrap_or_else(|_| "/protected".into());
  let client = reqwest::Client::builder()
    .no_proxy()
    .redirect(reqwest::redirect::Policy::none())
    .timeout(Duration::from_secs(120))
    .build()?;
  let response = client.get(format!("{base}{path}")).send().await?;
  println!("GET {path}: {}", response.status());
  let html = response.text().await?;
  let token: String = serde_json::from_str(
    html
      .split("window.POW_PRESENTATION_TOKEN = ")
      .nth(1)
      .context("presentation token in challenge page")?
      .split(';')
      .next()
      .unwrap(),
  )?;
  let config: Value = serde_json::from_slice(
    &client
      .get(format!("{base}{prefix}/api/client-config"))
      .send()
      .await?
      .bytes()
      .await?,
  )?;
  let receipt = if config["enforce"] == "stratum_pool_accepted" {
    let url = format!(
      "{}{prefix}/api/ws?presentation_token={token}",
      base.replacen("http", "ws", 1)
    );
    let (mut ws, response) = tokio_tungstenite::connect_async(url).await?;
    println!("WebSocket upgrade: {}", response.status());
    let frame = tokio::time::timeout(Duration::from_secs(30), ws.next())
      .await?
      .context("WebSocket closed")??;
    let mut message: Value = serde_json::from_str(frame.to_text()?)?;
    'jobs: loop {
      ensure!(message["type"] == "job", "pool response: {message}");
      println!("Pool job: {message}");
      let difficulty = rilgate_protocol::difficulty::deserialize(message["difficulty"].clone())?;
      let job = message["job"].clone();
      let offset = message["nonce_offset"].as_u64().context("offset")? as usize;
      let cancelled = Arc::new(AtomicBool::new(false));
      let signal = cancelled.clone();
      let mut solving =
        tokio::task::spawn_blocking(move || solve_cancellable(&job, difficulty, offset, &signal));
      let (nonce, result) = tokio::select! {
        result = &mut solving => result??.context("solver cancelled")?,
        frame = tokio::time::timeout(Duration::from_secs(30), ws.next()) => {
          cancelled.store(true, Ordering::Relaxed);
          let _ = solving.await;
          let frame = frame?.context("WebSocket closed")??;
          message = serde_json::from_str(frame.to_text()?)?;
          continue 'jobs;
        }
      };
      ws.send(tokio_tungstenite::tungstenite::Message::Text(
        json!({"type":"share","job_id":message["job"]["job_id"],"nonce":nonce,"result":result})
          .to_string()
          .into(),
      ))
      .await?;
      loop {
        let frame = tokio::time::timeout(Duration::from_secs(30), ws.next())
          .await?
          .context("WebSocket closed")??;
        let response: Value = serde_json::from_str(frame.to_text()?)?;
        println!("Share response: {response}");
        match response["type"].as_str() {
          Some("accepted") => {
            break 'jobs response["receipt"].as_str().context("receipt")?.to_owned();
          }
          Some("job") => message = response,
          Some("rejected")
            if response["reason"] == "Share must match the current job and required difficulty" =>
          {
            continue 'jobs;
          }
          _ => anyhow::bail!("pool did not accept share: {response}"),
        }
      }
    }
  } else {
    let response = client
      .post(format!("{base}{prefix}/api/challenge"))
      .header("content-type", "application/json")
      .body(json!({"presentation_token":token}).to_string())
      .send()
      .await?;
    let status = response.status();
    let text = response.text().await?;
    println!("POST /api/challenge: {status}, {text}");
    ensure!(status.is_success(), "pool job request failed: {text}");
    let challenge: Value = serde_json::from_str(&text)?;
    ensure!(challenge["algorithm"] == "randomx", "expected RandomX");
    let job = challenge["job"].clone();
    let offset = challenge["nonce_offset"].as_u64().context("offset")? as usize;
    let difficulty = rilgate_protocol::difficulty::deserialize(challenge["difficulty"].clone())?;
    let (nonce, hash) =
      tokio::task::spawn_blocking(move || solve(&job, difficulty, offset)).await??;
    println!("Solved nonce={nonce}, hash={hash}");
    let response = client
      .post(format!("{base}{prefix}/api/verify"))
      .header("content-type", "application/json")
      .body(json!({"challenge_token":challenge["challenge_token"],"nonce":nonce}).to_string())
      .send()
      .await?;
    let status = response.status();
    let text = response.text().await?;
    println!("POST /api/verify: {status}");
    ensure!(status.is_success(), "verification failed: {text}");
    serde_json::from_str::<Value>(&text)?["receipt"]
      .as_str()
      .context("receipt")?
      .to_owned()
  };
  let response = client
    .post(format!("{base}{prefix}/api/exchange"))
    .header("content-type", "application/json")
    .body(json!({"receipt":receipt}).to_string())
    .send()
    .await?;
  println!("POST /api/exchange: {}", response.status());
  ensure!(response.status().is_success(), "exchange failed");
  let cookie = response
    .headers()
    .get("set-cookie")
    .context("auth cookie")?
    .to_str()?
    .split(';')
    .next()
    .unwrap();
  let response = client
    .get(format!("{base}{path}"))
    .header("cookie", cookie)
    .send()
    .await?;
  println!("Authenticated GET {path}: {}", response.status());
  ensure!(response.status().is_success(), "upstream access failed");
  let body = response.text().await?;
  ensure!(
    body.contains("rilgate-e2e-upstream"),
    "unexpected upstream body: {body}"
  );
  if let Ok(operations) = std::env::var("RILGATE_E2E_OPERATIONS_URL") {
    let text = client
      .get(format!("{operations}{prefix}/api/metrics"))
      .send()
      .await?
      .text()
      .await?;
    for line in text
      .lines()
      .filter(|line| line.starts_with("rilgate_randomx_hash"))
    {
      println!("{line}");
    }
  }
  Ok(())
}
