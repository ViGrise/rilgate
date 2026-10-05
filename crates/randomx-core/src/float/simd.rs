//! Paired binary64 arithmetic with normalized exact rounding residuals.

use super::{FReg, RoundingMode};
use core::arch::wasm32::*;

#[inline]
#[target_feature(enable = "simd128")]
unsafe fn load(reg: FReg) -> v128 {
  unsafe { v128_load(reg.0.as_ptr().cast()) }
}
#[inline]
#[target_feature(enable = "simd128")]
unsafe fn store(value: v128) -> FReg {
  let mut out = FReg::default();
  unsafe {
    v128_store(out.0.as_mut_ptr().cast(), value);
  }
  out
}
#[inline]
#[target_feature(enable = "simd128")]
fn normal(value: v128) -> bool {
  let exponent = v128_and(value, i64x2_splat(0x7ff0_0000_0000_0000));
  !v128_any_true(v128_or(
    i64x2_eq(exponent, i64x2_splat(0)),
    i64x2_eq(exponent, i64x2_splat(0x7ff0_0000_0000_0000)),
  ))
}
#[inline]
#[target_feature(enable = "simd128")]
fn normalized(value: v128) -> v128 {
  v128_or(
    v128_and(value, i64x2_splat(0x000f_ffff_ffff_ffff)),
    i64x2_splat(0x3ff0_0000_0000_0000),
  )
}
#[inline]
#[target_feature(enable = "simd128")]
fn exponent(value: v128) -> v128 {
  v128_and(value, i64x2_splat(0x7ff0_0000_0000_0000))
}
#[inline]
#[target_feature(enable = "simd128")]
fn rescale(value: v128, exponent: v128) -> v128 {
  v128_or(
    v128_and(value, i64x2_splat(0x000f_ffff_ffff_ffff)),
    exponent,
  )
}
#[inline]
#[target_feature(enable = "simd128")]
fn finish(value: v128, residual: v128, mode: RoundingMode) -> v128 {
  let zero = f64x2_splat(0.0);
  let negative = i64x2_lt(value, i64x2_splat(0));
  let up = f64x2_gt(residual, zero);
  let down = f64x2_lt(residual, zero);
  let (mask, step) = match mode {
    RoundingMode::Nearest => return value,
    RoundingMode::Up => (
      up,
      v128_bitselect(i64x2_splat(-1), i64x2_splat(1), negative),
    ),
    RoundingMode::Down => (
      down,
      v128_bitselect(i64x2_splat(1), i64x2_splat(-1), negative),
    ),
    RoundingMode::TowardZero => (v128_bitselect(up, down, negative), i64x2_splat(-1)),
  };
  v128_bitselect(i64x2_add(value, step), value, mask)
}
#[inline]
#[target_feature(enable = "simd128")]
fn split(value: v128) -> (v128, v128) {
  let expanded = f64x2_mul(value, f64x2_splat(134217729.0));
  let high = f64x2_sub(expanded, f64x2_sub(expanded, value));
  (high, f64x2_sub(value, high))
}
#[inline]
#[target_feature(enable = "simd128")]
fn product_error(a: v128, b: v128, product: v128) -> v128 {
  // Normalization keeps Dekker's split and every residual far from underflow.
  let (ah, al) = split(a);
  let (bh, bl) = split(b);
  let first = f64x2_sub(product, f64x2_mul(ah, bh));
  let second = f64x2_sub(first, f64x2_mul(al, bh));
  let third = f64x2_sub(second, f64x2_mul(ah, bl));
  f64x2_sub(f64x2_mul(al, bl), third)
}
#[inline]
#[target_feature(enable = "simd128")]
fn signed_error(error: v128, value: v128) -> v128 {
  v128_xor(error, v128_and(value, i64x2_splat(i64::MIN)))
}

#[cfg_attr(not(target_feature = "simd128"), target_feature(enable = "simd128"))]
#[cfg_attr(target_feature = "simd128", inline(always))]
pub(super) unsafe fn add(a: FReg, b: FReg, mode: RoundingMode) -> Option<FReg> {
  let a = unsafe { load(a) };
  let b = unsafe { load(b) };
  let value = f64x2_add(a, b);
  if !normal(value) {
    return None;
  }
  if mode == RoundingMode::Nearest {
    return Some(unsafe { store(value) });
  }
  let recovered = f64x2_sub(value, a);
  let error = f64x2_add(
    f64x2_sub(a, f64x2_sub(value, recovered)),
    f64x2_sub(b, recovered),
  );
  Some(unsafe { store(finish(value, error, mode)) })
}

#[cfg_attr(not(target_feature = "simd128"), target_feature(enable = "simd128"))]
#[cfg_attr(target_feature = "simd128", inline(always))]
pub(super) unsafe fn mul(a: FReg, b: FReg, mode: RoundingMode) -> Option<FReg> {
  let a = unsafe { load(a) };
  let b = unsafe { load(b) };
  let value = f64x2_mul(a, b);
  if !normal(a) || !normal(b) || !normal(value) {
    return None;
  }
  if mode == RoundingMode::Nearest {
    return Some(unsafe { store(value) });
  }
  // At the normal/subnormal boundary, normalization changes the rounding grid.
  if v128_any_true(i64x2_eq(exponent(value), i64x2_splat(1 << 52))) {
    return None;
  }
  let a = normalized(a);
  let b = normalized(b);
  let product = f64x2_mul(a, b);
  let error = signed_error(product_error(a, b, product), value);
  Some(unsafe { store(finish(value, error, mode)) })
}

#[cfg_attr(not(target_feature = "simd128"), target_feature(enable = "simd128"))]
#[cfg_attr(target_feature = "simd128", inline(always))]
pub(super) unsafe fn div(a: FReg, b: FReg, mode: RoundingMode) -> Option<FReg> {
  let a = unsafe { load(a) };
  let b = unsafe { load(b) };
  let value = f64x2_div(a, b);
  if !normal(a) || !normal(b) || !normal(value) {
    return None;
  }
  if mode == RoundingMode::Nearest {
    return Some(unsafe { store(value) });
  }
  let quotient = rescale(
    value,
    i64x2_add(i64x2_sub(exponent(value), exponent(a)), exponent(b)),
  );
  let a = normalized(a);
  let b = normalized(b);
  let product = f64x2_mul(quotient, b);
  let error = f64x2_sub(f64x2_sub(a, product), product_error(quotient, b, product));
  Some(unsafe { store(finish(value, signed_error(error, value), mode)) })
}

#[cfg(feature = "std")]
#[cfg_attr(not(target_feature = "simd128"), target_feature(enable = "simd128"))]
#[cfg_attr(target_feature = "simd128", inline(always))]
pub(super) unsafe fn sqrt(a: FReg, mode: RoundingMode) -> Option<FReg> {
  let a = unsafe { load(a) };
  let value = f64x2_sqrt(a);
  if !normal(a) || !normal(value) || v128_any_true(i64x2_lt(a, i64x2_splat(0))) {
    return None;
  }
  if mode == RoundingMode::Nearest {
    return Some(unsafe { store(value) });
  }
  let odd = i64x2_eq(v128_and(a, i64x2_splat(1 << 52)), i64x2_splat(0));
  let a = v128_bitselect(
    f64x2_mul(normalized(a), f64x2_splat(2.0)),
    normalized(a),
    odd,
  );
  let root = normalized(value);
  let product = f64x2_mul(root, root);
  let error = f64x2_sub(f64x2_sub(a, product), product_error(root, root, product));
  Some(unsafe { store(finish(value, error, mode)) })
}
