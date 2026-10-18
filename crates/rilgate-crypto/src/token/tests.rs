use super::*;
use crate::Secret;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::BTreeMap;

type HmacSha256 = Hmac<Sha256>;

mod signing {
  use super::*;
  use serde_json::{Value, json};

  fn reference(value: &Value, key: &[u8], purpose: TokenPurpose) -> String {
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(value).unwrap());
    let mut mac = HmacSha256::new_from_slice(key).unwrap();
    mac.update(purpose.domain());
    mac.update(payload.as_bytes());
    format!(
      "{payload}.{}",
      URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    )
  }

  #[test]
  fn prepared_signers_preserve_wire_format_and_isolate_keys_and_purposes() {
    let purposes = [
      TokenPurpose::Presentation,
      TokenPurpose::Challenge,
      TokenPurpose::Receipt,
      TokenPurpose::Auth,
    ];
    for (index, purpose) in purposes.iter().copied().enumerate() {
      for size in [0, 1, 2, 3, 63, 64, 65, 1024] {
        let key = vec![b'k'; size];
        let signer = TokenSigner::new(&key, purpose).unwrap();
        let value =
          json!({"host": "example.test", "path": "</script>\n\"", "padding": "x".repeat(size)});
        let token = signer.sign_json(&value).unwrap();
        assert_eq!(token, reference(&value, &key, purpose));
        assert_eq!(signer.verify_json::<Value>(&token).unwrap(), value);
        assert_eq!(signer.sign_json(&value).unwrap(), token);
        assert!(
          TokenSigner::new(b"other", purpose)
            .unwrap()
            .verify_json::<Value>(&token)
            .is_err()
        );
        for (other_index, other) in purposes.iter().copied().enumerate() {
          if index != other_index {
            assert!(
              TokenSigner::new(&key, other)
                .unwrap()
                .verify_json::<Value>(&token)
                .is_err()
            );
          }
        }
      }
    }
  }

  #[test]
  fn malformed_and_tampered_tokens_are_rejected() {
    let signer = TokenSigner::new(b"secret", TokenPurpose::Auth).unwrap();
    let token = signer.sign_json(&json!({"host":"example.test"})).unwrap();
    let (payload, signature) = token.split_once('.').unwrap();
    for malformed in [
      String::new(),
      "abc".into(),
      format!("{payload}."),
      format!("{payload}.{}", "A".repeat(42)),
      format!("{payload}.{signature}A"),
      format!("{payload}.{signature}.extra"),
      format!("{payload}.{}", "!".repeat(43)),
      format!("e30.{signature}"),
      format!("{payload}.{}", "A".repeat(43)),
    ] {
      assert!(
        signer.verify_json::<Value>(&malformed).is_err(),
        "{malformed}"
      );
    }
  }
}

mod rotation_tests {
  use super::*;
  use serde_json::{Value, json};

  #[test]
  fn rotation_accepts_overlap_and_rejects_retired_keys_and_id_tampering() {
    let old = KeyRotation {
      active_id: Some("old".into()),
      ..Default::default()
    };
    let old_signer = TokenSigner::rotating(b"old-secret", TokenPurpose::Auth, &old).unwrap();
    let old_token = old_signer.sign_json(&json!({"scope": "auth"})).unwrap();
    let rotation = KeyRotation {
      active_id: Some("new".into()),
      previous: BTreeMap::from([("old".into(), Secret("old-secret".into()))]),
    };
    let signer = TokenSigner::rotating(b"new-secret", TokenPurpose::Auth, &rotation).unwrap();
    assert!(signer.verify_json::<Value>(&old_token).is_ok());
    let new_token = signer.sign_json(&json!({"scope": "auth"})).unwrap();
    assert!(new_token.starts_with("new."));
    assert!(
      signer
        .verify_json::<Value>(&new_token.replacen("new.", "old.", 1))
        .is_err()
    );
    assert!(old_signer.verify_json::<Value>(&new_token).is_err());
    let retired = TokenSigner::rotating(
      b"new-secret",
      TokenPurpose::Auth,
      &KeyRotation {
        active_id: Some("new".into()),
        ..Default::default()
      },
    )
    .unwrap();
    assert!(retired.verify_json::<Value>(&old_token).is_err());
    assert!(retired.verify_json::<Value>(&new_token).is_ok());
    assert!(
      TokenSigner::rotating(b"new-secret", TokenPurpose::Receipt, &rotation)
        .unwrap()
        .verify_json::<Value>(&new_token)
        .is_err()
    );
  }

  #[test]
  fn legacy_migration_requires_explicit_verification_key() {
    let token = TokenSigner::new(b"old", TokenPurpose::Auth)
      .unwrap()
      .sign_json(&json!({}))
      .unwrap();
    let mut rotation = KeyRotation {
      active_id: Some("current".into()),
      ..Default::default()
    };
    let signer = TokenSigner::rotating(b"old", TokenPurpose::Auth, &rotation).unwrap();
    assert!(signer.verify_json::<Value>(&token).is_err());
    rotation
      .previous
      .insert("legacy".into(), Secret("old".into()));
    assert!(
      TokenSigner::rotating(b"new", TokenPurpose::Auth, &rotation)
        .unwrap()
        .verify_json::<Value>(&token)
        .is_ok()
    );
    rotation
      .previous
      .insert("current".into(), Secret("duplicate".into()));
    assert!(rotation.validate().is_err());
  }
}
