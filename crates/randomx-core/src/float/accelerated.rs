//! Nearest-rounded binary64 arithmetic with exact directed-rounding corrections.

use core::cmp::Ordering;

use super::RoundingMode;

const SIGN: u64 = 1 << 63;
const FRACTION: u64 = (1 << 52) - 1;

#[inline]
fn normal(bits: u64) -> Option<(u128, i32)> {
  let exponent = (bits >> 52) & 0x7ff;
  if exponent == 0 || exponent == 0x7ff {
    return None;
  }
  Some((
    ((bits & FRACTION) | (1 << 52)) as u128,
    exponent as i32 - 1075,
  ))
}

#[inline]
fn compare_scaled(a: u128, ae: i32, b: u128, be: i32) -> Ordering {
  let aw = 128 - a.leading_zeros() as i32;
  let bw = 128 - b.leading_zeros() as i32;
  // Matching leading exponents keep alignment within the wider significand.
  match (aw + ae).cmp(&(bw + be)) {
    Ordering::Equal if ae >= be => (a << (ae - be)).cmp(&b),
    Ordering::Equal => a.cmp(&(b << (be - ae))),
    order => order,
  }
}

#[inline]
fn finish(bits: u64, exact: Ordering, mode: RoundingMode) -> u64 {
  let negative = bits & SIGN != 0;
  let upward = match mode {
    RoundingMode::Up => exact == Ordering::Greater,
    RoundingMode::TowardZero if negative => exact == Ordering::Greater,
    _ => false,
  };
  let downward = match mode {
    RoundingMode::Down => exact == Ordering::Less,
    RoundingMode::TowardZero if !negative => exact == Ordering::Less,
    _ => false,
  };
  if upward {
    if negative { bits - 1 } else { bits + 1 }
  } else if downward {
    if negative { bits + 1 } else { bits - 1 }
  } else {
    bits
  }
}

#[inline]
pub(super) fn add(a: u64, b: u64, mode: RoundingMode) -> Option<u64> {
  let a = f64::from_bits(a);
  let b = f64::from_bits(b);
  let result = a + b;
  if !a.is_finite() || !b.is_finite() || !result.is_normal() {
    return None;
  }
  if mode == RoundingMode::Nearest {
    return Some(result.to_bits());
  }
  // TwoSum recovers the exact residual under nearest rounding and gradual underflow.
  let recovered_b = result - a;
  let error = (a - (result - recovered_b)) + (b - recovered_b);
  let exact = error.partial_cmp(&0.0)?;
  if !error.is_finite() {
    return None;
  }
  Some(finish(result.to_bits(), exact, mode))
}

#[inline]
pub(super) fn mul(a: u64, b: u64, mode: RoundingMode) -> Option<u64> {
  let (am, ae) = normal(a)?;
  let (bm, be) = normal(b)?;
  let result = (f64::from_bits(a) * f64::from_bits(b)).to_bits();
  let (rm, re) = normal(result)?;
  if mode == RoundingMode::Nearest {
    return Some(result);
  }
  // A binary64 significand product fits exactly in 106 bits.
  let mut exact = compare_scaled(am * bm, ae + be, rm, re);
  if result & SIGN != 0 {
    exact = exact.reverse();
  }
  Some(finish(result, exact, mode))
}

#[inline]
pub(super) fn div(a: u64, b: u64, mode: RoundingMode) -> Option<u64> {
  let (am, ae) = normal(a)?;
  let (bm, be) = normal(b)?;
  let result = (f64::from_bits(a) / f64::from_bits(b)).to_bits();
  let (rm, re) = normal(result)?;
  if mode == RoundingMode::Nearest {
    return Some(result);
  }
  let mut exact = compare_scaled(am, ae, rm * bm, re + be);
  if result & SIGN != 0 {
    exact = exact.reverse();
  }
  Some(finish(result, exact, mode))
}

#[inline]
pub(super) fn sqrt(a: u64, mode: RoundingMode) -> Option<u64> {
  #[cfg(feature = "std")]
  {
    if a & SIGN != 0 {
      return None;
    }
    let (am, ae) = normal(a)?;
    let result = f64::from_bits(a).sqrt().to_bits();
    let (rm, re) = normal(result)?;
    if mode == RoundingMode::Nearest {
      return Some(result);
    }
    Some(finish(
      result,
      compare_scaled(am, ae, rm * rm, re * 2),
      mode,
    ))
  }
  #[cfg(not(feature = "std"))]
  {
    let _ = (a, mode);
    None
  }
}
