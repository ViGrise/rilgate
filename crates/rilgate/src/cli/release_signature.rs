//! Verifies the detached OpenPGP signatures that `scripts/release/sign.sh` writes.
//!
//! The verifier accepts exactly one shape: an ASCII-armored version 4 binary-document
//! signature made with EdDSA over Ed25519 and SHA-256 or SHA-512, by one of the embedded
//! release keys. Every other packet, version or algorithm is refused.
use anyhow::{Context, Result, bail, ensure};
use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256, Sha512};

/// Release signing keys as raw Ed25519 public keys, with their OpenPGP v4 fingerprints.
/// A key rotation adds the new key here one release before signing with it.
pub const RELEASE_KEYS: &[ReleaseKey] = &[ReleaseKey {
  fingerprint: "F3967F204825CDE607A81452B990C5BC82BD2323",
  ed25519: hex_literal(b"6786142d53e5d333410fdfcfe27b7533809c9c2d05915ad61628cb02b3c2fdee"),
}];

pub struct ReleaseKey {
  pub fingerprint: &'static str,
  pub ed25519: [u8; 32],
}

const TAG_SIGNATURE: u8 = 2;
const SIGNATURE_BINARY_DOCUMENT: u8 = 0x00;
const ALGORITHM_EDDSA: u8 = 22;
const HASH_SHA256: u8 = 8;
const HASH_SHA512: u8 = 10;

/// Checks that `armored` is a valid signature of `data` by one of `keys`.
pub fn verify(data: &[u8], armored: &str, keys: &[ReleaseKey]) -> Result<()> {
  let packet = dearmor(armored)?;
  let body = single_signature_packet(&packet)?;
  let signature = parse(body)?;
  let mut trailer = Vec::with_capacity(signature.hashed.len() + 6);
  trailer.extend_from_slice(signature.hashed);
  trailer.extend_from_slice(&[4, 0xff]);
  trailer.extend_from_slice(&(signature.hashed.len() as u32).to_be_bytes());
  let digest = match signature.hash {
    HASH_SHA256 => Sha256::new()
      .chain_update(data)
      .chain_update(&trailer)
      .finalize()
      .to_vec(),
    HASH_SHA512 => Sha512::new()
      .chain_update(data)
      .chain_update(&trailer)
      .finalize()
      .to_vec(),
    other => bail!("unsupported signature hash algorithm {other}"),
  };
  ensure!(
    digest[..2] == signature.digest_prefix,
    "signature does not match the signed file"
  );
  let ed25519 = Signature::from_bytes(&signature.ed25519);
  ensure!(
    keys.iter().any(|key| {
      VerifyingKey::from_bytes(&key.ed25519)
        .is_ok_and(|key| key.verify_strict(&digest, &ed25519).is_ok())
    }),
    "signature is not from a release key"
  );
  Ok(())
}

struct ParsedSignature<'a> {
  hash: u8,
  hashed: &'a [u8],
  digest_prefix: [u8; 2],
  ed25519: [u8; 64],
}

fn dearmor(armored: &str) -> Result<Vec<u8>> {
  let mut lines = armored.lines().map(str::trim_end);
  ensure!(
    lines.any(|line| line == "-----BEGIN PGP SIGNATURE-----"),
    "signature is not ASCII-armored"
  );
  // Armor headers end at the first empty line.
  for line in lines.by_ref() {
    if line.is_empty() {
      break;
    }
  }
  let mut encoded = String::new();
  let mut ended = false;
  for line in lines {
    if line == "-----END PGP SIGNATURE-----" {
      ended = true;
      break;
    }
    if line.starts_with('=') {
      continue;
    }
    encoded.push_str(line);
  }
  ensure!(ended, "signature armor has no end line");
  base64::engine::general_purpose::STANDARD
    .decode(encoded)
    .context("signature armor is not valid base64")
}

