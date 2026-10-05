use blake2::{Blake2b512, Digest};
use randomx_core::RandomX;
use std::{hint::black_box, time::Instant};

fn main() {
  let count: u32 = std::env::args()
    .nth(1)
    .map(|value| value.parse().expect("hash count"))
    .unwrap_or(8);
  assert!((1..=10_000).contains(&count));
  let mode = std::env::args()
    .nth(2)
    .unwrap_or_else(|| "reference".into());
  assert!(matches!(mode.as_str(), "reference" | "nonce"));
  let mut input = if mode == "reference" {
    b"This is a test".to_vec()
  } else {
    (0..87).map(|index| (index * 17 + 3) as u8).collect()
  };
  let started = Instant::now();
  let mut hasher = RandomX::new(b"test key 000").expect("cache");
  let cache_ms = started.elapsed().as_secs_f64() * 1000.0;
  let expected = [
    0x63, 0x91, 0x83, 0xaa, 0xe1, 0xbf, 0x4c, 0x9a, 0x35, 0x88, 0x4c, 0xb4, 0x6b, 0x09, 0xca, 0xd9,
    0x17, 0x5f, 0x04, 0xef, 0xd7, 0x68, 0x4e, 0x72, 0x62, 0xa0, 0xac, 0x1c, 0x2f, 0x0b, 0x4e, 0x3f,
  ];
  let mut hash = |nonce: u32| {
    if mode == "nonce" {
      input[39..43].copy_from_slice(&nonce.to_le_bytes());
    }
    let result = hasher.hash(black_box(&input)).unwrap();
    if mode == "reference" {
      assert_eq!(result, expected);
    }
    result
  };
  for index in 0..2 {
    hash(u32::MAX - index);
  }
  let mut checksum = Blake2b512::new();
  let start = Instant::now();
  for index in 0..count {
    checksum.update(hash(index));
  }
  let elapsed = start.elapsed().as_secs_f64();
  let checksum: String = checksum
    .finalize()
    .iter()
    .map(|byte| format!("{byte:02x}"))
    .collect();
  println!(
    "{{\"runtime\":\"native\",\"mode\":\"{mode}\",\"checksum\":\"{checksum}\",\"hashes\":{count},\"cacheMs\":{cache_ms},\"elapsedMs\":{},\"hashesPerSecond\":{}}}",
    elapsed * 1000.0,
    count as f64 / elapsed
  );
}
