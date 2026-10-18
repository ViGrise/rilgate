mod ranges;
pub use ranges::IpRanges;

pub const MAX_IP_SETS: usize = 64;
pub const MAX_SET_NAME_BYTES: usize = 64;
pub const MAX_SETS_PER_MATCHER: usize = 16;
pub const MAX_PREFIXES_PER_SET: usize = 65_536;
