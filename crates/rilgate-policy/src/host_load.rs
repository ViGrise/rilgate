use crate::policy::schema::LoadWindow;
use std::sync::atomic::{AtomicU64, Ordering};

// Bit pattern of a NaN that no sample produces, marking an empty slot.
const NO_SAMPLE: u64 = u64::MAX;

/// The most recent 1, 5 and 15 minute host load averages.
#[derive(Debug)]
pub struct HostLoad {
  averages: [AtomicU64; 3],
}

impl HostLoad {
  pub const fn new() -> Self {
    Self {
      averages: [
        AtomicU64::new(NO_SAMPLE),
        AtomicU64::new(NO_SAMPLE),
        AtomicU64::new(NO_SAMPLE),
      ],
    }
  }

  pub fn store(&self, averages: [f64; 3]) {
    for (slot, value) in self.averages.iter().zip(averages) {
      slot.store(value.to_bits(), Ordering::Relaxed);
    }
  }

  /// Returns the average for `window`, or `None` before the first sample.
  pub fn get(&self, window: LoadWindow) -> Option<f64> {
    let slot = match window {
      LoadWindow::OneMinute => 0,
      LoadWindow::FiveMinutes => 1,
      LoadWindow::FifteenMinutes => 2,
    };
    match self.averages[slot].load(Ordering::Relaxed) {
      NO_SAMPLE => None,
      bits => Some(f64::from_bits(bits)),
    }
  }
}

impl Default for HostLoad {
  fn default() -> Self {
    Self::new()
  }
}

/// The process-wide host load, filled by the gateway sampler.
pub static HOST_LOAD: HostLoad = HostLoad::new();

/// Reads the 1, 5 and 15 minute load averages from the operating system.
#[cfg(unix)]
pub fn read_host_load() -> Option<[f64; 3]> {
  let mut averages = [0.0_f64; 3];
  // SAFETY: the pointer addresses three writable f64 values, the count passed.
  let read = unsafe { libc::getloadavg(averages.as_mut_ptr(), 3) };
  (read == 3).then_some(averages)
}

/// Reads the 1, 5 and 15 minute load averages from the operating system.
#[cfg(not(unix))]
pub fn read_host_load() -> Option<[f64; 3]> {
  None
}
