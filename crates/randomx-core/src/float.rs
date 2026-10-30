mod accelerated;
#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
mod simd;

use ieee_apsqrt::sqrt_accurate;
use rustc_apfloat::{Float, Round, ieee::Double};

use crate::constants::{DYNAMIC_MANTISSA_MASK, FSCAL_MASK};

#[derive(Debug, Clone, Copy, Default)]
pub struct FReg(pub [u64; 2]);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RoundingMode {
  #[default]
  Nearest,
  Down,
  Up,
  TowardZero,
}

impl RoundingMode {
  pub fn from_randomx(value: u64) -> Self {
    match value & 3 {
      0 => Self::Nearest,
      1 => Self::Down,
      2 => Self::Up,
      _ => Self::TowardZero,
    }
  }

  fn ap(self) -> Round {
    match self {
      Self::Nearest => Round::NearestTiesToEven,
      Self::Down => Round::TowardNegative,
      Self::Up => Round::TowardPositive,
      Self::TowardZero => Round::TowardZero,
    }
  }
}

#[inline]
fn double(bits: u64) -> Double {
  Double::from_bits(bits as u128)
}

#[inline]
fn bits(value: Double) -> u64 {
  value.to_bits() as u64
}

#[inline]
pub fn add(a: FReg, b: FReg, mode: RoundingMode) -> FReg {
  #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
  if let Some(value) = unsafe { simd::add(a, b, mode) } {
    return value;
  }
  let lane = |index: usize| accelerated::add(a.0[index], b.0[index], mode);
  match (lane(0), lane(1)) {
    (Some(lo), Some(hi)) => FReg([lo, hi]),
    _ => software_add(a, b, mode),
  }
}

#[inline]
pub fn sub(a: FReg, b: FReg, mode: RoundingMode) -> FReg {
  #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
  if let Some(value) = unsafe { simd::add(a, FReg([b.0[0] ^ (1 << 63), b.0[1] ^ (1 << 63)]), mode) }
  {
    return value;
  }
  let lane = |index: usize| accelerated::add(a.0[index], b.0[index] ^ (1 << 63), mode);
  match (lane(0), lane(1)) {
    (Some(lo), Some(hi)) => FReg([lo, hi]),
    _ => software_sub(a, b, mode),
  }
}

#[inline]
pub fn mul(a: FReg, b: FReg, mode: RoundingMode) -> FReg {
  #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
  if let Some(value) = unsafe { simd::mul(a, b, mode) } {
    return value;
  }
  let lane = |index: usize| accelerated::mul(a.0[index], b.0[index], mode);
  match (lane(0), lane(1)) {
    (Some(lo), Some(hi)) => FReg([lo, hi]),
    _ => software_mul(a, b, mode),
  }
}

#[inline]
pub fn div(a: FReg, b: FReg, mode: RoundingMode) -> FReg {
  #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
  if let Some(value) = unsafe { simd::div(a, b, mode) } {
    return value;
  }
  let lane = |index: usize| accelerated::div(a.0[index], b.0[index], mode);
  match (lane(0), lane(1)) {
    (Some(lo), Some(hi)) => FReg([lo, hi]),
    _ => software_div(a, b, mode),
  }
}

#[inline]
pub fn sqrt(value: FReg, mode: RoundingMode) -> FReg {
  #[cfg(all(target_arch = "wasm32", target_feature = "simd128", feature = "std"))]
  if let Some(value) = unsafe { simd::sqrt(value, mode) } {
    return value;
  }
  match (
    accelerated::sqrt(value.0[0], mode),
    accelerated::sqrt(value.0[1], mode),
  ) {
    (Some(lo), Some(hi)) => FReg([lo, hi]),
    _ => software_sqrt(value, mode),
  }
}

fn software_add(a: FReg, b: FReg, mode: RoundingMode) -> FReg {
  FReg([
    bits(double(a.0[0]).add_r(double(b.0[0]), mode.ap()).value),
    bits(double(a.0[1]).add_r(double(b.0[1]), mode.ap()).value),
  ])
}

fn software_sub(a: FReg, b: FReg, mode: RoundingMode) -> FReg {
  FReg([
    bits(double(a.0[0]).sub_r(double(b.0[0]), mode.ap()).value),
    bits(double(a.0[1]).sub_r(double(b.0[1]), mode.ap()).value),
  ])
}

fn software_mul(a: FReg, b: FReg, mode: RoundingMode) -> FReg {
  FReg([
    bits(double(a.0[0]).mul_r(double(b.0[0]), mode.ap()).value),
    bits(double(a.0[1]).mul_r(double(b.0[1]), mode.ap()).value),
  ])
}

fn software_div(a: FReg, b: FReg, mode: RoundingMode) -> FReg {
  FReg([
    bits(double(a.0[0]).div_r(double(b.0[0]), mode.ap()).value),
    bits(double(a.0[1]).div_r(double(b.0[1]), mode.ap()).value),
  ])
}

fn software_sqrt(value: FReg, mode: RoundingMode) -> FReg {
  let lo = sqrt_accurate(value.0[0], mode.ap()).0.value;
  let hi = sqrt_accurate(value.0[1], mode.ap()).0.value;
  FReg([lo, hi])
}

pub fn swap(value: FReg) -> FReg {
  FReg([value.0[1], value.0[0]])
}

