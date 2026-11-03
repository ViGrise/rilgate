use rilgate_protocol::{PowAlgorithm, PowVerification};
use serde::{Deserialize, Serialize};
#[derive(Debug, Serialize)]
pub struct ClientAlgorithm {
  #[serde(flatten)]
  pub descriptor: rilgate_pow::algorithm::AlgorithmDescriptor,
  pub verification: PowVerification,
}

#[derive(Debug, Serialize)]
pub struct ClientConfigResponse {
  pub algorithms: Vec<ClientAlgorithm>,
  pub internal_prefix: String,
  pub solver_module_url: String,
  pub websocket_url: String,
}

#[derive(Debug, Serialize)]
pub struct SessionResponse {
  pub authenticated: bool,
}

#[derive(Debug, Deserialize)]
pub struct ChallengeRequest {
  pub presentation_token: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifyRequest {
  #[serde(default)]
  pub presentation_token: Option<String>,
  pub challenge_token: String,
  pub nonce: String,
  #[serde(default)]
  pub set_cookie: bool,
}

#[derive(Debug, Serialize)]
pub struct VerifyResponse {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub next: Option<NextStep>,
  pub receipt: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub cookie_set: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ExchangeRequest {
  #[serde(default)]
  pub presentation_token: Option<String>,
  pub receipt: String,
}

#[derive(Debug, Serialize)]
pub struct ExchangeResponse {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub next: Option<NextStep>,
  pub status: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct NextStep {
  pub presentation_token: String,
  pub step: usize,
  pub steps: usize,
  pub method: &'static str,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub algorithm: Option<PowAlgorithm>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub difficulty: Option<u64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub solver_module_url: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub hook: Option<serde_json::Value>,
}
