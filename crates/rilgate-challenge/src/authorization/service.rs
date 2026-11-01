use crate::{
  ChallengeService,
  authorization::binding::{BindingInput, BindingMismatch},
  tokens::validation::{unix_now, validate_lifetime},
};
use anyhow::{Context, Result, anyhow};
use rilgate_protocol::{PowAlgorithm, PowEnforcement, claims::*};
use uuid::Uuid;
pub struct IssuedAuthToken {
  pub token: String,
  pub claims: AuthClaims,
  pub network_merged: bool,
}

impl ChallengeService {
  pub fn issue_stratum_pool_receipt(&self, difficulty: u64, host: String) -> Result<String> {
    self.issue_receipt(
      host,
      Uuid::now_v7().to_string(),
      PowAlgorithm::RandomX,
      difficulty,
      PowEnforcement::StratumPoolAccepted,
    )
  }

  pub(crate) fn issue_receipt(
    &self,
    host: String,
    challenge_id: String,
    algorithm: PowAlgorithm,
    difficulty: u64,
    enforce: PowEnforcement,
  ) -> Result<String> {
    let now = unix_now()?;
    let claims = ReceiptClaims {
      host,
      v: 1,
      challenge_id,
      algorithm,
      difficulty,
      enforce,
      issued_at: now,
      expires_at: now + self.cfg.receipt_ttl_seconds as i64,
    };

    self.tokens.receipt.sign_json(&claims)
  }

  pub fn exchange_receipt(
    &self,
    receipt: &str,
    request_host: &str,
    binding: &BindingInput<'_>,
  ) -> Result<String> {
    Ok(
      self
        .exchange_receipt_with_claims(receipt, request_host, binding, None)?
        .token,
    )
  }

  pub fn exchange_receipt_with_claims(
    &self,
    receipt: &str,
    request_host: &str,
    binding: &BindingInput<'_>,
    previous: Option<&str>,
  ) -> Result<IssuedAuthToken> {
    let receipt: ReceiptClaims = self
      .tokens
      .receipt
      .verify_json(receipt)
      .context("Receipt signature validation failed")?;

    validate_lifetime(
      receipt.v,
      receipt.issued_at,
      receipt.expires_at,
      self.cfg.receipt_ttl_seconds,
    )?;
    if receipt.host != request_host {
      return Err(anyhow!("Proof receipt host does not match request host"));
    }

    if receipt.enforce != self.cfg.enforce {
      return Err(anyhow!("Proof receipt uses a different enforcement mode"));
    }

    self
      .algorithms
      .get(receipt.algorithm)?
      .validate_difficulty(receipt.difficulty)?;

    self.issue_auth_token(
      receipt.host,
      receipt.algorithm,
      receipt.difficulty,
      receipt.enforce,
      receipt.issued_at,
      binding,
      previous,
    )
  }

  #[allow(clippy::too_many_arguments)]
  pub(crate) fn issue_auth_token(
    &self,
    host: String,
    algorithm: PowAlgorithm,
    difficulty: u64,
    enforce: PowEnforcement,
    issued_at: i64,
    binding: &BindingInput<'_>,
    previous: Option<&str>,
  ) -> Result<IssuedAuthToken> {
    let mut claims = AuthClaims {
      host,
      v: 1,
      algorithm,
      difficulty,
      enforce,
      issued_at,
      expires_at: issued_at + self.cfg.auth_ttl_seconds as i64,
      binding: self.cfg.auth_binding.digest(&self.binding_key, binding),
      networks: None,
    };

    let mut network_merged = false;
    if let Some(ip) = self
      .cfg
      .auth_binding
      .client_ip
      .filter(|ip| ip.max_networks > 1)
    {
      let now = unix_now()?;
      claims.binding = self
        .cfg
        .auth_binding
        .client_digest(&self.binding_key, binding);
      let network = NetworkBinding {
        digest: self
          .cfg
          .auth_binding
          .network_digest(&self.binding_key, binding)
          .expect("IP binding is enabled"),
        expires_at: claims.expires_at,
      };
      let mut networks = Vec::new();
      if let Some(previous) = previous
        .and_then(|token| self.tokens.auth.verify_json::<AuthClaims>(token).ok())
        .filter(|old| {
          validate_lifetime(
            old.v,
            old.issued_at,
            old.expires_at,
            self.cfg.auth_ttl_seconds,
          )
          .is_ok()
            && old.host == claims.host
            && old.enforce == self.cfg.enforce
            && old.binding == claims.binding
            && old.algorithm == claims.algorithm
            && old.networks.is_some()
        })
      {
        networks = previous
          .networks
          .unwrap()
          .into_iter()
          .filter(|entry| entry.expires_at > now && entry.digest != network.digest)
          .collect();
        claims.difficulty = claims.difficulty.min(previous.difficulty);
        network_merged = true;
      }
      networks.push(network);
      networks.sort_by_key(|entry| std::cmp::Reverse(entry.expires_at));
      networks.truncate(ip.max_networks as usize);
      claims.networks = Some(networks);
    }

    validate_lifetime(
      claims.v,
      claims.issued_at,
      claims.expires_at,
      self.cfg.auth_ttl_seconds,
    )?;
    let token = self.tokens.auth.sign_json(&claims)?;
    Ok(IssuedAuthToken {
      token,
      claims,
      network_merged,
    })
  }