fn single_signature_packet(packet: &[u8]) -> Result<&[u8]> {
  let (&ctb, rest) = packet.split_first().context("signature is empty")?;
  ensure!(ctb & 0x80 != 0, "signature does not start with a packet");
  let (tag, length, rest) = if ctb & 0x40 == 0 {
    let tag = (ctb >> 2) & 0x0f;
    let width = match ctb & 0x03 {
      0 => 1,
      1 => 2,
      2 => 4,
      _ => bail!("signature packet has an indeterminate length"),
    };
    ensure!(rest.len() >= width, "signature packet is truncated");
    let length = rest[..width]
      .iter()
      .fold(0usize, |length, byte| length << 8 | usize::from(*byte));
    (tag, length, &rest[width..])
  } else {
    let tag = ctb & 0x3f;
    match rest {
      [first, rest @ ..] if *first < 192 => (tag, usize::from(*first), rest),
      [first, second, rest @ ..] if *first < 224 => (
        tag,
        (usize::from(*first - 192) << 8) + usize::from(*second) + 192,
        rest,
      ),
      [255, a, b, c, d, rest @ ..] => (tag, u32::from_be_bytes([*a, *b, *c, *d]) as usize, rest),
      _ => bail!("signature packet length is unsupported"),
    }
  };
  ensure!(tag == TAG_SIGNATURE, "file holds no signature packet");
  ensure!(
    rest.len() == length,
    "signature must hold exactly one packet"
  );
  Ok(rest)
}

fn parse(body: &[u8]) -> Result<ParsedSignature<'_>> {
  let mut reader = Reader(body);
  ensure!(reader.byte()? == 4, "signature version must be 4");
  ensure!(
    reader.byte()? == SIGNATURE_BINARY_DOCUMENT,
    "signature must cover a binary document"
  );
  ensure!(
    reader.byte()? == ALGORITHM_EDDSA,
    "signature must use EdDSA"
  );
  let hash = reader.byte()?;
  let hashed_length = usize::from(reader.u16()?);
  reader.take(hashed_length)?;
  let hashed = &body[..body.len() - reader.0.len()];
  let unhashed_length = usize::from(reader.u16()?);
  reader.take(unhashed_length)?;
  let digest_prefix = [reader.byte()?, reader.byte()?];
  let mut ed25519 = [0; 64];
  ed25519[..32].copy_from_slice(&reader.scalar()?);
  ed25519[32..].copy_from_slice(&reader.scalar()?);
  ensure!(reader.0.is_empty(), "signature packet has trailing data");
  Ok(ParsedSignature {
    hash,
    hashed,
    digest_prefix,
    ed25519,
  })
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
  fn take(&mut self, count: usize) -> Result<&'a [u8]> {
    ensure!(self.0.len() >= count, "signature packet is truncated");
    let (taken, rest) = self.0.split_at(count);
    self.0 = rest;
    Ok(taken)
  }
  fn byte(&mut self) -> Result<u8> {
    Ok(self.take(1)?[0])
  }
  fn u16(&mut self) -> Result<u16> {
    let bytes = self.take(2)?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
  }
  /// Reads a multiprecision integer of at most 256 bits, left-padded to 32 bytes.
  fn scalar(&mut self) -> Result<[u8; 32]> {
    let bits = usize::from(self.u16()?);
    ensure!(bits <= 256, "signature value is too large");
    let bytes = self.take(bits.div_ceil(8))?;
    let mut scalar = [0; 32];
    scalar[32 - bytes.len()..].copy_from_slice(bytes);
    Ok(scalar)
  }
}

const fn hex_literal<const N: usize>(text: &[u8]) -> [u8; N] {
  const fn nibble(byte: u8) -> u8 {
    match byte {
      b'0'..=b'9' => byte - b'0',
      b'a'..=b'f' => byte - b'a' + 10,
      _ => panic!("hex literal must be lowercase hex"),
    }
  }
  assert!(text.len() == N * 2, "hex literal has the wrong length");
  let mut bytes = [0; N];
  let mut index = 0;
  while index < N {
    bytes[index] = nibble(text[2 * index]) << 4 | nibble(text[2 * index + 1]);
    index += 1;
  }
  bytes
}

#[cfg(test)]
mod tests {
  use super::*;

  const SUMS: &[u8] = include_bytes!("../../tests/fixtures/release/SHA256SUMS");
  const SUMS_SIGNATURE: &str = include_str!("../../tests/fixtures/release/SHA256SUMS.asc");

  #[test]
  fn accepts_a_published_release_signature() {
    verify(SUMS, SUMS_SIGNATURE, RELEASE_KEYS).unwrap();
  }

  #[test]
  fn refuses_a_changed_checksum_file() {
    let mut changed = SUMS.to_vec();
    changed[0] ^= 1;
    assert!(verify(&changed, SUMS_SIGNATURE, RELEASE_KEYS).is_err());
  }

  #[test]
  fn refuses_a_signature_from_another_key() {
    let other = [ReleaseKey {
      fingerprint: "",
      ed25519: VerifyingKey::from(&ed25519_dalek::SigningKey::from_bytes(&[7; 32])).to_bytes(),
    }];
    assert!(verify(SUMS, SUMS_SIGNATURE, &other).is_err());
  }
}
