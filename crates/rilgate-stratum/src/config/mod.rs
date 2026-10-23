use rilgate_crypto::Secret;
#[derive(Debug, Clone)]
pub struct StratumConfig {
  pub name: String,
  pub difficulty_mismatches: std::sync::Arc<std::sync::atomic::AtomicU64>,
  pub tls: TlsConfig,
  pub proxies: Option<Vec<Proxy>>,
  pub user: Secret,
  pub host: String,
  pub port: u16,
  pub password: Secret,
  pub agent: String,
  pub connect_timeout: std::time::Duration,
  pub submit_timeout: std::time::Duration,
}

#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct TlsConfig {
  pub enabled: bool,
  pub server_name: Option<String>,
  pub ca: Option<std::path::PathBuf>,
}

impl TlsConfig {
  pub fn validate(&self) -> anyhow::Result<()> {
    if let Some(name) = &self.server_name {
      anyhow::ensure!(
        rustls::pki_types::ServerName::try_from(name.as_str()).is_ok(),
        "expected a DNS name or an IP address"
      );
    }
    Ok(())
  }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Proxy {
  pub name: String,
  pub url: Secret,
  #[serde(default)]
  pub username: Option<Secret>,
  #[serde(default)]
  pub password: Option<Secret>,
}

impl Proxy {
  pub fn endpoint(&self) -> anyhow::Result<url::Url> {
    let url =
      url::Url::parse(self.url.expose()).map_err(|_| anyhow::anyhow!("expected a proxy URL"))?;
    let authority = self
      .url
      .expose()
      .split_once("://")
      .map(|(_, authority)| authority)
      .unwrap_or_default();
    let explicit_port = authority
      .rsplit_once(':')
      .is_some_and(|(_, port)| port.parse::<u16>().is_ok_and(|port| port > 0));
    anyhow::ensure!(
      explicit_port
        && !authority.contains('/')
        && matches!(url.scheme(), "socks5" | "socks5h" | "http")
        && url.host_str().is_some()
        && url.port_or_known_default().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(url.path(), "" | "/")
        && url.query().is_none()
        && url.fragment().is_none(),
      "expected socks5, socks5h or http with HOST:PORT and no credentials, path, query or fragment"
    );
    anyhow::ensure!(
      self.username.is_some() == self.password.is_some(),
      "set both username and password, or neither"
    );
    if url.scheme() != "http"
      && let (Some(user), Some(password)) = (&self.username, &self.password)
    {
      anyhow::ensure!(
        (1..=255).contains(&user.expose().len()) && (1..=255).contains(&password.expose().len()),
        "proxy credentials must be 1 to 255 bytes"
      );
    }
    Ok(url)
  }
}
