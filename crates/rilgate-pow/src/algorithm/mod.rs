mod contract;
mod descriptor;
mod refresh;
mod registry;
pub use contract::{Algorithm, AlgorithmFuture, HashDiagnostics, VerifierDiagnostics};
pub use descriptor::{AlgorithmDescriptor, BrowserSolverOptions};
pub use refresh::RefreshAlgorithm;
pub use registry::AlgorithmRegistry;
pub use rilgate_protocol::PowAlgorithm;

#[derive(Debug)]
pub struct WorkUnavailable;
impl std::fmt::Display for WorkUnavailable {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str("All Stratum pools are unavailable")
  }
}
impl std::error::Error for WorkUnavailable {}
