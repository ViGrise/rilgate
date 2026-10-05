use crate::{
  aes::fill_aes_4r,
  constants::{
    CACHE_LINE_ALIGN_MASK, CACHE_LINE_SIZE, CONST_EXPONENT_BITS, DATASET_EXTRA_ITEMS,
    DYNAMIC_EXPONENT_BITS, EXPONENT_BIAS, EXPONENT_MASK, MANTISSA_MASK, MANTISSA_SIZE,
    PROGRAM_BYTES, PROGRAM_ENTROPY_SIZE, PROGRAM_SIZE, STATIC_EXPONENT_BITS,
  },
  helpers::load_u64,
  instruction::Instruction,
};

#[derive(Debug, Clone)]
pub struct Program {
  pub entropy: [u64; 16],
  pub instructions: [Instruction; PROGRAM_SIZE],
}

#[derive(Debug, Clone, Copy)]
pub struct ProgramConfiguration {
  pub e_mask: [u64; 2],
  pub read_reg: [usize; 4],
  pub ma: u64,
  pub mx: u64,
  pub dataset_offset: u64,
}

pub fn generate_program(seed: &[u8; 64]) -> Program {
  let mut bytes = [0u8; PROGRAM_BYTES];
  fill_aes_4r(seed, &mut bytes);

  let mut entropy = [0u64; 16];
  for (i, item) in entropy.iter_mut().enumerate() {
    *item = load_u64(&bytes, i * 8);
  }

  let mut instructions = [Instruction::default(); PROGRAM_SIZE];
  for (i, instruction) in instructions.iter_mut().enumerate() {
    let offset = PROGRAM_ENTROPY_SIZE + i * 8;
    *instruction = Instruction::from_bytes(&bytes[offset..offset + 8]);
  }

  Program {
    entropy,
    instructions,
  }
}

pub fn configure(program: &Program) -> ProgramConfiguration {
  let address = program.entropy[12];

  let read_reg = [
    (address & 1) as usize,
    2 + ((address >> 1) & 1) as usize,
    4 + ((address >> 2) & 1) as usize,
    6 + ((address >> 3) & 1) as usize,
  ];

  ProgramConfiguration {
    e_mask: [
      float_mask(program.entropy[14]),
      float_mask(program.entropy[15]),
    ],
    read_reg,
    ma: program.entropy[8] & CACHE_LINE_ALIGN_MASK,
    mx: program.entropy[10],
    dataset_offset: (program.entropy[13] % (DATASET_EXTRA_ITEMS + 1)) * CACHE_LINE_SIZE as u64,
  }
}

#[inline]
pub fn small_positive_float_bits(entropy: u64) -> u64 {
  let exponent = ((entropy >> 59) + EXPONENT_BIAS) & EXPONENT_MASK;
  (exponent << MANTISSA_SIZE) | (entropy & MANTISSA_MASK)
}

#[inline]
pub fn static_exponent(entropy: u64) -> u64 {
  let exponent =
    CONST_EXPONENT_BITS | ((entropy >> (64 - STATIC_EXPONENT_BITS)) << DYNAMIC_EXPONENT_BITS);

  exponent << MANTISSA_SIZE
}

#[inline]
pub fn float_mask(entropy: u64) -> u64 {
  const MASK_22: u64 = (1u64 << 22) - 1;
  (entropy & MASK_22) | static_exponent(entropy)
}
