use super::{HashError, HasherKind, RandomXHasher};
use anyhow::{Context, anyhow, ensure};
use futures_util::{StreamExt, future::BoxFuture};
use rustls::{
  client::{
    WebPkiServerVerifier,
    danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
  },
  pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime, pem::PemObject},
};
use serde::{Deserialize, Serialize};
use std::{
  path::Path,
  sync::Arc,
  time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Copy)]
pub(super) enum UnavailableReason {
  Deadline,
  Timeout,
  Connection,
  Overloaded,
  Protocol,
}
impl std::fmt::Display for UnavailableReason {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{self:?}")
  }
}
impl std::error::Error for UnavailableReason {}
fn unavailable(reason: UnavailableReason, error: impl Into<anyhow::Error>) -> HashError {
  HashError::Unavailable(error.into().context(reason))
}
fn transport(error: reqwest::Error) -> HashError {
  unavailable(
    if error.is_timeout() {
      UnavailableReason::Timeout
    } else {
      UnavailableReason::Connection
    },
    error,
  )
}

async fn read_limited(response: reqwest::Response) -> Result<Vec<u8>, HashError> {
  let mut stream = response.bytes_stream();
  let mut body = Vec::new();
  while let Some(chunk) = stream.next().await {
    let chunk = chunk.map_err(transport)?;
    if chunk.len() > 4096 - body.len() {
      return Err(unavailable(
        UnavailableReason::Protocol,
        anyhow!("Remote response exceeds 4 KiB"),
      ));
    }
    body.extend_from_slice(&chunk);
  }
  Ok(body)
}

pub struct RemoteHasher {
  client: reqwest::Client,
  endpoint: reqwest::Url,
  timeout: Duration,
}

impl RemoteHasher {
  pub fn new(
    endpoint: &str,
    server_name: &str,
    timeout: Duration,
    ca: Option<&Path>,
    client_cert: Option<&Path>,
    client_key: Option<&Path>,
  ) -> anyhow::Result<Self> {
    let endpoint = reqwest::Url::parse(endpoint)?;
    let roots = trust_roots(ca)?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier =
      WebPkiServerVerifier::builder_with_provider(Arc::new(roots.clone()), provider.clone())
        .build()?;
    let builder = rustls::ClientConfig::builder_with_provider(provider)
      .with_safe_default_protocol_versions()?
      .with_root_certificates(roots);
    let mut tls = match (client_cert, client_key) {
      (Some(cert), Some(key)) => {
        let cert =
          std::fs::read(cert).context("pow.algorithms.randomx.verify.remote.tls.client_cert")?;
        let key =
          std::fs::read(key).context("pow.algorithms.randomx.verify.remote.tls.client_key")?;
        let certs = CertificateDer::pem_slice_iter(&cert).collect::<Result<Vec<_>, _>>()?;
        builder.with_client_auth_cert(certs, PrivateKeyDer::from_pem_slice(&key)?)?
      }
      (None, None) => builder.with_no_client_auth(),
      _ => anyhow::bail!("remote TLS client certificate and key must both be set or absent"),
    };
    let name = ServerName::try_from(server_name.trim_matches(['[', ']']).to_owned())
      .context("remote TLS server_name")?;
    tls
      .dangerous()
      .set_certificate_verifier(Arc::new(CertificateName {
        inner: verifier,
        name,
      }));
    let client = reqwest::Client::builder()
      .use_preconfigured_tls(tls)
      .redirect(reqwest::redirect::Policy::none())
      .no_proxy()
      .build()?;
    Ok(Self {
      client,
      endpoint,
      timeout,
    })
  }

  pub async fn ready(&self, timeout: Duration) -> bool {
    self
      .client
      .get(self.endpoint.join("/v1/ready").expect("valid endpoint"))
      .timeout(timeout)
      .send()
      .await
      .is_ok_and(|response| response.status() == reqwest::StatusCode::OK)
  }

