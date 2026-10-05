pub fn reciprocal(divisor: u32) -> u64 {
  debug_assert!(divisor != 0 && !divisor.is_power_of_two());

  let divisor = divisor as u64;
  let mut quotient = (1u64 << 63) / divisor;
  let remainder = (1u64 << 63) % divisor;
  let shift = 64 - divisor.leading_zeros();

  quotient = quotient.wrapping_shl(shift);
  quotient.wrapping_add(remainder.wrapping_shl(shift) / divisor)
}