  pub fn verify_auth_token(
    &self,
    token: &str,
    request_host: &str,
    binding: &BindingInput<'_>,
  ) -> Result<AuthClaims> {
    let claims: AuthClaims = self.tokens.auth.verify_json(token)?;

    validate_lifetime(
      claims.v,
      claims.issued_at,
      claims.expires_at,
      self.cfg.auth_ttl_seconds,
    )?;
    if claims.host != request_host || claims.enforce != self.cfg.enforce {
      return Err(anyhow!("Access token scope does not match request"));
    }
    self
      .algorithms
      .get(claims.algorithm)?
      .validate_difficulty(claims.difficulty)?;
    if let Some(networks) = &claims.networks {
      let now = unix_now()?;
      let expected_network = self
        .cfg
        .auth_binding
        .network_digest(&self.binding_key, binding);
      if !self
        .cfg
        .auth_binding
        .client_ip
        .is_some_and(|ip| ip.max_networks > 1)
        || claims.binding
          != self
            .cfg
            .auth_binding
            .client_digest(&self.binding_key, binding)
        || !networks.iter().any(|entry| {
          entry.expires_at > now && Some(entry.digest.as_str()) == expected_network.as_deref()
        })
      {
        return Err(BindingMismatch.into());
      }
    } else if let Some(expected) = self.cfg.auth_binding.digest(&self.binding_key, binding)
      && claims.binding.as_deref() != Some(expected.as_str())
    {
      return Err(BindingMismatch.into());
    }

    Ok(claims)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    ChallengeConfig,
    authorization::binding::{AuthBinding, ClientIpBinding, UserAgentBinding},
  };
  use http::HeaderMap;
  use rilgate_pow::{AlgorithmRegistry, RefreshAlgorithm};
  use std::sync::Arc;

  fn service(max_networks: u8) -> ChallengeService {
    let cfg = ChallengeConfig {
      enforce: PowEnforcement::LocalVerify,
      default_algorithm: PowAlgorithm::Refresh,
      signing_key: rilgate_crypto::Secret("test-signing-key".into()),
      auth_signing_key: rilgate_crypto::Secret("test-auth-key".into()),
      signing_keys: Default::default(),
      auth_signing_keys: Default::default(),
      challenge_ttl_seconds: 60,
      receipt_ttl_seconds: 30,
      auth_ttl_seconds: 300,
      auth_binding: AuthBinding {
        client_ip: Some(ClientIpBinding {
          max_networks,
          ..Default::default()
        }),
        user_agent: Some(UserAgentBinding::Family),
        ..Default::default()
      },
      policy: rilgate_policy::PolicyDocument {
        policy: rilgate_policy::policy::schema::Definition {
          default_action: Some(rilgate_policy::policy::schema::Action::Allow),
          rules: Some(vec![]),
          ..Default::default()
        },
        ..Default::default()
      },
    };
    let mut registry = AlgorithmRegistry::default();
    registry
      .register(Arc::new(RefreshAlgorithm::new(60)))
      .unwrap();
    ChallengeService::with_registry(Arc::new(cfg), registry).unwrap()
  }

