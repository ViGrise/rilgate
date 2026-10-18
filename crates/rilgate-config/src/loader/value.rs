use serde::de::{self, Deserializer, IntoDeserializer, Visitor};
use serde_yaml_ng::Value;

type Error = serde_yaml_ng::Error;

pub(super) enum Expanded {
  Scalar(Value),
  Environment(String),
  Mapping(Vec<(String, Self)>),
  Sequence(Vec<Self>),
}

macro_rules! number {
  ($method:ident, $visit:ident, $kind:ty, $expected:literal) => {
    fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
      match self {
        Self::Environment(text) => visitor.$visit(
          text
            .parse::<$kind>()
            .map_err(|_| <Error as de::Error>::custom($expected))?,
        ),
        other => other.deserialize_any(visitor),
      }
    }
  };
}

impl<'de> Deserializer<'de> for Expanded {
  type Error = Error;

  fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
    match self {
      Self::Scalar(value) => value.deserialize_any(visitor),
      Self::Environment(text) => visitor.visit_string(text),
      Self::Mapping(values) => {
        visitor.visit_map(de::value::MapDeserializer::new(values.into_iter()))
      }
      Self::Sequence(values) => {
        visitor.visit_seq(de::value::SeqDeserializer::new(values.into_iter()))
      }
    }
  }
  number!(
    deserialize_bool,
    visit_bool,
    bool,
    "expected true or false in environment reference"
  );
  number!(
    deserialize_u8,
    visit_u8,
    u8,
    "expected an unsigned 8-bit integer in environment reference"
  );
  number!(
    deserialize_u16,
    visit_u16,
    u16,
    "expected an unsigned 16-bit integer in environment reference"
  );
  number!(
    deserialize_u32,
    visit_u32,
    u32,
    "expected an unsigned 32-bit integer in environment reference"
  );
  number!(
    deserialize_u64,
    visit_u64,
    u64,
    "expected an unsigned 64-bit integer in environment reference"
  );
  number!(
    deserialize_u128,
    visit_u128,
    u128,
    "expected an unsigned integer in environment reference"
  );
  number!(
    deserialize_i8,
    visit_i8,
    i8,
    "expected a signed 8-bit integer in environment reference"
  );
  number!(
    deserialize_i16,
    visit_i16,
    i16,
    "expected a signed 16-bit integer in environment reference"
  );
  number!(
    deserialize_i32,
    visit_i32,
    i32,
    "expected a signed 32-bit integer in environment reference"
  );
  number!(
    deserialize_i64,
    visit_i64,
    i64,
    "expected a signed 64-bit integer in environment reference"
  );
  number!(
    deserialize_i128,
    visit_i128,
    i128,
    "expected a signed integer in environment reference"
  );
  number!(
    deserialize_f32,
    visit_f32,
    f32,
    "expected a number in environment reference"
  );
  number!(
    deserialize_f64,
    visit_f64,
    f64,
    "expected a number in environment reference"
  );

  fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
    match self {
      Self::Scalar(Value::Null) => visitor.visit_none(),
      other => visitor.visit_some(other),
    }
  }
  fn deserialize_newtype_struct<V: Visitor<'de>>(
    self,
    _: &'static str,
    visitor: V,
  ) -> Result<V::Value, Error> {
    visitor.visit_newtype_struct(self)
  }
  fn deserialize_enum<V: Visitor<'de>>(
    self,
    name: &'static str,
    variants: &'static [&'static str],
    visitor: V,
  ) -> Result<V::Value, Error> {
    match self {
      Self::Scalar(value) => value.deserialize_enum(name, variants, visitor),
      Self::Environment(text) => Value::String(text)
        .deserialize_enum(name, variants, visitor)
        .map_err(|_| {
          <Error as de::Error>::custom(format!(
            "expected one of {variants:?} in environment reference"
          ))
        }),
      _ => Err(<Error as de::Error>::custom("expected a string enum value")),
    }
  }
  fn deserialize_struct<V: Visitor<'de>>(
    self,
    _: &'static str,
    _: &'static [&'static str],
    visitor: V,
  ) -> Result<V::Value, Error> {
    self.deserialize_map(visitor)
  }
  fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
    match self {
      Self::Mapping(values) => {
        visitor.visit_map(de::value::MapDeserializer::new(values.into_iter()))
      }
      _ => Err(<Error as de::Error>::custom(
        "expected a YAML mapping; environment references supply scalars only",
      )),
    }
  }
  fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
    match self {
      Self::Sequence(values) => {
        visitor.visit_seq(de::value::SeqDeserializer::new(values.into_iter()))
      }
      _ => Err(<Error as de::Error>::custom(
        "expected a YAML sequence; environment references supply scalars only",
      )),
    }
  }
  serde::forward_to_deserialize_any! { char str string bytes byte_buf unit unit_struct tuple tuple_struct identifier ignored_any }
}
impl<'de> IntoDeserializer<'de, Error> for Expanded {
  type Deserializer = Self;
  fn into_deserializer(self) -> Self {
    self
  }
}
