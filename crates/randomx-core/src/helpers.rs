#[inline]
pub fn load_u64(input: &[u8], offset: usize) -> u64 {
  u64::from_le_bytes(input[offset..offset + 8].try_into().expect("u64"))
}

#[inline]
pub fn store_u64(output: &mut [u8], offset: usize, value: u64) {
  output[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

#[inline]
pub fn sign_extend_u32(value: u32) -> u64 {
  (value as i32 as i64) as u64
}

#[inline]
pub fn mulh(a: u64, b: u64) -> u64 {
  (((a as u128) * (b as u128)) >> 64) as u64
}

#[inline]
pub fn smulh(a: u64, b: u64) -> u64 {
  let product = (a as i64 as i128).wrapping_mul(b as i64 as i128);
  (product >> 64) as u64
}

#[inline]
pub fn is_zero_or_power_of_two(value: u32) -> bool {
  value == 0 || value.is_power_of_two()
}
