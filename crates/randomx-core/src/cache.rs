use alloc::vec;
use alloc::vec::Vec;

use argon2::{Algorithm, Argon2, Block, Params, Version};

use crate::{
  Error, Result,
  blake_generator::Blake2Generator,
  constants::{
    ARGON_ITERATIONS, ARGON_LANES, ARGON_MEMORY, ARGON_SALT, CACHE_ACCESSES, CACHE_LINE_SIZE,
    CACHE_SIZE, DATASET_ADD, DATASET_MUL,
  },
  superscalar::{CompiledSuperscalarProgram, SuperscalarProgram, generate_superscalar},
};

#[derive(Debug)]
pub struct RandomXCache {
  memory: Vec<Block>,
  programs: Vec<SuperscalarProgram>,
  compiled_programs: Vec<CompiledSuperscalarProgram>,
  #[cfg(target_arch = "wasm32")]
  dataset_function: Option<unsafe extern "C" fn(u64, *mut u64)>,
  #[cfg(target_arch = "wasm32")]
  dataset_pair_function: Option<unsafe extern "C" fn(u64, u64, *mut u64, *mut u64)>,
  #[cfg(target_arch = "wasm32")]
  dataset_triple_function: Option<unsafe extern "C" fn(*const u64, *mut u64)>,
}

impl RandomXCache {
  pub fn new(key: &[u8]) -> Result<Self> {
    let params =
      Params::new(ARGON_MEMORY, ARGON_ITERATIONS, ARGON_LANES, None).map_err(Error::Argon2)?;

    let argon = Argon2::new(Algorithm::Argon2d, Version::V0x13, params);

    let mut memory = vec![Block::new(); ARGON_MEMORY as usize];

    argon
      .fill_memory(key, ARGON_SALT, &mut memory)
      .map_err(Error::Argon2)?;

    let mut generator = Blake2Generator::new(key, 0);
    let mut programs = Vec::with_capacity(CACHE_ACCESSES);

    for _ in 0..CACHE_ACCESSES {
      programs.push(generate_superscalar(&mut generator));
    }

    let compiled_programs = programs
      .iter()
      .map(CompiledSuperscalarProgram::new)
      .collect();
    Ok(Self {
      memory,
      compiled_programs,
      programs,
      #[cfg(target_arch = "wasm32")]
      dataset_function: None,
      #[cfg(target_arch = "wasm32")]
      dataset_pair_function: None,
      #[cfg(target_arch = "wasm32")]
      dataset_triple_function: None,
    })
  }

  #[inline]
  pub fn cache_word(&self, word_index: usize) -> u64 {
    let block = word_index / 128;
    let word = word_index % 128;
    self.memory[block].as_ref()[word]
  }

  #[inline]
  pub fn cache_line(&self, line_index: usize) -> [u64; 8] {
    let words_per_line = CACHE_LINE_SIZE / 8;
    let first = line_index * words_per_line;

    let mut out = [0u64; 8];
    for (i, value) in out.iter_mut().enumerate() {
      *value = self.cache_word(first + i);
    }
    out
  }

  pub fn dataset_item(&self, item_number: u64) -> [u64; 8] {
    #[cfg(target_arch = "wasm32")]
    if let Some(function) = self.dataset_function {
      let mut output = core::mem::MaybeUninit::<[u64; 8]>::uninit();
      // The installed kernel initializes every output word before returning.
      unsafe {
        function(item_number, output.as_mut_ptr().cast());
        return output.assume_init();
      }
    }
    self.dataset_item_interpreted(item_number)
  }

  pub fn dataset_pair(&self, items: [u64; 2]) -> [[u64; 8]; 2] {
    #[cfg(target_arch = "wasm32")]
    if let Some(function) = self.dataset_pair_function {
      let mut out = core::mem::MaybeUninit::<[[u64; 8]; 2]>::uninit();
      // The installed kernel initializes both output rows before returning.
      unsafe {
        let first = out.as_mut_ptr().cast::<u64>();
        function(items[0], items[1], first, first.add(8));
        return out.assume_init();
      }
    }
    [self.dataset_item(items[0]), self.dataset_item(items[1])]
  }

  /// Installs a paired dataset kernel bound to this cache.
  ///
  /// # Safety
  /// The function must compute two independent items for this cache and write
  /// eight words to each output without retaining either pointer.
  #[cfg(target_arch = "wasm32")]
  pub unsafe fn set_dataset_pair_function(
    &mut self,
    function: Option<unsafe extern "C" fn(u64, u64, *mut u64, *mut u64)>,
  ) {
    self.dataset_pair_function = function;
  }

  pub fn dataset_triple(&self, items: [u64; 3]) -> [[u64; 8]; 3] {
    #[cfg(target_arch = "wasm32")]
    if let Some(function) = self.dataset_triple_function {
      let mut out = core::mem::MaybeUninit::<[[u64; 8]; 3]>::uninit();
      // The installed kernel initializes all three output rows before returning.
      unsafe {
        function(items.as_ptr(), out.as_mut_ptr().cast());
        return out.assume_init();
      }
    }
    items.map(|item| self.dataset_item(item))
  }

  /// Installs a three-item dataset kernel bound to this cache.
  ///
  /// # Safety
  /// The function must read three item indices and write three consecutive
  /// eight-word outputs for this cache without retaining either pointer.
  #[cfg(target_arch = "wasm32")]
  pub unsafe fn set_dataset_triple_function(
    &mut self,
    function: Option<unsafe extern "C" fn(*const u64, *mut u64)>,
  ) {
    self.dataset_triple_function = function;
  }

  pub fn dataset_item_interpreted(&self, item_number: u64) -> [u64; 8] {
    let mut registers = [0u64; 8];

    registers[0] = item_number.wrapping_add(1).wrapping_mul(DATASET_MUL);

    for i in 1..8 {
      registers[i] = registers[0] ^ DATASET_ADD[i];
    }

    let mut register_value = item_number;

    for program in &self.compiled_programs {
      let line_mask = CACHE_SIZE / CACHE_LINE_SIZE - 1;
      let line_index = register_value as usize & line_mask;

      let mix = self.cache_line(line_index);
      program.execute(&mut registers);

      for i in 0..8 {
        registers[i] ^= mix[i];
      }

      register_value = registers[program.address_register];
    }

    registers
  }

  #[cfg(target_arch = "wasm32")]
  pub fn dataset_module(&self) -> Vec<u8> {
    assert_eq!(core::mem::size_of::<Block>(), 1024);
    crate::wasm_dataset::generate(&self.programs, self.memory[0].as_ref().as_ptr() as u32)
  }

  /// Installs a dataset kernel bound to this cache.
  ///
  /// # Safety
  /// The function must implement this cache's dataset algorithm and write exactly
  /// eight words to its output pointer without retaining the pointer.
  #[cfg(target_arch = "wasm32")]
  pub unsafe fn set_dataset_function(
    &mut self,
    function: Option<unsafe extern "C" fn(u64, *mut u64)>,
  ) {
    self.dataset_function = function;
  }

  pub fn programs(&self) -> &[SuperscalarProgram] {
    &self.programs
  }
}
