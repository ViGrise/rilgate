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
pub struct Size(pub usize);
impl Serialize for Size {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format!("{}B", self.0))
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