  async fn request(
    &self,
    seed: Vec<u8>,
    input: Vec<u8>,
    deadline: Instant,
  ) -> Result<[u8; 32], HashError> {
    if !(1..=256).contains(&seed.len()) || !(1..=256).contains(&input.len()) {
      return Err(HashError::Invalid(anyhow!(
        "Invalid RandomX hash dimensions"
      )));
    }
    let wall = SystemTime::now();
    let started = Instant::now();
    let remaining = deadline
      .saturating_duration_since(started)
      .min(self.timeout);
    if remaining.is_zero() {
      return Err(unavailable(
        UnavailableReason::Timeout,
        anyhow!("Remote hash deadline elapsed"),
      ));
    }
    // Flooring milliseconds keeps the wire deadline inside the caller's budget.
    let deadline_unix_ms = wall
      .duration_since(UNIX_EPOCH)
      .map_err(|error| unavailable(UnavailableReason::Protocol, error))?
      .saturating_add(remaining)
      .as_millis()
      .min(u64::MAX as u128) as u64;
    let body = serde_json::to_vec(&HashRequest {
      seed: hex::encode(seed),
      input: hex::encode(input),
      deadline_unix_ms,
    })
    .map_err(|error| unavailable(UnavailableReason::Protocol, error))?;
    let request = async {
      let response = self
        .client
        .post(
          self
            .endpoint
            .join("/v1/randomx/hash")
            .expect("valid endpoint"),
        )
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .timeout(remaining)
        .send()
        .await
        .map_err(transport)?;
      let status = response.status();
      match status {
        reqwest::StatusCode::OK => {}
        reqwest::StatusCode::BAD_REQUEST => {
          return Err(HashError::Invalid(anyhow!("Remote hash request rejected")));
        }
        reqwest::StatusCode::SERVICE_UNAVAILABLE => {
          let body = read_limited(response).await?;
          let deadline = serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .is_some_and(|body| body["reason"] == "deadline");
          return Err(unavailable(
            if deadline {
              UnavailableReason::Deadline
            } else {
              UnavailableReason::Overloaded
            },
            anyhow!("Remote hasher unavailable"),
          ));
        }
        _ => {
          return Err(unavailable(
            UnavailableReason::Protocol,
            anyhow!("Unexpected remote hash status"),
          ));
        }
      }
      let body = read_limited(response).await?;
      let response: HashResponse = serde_json::from_slice(&body)
        .map_err(|error| unavailable(UnavailableReason::Protocol, error))?;
      let mut hash = [0; 32];
      hex::decode_to_slice(response.hash, &mut hash)
        .map_err(|error| unavailable(UnavailableReason::Protocol, error))?;
      Ok(hash)
    };
    let result = tokio::time::timeout_at((started + remaining).into(), request)
      .await
      .map_err(|error| unavailable(UnavailableReason::Timeout, error))?;
    if started.elapsed() >= remaining {
      return Err(unavailable(
        UnavailableReason::Timeout,
        anyhow!("Remote hash deadline elapsed"),
      ));
    }
    result
  }
}
impl RandomXHasher for RemoteHasher {
  fn kind(&self) -> HasherKind {
    HasherKind::Remote
  }
  fn hash(
    &self,
    seed: Vec<u8>,
    input: Vec<u8>,
    deadline: Instant,
  ) -> BoxFuture<'_, Result<[u8; 32], HashError>> {
    Box::pin(self.request(seed, input, deadline))
  }
  fn ready(&self, timeout: Duration) -> BoxFuture<'_, bool> {
    Box::pin(self.ready(timeout))
  }
}
#[derive(Serialize)]
struct HashRequest {
  seed: String,
  input: String,
  deadline_unix_ms: u64,
}
#[derive(Deserialize)]
struct HashResponse {
  hash: String,
}

#[derive(Debug)]
struct CertificateName {
  inner: Arc<WebPkiServerVerifier>,
  name: ServerName<'static>,
}
impl ServerCertVerifier for CertificateName {
  fn verify_server_cert(
    &self,
    end_entity: &CertificateDer<'_>,
    intermediates: &[CertificateDer<'_>],
    _: &ServerName<'_>,
    ocsp: &[u8],
    now: UnixTime,
  ) -> Result<ServerCertVerified, rustls::Error> {
    // Certificate identity can differ from the endpoint host.
    self
      .inner
      .verify_server_cert(end_entity, intermediates, &self.name, ocsp, now)
  }
  fn verify_tls12_signature(
    &self,
    message: &[u8],
    cert: &CertificateDer<'_>,
    dss: &rustls::DigitallySignedStruct,
  ) -> Result<HandshakeSignatureValid, rustls::Error> {
    self.inner.verify_tls12_signature(message, cert, dss)
  }
  fn verify_tls13_signature(
    &self,
    message: &[u8],
    cert: &CertificateDer<'_>,
    dss: &rustls::DigitallySignedStruct,
  ) -> Result<HandshakeSignatureValid, rustls::Error> {
    self.inner.verify_tls13_signature(message, cert, dss)
  }
  fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
    self.inner.supported_verify_schemes()
  }
}

#[cfg(test)]
mod tests;

fn trust_roots(ca: Option<&Path>) -> anyhow::Result<rustls::RootCertStore> {
  let mut roots = rustls::RootCertStore::empty();
  if let Some(path) = ca {
    let pem = std::fs::read(path).context("pow.algorithms.randomx.verify.remote.tls.ca")?;
    let certs = CertificateDer::pem_slice_iter(&pem).collect::<Result<Vec<_>, _>>()?;
    ensure!(
      !certs.is_empty(),
      "remote TLS CA file contains no certificates"
    );
    for cert in certs {
      roots.add(cert)?;
    }
  } else {
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
  }
  Ok(roots)
}
