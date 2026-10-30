//! Portable RandomX implementation shared by the native gateway verifier and
//! the browser WASM solver.
//!
//! The crate targets RandomX v1 semantics in Light mode: the Argon2d cache is
//! kept in memory and dataset items are derived on demand, so the resident
//! footprint is the cache instead of the full ~2 GiB dataset.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod aes;
pub mod blake_generator;
pub mod cache;
pub mod constants;
pub mod float;
pub mod helpers;
pub mod instruction;
pub mod program;
pub mod reciprocal;
pub mod superscalar;
pub mod vm;
#[cfg(target_arch = "wasm32")]
mod wasm_dataset;

use alloc::boxed::Box;

use blake2::{Blake2b256, Blake2b512, Digest};

use crate::{
  aes::{fill_aes_1r, hash_aes_1r},
  cache::RandomXCache,
  constants::PROGRAM_COUNT,
  vm::VirtualMachine,
};

/// Byte offset of the `a` registers inside the serialized register file.
/// RandomX writes the final scratchpad hash there before the closing
/// Blake2b-256 pass.
const REGISTER_FILE_A_OFFSET: usize = 192;

#[derive(Debug, thiserror::Error)]
pub enum Error {
  #[error("Argon2d cache initialization failed: {0}")]
  Argon2(#[from] argon2::Error),
}

pub type Result<T> = core::result::Result<T, Error>;

/// Light mode RandomX hasher bound to a single Argon2d cache key.
pub struct RandomX {
  cache: RandomXCache,
  vm: VirtualMachine,
  pair_vm: Option<Box<VirtualMachine>>,
  third_vm: Option<Box<VirtualMachine>>,
}

impl core::fmt::Debug for RandomX {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.debug_struct("RandomX").finish_non_exhaustive()
  }
}

impl RandomX {
  /// Builds a hasher for `seed`, the Monero block seed hash or P2Pool seed hash.
  ///
  /// Cache construction is CPU and memory heavy, so callers should keep the
  /// instance alive and reuse it across hashes.
  pub fn new(seed: &[u8]) -> Result<Self> {
    Ok(Self {
      cache: RandomXCache::new(seed)?,
      vm: VirtualMachine::new(),
      pair_vm: None,
      third_vm: None,
    })
  }

  /// Creates a hasher with an imported cache.
  pub fn from_cache(cache: RandomXCache) -> Self {
    Self {
      cache,
      vm: VirtualMachine::new(),
      pair_vm: None,
      third_vm: None,
    }
  }

  #[cfg(target_arch = "wasm32")]
  pub fn cache_mut(&mut self) -> &mut RandomXCache {
    &mut self.cache
  }

  pub fn cache(&self) -> &RandomXCache {
    &self.cache
  }

  pub fn vm(&self) -> &VirtualMachine {
    &self.vm
  }

  /// Computes the 32-byte RandomX hash of `input`.
  pub fn hash(&mut self, input: &[u8]) -> Result<[u8; 32]> {
    let mut seed: [u8; 64] = Blake2b512::digest(input).into();

    self.vm.reset_rounding();
    fill_aes_1r(&mut seed, self.vm.scratchpad_mut());

    for _ in 0..PROGRAM_COUNT - 1 {
      self.vm.run(&seed, &self.cache);
      seed = Blake2b512::digest(self.vm.registers().to_bytes()).into();
    }

    self.vm.run(&seed, &self.cache);

    Ok(self.final_result())
  }

  /// Hashes two independent inputs using one cache and interleaved dataset reads.
  pub fn hash_pair(&mut self, inputs: [&[u8]; 2]) -> Result<[[u8; 32]; 2]> {
    let other = self
      .pair_vm
      .get_or_insert_with(|| Box::new(VirtualMachine::new()));
    let mut seeds: [[u8; 64]; 2] = inputs.map(|input| Blake2b512::digest(input).into());
    self.vm.reset_rounding();
    other.reset_rounding();
    fill_aes_1r(&mut seeds[0], self.vm.scratchpad_mut());
    fill_aes_1r(&mut seeds[1], other.scratchpad_mut());
    for index in 0..PROGRAM_COUNT {
      let programs = seeds.each_ref().map(program::generate_program);
      self.vm.run_pair(other, programs.each_ref(), &self.cache);
      if index + 1 < PROGRAM_COUNT {
        seeds[0] = Blake2b512::digest(self.vm.registers().to_bytes()).into();
        seeds[1] = Blake2b512::digest(other.registers().to_bytes()).into();
      }
    }
    Ok([Self::finalize(&self.vm), Self::finalize(other)])
  }

  /// Hashes three independent inputs using one cache and interleaved dataset reads.
  pub fn hash_triple(&mut self, inputs: [&[u8]; 3]) -> Result<[[u8; 32]; 3]> {
    let other = self
      .pair_vm
      .get_or_insert_with(|| Box::new(VirtualMachine::new()));
    let third = self
      .third_vm
      .get_or_insert_with(|| Box::new(VirtualMachine::new()));
    let mut vms = [&mut self.vm, other.as_mut(), third.as_mut()];
    let mut seeds: [[u8; 64]; 3] = inputs.map(|input| Blake2b512::digest(input).into());
    for (vm, seed) in vms.iter_mut().zip(&mut seeds) {
      vm.reset_rounding();
      fill_aes_1r(seed, vm.scratchpad_mut());
    }
    for index in 0..PROGRAM_COUNT {
      let programs = seeds.each_ref().map(program::generate_program);
      VirtualMachine::run_triple(&mut vms, &programs, &self.cache);
      if index + 1 < PROGRAM_COUNT {
        for (seed, vm) in seeds.iter_mut().zip(&vms) {
          *seed = Blake2b512::digest(vm.registers().to_bytes()).into();
        }
      }
    }
    Ok(vms.map(|vm| Self::finalize(vm)))
  }

  /// Hashes the filled scratchpad into the register file and digests it.
  fn final_result(&self) -> [u8; 32] {
    Self::finalize(&self.vm)
  }

  fn finalize(vm: &VirtualMachine) -> [u8; 32] {
    let scratchpad_hash = hash_aes_1r(vm.scratchpad());
    let mut register_file = vm.registers().to_bytes();
    register_file[REGISTER_FILE_A_OFFSET..REGISTER_FILE_A_OFFSET + scratchpad_hash.len()]
      .copy_from_slice(&scratchpad_hash);

    Blake2b256::digest(register_file).into()
  }
}
