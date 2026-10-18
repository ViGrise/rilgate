use std::fmt::Write;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub struct Metrics {
  requests: [AtomicU64; 6],
  duration: [AtomicU64; 8],
  micros: AtomicU64,
  pub connection_rejections: AtomicU64,
  pub request_rejections: AtomicU64,
}

const BOUNDS: [u64; 7] = [1000, 5000, 10000, 50000, 100000, 1000000, 10000000];

impl Metrics {
  pub fn observe(&self, status: u16, micros: u64) {
    self.requests[usize::from(status / 100).min(5)].fetch_add(1, Ordering::Relaxed);
    let bucket = BOUNDS
      .iter()
      .position(|bound| micros <= *bound)
      .unwrap_or(7);
    self.duration[bucket].fetch_add(1, Ordering::Relaxed);
    self.micros.fetch_add(micros, Ordering::Relaxed);
  }

  pub fn render(&self) -> String {
    let mut text = String::from("# TYPE rilgate_http_requests_total counter\n");
    for (class, count) in self.requests.iter().enumerate() {
      let _ = writeln!(
        text,
        "rilgate_http_requests_total{{status_class=\"{class}xx\"}} {}",
        count.load(Ordering::Relaxed)
      );
    }
    text.push_str("# TYPE rilgate_http_request_duration_seconds histogram\n");
    let mut count = 0;
    for (index, value) in self.duration.iter().enumerate() {
      count += value.load(Ordering::Relaxed);
      let bound = BOUNDS.get(index).map_or_else(
        || "+Inf".into(),
        |value| (*value as f64 / 1_000_000.0).to_string(),
      );
      let _ = writeln!(
        text,
        "rilgate_http_request_duration_seconds_bucket{{le=\"{bound}\"}} {count}"
      );
    }
    let _ = writeln!(text, "rilgate_http_request_duration_seconds_count {count}");
    let _ = writeln!(
      text,
      "rilgate_http_request_duration_seconds_sum {}",
      self.micros.load(Ordering::Relaxed) as f64 / 1_000_000.0
    );
    for (name, value) in [
      ("connections_rejected", &self.connection_rejections),
      ("requests_rejected", &self.request_rejections),
    ] {
      let _ = writeln!(
        text,
        "# TYPE rilgate_{name}_total counter\nrilgate_{name}_total {}",
        value.load(Ordering::Relaxed)
      );
    }
    text
  }
}
