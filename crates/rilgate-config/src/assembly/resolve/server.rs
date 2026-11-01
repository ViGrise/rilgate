use crate::{
  ServerConfig, UpstreamConfig,
  assembly::{listen_addresses, listen_path},
  schema::Document,
};
use anyhow::{Context, Result, ensure};
use std::net::SocketAddr;

const MAX_CLIENT_IP_HEADERS: usize = 8;
const RESERVED_CLIENT_IP_HEADERS: [&str; 12] = [
  "host",
  "cookie",
  "content-length",
  "transfer-encoding",
  "connection",
  "upgrade",
  "te",
  "trailer",
  "keep-alive",
  "proxy-connection",
  "proxy-authorization",
  "x-request-id",
];

fn client_ip_headers(names: &[String]) -> Result<Vec<http::HeaderName>> {
  ensure!(
    names.len() <= MAX_CLIENT_IP_HEADERS,
    "server.upstream.client_ip_headers: at most {MAX_CLIENT_IP_HEADERS} headers are supported"
  );
  let mut headers: Vec<http::HeaderName> = Vec::with_capacity(names.len());
  for (index, name) in names.iter().enumerate() {
    let header: http::HeaderName = name.parse().with_context(|| {
      format!("server.upstream.client_ip_headers[{index}]: expected an HTTP header name")
    })?;
    ensure!(
      !headers.contains(&header),
      "server.upstream.client_ip_headers[{index}]: duplicate header {header}"
    );
    ensure!(
      !RESERVED_CLIENT_IP_HEADERS.contains(&header.as_str()),
      "server.upstream.client_ip_headers[{index}]: {header} cannot carry the client address"
    );
    headers.push(header);
  }
  Ok(headers)
}

impl Document {
  pub(super) fn resolve_listen_addresses(&self) -> Result<(Vec<SocketAddr>, Vec<SocketAddr>)> {
    let addresses = listen_addresses(&self.server.listen, "server.listen", 1)?;
    let operations_listen_addresses = listen_addresses(
      &self.server.operations.listen,
      "server.operations.listen",
      0,
    )?;
    for (index, address) in operations_listen_addresses.iter().enumerate() {
      ensure!(
        !addresses.contains(address),
        "{}: address {address} overlaps server.listen",
        listen_path(
          &self.server.operations.listen,
          index,
          "server.operations.listen"
        )
      );
    }
    Ok((addresses, operations_listen_addresses))
  }

  pub(super) fn resolve_server(&self) -> Result<ServerConfig> {
    Ok(ServerConfig {
      operations_reload: self.server.operations.reload,
      watch_config: self.server.watch_config,
      max_connections: self.server.max_connections,
      max_requests: self.server.max_requests,
      h2_max_streams: self.server.h2_max_streams,
      trusted_proxies: self
        .server
        .trusted_proxies
        .iter()
        .enumerate()
        .map(|(index, net)| {
          net.parse().with_context(|| {
            format!("server.trusted_proxies[{index}]: expected an IPv4 or IPv6 CIDR")
          })
        })
        .collect::<Result<_>>()?,
      client_ip_header: self
        .server
        .client_ip_header
        .as_ref()
        .map(|name| name.parse())
        .transpose()
        .context("server.client_ip_header: expected an HTTP header name")?,
      decision_log: self.server.decision_log,
      metrics_enabled: self.server.metrics_enabled,
      readiness_timeout: self.server.readiness_timeout.0,
      request_timeout: self.server.request_timeout.0,
    })
  }

  pub(super) fn resolve_upstream(&self) -> Result<UpstreamConfig> {
    Ok(UpstreamConfig {
      url: self.server.upstream.url.clone(),
      preserve_host: self.server.upstream.preserve_host,
      max_body_bytes: self.server.upstream.max_body_size.0,
      client_ip_headers: client_ip_headers(&self.server.upstream.client_ip_headers)?,
      connect_timeout: self.server.upstream.timeout.connect.0,
      read_timeout: self.server.upstream.timeout.read.0,
    })
  }
}
