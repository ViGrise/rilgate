use crate::{
  ChallengeService,
  authorization::binding::{BindingInput, BindingMismatch},
  tokens::validation::{unix_now, validate_lifetime},
};
use anyhow::{Context, Result, anyhow};
use rilgate_protocol::{
  Grant, PowAlgorithm, PowGrant as StoredPowGrant, PowVerification, claims::*,
};
use uuid::Uuid;
pub enum NewGrant {
  Pow {
    algorithm: PowAlgorithm,
    difficulty: u64,
    verification: PowVerification,
    issued_at: i64,
    fallback_for: Option<String>,
  },
  Hook {
    name: String,
    issued_at: i64,
  },
}
#[derive(Debug)]
pub struct Access {
  pub version: u8,
  pub grants: Vec<Grant>,
  pub expires_at: i64,
}
pub struct IssuedAuthToken {
  pub token: String,
  pub expires_at: i64,
  pub grants: Vec<Grant>,
  pub network_merged: bool,
}

impl ChallengeService {
  pub fn issue_stratum_pool_receipt(&self, difficulty: u64, host: String) -> Result<String> {
    self.issue_receipt(
      host,
      Uuid::now_v7().to_string(),
      PowAlgorithm::RandomX,
      difficulty,
      PowVerification::PoolShare,
    )
  }

  pub(crate) fn issue_receipt(
    &self,
    host: String,
    challenge_id: String,
    algorithm: PowAlgorithm,
    difficulty: u64,
    verification: PowVerification,
  ) -> Result<String> {
    let now = unix_now()?;
    let claims = ReceiptClaims {
      host,
      v: 1,
      challenge_id,
      algorithm,
      difficulty,
      verification,
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
        .exchange_receipt_with_claims(receipt, request_host, binding, None, None)?
        .token,
    )
  }

  pub fn exchange_receipt_with_claims(
    &self,
    receipt: &str,
    request_host: &str,
    binding: &BindingInput<'_>,
    previous: Option<&str>,
    fallback_for: Option<String>,
  ) -> Result<IssuedAuthToken> {
    let receipt: ReceiptClaims = self
      .tokens
      .receipt
      .verify_json(receipt)
      .context("Receipt signature validation failed")?;

    validate_lifetime(
      receipt.v,
      &[1],
      receipt.issued_at,
      receipt.expires_at,
      self.cfg.receipt_ttl_seconds,
    )?;
    if receipt.host != request_host {
      return Err(anyhow!("Proof receipt host does not match request host"));
    }

    if receipt.verification != self.cfg.verification.of(receipt.algorithm) {
      return Err(anyhow!("Proof receipt uses a different verification mode"));
    }

    self
      .algorithms
      .get(receipt.algorithm)?
      .validate_difficulty(receipt.difficulty)?;

    self.issue_access(
      receipt.host,
      NewGrant::Pow {
        fallback_for,
        algorithm: receipt.algorithm,
        difficulty: receipt.difficulty,
        verification: receipt.verification,
        issued_at: receipt.issued_at,
      },
      binding,
      previous,
    )
  }

