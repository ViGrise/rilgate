use anyhow::{Context, Result, bail};
use rilgate_config::Configuration;
use std::{net::IpAddr, time::Duration};

const HINT: &str = "the gateway is not running at that address or server.operations.reload is off";

pub async fn run(config: &Configuration) -> Result<()> {
  match request(config).await {
    Ok(message) => {
      println!("{message}");
      Ok(())
    }
    Err(error) => {
      eprintln!("Reload failed: {error:#}");
      std::process::exit(1);
    }
  }
}

async fn request(config: &Configuration) -> Result<String> {
  let mut address = *config
    .operations_listen_addresses
    .first()
    .context("the reload route needs the operations listener (server.operations.listen)")?;
  if address.ip().is_unspecified() {
    address.set_ip(match address.ip() {
      IpAddr::V4(_) => IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
      IpAddr::V6(_) => IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
    });
  }
  let url = format!("http://{address}{}/reload", config.web.base_path);
  let response = reqwest::Client::builder()
    .http1_only()
    .no_proxy()
    .redirect(reqwest::redirect::Policy::none())
    .timeout(Duration::from_secs(10))
    .build()?
    .post(&url)
    .send()
    .await
    .with_context(|| format!("cannot reach {address}; {HINT}"))?;
  let status = response.status();
  if status == reqwest::StatusCode::NOT_FOUND {
    bail!("{address}: {HINT}");
  }
  let body: serde_json::Value = serde_json::from_slice(&response.bytes().await?)?;
  match (status.as_u16(), body["status"].as_str()) {
    (200, Some("reloaded")) => Ok("Policy reloaded".into()),
    (200, Some("unchanged")) => Ok("Policy is unchanged".into()),
    (409, Some("restart_required")) => {
      let sections = body["sections"]
        .as_array()
        .context("missing restart sections")?
        .iter()
        .filter_map(|section| section.as_str())
        .collect::<Vec<_>>()
        .join(", ");
      eprintln!("Configuration change needs a restart: {sections}");
      std::process::exit(1);
    }
    _ => bail!(
      "{}",
      body["error"]
        .as_str()
        .unwrap_or("unexpected reload response")
    ),
  }
}
