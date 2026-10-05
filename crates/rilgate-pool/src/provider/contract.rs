use rilgate_core::BoxFuture;
pub trait WorkProvider: Send + Sync {
  type Work: Send;
  fn fetch(&self, difficulty: u64) -> BoxFuture<'_, Self::Work>;
}
