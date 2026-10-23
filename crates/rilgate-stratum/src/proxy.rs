use crate::{StratumConfig, config::Proxy};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::{TcpStream, lookup_host},
};
use tokio_socks::{TargetAddr, tcp::Socks5Stream};

pub(crate) async fn connect(config: &StratumConfig, proxy: &Proxy) -> Result<TcpStream> {
  let endpoint = proxy.endpoint()?;
  let host = endpoint.host_str().unwrap().trim_matches(['[', ']']);
  let stream = TcpStream::connect((host, endpoint.port_or_known_default().unwrap()))
    .await
    .context("Proxy TCP connection failed")?;
  if endpoint.scheme() == "http" {
    return http_connect(stream, config, proxy).await;
  }
  let target = if endpoint.scheme() == "socks5h" {
    TargetAddr::Domain(config.host.as_str().into(), config.port)
  } else {
    TargetAddr::Ip(
      lookup_host((config.host.as_str(), config.port))
        .await
        .context("Pool DNS lookup failed")?
        .next()
        .context("Pool DNS lookup returned no addresses")?,
    )
  };
  let stream = match (&proxy.username, &proxy.password) {
    (Some(user), Some(password)) => {
      Socks5Stream::connect_with_password_and_socket(
        stream,
        target,
        user.expose(),
        password.expose(),
      )
      .await
    }
    _ => Socks5Stream::connect_with_socket(stream, target).await,
  }
  .map_err(|_| anyhow::anyhow!("SOCKS5 handshake failed"))?;
  Ok(stream.into_inner())
}

async fn http_connect(
  mut stream: TcpStream,
  config: &StratumConfig,
  proxy: &Proxy,
) -> Result<TcpStream> {
  let authority = if config.host.contains(':') {
    format!("[{}]:{}", config.host, config.port)
  } else {
    format!("{}:{}", config.host, config.port)
  };
  let mut request = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n");
  if let (Some(user), Some(password)) = (&proxy.username, &proxy.password) {
    let credentials = STANDARD.encode(format!("{}:{}", user.expose(), password.expose()));
    request.push_str(&format!("Proxy-Authorization: Basic {credentials}\r\n"));
  }
  request.push_str("\r\n");
  stream
    .write_all(request.as_bytes())
    .await
    .context("HTTP proxy request failed")?;
  let mut response = Vec::new();
  loop {
    ensure!(response.len() < 8192, "HTTP proxy response exceeds 8 KiB");
    response.push(
      stream
        .read_u8()
        .await
        .context("HTTP proxy closed before response headers")?,
    );
    if response.ends_with(b"\r\n\r\n") {
      break;
    }
  }
  let status = response
    .split(|byte| *byte == b'\n')
    .next()
    .unwrap_or_default();
  let status = status.strip_suffix(b"\r").unwrap_or(status);
  let mut fields = status.split(|byte| *byte == b' ');
  ensure!(
    matches!(fields.next(), Some(b"HTTP/1.0" | b"HTTP/1.1")) && fields.next() == Some(b"200"),
    "HTTP proxy refused CONNECT"
  );
  Ok(stream)
}
