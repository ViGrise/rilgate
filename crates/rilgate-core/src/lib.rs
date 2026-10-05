pub mod error;
pub mod time;
pub type BoxFuture<'a, T> =
  std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<T>> + Send + 'a>>;