  fn input<'a>(ip: &str, headers: &'a HeaderMap) -> BindingInput<'a> {
    BindingInput {
      client_ip: Some(ip.parse().unwrap()),
      headers,
      trusted_peer: false,
    }
  }

  fn browser(name: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("user-agent", name.parse().unwrap());
    headers
  }

  fn issue(
    service: &ChallengeService,
    ip: &str,
    at: i64,
    difficulty: u64,
    previous: Option<&str>,
  ) -> IssuedAuthToken {
    service
      .issue_auth_token(
        "example.test".into(),
        PowAlgorithm::Refresh,
        difficulty,
        PowEnforcement::LocalVerify,
        at,
        &input(ip, &browser("Chrome/130")),
        previous,
      )
      .unwrap()
  }

  #[test]
  fn merges_proven_networks_with_independent_expiry_and_lower_difficulty() {
    let service = service(3);
    let now = unix_now().unwrap();
    let a = issue(&service, "192.0.2.10", now - 30, 2, None);
    let b = issue(&service, "198.51.100.10", now - 20, 5, Some(&a.token));
    assert!(b.network_merged);
    assert_eq!(b.claims.difficulty, 2);
    let entries = b.claims.networks.as_ref().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(
      entries[1].digest,
      a.claims.networks.as_ref().unwrap()[0].digest
    );
    assert_eq!(entries[1].expires_at, a.claims.expires_at);
    let c = issue(&service, "203.0.113.10", now - 10, 1, Some(&b.token));
    assert_eq!(c.claims.difficulty, 1);
    let d = issue(&service, "203.0.114.10", now, 4, Some(&c.token));
    let entries = d.claims.networks.as_ref().unwrap();
    assert_eq!(entries.len(), 3);
    assert!(
      !entries
        .iter()
        .any(|entry| entry.digest == a.claims.networks.as_ref().unwrap()[0].digest)
    );
    let mut expired = b.claims;
    expired.networks.as_mut().unwrap()[1].expires_at = now;
    let token = service.tokens.auth.sign_json(&expired).unwrap();
    let fresh = issue(&service, "203.0.113.10", now, 4, Some(&token));
    assert_eq!(fresh.claims.networks.as_ref().unwrap().len(), 2);
    assert_eq!(fresh.claims.issued_at, now);
    assert_eq!(fresh.claims.expires_at, now + 300);
  }

  #[test]
  fn incompatible_previous_cookies_start_one_network() {
    let service = service(3);
    let now = unix_now().unwrap();
    let a = issue(&service, "192.0.2.10", now, 2, None);
    for condition in [
      "browser",
      "algorithm",
      "host",
      "enforcement",
      "legacy",
      "signature",
      "expired",
    ] {
      let mut claims = a.claims.clone();
      match condition {
        "browser" => {
          claims.binding = service.cfg.auth_binding.client_digest(
            &service.binding_key,
            &input("192.0.2.10", &browser("Firefox/130")),
          )
        }
        "algorithm" => claims.algorithm = PowAlgorithm::Fast,
        "host" => claims.host = "other.test".into(),
        "enforcement" => claims.enforce = PowEnforcement::StratumPoolAccepted,
        "legacy" => {
          claims.networks = None;
          claims.binding = service.cfg.auth_binding.digest(
            &service.binding_key,
            &input("192.0.2.10", &browser("Chrome/130")),
          );
        }
        "expired" => {
          claims.issued_at = now - 301;
          claims.expires_at = now - 1;
        }
        "signature" => {}
        _ => unreachable!(),
      }
      let token = if condition == "signature" {
        rilgate_crypto::token::sign_json(
          &claims,
          b"other-auth-key",
          rilgate_crypto::token::TokenPurpose::Auth,
        )
        .unwrap()
      } else {
        service.tokens.auth.sign_json(&claims).unwrap()
      };
      let fresh = issue(&service, "198.51.100.10", now, 5, Some(&token));
      assert!(!fresh.network_merged, "{condition}");
      assert_eq!(fresh.claims.networks.unwrap().len(), 1, "{condition}");
      assert_eq!(fresh.claims.difficulty, 5, "{condition}");
    }
  }

  #[test]
  fn verifies_only_live_proven_networks_for_the_bound_browser() {
    let service = service(3);
    let now = unix_now().unwrap();
    let a = issue(&service, "192.0.2.10", now - 10, 2, None);
    let b = issue(&service, "198.51.100.10", now, 5, Some(&a.token));
    let headers = browser("Chrome/131");
    for ip in ["192.0.2.10", "198.51.100.10"] {
      service
        .verify_auth_token(&b.token, "example.test", &input(ip, &headers))
        .unwrap();
    }
    let mismatch = |service: &ChallengeService, token: &str, ip: &str, headers: &HeaderMap| {
      assert!(
        service
          .verify_auth_token(token, "example.test", &input(ip, headers))
          .unwrap_err()
          .is::<BindingMismatch>()
      );
    };
    mismatch(&service, &b.token, "203.0.113.10", &headers);
    mismatch(&service, &b.token, "192.0.2.10", &browser("Firefox/130"));
    let mut expired = b.claims;
    expired.networks.as_mut().unwrap()[1].expires_at = now;
    let token = service.tokens.auth.sign_json(&expired).unwrap();
    mismatch(&service, &token, "192.0.2.10", &headers);
    let mut single = service.cfg.as_ref().clone();
    single.auth_binding.client_ip.as_mut().unwrap().max_networks = 1;
    let mut registry = AlgorithmRegistry::default();
    registry
      .register(Arc::new(RefreshAlgorithm::new(60)))
      .unwrap();
    let single = ChallengeService::with_registry(Arc::new(single), registry).unwrap();
    mismatch(&single, &b.token, "198.51.100.10", &headers);
  }
}
