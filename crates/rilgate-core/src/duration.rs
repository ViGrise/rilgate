//! Human-readable durations such as `300ms`, `5s`, `1h 30m` or `7d`.
//!
//! A duration is one or more `<integer><unit>` items, with optional spaces between
//! items and between a number and its unit. The total is the sum of all items.
use serde::{Deserialize, Deserializer, Serializer, de::Error as _};
use std::time::Duration;
use thiserror::Error;

const NANOS_PER_SECOND: u128 = 1_000_000_000;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum ParseDurationError {
  #[error("duration is empty")]
  Empty,
  #[error("expected a number at position {0}")]
  NumberExpected(usize),
  #[error("time unit needed after the number at position {0}, for example 5s")]
  UnitNeeded(usize),
  #[error("unknown time unit {0:?}")]
  UnknownUnit(String),
  #[error("duration is too large")]
  Overflow,
}

/// Nanoseconds in one `unit`. Months and years use the average Gregorian lengths.
fn unit_nanos(unit: &str) -> Option<u128> {
  let seconds = match unit {
    "nanos" | "nsec" | "ns" => return Some(1),
    "usec" | "us" | "µs" => return Some(1_000),
    "millis" | "msec" | "ms" => return Some(1_000_000),
    "seconds" | "second" | "secs" | "sec" | "s" => 1,
    "minutes" | "minute" | "mins" | "min" | "m" => 60,
    "hours" | "hour" | "hrs" | "hr" | "h" => 3_600,
    "days" | "day" | "d" => 86_400,
    "weeks" | "week" | "w" => 604_800,
    "months" | "month" | "M" => 2_630_016,
    "years" | "year" | "y" => 31_557_600,
    _ => return None,
  };
  Some(seconds * NANOS_PER_SECOND)
}

pub fn parse(value: &str) -> Result<Duration, ParseDurationError> {
  let mut rest = value.trim_start();
  if rest.trim_end().is_empty() {
    return Err(ParseDurationError::Empty);
  }
  let mut total: u128 = 0;
  while !rest.is_empty() {
    let position = value.len() - rest.len();
    let digits = rest
      .find(|c: char| !c.is_ascii_digit())
      .unwrap_or(rest.len());
    if digits == 0 {
      return Err(ParseDurationError::NumberExpected(position));
    }
    let number: u128 = rest[..digits]
      .parse()
      .map_err(|_| ParseDurationError::Overflow)?;
    rest = rest[digits..].trim_start();
    let letters = rest
      .find(|c: char| !c.is_alphabetic())
      .unwrap_or(rest.len());
    if letters == 0 {
      return Err(ParseDurationError::UnitNeeded(position));
    }
    let unit = &rest[..letters];
    let nanos = unit_nanos(unit).ok_or_else(|| ParseDurationError::UnknownUnit(unit.to_owned()))?;
    total = number
      .checked_mul(nanos)
      .and_then(|item| total.checked_add(item))
      .ok_or(ParseDurationError::Overflow)?;
    rest = rest[letters..].trim_start();
  }
  let seconds =
    u64::try_from(total / NANOS_PER_SECOND).map_err(|_| ParseDurationError::Overflow)?;
  Ok(Duration::new(seconds, (total % NANOS_PER_SECOND) as u32))
}

/// Formats `duration` as space-separated items from days down to nanoseconds, such
/// as `1d 2h 30m` or `1s 500ms`. The output parses back to the same duration.
pub fn format(duration: Duration) -> String {
  if duration.is_zero() {
    return "0s".to_owned();
  }
  const UNITS: [(u128, &str); 7] = [
    (86_400 * NANOS_PER_SECOND, "d"),
    (3_600 * NANOS_PER_SECOND, "h"),
    (60 * NANOS_PER_SECOND, "m"),
    (NANOS_PER_SECOND, "s"),
    (1_000_000, "ms"),
    (1_000, "us"),
    (1, "ns"),
  ];
  let mut remaining = duration.as_nanos();
  let mut items = Vec::new();
  for (nanos, name) in UNITS {
    let count = remaining / nanos;
    if count > 0 {
      items.push(format!("{count}{name}"));
      remaining %= nanos;
    }
  }
  items.join(" ")
}

/// Serde `with` helper for a required [`Duration`] field.
pub fn serialize<S: Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
  serializer.serialize_str(&format(*duration))
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
  let value = String::deserialize(deserializer)?;
  parse(&value).map_err(D::Error::custom)
}

/// Serde `with` helper for an optional [`Duration`] field, where `null` means `None`.
pub mod option {
  use super::*;

  pub fn serialize<S: Serializer>(
    duration: &Option<Duration>,
    serializer: S,
  ) -> Result<S::Ok, S::Error> {
    match duration {
      Some(duration) => super::serialize(duration, serializer),
      None => serializer.serialize_none(),
    }
  }

  pub fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
  ) -> Result<Option<Duration>, D::Error> {
    Option::<String>::deserialize(deserializer)?
      .map(|value| parse(&value).map_err(D::Error::custom))
      .transpose()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_every_unit_and_composite_values() {
    let cases = [
      ("0s", Duration::ZERO),
      ("300ms", Duration::from_millis(300)),
      ("5s", Duration::from_secs(5)),
      ("1m", Duration::from_secs(60)),
      ("6h", Duration::from_secs(21_600)),
      ("7d", Duration::from_secs(604_800)),
      ("2w", Duration::from_secs(1_209_600)),
      ("1M", Duration::from_secs(2_630_016)),
      ("1y", Duration::from_secs(31_557_600)),
      ("250us", Duration::from_micros(250)),
      ("250µs", Duration::from_micros(250)),
      ("10ns", Duration::from_nanos(10)),
      ("30min", Duration::from_secs(1_800)),
      ("2hours", Duration::from_secs(7_200)),
      ("1h 30m", Duration::from_secs(5_400)),
      ("1h30m", Duration::from_secs(5_400)),
      (" 1 day 2 hours ", Duration::from_secs(93_600)),
    ];
    for (input, expected) in cases {
      assert_eq!(parse(input), Ok(expected), "{input}");
    }
  }

  #[test]
  fn rejects_invalid_values() {
    for input in ["", "  ", "5", "s", "5 parsecs", "1.5s", "-1s", "1h,30m"] {
      assert!(parse(input).is_err(), "{input}");
    }
    assert_eq!(
      parse("99999999999999999999999999999999999999999y"),
      Err(ParseDurationError::Overflow)
    );
    assert_eq!(parse("600000000000y"), Err(ParseDurationError::Overflow));
  }

  #[test]
  fn formatted_durations_parse_back() {
    for duration in [
      Duration::ZERO,
      Duration::from_millis(300),
      Duration::from_secs(5_400),
      Duration::from_secs(93_600),
      Duration::new(90_061, 1_001_001),
    ] {
      assert_eq!(parse(&format(duration)), Ok(duration));
    }
    assert_eq!(format(Duration::from_secs(5_400)), "1h 30m");
  }
}
