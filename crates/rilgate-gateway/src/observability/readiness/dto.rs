use serde::Serialize;

#[derive(Serialize)]
pub(super) struct Readiness {
  pub ready: bool,
  pub upstream: bool,
  pub pool: Option<bool>,
  pub remote: Option<bool>,
  pub ip_sets: bool,
}
