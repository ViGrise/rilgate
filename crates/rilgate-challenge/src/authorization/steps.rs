use rilgate_protocol::{ChallengeStep, Grant, PowAlgorithm};

pub fn satisfies(
  step: &ChallengeStep,
  grant: &Grant,
  until_expiry: &std::collections::BTreeSet<String>,
) -> bool {
  if let ChallengeStep::Hook(step) = step {
    return match grant {
      Grant::Hook(grant) => step.name == grant.name,
      Grant::Pow(grant) => {
        grant.fallback_for.as_ref() == Some(&step.name) && until_expiry.contains(&step.name)
      }
    };
  }
  let ChallengeStep::Pow(step) = step else {
    unreachable!()
  };
  let Grant::Pow(grant) = grant else {
    return false;
  };
  (step.algorithm == PowAlgorithm::Refresh && grant.algorithm != PowAlgorithm::Refresh)
    || (step.algorithm == grant.algorithm
      && step.verification == grant.verification
      && grant.difficulty >= step.difficulty)
}

pub fn first_unsatisfied(
  required: &[ChallengeStep],
  effective: &[ChallengeStep],
  grants: &[Grant],
  until_expiry: &std::collections::BTreeSet<String>,
) -> Option<usize> {
  required
    .iter()
    .zip(effective)
    .position(|(required, effective)| {
      !grants.iter().any(|grant| {
        satisfies(required, grant, until_expiry)
          || (effective != required && satisfies(effective, grant, until_expiry))
      })
    })
}

#[cfg(test)]
mod tests {
  use super::*;
  use rilgate_protocol::{PowGrant, PowStep, PowVerification};
  fn step(algorithm: PowAlgorithm, difficulty: u64) -> ChallengeStep {
    ChallengeStep::Pow(PowStep {
      fallback_for: None,
      algorithm,
      difficulty,
      verification: PowVerification::Gateway,
    })
  }
  fn grant(algorithm: PowAlgorithm, difficulty: u64) -> Grant {
    Grant::Pow(PowGrant {
      fallback_for: None,
      algorithm,
      difficulty,
      verification: PowVerification::Gateway,
      expires_at: i64::MAX,
    })
  }
  #[test]
  fn fast_proof_satisfies_refresh_wait() {
    assert!(satisfies(
      &step(PowAlgorithm::Refresh, 10),
      &grant(PowAlgorithm::Fast, 1),
      &Default::default()
    ));
  }
  #[test]
  fn higher_fast_difficulty_cannot_replace_hashwx() {
    assert!(!satisfies(
      &step(PowAlgorithm::Hashwx, 1),
      &grant(PowAlgorithm::Fast, 64),
      &Default::default()
    ));
  }
  #[test]
  fn completed_first_step_preserves_second_requirement() {
    let steps = [step(PowAlgorithm::Fast, 1), step(PowAlgorithm::Hashwx, 1)];
    assert_eq!(
      first_unsatisfied(
        &steps,
        &steps,
        &[grant(PowAlgorithm::Fast, 1)],
        &Default::default()
      ),
      Some(1)
    );
  }
  #[test]
  fn effective_fallback_grant_satisfies_required_step() {
    assert_eq!(
      first_unsatisfied(
        &[step(PowAlgorithm::RandomX, 1000)],
        &[step(PowAlgorithm::Fast, 1)],
        &[grant(PowAlgorithm::Fast, 1)],
        &Default::default()
      ),
      None
    );
  }
  #[test]
  fn hook_grant_requires_the_same_name() {
    let step = ChallengeStep::Hook(rilgate_protocol::HookStep {
      name: "captcha".into(),
    });
    let grant = |name: &str| {
      Grant::Hook(rilgate_protocol::HookGrant {
        name: name.into(),
        expires_at: i64::MAX,
      })
    };
    assert!(satisfies(&step, &grant("captcha"), &Default::default()));
    assert!(!satisfies(&step, &grant("other"), &Default::default()));
  }
  #[test]
  fn hook_fallback_grant_requires_until_expiry_policy() {
    let step = ChallengeStep::Hook(rilgate_protocol::HookStep {
      name: "captcha".into(),
    });
    let mut grant = grant(PowAlgorithm::Fast, 1);
    if let Grant::Pow(g) = &mut grant {
      g.fallback_for = Some("captcha".into());
    }
    assert!(!satisfies(&step, &grant, &Default::default()));
    assert!(satisfies(&step, &grant, &["captcha".into()].into()));
  }
}
