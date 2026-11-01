pub const VERSION: &str = env!("RILGATE_VERSION");

pub mod app;
pub mod cli;
mod host_load;
pub mod reloader;
pub mod shutdown;

pub mod verifier;
