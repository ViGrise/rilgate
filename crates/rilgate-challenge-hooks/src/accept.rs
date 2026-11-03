use crate::{
  definition::{Accept, Operator},
  template::{TemplateValue, Values},
};
use serde_json::Value;
fn equals(actual: &Value, expected: &TemplateValue) -> bool {
  match expected {
    TemplateValue::String(s) => actual.as_str() == Some(s),
    TemplateValue::Bool(b) => actual.as_bool() == Some(*b),
    TemplateValue::Number(n) => actual.is_number() && actual.as_f64() == n.as_f64(),
  }
}
pub fn accepts(conditions: &[Accept], response: &Value, values: &Values) -> bool {
  conditions.iter().all(|condition| {
    let Some(actual) = response.pointer(&condition.field) else {
      return false;
    };
    match &condition.operator {
      Operator::Equals(v) => equals(actual, &v.render(values)),
      Operator::In(v) => v.iter().any(|v| equals(actual, &v.render(values))),
      Operator::Min(v) | Operator::Max(v) => {
        let TemplateValue::Number(expected) = v.render(values) else {
          return false;
        };
        match (actual.as_f64(), expected.as_f64()) {
          (Some(a), Some(b)) => {
            if matches!(condition.operator, Operator::Min(_)) {
              a >= b
            } else {
              a <= b
            }
          }
          _ => false,
        }
      }
    }
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::definition::Operand;
  #[test]
  fn boolean_condition_rejects_strings_and_missing_fields() {
    let conditions = [Accept {
      field: "/success".into(),
      operator: Operator::Equals(Operand::Literal(TemplateValue::Bool(true))),
    }];
    assert!(!accepts(
      &conditions,
      &serde_json::json!({"success":"true"}),
      &Values::default()
    ));
    assert!(!accepts(
      &conditions,
      &serde_json::json!({}),
      &Values::default()
    ));
  }
}
