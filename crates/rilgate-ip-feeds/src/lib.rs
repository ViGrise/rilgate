mod fetch;
mod geoip;
mod parse;
mod refresh;
pub use fetch::{FeedClient, FetchOptions, Fetched, Validators};
pub use geoip::{GeoRefresher, GeoState, MAXMIND_DOWNLOAD_BASE, spawn_geoip};
pub use parse::parse;
pub use refresh::{Outcome, next_delay, refresh_once, spawn};
use std::time::Duration;

pub const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_REDIRECTS: usize = 3;
pub const FIRST_LOAD_RETRY: Duration = Duration::from_secs(15);
pub const FIRST_RETRY: Duration = Duration::from_secs(5 * 60);
pub const JITTER_FRACTION: f64 = 0.10;
