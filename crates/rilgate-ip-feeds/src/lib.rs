mod fetch;
mod parse;
pub use fetch::{FeedClient, Fetched, Validators};
pub use parse::parse;
use std::time::Duration;

pub const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_REDIRECTS: usize = 3;
pub const FIRST_RETRY: Duration = Duration::from_secs(5 * 60);
pub const JITTER_FRACTION: f64 = 0.10;
