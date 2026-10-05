use super::{Algorithm, PowAlgorithm};
use anyhow::{Result, anyhow, ensure};
use std::{collections::HashMap, sync::Arc};
#[derive(Default)]
pub struct AlgorithmRegistry {
  algorithms: HashMap<PowAlgorithm, Arc<dyn Algorithm>>,
}
impl AlgorithmRegistry {
  pub fn register(&mut self, algorithm: Arc<dyn Algorithm>) -> Result<()> {
    let id = algorithm.id();
    ensure!(
      !self.algorithms.contains_key(&id),
      "Algorithm {id:?} is already registered"
    );
    self.algorithms.insert(id, algorithm);
    Ok(())
  }
  pub fn get(&self, id: PowAlgorithm) -> Result<&Arc<dyn Algorithm>> {
    self
      .algorithms
      .get(&id)
      .ok_or_else(|| anyhow!("Algorithm {id:?} is not enabled"))
  }
  pub fn descriptors(&self) -> Vec<super::AlgorithmDescriptor> {
    let mut descriptors: Vec<_> = self.algorithms.values().map(|a| a.descriptor()).collect();
    descriptors.sort_by_key(|d| d.id);
    descriptors
  }
}
