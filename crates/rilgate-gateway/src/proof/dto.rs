use rilgate_protocol::PowEnforcement;
use serde::{Deserialize, Serialize};
#[derive(Debug, Serialize)]
pub struct ClientConfigResponse {
  pub algorithms: Vec<rilgate_pow::algorithm::AlgorithmDescriptor>,
  pub enforce: PowEnforcement,
  pub internal_prefix: String,
  pub solver_module_url: String,
  pub websocket_url: String,
}

#[derive(Debug, Deserialize)]
pub struct ChallengeRequest {
  pub presentation_token: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifyRequest {
  pub challenge_token: String,
  pub nonce: String,
  #[serde(default)]
  pub set_cookie: bool,
}

#[derive(Debug, Serialize)]
pub struct VerifyResponse {
  pub receipt: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub cookie_set: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ExchangeRequest {
  pub receipt: String,
}

#[derive(Debug, Serialize)]
pub struct ExchangeResponse {
  pub status: &'static str,
}
