#[derive(Debug, Clone)]
pub struct DifficultyConfig {
  pub base: u64,
  pub weight_step: i32,
  pub max: u64,
}
