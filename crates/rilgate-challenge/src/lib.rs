pub mod authorization;
pub mod challenge;
pub mod engine;
mod tokens;
pub use engine::config::ChallengeConfig;
pub use engine::service::ChallengeService;

/// A server-issued token and the claims used to sign it.
pub struct IssuedToken<C> {
  pub token: String,
  pub claims: C,
}