pub fn scale(value: FReg) -> FReg {
  FReg([value.0[0] ^ FSCAL_MASK, value.0[1] ^ FSCAL_MASK])
}

pub fn xor(a: FReg, b: FReg) -> FReg {
  FReg([a.0[0] ^ b.0[0], a.0[1] ^ b.0[1]])
}

pub fn from_i32_pair(bytes: &[u8]) -> FReg {
  let lo = i32::from_le_bytes(bytes[0..4].try_into().expect("i32"));
  let hi = i32::from_le_bytes(bytes[4..8].try_into().expect("i32"));

  FReg([(lo as f64).to_bits(), (hi as f64).to_bits()])
}

pub fn mask_exponent_mantissa(value: FReg, mask: [u64; 2]) -> FReg {
  FReg([
    (value.0[0] & DYNAMIC_MANTISSA_MASK) | mask[0],
    (value.0[1] & DYNAMIC_MANTISSA_MASK) | mask[1],
  ])
}

#[cfg(feature = "validation")]
pub fn validate_arithmetic(mut state: u64, count: u32) -> bool {
  if count > 1_000_000 {
    return false;
  }
  let mut next = || {
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    state
  };
  let boundaries = [
    0,
    1 << 63,
    1,
    0x000f_ffff_ffff_ffff,
    0x0010_0000_0000_0000,
    0x0010_0000_0000_0001,
    0x0010_0000_0000_0002,
    0x3fe0_0000_0000_0000,
    0x3fef_ffff_ffff_ffff,
    0x3ff0_0000_0000_0000,
    0x3ff0_0000_0000_0001,
    0x3ca0_0000_0000_0000,
    0x7fef_ffff_ffff_ffff,
    0xffef_ffff_ffff_ffff,
    0x7ff0_0000_0000_0000,
    0xfff0_0000_0000_0000,
    0x7ff8_0000_0000_1234,
    0x7ff0_0000_0000_1234,
  ];
  for index in 0..count + (boundaries.len() * boundaries.len()) as u32 {
    let (x, y) = if (index as usize) < boundaries.len() * boundaries.len() {
      (
        boundaries[index as usize / boundaries.len()],
        boundaries[index as usize % boundaries.len()],
      )
    } else {
      (next(), next())
    };
    let a = FReg([x, y]);
    let b = FReg([y, x]);
    for mode in [
      RoundingMode::Nearest,
      RoundingMode::Down,
      RoundingMode::Up,
      RoundingMode::TowardZero,
    ] {
      if add(a, b, mode).0 != software_add(a, b, mode).0
        || sub(a, b, mode).0 != software_sub(a, b, mode).0
        || mul(a, b, mode).0 != software_mul(a, b, mode).0
        || div(a, b, mode).0 != software_div(a, b, mode).0
        || sqrt(a, mode).0 != software_sqrt(a, mode).0
      {
        return false;
      }
    }
  }
  true
}

#[cfg(test)]
mod tests {
  use super::*;

  const MODES: [RoundingMode; 4] = [
    RoundingMode::Nearest,
    RoundingMode::Down,
    RoundingMode::Up,
    RoundingMode::TowardZero,
  ];

  fn check_pair(a: u64, b: u64) {
    let a = FReg([a, b]);
    let b = FReg([b, a.0[0]]);
    for mode in MODES {
      for (name, actual, expected) in [
        ("add", add(a, b, mode), software_add(a, b, mode)),
        ("sub", sub(a, b, mode), software_sub(a, b, mode)),
        ("mul", mul(a, b, mode), software_mul(a, b, mode)),
        ("div", div(a, b, mode), software_div(a, b, mode)),
      ] {
        assert_eq!(
          actual.0, expected.0,
          "{name} {mode:?} a={:x?} b={:x?}",
          a.0, b.0
        );
      }
      assert_eq!(
        sqrt(a, mode).0,
        software_sqrt(a, mode).0,
        "sqrt {mode:?} a={:x?}",
        a.0
      );
    }
  }

  #[test]
  fn accelerated_rounding_matches_software_at_ieee_boundaries() {
    let values = [
      0,
      1 << 63,
      1,
      (1 << 63) | 1,
      0x000f_ffff_ffff_ffff,
      0x0010_0000_0000_0000,
      0x0010_0000_0000_0001,
      0x3fe0_0000_0000_0000,
      0x3ff0_0000_0000_0000,
      0x3ff0_0000_0000_0001,
      0x3ca0_0000_0000_0000,
      0x3c90_0000_0000_0000,
      0x4000_0000_0000_0000,
      0x4008_0000_0000_0000,
      0x7fef_ffff_ffff_ffff,
      0xffef_ffff_ffff_ffff,
      0x7ff0_0000_0000_0000,
      0xfff0_0000_0000_0000,
      0x7ff8_0000_0000_1234,
      0x7ff0_0000_0000_1234,
      0xbff0_0000_0000_0000,
      0x0010_0000_0000_0002,
      0x3fef_ffff_ffff_ffff,
    ];
    for a in values {
      for b in values {
        check_pair(a, b);
      }
    }
  }

  #[test]
  fn accelerated_rounding_matches_software_for_random_bit_patterns() {
    let mut state = 0x4979_706f_7758_1234_u64;
    let mut next = || {
      state ^= state << 13;
      state ^= state >> 7;
      state ^= state << 17;
      state
    };
    for _ in 0..20_000 {
      check_pair(next(), next());
    }
  }
}
