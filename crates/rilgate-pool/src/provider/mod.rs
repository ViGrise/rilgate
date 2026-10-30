mod connection;
mod contract;
mod stratum_pool;
pub use contract::WorkProvider;
pub use stratum_pool::StratumPoolProvider;

#[derive(Debug)]
pub struct AllPoolsUnavailable;
impl std::fmt::Display for AllPoolsUnavailable {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str("All Stratum pools are unavailable")
  }
}
impl std::error::Error for AllPoolsUnavailable {}
