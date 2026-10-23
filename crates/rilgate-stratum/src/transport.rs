use crate::StratumConfig;
use anyhow::{Context, Result, ensure};
use rustls::pki_types::{CertificateDer, ServerName, pem::PemObject};
use std::sync::Arc;
use tokio::{
  io::{AsyncRead, AsyncWrite},
  net::TcpStream,
};

pub trait AsyncStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> AsyncStream for T {}
pub type Stream = Box<dyn AsyncStream>;

pub async fn connect_transport(config: &StratumConfig) -> Result<Stream> {
  let stream = TcpStream::connect((config.host.as_str(), config.port))
    .await
    .context("Stratum pool TCP connection failed")?;
  if !config.tls.enabled {
    return Ok(Box::new(stream));
  }
  let mut roots = rustls::RootCertStore::empty();
  if let Some(path) = &config.tls.ca {
    let pem = tokio::fs::read(path)
      .await
      .context("Unable to read Stratum pool CA")?;
    let certs = CertificateDer::pem_slice_iter(&pem)
      .collect::<Result<Vec<_>, _>>()
      .context("Invalid Stratum pool CA PEM")?;
    ensure!(
      !certs.is_empty(),
      "Stratum pool CA contains no certificates"
    );
    for cert in certs {
      roots
        .add(cert)
        .context("Invalid Stratum pool CA certificate")?;
    }
  } else {
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
  }
  let client = rustls::ClientConfig::builder()
    .with_root_certificates(roots)
    .with_no_client_auth();
  let name = ServerName::try_from(
    config
      .tls
      .server_name
      .clone()
      .unwrap_or_else(|| config.host.clone()),
  )
  .context("Invalid Stratum pool TLS server name")?;
  let stream = tokio_rustls::TlsConnector::from(Arc::new(client))
    .connect(name, stream)
    .await
    .context("Stratum pool TLS handshake failed")?;
  Ok(Box::new(stream))
}
