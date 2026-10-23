use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct Time(pub Duration);
impl Time {
  pub fn seconds(value: u64) -> Self {
    Self(Duration::from_secs(value))
  }
  pub fn whole_seconds(self, path: &str) -> anyhow::Result<u64> {
    anyhow::ensure!(
      self.0.subsec_nanos() == 0 && !self.0.is_zero(),
      "{path}: expected a positive whole-second duration"
    );
    Ok(self.0.as_secs())
  }
}
impl Serialize for Time {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&humantime::format_duration(self.0).to_string())
  }
}
impl<'de> Deserialize<'de> for Time {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let value = String::deserialize(deserializer)?;
    let duration = humantime::parse_duration(&value)
      .map_err(|_| D::Error::custom("expected a duration such as 300ms, 5s, 1m or 6h"))?;
    if duration.is_zero() || duration > Duration::from_secs(31_536_000) {
      return Err(D::Error::custom(
        "duration must be positive and at most one year",
      ));
    }
    Ok(Self(duration))
  }
}
#[derive(Debug, Clone, Copy)]
pub struct IdleTimeout(pub Duration);
impl Serialize for IdleTimeout {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&humantime::format_duration(self.0).to_string())
  }
}
impl<'de> Deserialize<'de> for IdleTimeout {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let value = String::deserialize(deserializer)?;
    let duration = humantime::parse_duration(&value)
      .map_err(|_| D::Error::custom("expected a duration such as 0s, 300ms, 5s, 1m or 7d"))?;
    if duration > Duration::from_secs(604_800) {
      return Err(D::Error::custom("idle timeout must be at most 7d"));
    }
    Ok(Self(duration))
  }
}
#[derive(Debug, Clone, Copy)]
pub struct Size(pub usize);
impl Serialize for Size {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    // The largest unit that represents the size exactly, as written in YAML.
    let (value, unit) = [(1 << 30, "GiB"), (1 << 20, "MiB"), (1 << 10, "KiB")]
      .into_iter()
      .find(|(unit, _)| self.0.is_multiple_of(*unit))
      .map_or((self.0, "B"), |(unit, name)| (self.0 / unit, name));
    serializer.serialize_str(&format!("{value}{unit}"))
  }
}
impl<'de> Deserialize<'de> for Size {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let value = String::deserialize(deserializer)?;
    let index = value
      .find(|c: char| !c.is_ascii_digit())
      .ok_or_else(|| D::Error::custom("size requires B, KiB, MiB or GiB units"))?;
    let (number, unit) = value.split_at(index);
    let multiplier = match unit {
      "B" => 1,
      "KiB" => 1024,
      "MiB" => 1024 * 1024,
      "GiB" => 1024 * 1024 * 1024,
      _ => return Err(D::Error::custom("size requires B, KiB, MiB or GiB units")),
    };
    let bytes = number
      .parse::<usize>()
      .ok()
      .and_then(|v| v.checked_mul(multiplier))
      .filter(|v| *v > 0)
      .ok_or_else(|| D::Error::custom("size must be a positive integer that fits this platform"))?;
    Ok(Self(bytes))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn idle_timeout_accepts_zero_and_seven_days() {
    for (text, seconds) in [("0s", 0), ("7d", 604800), ("300ms", 0)] {
      let timeout: IdleTimeout = serde_yaml_ng::from_str(text).unwrap();
      assert_eq!(timeout.0.as_secs(), seconds);
      let serialized = serde_yaml_ng::to_string(&timeout).unwrap();
      let repeated: IdleTimeout = serde_yaml_ng::from_str(&serialized).unwrap();
      assert_eq!(timeout.0, repeated.0);
    }
  }

  #[test]
  fn idle_timeout_rejects_excessive_and_invalid_durations() {
    for text in ["8d", "garbage"] {
      assert!(serde_yaml_ng::from_str::<IdleTimeout>(text).is_err());
    }
    assert!(serde_yaml_ng::from_str::<Time>("0s").is_err());
  }
}
