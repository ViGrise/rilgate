use rilgate_protocol::{ChallengeStep, Grant, PowAlgorithm};

pub fn satisfies(step: &ChallengeStep, grant: &Grant) -> bool {
  let ChallengeStep::Pow(step) = step;
  let Grant::Pow(grant) = grant;
  (step.algorithm == PowAlgorithm::Refresh && grant.algorithm != PowAlgorithm::Refresh)
    || (step.algorithm == grant.algorithm
      && step.verification == grant.verification
      && grant.difficulty >= step.difficulty)
}

pub fn first_unsatisfied(
  required: &[ChallengeStep],
  effective: &[ChallengeStep],
  grants: &[Grant],
) -> Option<usize> {
  required
    .iter()
    .zip(effective)
    .position(|(required, effective)| {
      !grants.iter().any(|grant| {
        satisfies(required, grant) || (effective != required && satisfies(effective, grant))
      })
    })
}

#[cfg(test)]
mod tests {
  use super::*;
  use rilgate_protocol::{PowGrant, PowStep, PowVerification};
  fn step(algorithm: PowAlgorithm, difficulty: u64) -> ChallengeStep {
    ChallengeStep::Pow(PowStep {
      algorithm,
      difficulty,
      verification: PowVerification::Gateway,
    })
  }
  fn grant(algorithm: PowAlgorithm, difficulty: u64) -> Grant {
    Grant::Pow(PowGrant {
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
      &grant(PowAlgorithm::Fast, 1)
    ));
  }
  #[test]
  fn higher_fast_difficulty_cannot_replace_hashwx() {
    assert!(!satisfies(
      &step(PowAlgorithm::Hashwx, 1),
      &grant(PowAlgorithm::Fast, 64)
    ));
  }
  #[test]
  fn completed_first_step_preserves_second_requirement() {
    let steps = [step(PowAlgorithm::Fast, 1), step(PowAlgorithm::Hashwx, 1)];
    assert_eq!(
      first_unsatisfied(&steps, &steps, &[grant(PowAlgorithm::Fast, 1)]),
      Some(1)
    );
  }
  #[test]
  fn effective_fallback_grant_satisfies_required_step() {
    assert_eq!(
      first_unsatisfied(
        &[step(PowAlgorithm::RandomX, 1000)],
        &[step(PowAlgorithm::Fast, 1)],
        &[grant(PowAlgorithm::Fast, 1)]
      ),
      None
    );
  }
}
