use serde::{Deserialize, Deserializer, Serializer};

pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
  if *value <= 9_007_199_254_740_991 {
    serializer.serialize_u64(*value)
  } else {
    serializer.serialize_str(&value.to_string())
  }
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
  #[derive(Deserialize)]
  #[serde(untagged)]
  enum Value {
    Number(u64),
    Text(String),
  }
  match Value::deserialize(deserializer)? {
    Value::Number(value) => Ok(value),
    Value::Text(value) => value.parse().map_err(serde::de::Error::custom),
  }
}
