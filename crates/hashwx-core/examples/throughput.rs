use hashwx_core::HashWx;
use std::{hint::black_box, time::Instant};

fn main() {
  let count: u32 = std::env::args()
    .nth(1)
    .map(|value| value.parse().expect("hash count"))
    .unwrap_or(10_000);
  assert!((1..=10_000_000).contains(&count));
  let seed = *b"This is a test seed for hashwx\0\0";
  let started = Instant::now();
  let mut hasher = HashWx::new(black_box(&seed));
  let generation_us = started.elapsed().as_secs_f64() * 1_000_000.0;
  for nonce in 0..10 {
    black_box(hasher.hash(nonce));
  }
  let started = Instant::now();
  let mut checksum = 0u64;
  for nonce in 0..count {
    checksum ^= black_box(hasher.hash(black_box(nonce as u64)));
  }
  let elapsed = started.elapsed().as_secs_f64();
  println!(
    "{{\"runtime\":\"native\",\"checksum\":\"{checksum:016x}\",\"hashes\":{count},\"generationUs\":{generation_us},\"elapsedMs\":{},\"hashesPerSecond\":{}}}",
    elapsed * 1000.0,
    count as f64 / elapsed
  );
}
