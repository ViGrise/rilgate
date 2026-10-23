use hashwx_core::HashWx;
use std::{hint::black_box, time::Instant};

fn seed(mut state: u32) -> [u8; 32] {
  if state == 0 {
    state = 1;
  }
  core::array::from_fn(|_| {
    state ^= state << 13;
    state ^= state >> 17;
    state ^= state << 5;
    state as u8
  })
}

fn main() {
  let nonces_per_seed: u32 = std::env::args()
    .nth(1)
    .map(|value| value.parse().expect("nonces per seed"))
    .unwrap_or(10_000);
  assert!((1..=10_000_000).contains(&nonces_per_seed));
  let mut warmup = HashWx::new(&seed(1));
  for nonce in 0..256 {
    black_box(warmup.hash(nonce));
  }
  let mut generation_us = 0.0;
  let mut hash_ms = 0.0;
  let mut checksum = 0u64;
  for repetition in 0..3 {
    let started = Instant::now();
    let mut hasher = HashWx::new(black_box(&seed(nonces_per_seed + (repetition + 2) * 71)));
    generation_us += started.elapsed().as_secs_f64() * 1_000_000.0;
    let started = Instant::now();
    for nonce in 0..nonces_per_seed {
      checksum ^= black_box(hasher.hash(black_box(u64::from(nonce))));
    }
    hash_ms += started.elapsed().as_secs_f64() * 1000.0;
  }
  let verification_started = Instant::now();
  let verification_samples = 10_000;
  for nonce in 0..verification_samples {
    black_box(HashWx::new(black_box(&seed(nonce + 1))).hash(black_box(u64::from(nonce))));
  }
  let verification_us =
    verification_started.elapsed().as_secs_f64() * 1_000_000.0 / f64::from(verification_samples);
  let mut solves = Vec::new();
  for difficulty in [1000u64, 10000, 50000] {
    let target = u64::MAX / difficulty;
    for trial in 0..3u32 {
      let started = Instant::now();
      let mut batch = 0u32;
      let mut attempts = 0u64;
      'search: loop {
        let salt = difficulty as u32 ^ ((trial + 1) * 104729) ^ batch.wrapping_mul(15485863);
        let mut hasher = HashWx::new(black_box(&seed(salt)));
        for offset in 0..nonces_per_seed {
          attempts += 1;
          if hasher.hash(u64::from(batch) * u64::from(nonces_per_seed) + u64::from(offset)) < target
          {
            break 'search;
          }
        }
        batch += 1;
      }
      solves.push(format!(
        "{{\"difficulty\":{difficulty},\"trial\":{},\"attempts\":{attempts},\"batches\":{},\"elapsedMs\":{}}}",
        trial + 1,
        batch + 1,
        started.elapsed().as_secs_f64() * 1000.0
      ));
    }
  }
  println!(
    "{{\"runtime\":\"native interpreter\",\"checksum\":\"{checksum:016x}\",\"noncesPerSeed\":{nonces_per_seed},\"repetitions\":3,\"generationUs\":{},\"hashMs\":{},\"hashesPerSecond\":{},\"totalHashesPerSecond\":{},\"generationAndHashUs\":{verification_us},\"solves\":[{}]}}",
    generation_us / 3.0,
    hash_ms / 3.0,
    f64::from(nonces_per_seed) * 3000.0 / hash_ms,
    f64::from(nonces_per_seed) * 3000.0 / (hash_ms + generation_us / 1000.0),
    solves.join(",")
  );
}
