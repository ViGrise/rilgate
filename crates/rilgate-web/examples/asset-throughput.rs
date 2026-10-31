use http::HeaderMap;
use http_body_util::BodyExt;
use rilgate_web::assets::{AssetCache, AssetMetadata};
use std::{hint::black_box, time::Instant};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
  let iterations = std::env::args()
    .nth(1)
    .map(|value| value.parse::<u32>())
    .transpose()?
    .unwrap_or(200);
  anyhow::ensure!(iterations > 0, "iterations must be positive");
  let root = tempfile::tempdir()?;
  let headers = HeaderMap::new();
  for size in [4096, 140 * 1024, 1024 * 1024] {
    let bytes = vec![b'x'; size];
    let path = root.path().join("asset.wasm");
    std::fs::write(&path, &bytes)?;
    let memory = AssetMetadata::memory("asset.wasm", bytes);
    let file = AssetMetadata::file(root.path(), "asset.wasm", &path)?;
    for (source, asset) in [("memory", memory), ("file", file)] {
      let mut samples = Vec::new();
      for _ in 0..5 {
        let start = Instant::now();
        let mut frames = 0u64;
        for _ in 0..iterations {
          let mut body = asset
            .response(black_box(&headers), false, AssetCache::Revalidate)?
            .into_body();
          let mut received = 0;
          while let Some(frame) = body.frame().await {
            if let Ok(data) = frame?.into_data() {
              received += black_box(data).len();
              frames += 1;
            }
          }
          assert_eq!(received, size);
        }
        samples.push((start.elapsed(), frames));
      }
      samples.sort_unstable();
      let (elapsed, frames) = samples[2];
      println!(
        "source={source} bytes={size} iterations={iterations} median_us={:.2} frames_per_response={:.1}",
        elapsed.as_secs_f64() * 1_000_000.0 / f64::from(iterations),
        frames as f64 / f64::from(iterations),
      );
    }
  }
  Ok(())
}
