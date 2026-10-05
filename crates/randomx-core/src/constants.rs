pub const ARGON_MEMORY: u32 = 262_144;
pub const ARGON_ITERATIONS: u32 = 3;
pub const ARGON_LANES: u32 = 1;
pub const ARGON_SALT: &[u8] = b"RandomX\x03";

pub const CACHE_ACCESSES: usize = 8;
pub const SUPERSCALAR_LATENCY: usize = 170;
pub const SUPERSCALAR_MAX_SIZE: usize = SUPERSCALAR_LATENCY * 3 + 2;

pub const DATASET_BASE_SIZE: u64 = 2_147_483_648;
pub const DATASET_EXTRA_SIZE: u64 = 33_554_368;
pub const DATASET_ITEM_SIZE: usize = 64;
pub const DATASET_EXTRA_ITEMS: u64 = DATASET_EXTRA_SIZE / DATASET_ITEM_SIZE as u64;

pub const PROGRAM_SIZE: usize = 256;
pub const PROGRAM_ITERATIONS: usize = 2048;
pub const PROGRAM_COUNT: usize = 8;
pub const PROGRAM_ENTROPY_SIZE: usize = 128;
pub const PROGRAM_BYTES: usize = PROGRAM_ENTROPY_SIZE + PROGRAM_SIZE * 8;

pub const SCRATCHPAD_L3: usize = 2_097_152;
pub const SCRATCHPAD_L2: usize = 262_144;
pub const SCRATCHPAD_L1: usize = 16_384;

pub const JUMP_BITS: u32 = 8;
pub const JUMP_OFFSET: u32 = 8;
pub const CONDITION_MASK: u64 = (1u64 << JUMP_BITS) - 1;
pub const CONDITION_OFFSET: u32 = JUMP_OFFSET;
pub const STORE_L3_CONDITION: u8 = 14;

pub const CACHE_SIZE: usize = ARGON_MEMORY as usize * 1024;
pub const CACHE_LINE_SIZE: usize = 64;
pub const CACHE_LINE_ALIGN_MASK: u64 = (DATASET_BASE_SIZE - 1) & !(CACHE_LINE_SIZE as u64 - 1);

pub const SCRATCHPAD_L1_MASK: u64 = ((SCRATCHPAD_L1 / 8 - 1) * 8) as u64;
pub const SCRATCHPAD_L2_MASK: u64 = ((SCRATCHPAD_L2 / 8 - 1) * 8) as u64;
pub const SCRATCHPAD_L3_MASK: u64 = ((SCRATCHPAD_L3 / 8 - 1) * 8) as u64;
pub const SCRATCHPAD_L3_MASK64: u64 = ((SCRATCHPAD_L3 / 64 - 1) * 64) as u64;

pub const REGISTERS_COUNT: usize = 8;
pub const REGISTER_COUNT_FLT: usize = 4;
pub const REGISTER_NEEDS_DISPLACEMENT: usize = 5;

pub const MANTISSA_SIZE: u32 = 52;
pub const EXPONENT_SIZE: u32 = 11;
pub const MANTISSA_MASK: u64 = (1u64 << MANTISSA_SIZE) - 1;
pub const EXPONENT_MASK: u64 = (1u64 << EXPONENT_SIZE) - 1;
pub const EXPONENT_BIAS: u64 = 1023;
pub const DYNAMIC_EXPONENT_BITS: u32 = 4;
pub const STATIC_EXPONENT_BITS: u32 = 4;
pub const CONST_EXPONENT_BITS: u64 = 0x300;
pub const DYNAMIC_MANTISSA_MASK: u64 = (1u64 << (MANTISSA_SIZE + DYNAMIC_EXPONENT_BITS)) - 1;

pub const FSCAL_MASK: u64 = 0x80f0_0000_0000_0000;

pub const DATASET_MUL: u64 = 6_364_136_223_846_793_005;
pub const DATASET_ADD: [u64; 8] = [
  0,
  9_298_411_001_130_361_340,
  12_065_312_585_734_608_966,
  9_306_329_213_124_626_780,
  5_281_919_268_842_080_866,
  10_536_153_434_571_861_004,
  3_398_623_926_847_679_864,
  9_549_104_520_008_361_294,
];

pub const FREQS: [u8; 29] = [
  16, 7, 16, 7, 16, 4, 4, 1, 4, 1, 8, 2, 15, 5, 8, 2, 4, 4, 16, 5, 16, 5, 6, 32, 4, 6, 25, 1, 16,
];