  pub fn issue_access(
    &self,
    host: String,
    grant: NewGrant,
    binding: &BindingInput<'_>,
    previous: Option<&str>,
  ) -> Result<IssuedAuthToken> {
    let now = unix_now()?;
    let (fresh, issued_at) = match grant {
      NewGrant::Pow {
        algorithm,
        difficulty,
        verification,
        issued_at,
        fallback_for,
      } => (
        Grant::Pow(StoredPowGrant {
          algorithm,
          difficulty,
          verification,
          fallback_for,
          expires_at: issued_at + self.cfg.auth_ttl_seconds as i64,
        }),
        issued_at,
      ),
      NewGrant::Hook { name, issued_at } => {
        let hook = self
          .cfg
          .hooks
          .get(&name)
          .ok_or_else(|| anyhow!("Unknown challenge hook"))?;
        (
          Grant::Hook(rilgate_protocol::HookGrant {
            name,
            expires_at: issued_at + hook.grant_ttl_seconds as i64,
          }),
          issued_at,
        )
      }
    };
    let mut claims = AuthClaims {
      host,
      v: 1,
      algorithm: match &fresh {
        Grant::Pow(g) => g.algorithm,
        Grant::Hook(_) => self.cfg.default_algorithm,
      },
      difficulty: match &fresh {
        Grant::Pow(g) => g.difficulty,
        Grant::Hook(_) => 0,
      },
      verification: match &fresh {
        Grant::Pow(g) => g.verification,
        Grant::Hook(_) => self.cfg.verification.of(self.cfg.default_algorithm),
      },
      issued_at,
      expires_at: fresh.expires_at(),
      binding: self.cfg.auth_binding.digest(&self.binding_key, binding),
      networks: None,
    };
    let old = previous.and_then(|token| self.parse_auth_token(token, &claims.host).ok());
    let mut grants = old
      .as_ref()
      .filter(|old| {
        previous.is_some_and(|token| self.verify_auth_token(token, &claims.host, binding).is_ok())
          || (self
            .cfg
            .auth_binding
            .client_ip
            .is_some_and(|ip| ip.max_networks > 1)
            && old.networks.is_some()
            && (old.v == 2
              || old
                .grants
                .iter()
                .all(|g| self.valid_grant(g, now, old.expires_at)))
            && old.binding
              == self
                .cfg
                .auth_binding
                .client_digest(&self.binding_key, binding))
      })
      .map(|old| old.grants.clone())
      .unwrap_or_default();
    let old_expiry = old.as_ref().map_or(now, |c| c.expires_at);
    grants.retain(|g| g.method_key() != fresh.method_key() && self.valid_grant(g, now, old_expiry));
    let v1 = grants.is_empty() && matches!(&fresh,Grant::Pow(g) if g.fallback_for.is_none());
    let network_merged = self.merge_networks(&mut claims, binding, old.as_ref(), !v1)?;
    let mut fresh = fresh;
    if let Grant::Pow(g) = &mut fresh {
      g.difficulty = claims.difficulty;
    }
    grants.push(fresh);
    let (token, expires_at) = if v1 {
      validate_lifetime(
        1,
        &[1, 2],
        claims.issued_at,
        claims.expires_at,
        self.cfg.auth_ttl_seconds,
      )?;
      (self.tokens.auth.sign_json(&claims)?, claims.expires_at)
    } else {
      grants.sort_by_key(Grant::method_key);
      let expires_at = grants
        .iter()
        .map(Grant::expires_at)
        .max()
        .expect("Access grant exists");
      let claims = AuthClaimsV2 {
        v: 2,
        host: claims.host,
        grants: grants.clone(),
        issued_at: now,
        expires_at,
        binding: claims.binding,
        networks: claims.networks,
      };
      validate_lifetime(2, &[1, 2], now, expires_at, self.cfg.auth_ttl_seconds)?;
      (self.tokens.auth.sign_json(&claims)?, expires_at)
    };
    Ok(IssuedAuthToken {
      token,
      expires_at,
      grants,
      network_merged,
    })
  }

  fn valid_grant(&self, grant: &Grant, now: i64, expires_at: i64) -> bool {
    grant.expires_at() > now
      && grant.expires_at() <= expires_at
      && match grant {
        Grant::Hook(g) => self.cfg.hooks.contains_key(&g.name),
        Grant::Pow(g) => {
          g.verification == self.cfg.verification.of(g.algorithm)
            && self
              .algorithms
              .get(g.algorithm)
              .is_ok_and(|backend| backend.validate_difficulty(g.difficulty).is_ok())
        }
      }
  }

