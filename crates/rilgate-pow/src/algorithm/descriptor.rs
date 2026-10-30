use super::PowAlgorithm;
#[derive(Debug, Clone, serde::Serialize)]
pub struct BrowserSolverOptions {
  pub max_workers: u8,
  pub store_cache: bool,
}
#[derive(Debug, Clone, serde::Serialize)]
pub struct AlgorithmDescriptor {
  pub id: PowAlgorithm,
  pub version: u8,
  pub display_name: &'static str,
  pub solver_module_path: String,
  pub requires_consent: bool,
  pub memory_bytes: u64,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub browser: Option<BrowserSolverOptions>,
}
