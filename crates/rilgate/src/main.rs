use clap::Parser;
use rilgate::cli::Args;

// Per-thread heaps keep concurrent request handling from contending on one allocator lock.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  // Load the environment file before parsing, because its variables can supply flag values.
  match rilgate::cli::env_file(&std::env::args_os().collect::<Vec<_>>()) {
    Some(path) => {
      dotenvy::from_path(&path)
        .map_err(|error| anyhow::anyhow!("loading environment file {}: {error}", path.display()))?;
    }
    None => {
      dotenvy::dotenv().ok();
    }
  }
  rilgate::cli::run(Args::parse()).await
}