  fn merge_networks(
    &self,
    claims: &mut AuthClaims,
    binding: &BindingInput<'_>,
    previous: Option<&AuthClaimsV2>,
    multiple: bool,
  ) -> Result<bool> {
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
      if let Some(previous) = previous.filter(|old| {
        old.binding == claims.binding
          && old.networks.is_some()
          && (old.v == 2
            || old
              .grants
              .iter()
              .all(|grant| self.valid_grant(grant, now, old.expires_at)))
          && (multiple
            || old
              .grants
              .iter()
              .any(|g| matches!(g, Grant::Pow(g) if g.algorithm == claims.algorithm)))
      }) {
        networks = previous
          .networks
          .as_ref()
          .unwrap()
          .iter()
          .filter(|entry| entry.expires_at > now && entry.digest != network.digest)
          .cloned()
          .collect();
        if !multiple
          && let Some(Grant::Pow(old)) = previous
            .grants
            .iter()
            .find(|g| matches!(g, Grant::Pow(g) if g.algorithm == claims.algorithm))
        {
          claims.difficulty = claims.difficulty.min(old.difficulty);
        }
        network_merged = true;
      }
      networks.push(network);
      networks.sort_by_key(|entry| std::cmp::Reverse(entry.expires_at));
      networks.truncate(ip.max_networks as usize);
      claims.networks = Some(networks);
    }
    Ok(network_merged)
  }

  fn parse_auth_token(&self, token: &str, request_host: &str) -> Result<AuthClaimsV2> {
    let claims: AnyAuthClaims = self.tokens.auth.verify_json(token)?;
    let claims = match claims {
      AnyAuthClaims::V1(c) => {
        anyhow::ensure!(c.v == 1, "Token version or lifetime is invalid or expired");
        validate_lifetime(
          c.v,
          &[1, 2],
          c.issued_at,
          c.expires_at,
          self.cfg.auth_ttl_seconds,
        )?;
        AuthClaimsV2 {
          host: c.host,
          v: c.v,
          grants: vec![Grant::Pow(StoredPowGrant {
            fallback_for: None,
            algorithm: c.algorithm,
            difficulty: c.difficulty,
            verification: c.verification,
            expires_at: c.expires_at,
          })],
          issued_at: c.issued_at,
          expires_at: c.expires_at,
          binding: c.binding,
          networks: c.networks,
        }
      }
      AnyAuthClaims::V2(c) => {
        anyhow::ensure!(c.v == 2, "Token version or lifetime is invalid or expired");
        c
      }
    };
    validate_lifetime(
      claims.v,
      &[1, 2],
      claims.issued_at,
      claims.expires_at,
      self.cfg.auth_ttl_seconds,
    )?;
    if claims.host != request_host {
      return Err(anyhow!("Access token scope does not match request"));
    }
    Ok(claims)
  }

  pub fn verify_auth_token(
    &self,
    token: &str,
    request_host: &str,
    binding: &BindingInput<'_>,
  ) -> Result<Access> {
    let mut claims = self.parse_auth_token(token, request_host)?;
    if claims.v == 1 {
      let Grant::Pow(grant) = &claims.grants[0] else {
        return Err(anyhow!("Invalid v1 grant"));
      };
      if grant.verification != self.cfg.verification.of(grant.algorithm) {
        return Err(anyhow!("Access token scope does not match request"));
      }
      self
        .algorithms
        .get(grant.algorithm)?
        .validate_difficulty(grant.difficulty)?;
    }
    let now = unix_now()?;
    claims
      .grants
      .retain(|g| self.valid_grant(g, now, claims.expires_at));
    if claims.grants.is_empty() {
      return Err(anyhow!("Access token has no valid grant"));
    }

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

    Ok(Access {
      version: claims.v,
      grants: claims.grants,
      expires_at: claims.expires_at,
    })
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
      hooks: Default::default(),
      verification: rilgate_protocol::VerificationModes::default(),
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
      .register(Arc::new(RefreshAlgorithm::new(60, 10)))
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

  struct Issued {
    token: String,
    claims: AuthClaims,
    network_merged: bool,
  }
  fn issue(
    service: &ChallengeService,
    ip: &str,
    at: i64,
    difficulty: u64,
    previous: Option<&str>,
  ) -> Issued {
    let issued = service
      .issue_access(
        "example.test".into(),
        NewGrant::Pow {
          fallback_for: None,
          algorithm: PowAlgorithm::Refresh,
          difficulty,
          verification: PowVerification::Gateway,
          issued_at: at,
        },
        &input(ip, &browser("Chrome/130")),
        previous,
      )
      .unwrap();
    let claims: AuthClaims = service.tokens.auth.verify_json(&issued.token).unwrap();
    Issued {
      token: issued.token,
      claims,
      network_merged: issued.network_merged,
    }
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
      "verification",
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
        "verification" => claims.verification = PowVerification::PoolShare,
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
      .register(Arc::new(RefreshAlgorithm::new(60, 10)))
      .unwrap();
    let single = ChallengeService::with_registry(Arc::new(single), registry).unwrap();
    mismatch(&single, &b.token, "198.51.100.10", &headers);
  }
  #[test]
  fn receipt_merge_keeps_both_algorithms_and_single_receipt_keeps_v1() {
    struct ReceiptAlgorithm(PowAlgorithm);
    impl rilgate_pow::Algorithm for ReceiptAlgorithm {
      fn descriptor(&self) -> rilgate_pow::AlgorithmDescriptor {
        rilgate_pow::AlgorithmDescriptor {
          id: self.0,
          version: 1,
          display_name: "Receipt test",
          solver_module_path: String::new(),
          requires_consent: false,
          memory_bytes: 0,
          browser: None,
        }
      }
      fn difficulty_for_weight(&self, _: i32) -> u64 {
        1
      }
      fn validate_difficulty(&self, difficulty: u64) -> Result<()> {
        anyhow::ensure!(difficulty > 0, "Expected positive difficulty");
        Ok(())
      }
      fn challenge_ttl(&self) -> u64 {
        60
      }
      fn issue(
        &self,
        _: rilgate_pow::WorkRequest,
      ) -> rilgate_pow::AlgorithmFuture<'_, rilgate_pow::PreparedWork> {
        unreachable!()
      }
      fn verify<'a>(
        &'a self,
        _: &'a rilgate_pow::PreparedWork,
        _: rilgate_pow::Proof<'a>,
      ) -> rilgate_pow::AlgorithmFuture<'a, ()> {
        unreachable!()
      }
    }
    let mut service = service(1);
    for algorithm in [PowAlgorithm::Fast, PowAlgorithm::Hashwx] {
      service
        .algorithms
        .register(Arc::new(ReceiptAlgorithm(algorithm)))
        .unwrap();
    }
    let headers = browser("Chrome/130");
    let binding = input("192.0.2.10", &headers);
    let receipt = |algorithm| {
      service
        .issue_receipt(
          "example.test".into(),
          Uuid::now_v7().to_string(),
          algorithm,
          1,
          PowVerification::Gateway,
        )
        .unwrap()
    };
    let first = service
      .exchange_receipt_with_claims(
        &receipt(PowAlgorithm::Fast),
        "example.test",
        &binding,
        None,
        None,
      )
      .unwrap();
    assert!(matches!(
      service
        .tokens
        .auth
        .verify_json::<AnyAuthClaims>(&first.token)
        .unwrap(),
      AnyAuthClaims::V1(_)
    ));
    let second = service
      .exchange_receipt_with_claims(
        &receipt(PowAlgorithm::Hashwx),
        "example.test",
        &binding,
        Some(&first.token),
        None,
      )
      .unwrap();
    let access = service
      .verify_auth_token(&second.token, "example.test", &binding)
      .unwrap();
    assert_eq!(
      access
        .grants
        .iter()
        .map(Grant::method_key)
        .collect::<Vec<_>>(),
      ["pow:fast", "pow:hashwx"]
    );
  }
}
