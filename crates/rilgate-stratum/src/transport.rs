use crate::StratumConfig;
use crate::config::Proxy;
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

#[derive(Clone)]
pub struct Transport {
  tls: Option<(tokio_rustls::TlsConnector, ServerName<'static>)>,
}

impl Transport {
  pub fn new(config: &StratumConfig) -> Result<Self> {
    let tls = if config.tls.enabled {
      Some(
        Self::tls(config)
          .with_context(|| format!("Stratum pool {} TLS configuration", config.name))?,
      )
    } else {
      None
    };
    Ok(Self { tls })
  }

  fn tls(config: &StratumConfig) -> Result<(tokio_rustls::TlsConnector, ServerName<'static>)> {
    let mut roots = rustls::RootCertStore::empty();
    if let Some(path) = &config.tls.ca {
      let pem = std::fs::read(path).context("Unable to read Stratum pool CA")?;
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
    let client = rustls::ClientConfig::builder_with_provider(Arc::new(
      rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
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
    Ok((tokio_rustls::TlsConnector::from(Arc::new(client)), name))
  }

  pub async fn connect(&self, config: &StratumConfig, proxy: Option<&Proxy>) -> Result<Stream> {
    let stream = match proxy {
      Some(proxy) => crate::proxy::connect(config, proxy).await?,
      None => TcpStream::connect((config.host.as_str(), config.port))
        .await
        .context("Stratum pool TCP connection failed")?,
    };
    if let Some((connector, name)) = &self.tls {
      Ok(Box::new(
        connector
          .connect(name.clone(), stream)
          .await
          .context("Stratum pool TLS handshake failed")?,
      ))
    } else {
      Ok(Box::new(stream))
    }
  }
}

pub async fn connect_transport(config: &StratumConfig) -> Result<Stream> {
  connect_via(config, None).await
}

pub async fn connect_via(config: &StratumConfig, proxy: Option<&Proxy>) -> Result<Stream> {
  Transport::new(config)?.connect(config, proxy).await
}
