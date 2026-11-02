pub mod client;
pub mod config;
pub use client::{StratumEvent, StratumSession, fetch_job};
pub use config::StratumConfig;

mod codec;
mod job;

pub mod transport;

mod proxy;

#[cfg(feature = "fuzzing")]
#[doc(hidden)]
pub mod fuzzing {
  pub fn parse_job(value: &serde_json::Value) -> bool {
    crate::job::parse_job(value).is_ok()
  }
  pub fn read_json_lines(bytes: &[u8]) -> usize {
    let runtime = tokio::runtime::Builder::new_current_thread()
      .build()
      .expect("Create fuzzing runtime");
    runtime.block_on(async {
      let mut reader = crate::codec::JsonLineReader::new(bytes);
      let mut count = 0;
      while reader.next_json().await.is_ok() {
        count += 1;
      }
      count
    })
  }
}
