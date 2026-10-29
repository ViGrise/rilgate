use crate::{LookupError, Resolver};
use async_trait::async_trait;
use hickory_resolver::TokioResolver;
use hickory_resolver::net::NetError;
use hickory_resolver::proto::rr::RData;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

/// A resolver that reads the DNS servers from the system configuration.
pub(crate) struct SystemResolver(TokioResolver);

impl SystemResolver {
  pub(crate) fn new(timeout: Duration) -> anyhow::Result<Self> {
    let mut builder = TokioResolver::builder_tokio()?;
    let options = builder.options_mut();
    options.timeout = timeout;
    options.attempts = 1;
    Ok(Self(builder.build()?))
  }
}

fn failure(error: NetError) -> LookupError {
  if error.is_no_records_found() {
    LookupError::NotFound
  } else {
    tracing::debug!(%error, "DNS lookup failed");
    LookupError::Failed
  }
}

/// Marks a name as fully qualified, so the search domains of the host do not apply.
fn absolute(name: &str) -> String {
  if name.ends_with('.') {
    name.to_string()
  } else {
    format!("{name}.")
  }
}

#[async_trait]
impl Resolver for SystemResolver {
  async fn ptr(&self, ip: IpAddr) -> Result<Vec<String>, LookupError> {
    let lookup = self.0.reverse_lookup(ip).await.map_err(failure)?;
    Ok(
      lookup
        .answers()
        .iter()
        .filter_map(|record| match &record.data {
          RData::PTR(name) => Some(name.0.to_string()),
          _ => None,
        })
        .collect(),
    )
  }

  async fn addresses(&self, name: &str) -> Result<Vec<IpAddr>, LookupError> {
    let lookup = self.0.lookup_ip(absolute(name)).await.map_err(failure)?;
    Ok(lookup.iter().collect())
  }

  async fn ipv4(&self, name: &str) -> Result<Vec<Ipv4Addr>, LookupError> {
    let lookup = self.0.ipv4_lookup(absolute(name)).await.map_err(failure)?;
    Ok(
      lookup
        .answers()
        .iter()
        .filter_map(|record| match &record.data {
          RData::A(address) => Some(address.0),
          _ => None,
        })
        .collect(),
    )
  }
}
