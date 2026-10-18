use super::schema::*;
use crate::ChallengePolicy;
use rilgate_protocol::PowAlgorithm;

pub(super) fn rules(mode: Mode) -> (Vec<Rule>, Action) {
  let challenge = |name: &str, when: Condition, difficulty| Rule {
    name: name.into(),
    when,
    action: Action::Challenge,
    score: None,
    challenge: Some(ChallengePolicy {
      algorithm: Some(PowAlgorithm::Fast),
      difficulty: Some(difficulty),
    }),
  };
  if mode == Mode::Off {
    return (vec![], Action::Allow);
  }
  let mut rules = vec![Rule {
    name: "public-endpoints".into(),
    when: Condition {
      any: Some(vec![
        Condition {
          path: Some(StringMatch {
            values: Some(vec!["/robots.txt".into(), "/favicon.ico".into()]),
            ..Default::default()
          }),
          ..Default::default()
        },
        Condition {
          path: Some(StringMatch {
            prefix: Some("/.well-known/acme-challenge/".into()),
            ..Default::default()
          }),
          ..Default::default()
        },
      ]),
      ..Default::default()
    },
    action: Action::Allow,
    score: None,
    challenge: None,
  }];
  if mode == Mode::Light {
    rules.push(challenge(
      "suspicious-client",
      Condition {
        user_agent: Some(StringMatch {
          regex: Some("(?i)(^$|bot|crawler|spider|curl|wget|python|scrapy)".into()),
          ..Default::default()
        }),
        ..Default::default()
      },
      1,
    ));
    (rules, Action::Allow)
  } else {
    rules.push(challenge(
      "protected-traffic",
      Condition {
        path: Some(StringMatch {
          prefix: Some(String::new()),
          ..Default::default()
        }),
        ..Default::default()
      },
      if mode == Mode::Strict { 4 } else { 2 },
    ));
    (rules, Action::Allow)
  }
}
