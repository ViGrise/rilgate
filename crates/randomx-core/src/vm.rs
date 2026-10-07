use alloc::vec::Vec;

use crate::{
  cache::RandomXCache,
  constants::{
    CONDITION_MASK, CONDITION_OFFSET, PROGRAM_ITERATIONS, REGISTER_NEEDS_DISPLACEMENT,
    SCRATCHPAD_L1_MASK, SCRATCHPAD_L2_MASK, SCRATCHPAD_L3_MASK, SCRATCHPAD_L3_MASK64,
    STORE_L3_CONDITION,
  },
  float::{
    FReg, RoundingMode, add as fadd, div as fdiv, from_i32_pair, mask_exponent_mantissa,
    mul as fmul, scale as fscale, sqrt as fsqrt, sub as fsub, swap as fswap, xor as fxor,
  },
  helpers::{is_zero_or_power_of_two, load_u64, mulh, sign_extend_u32, smulh, store_u64},
  instruction::{Instruction, VmOpcode, decode_vm_opcode},
  program::{
    Program, ProgramConfiguration, configure, generate_program, small_positive_float_bits,
  },
  reciprocal::reciprocal,
};

#[derive(Debug, Clone)]
enum IntSource {
  Register(usize),
  Immediate(u64),
}

#[derive(Debug, Clone)]
enum MemBase {
  Register(usize),
  Zero,
}

#[derive(Debug, Clone)]
enum CompiledInstruction {
  IAddRs {
    dst: usize,
    src: usize,
    shift: u32,
    imm: u64,
  },
  IAddM {
    dst: usize,
    base: MemBase,
    imm: u64,
    mask: u64,
  },
  ISubR {
    dst: usize,
    src: IntSource,
  },
  ISubM {
    dst: usize,
    base: MemBase,
    imm: u64,
    mask: u64,
  },
  IMulR {
    dst: usize,
    src: IntSource,
  },
  IMulM {
    dst: usize,
    base: MemBase,
    imm: u64,
    mask: u64,
  },
  IMulhR {
    dst: usize,
    src: usize,
  },
  IMulhM {
    dst: usize,
    base: MemBase,
    imm: u64,
    mask: u64,
  },
  ISMulhR {
    dst: usize,
    src: usize,
  },
  ISMulhM {
    dst: usize,
    base: MemBase,
    imm: u64,
    mask: u64,
  },
  INegR {
    dst: usize,
  },
  IXorR {
    dst: usize,
    src: IntSource,
  },
  IXorM {
    dst: usize,
    base: MemBase,
    imm: u64,
    mask: u64,
  },
  IRorR {
    dst: usize,
    src: IntSource,
  },
  IRolR {
    dst: usize,
    src: IntSource,
  },
  ISwapR {
    dst: usize,
    src: usize,
  },
  FSwapR {
    dst: usize,
  },
  FAddR {
    dst: usize,
    src: usize,
  },
  FAddM {
    dst: usize,
    base: usize,
    imm: u64,
    mask: u64,
  },
  FSubR {
    dst: usize,
    src: usize,
  },
  FSubM {
    dst: usize,
    base: usize,
    imm: u64,
    mask: u64,
  },
  FScalR {
    dst: usize,
  },
  FMulR {
    dst: usize,
    src: usize,
  },
  FDivM {
    dst: usize,
    base: usize,
    imm: u64,
    mask: u64,
  },
  FSqrtR {
    dst: usize,
  },
  CBranch {
    reg: usize,
    target: isize,
    imm: u64,
    mask: u64,
  },
  CFRound {
    src: usize,
    rotate: u32,
  },
  IStore {
    base: usize,
    src: usize,
    imm: u64,
    mask: u64,
  },
  Nop,
}

#[derive(Debug, Clone)]
struct Bytecode {
  instructions: Vec<CompiledInstruction>,
}

struct ExecutionState {
  config: ProgramConfiguration,
  bytecode: Bytecode,
  sp_addr0: u64,
  sp_addr1: u64,
}

#[derive(Debug, Clone, Default)]
pub struct RegisterFile {
  pub r: [u64; 8],
  pub f: [FReg; 4],
  pub e: [FReg; 4],
  pub a: [FReg; 4],
}

impl RegisterFile {
  pub fn to_bytes(&self) -> [u8; 256] {
    let mut out = [0u8; 256];

    for (i, value) in self.r.iter().copied().enumerate() {
      store_u64(&mut out, i * 8, value);
    }

    let mut offset = 64usize;
    for bank in [&self.f, &self.e, &self.a] {
      for reg in bank.iter() {
        store_u64(&mut out, offset, reg.0[0]);
        store_u64(&mut out, offset + 8, reg.0[1]);
        offset += 16;
      }
    }

    out
  }
}

#[derive(Debug, Clone, Copy)]
struct MemoryRegisters {
  ma: u64,
  mx: u64,
}

#[derive(Debug)]
pub struct VirtualMachine {
  scratchpad: Vec<u8>,
  registers: RegisterFile,
  memory: MemoryRegisters,
  rounding: RoundingMode,
}

impl Default for VirtualMachine {
  fn default() -> Self {
    Self::new()
  }
}

impl VirtualMachine {
  pub fn new() -> Self {
    Self {
      scratchpad: alloc::vec![0u8; crate::constants::SCRATCHPAD_L3],
      registers: RegisterFile::default(),
      memory: MemoryRegisters { ma: 0, mx: 0 },
      rounding: RoundingMode::default(),
    }
  }

  pub fn scratchpad_mut(&mut self) -> &mut [u8] {
    &mut self.scratchpad
  }

  pub fn scratchpad(&self) -> &[u8] {
    &self.scratchpad
  }

  pub fn registers(&self) -> &RegisterFile {
    &self.registers
  }

  pub fn reset_rounding(&mut self) {
    self.rounding = RoundingMode::default();
  }

  pub fn initialize_program(&mut self, program: &Program) -> ProgramConfiguration {
    for (i, a) in self.registers.a.iter_mut().enumerate() {
      *a = FReg([
        small_positive_float_bits(program.entropy[i * 2]),
        small_positive_float_bits(program.entropy[i * 2 + 1]),
      ]);
    }

    self.memory.ma = program.entropy[8] & crate::constants::CACHE_LINE_ALIGN_MASK;
    self.memory.mx = program.entropy[10];

    configure(program)
  }

  /// Generates a RandomX program from `seed` and executes it for
  /// `PROGRAM_ITERATIONS` iterations.
  pub fn run(&mut self, seed: &[u8; 64], cache: &RandomXCache) {
    let program = generate_program(seed);
    self.run_program(&program, cache);
  }

  pub fn run_program(&mut self, program: &Program, cache: &RandomXCache) {
    let mut state = self.prepare(program);
    for _ in 0..PROGRAM_ITERATIONS {
      let (item, sp0, sp1) = self.begin_iteration(&mut state);
      self.finish_iteration(cache.dataset_item(item), sp0, sp1);
    }
  }

  pub(crate) fn run_pair(
    &mut self,
    other: &mut Self,
    programs: [&Program; 2],
    cache: &RandomXCache,
  ) {
    let mut first = self.prepare(programs[0]);
    let mut second = other.prepare(programs[1]);
    for _ in 0..PROGRAM_ITERATIONS {
      let (a, sp0, sp1) = self.begin_iteration(&mut first);
      let (b, other_sp0, other_sp1) = other.begin_iteration(&mut second);
      let items = cache.dataset_pair([a, b]);
      self.finish_iteration(items[0], sp0, sp1);
      other.finish_iteration(items[1], other_sp0, other_sp1);
    }
  }

  pub(crate) fn run_triple(
    vms: &mut [&mut Self; 3],
    programs: &[Program; 3],
    cache: &RandomXCache,
  ) {
    let mut states: [ExecutionState; 3] = core::array::from_fn(|i| vms[i].prepare(&programs[i]));
    for _ in 0..PROGRAM_ITERATIONS {
      let starts: [(u64, usize, usize); 3] =
        core::array::from_fn(|i| vms[i].begin_iteration(&mut states[i]));
      let items = cache.dataset_triple(starts.map(|start| start.0));
      for i in 0..3 {
        vms[i].finish_iteration(items[i], starts[i].1, starts[i].2);
      }
    }
  }

  fn prepare(&mut self, program: &Program) -> ExecutionState {
    let config = self.initialize_program(program);
    self.registers.r = [0; 8];
    ExecutionState {
      config,
      bytecode: compile_program(&program.instructions),
      sp_addr0: self.memory.mx,
      sp_addr1: self.memory.ma,
    }
  }

  fn begin_iteration(&mut self, state: &mut ExecutionState) -> (u64, usize, usize) {
    let scratchpad: &mut [u8; crate::constants::SCRATCHPAD_L3] = self
      .scratchpad
      .as_mut_slice()
      .try_into()
      .expect("RandomX scratchpad size");
    let sp_mix =
      self.registers.r[state.config.read_reg[0]] ^ self.registers.r[state.config.read_reg[1]];

    state.sp_addr0 ^= sp_mix;
    state.sp_addr0 &= SCRATCHPAD_L3_MASK64;

    state.sp_addr1 ^= sp_mix >> 32;
    state.sp_addr1 &= SCRATCHPAD_L3_MASK64;

    let sp0 = state.sp_addr0 as usize;
    let sp1 = state.sp_addr1 as usize;

    for (i, reg) in self.registers.r.iter_mut().enumerate() {
      *reg ^= load_u64(scratchpad, sp0 + i * 8);
    }

    for i in 0..4 {
      self.registers.f[i] = from_i32_pair(&scratchpad[sp1 + i * 8..sp1 + i * 8 + 8]);

      let raw = from_i32_pair(&scratchpad[sp1 + (4 + i) * 8..sp1 + (4 + i) * 8 + 8]);

      self.registers.e[i] = mask_exponent_mantissa(raw, state.config.e_mask);
    }

    execute_bytecode(
      &state.bytecode,
      &state.config,
      &mut self.registers,
      scratchpad,
      &mut self.rounding,
    );

    // RandomX v1: read the item selected by the old ma, modify mx,
    // then swap the two memory registers.
    let read_address =
      state.config.dataset_offset + (self.memory.ma & crate::constants::CACHE_LINE_ALIGN_MASK);

    self.memory.mx ^=
      self.registers.r[state.config.read_reg[2]] ^ self.registers.r[state.config.read_reg[3]];

    // Full mode would prefetch datasetOffset + (mx & mask) here.
    // Light mode calculates the actual read item on demand only.
    state.sp_addr0 = 0;
    state.sp_addr1 = 0;
    (read_address / 64, sp0, sp1)
  }

  fn finish_iteration(&mut self, item: [u64; 8], sp0: usize, sp1: usize) {
    let scratchpad: &mut [u8; crate::constants::SCRATCHPAD_L3] = self
      .scratchpad
      .as_mut_slice()
      .try_into()
      .expect("RandomX scratchpad size");
    for (reg, mixed) in self.registers.r.iter_mut().zip(item) {
      *reg ^= mixed;
    }

    core::mem::swap(&mut self.memory.mx, &mut self.memory.ma);

    for (i, reg) in self.registers.r.iter().copied().enumerate() {
      store_u64(scratchpad, sp1 + i * 8, reg);
    }

    for (i, (f, e)) in self
      .registers
      .f
      .iter_mut()
      .zip(self.registers.e)
      .enumerate()
    {
      *f = fxor(*f, e);

      store_u64(scratchpad, sp0 + i * 16, f.0[0]);
      store_u64(scratchpad, sp0 + i * 16 + 8, f.0[1]);
    }
  }
}

fn compile_program(instructions: &[Instruction; crate::constants::PROGRAM_SIZE]) -> Bytecode {
  let mut usage = [-1isize; 8];
  let mut out = Vec::with_capacity(instructions.len());

  for (pc, instr) in instructions.iter().copied().enumerate() {
    let compiled = compile_instruction(instr, pc, &mut usage);
    out.push(compiled);
  }

  Bytecode { instructions: out }
}

fn memory_operand(instr: Instruction, dst: usize) -> (MemBase, u64, u64) {
  let src = instr.src as usize % 8;
  let imm = sign_extend_u32(instr.imm32);

  if src != dst {
    let mask = if instr.mod_mem() != 0 {
      SCRATCHPAD_L1_MASK
    } else {
      SCRATCHPAD_L2_MASK
    };

    (MemBase::Register(src), imm, mask)
  } else {
    (MemBase::Zero, imm, SCRATCHPAD_L3_MASK)
  }
}

fn reg_or_imm(instr: Instruction, signed: bool) -> IntSource {
  let dst = instr.dst as usize % 8;
  let src = instr.src as usize % 8;

  if src != dst {
    IntSource::Register(src)
  } else if signed {
    IntSource::Immediate(sign_extend_u32(instr.imm32))
  } else {
    IntSource::Immediate(instr.imm32 as u64)
  }
}

fn compile_instruction(
  instr: Instruction,
  pc: usize,
  usage: &mut [isize; 8],
) -> CompiledInstruction {
  let dst = instr.dst as usize % 8;
  let src = instr.src as usize % 8;

  match decode_vm_opcode(instr.opcode) {
    VmOpcode::IAddRs => {
      usage[dst] = pc as isize;
      CompiledInstruction::IAddRs {
        dst,
        src,
        shift: instr.mod_shift() as u32,
        imm: if dst == REGISTER_NEEDS_DISPLACEMENT {
          sign_extend_u32(instr.imm32)
        } else {
          0
        },
      }
    }

    VmOpcode::IAddM => {
      usage[dst] = pc as isize;
      let (base, imm, mask) = memory_operand(instr, dst);
      CompiledInstruction::IAddM {
        dst,
        base,
        imm,
        mask,
      }
    }

    VmOpcode::ISubR => {
      usage[dst] = pc as isize;
      CompiledInstruction::ISubR {
        dst,
        src: reg_or_imm(instr, true),
      }
    }

    VmOpcode::ISubM => {
      usage[dst] = pc as isize;
      let (base, imm, mask) = memory_operand(instr, dst);
      CompiledInstruction::ISubM {
        dst,
        base,
        imm,
        mask,
      }
    }

    VmOpcode::IMulR => {
      usage[dst] = pc as isize;
      CompiledInstruction::IMulR {
        dst,
        src: reg_or_imm(instr, true),
      }
    }

    VmOpcode::IMulM => {
      usage[dst] = pc as isize;
      let (base, imm, mask) = memory_operand(instr, dst);
      CompiledInstruction::IMulM {
        dst,
        base,
        imm,
        mask,
      }
    }

    VmOpcode::IMulhR => {
      usage[dst] = pc as isize;
      CompiledInstruction::IMulhR { dst, src }
    }

    VmOpcode::IMulhM => {
      usage[dst] = pc as isize;
      let (base, imm, mask) = memory_operand(instr, dst);
      CompiledInstruction::IMulhM {
        dst,
        base,
        imm,
        mask,
      }
    }

    VmOpcode::ISMulhR => {
      usage[dst] = pc as isize;
      CompiledInstruction::ISMulhR { dst, src }
    }

    VmOpcode::ISMulhM => {
      usage[dst] = pc as isize;
      let (base, imm, mask) = memory_operand(instr, dst);
      CompiledInstruction::ISMulhM {
        dst,
        base,
        imm,
        mask,
      }
    }

    VmOpcode::IMulRcp => {
      if is_zero_or_power_of_two(instr.imm32) {
        CompiledInstruction::Nop
      } else {
        usage[dst] = pc as isize;
        CompiledInstruction::IMulR {
          dst,
          src: IntSource::Immediate(reciprocal(instr.imm32)),
        }
      }
    }

    VmOpcode::INegR => {
      usage[dst] = pc as isize;
      CompiledInstruction::INegR { dst }
    }

    VmOpcode::IXorR => {
      usage[dst] = pc as isize;
      CompiledInstruction::IXorR {
        dst,
        src: reg_or_imm(instr, true),
      }
    }

    VmOpcode::IXorM => {
      usage[dst] = pc as isize;
      let (base, imm, mask) = memory_operand(instr, dst);
      CompiledInstruction::IXorM {
        dst,
        base,
        imm,
        mask,
      }
    }

    VmOpcode::IRorR => {
      usage[dst] = pc as isize;
      CompiledInstruction::IRorR {
        dst,
        src: reg_or_imm(instr, false),
      }
    }

    VmOpcode::IRolR => {
      usage[dst] = pc as isize;
      CompiledInstruction::IRolR {
        dst,
        src: reg_or_imm(instr, false),
      }
    }

    VmOpcode::ISwapR => {
      if src == dst {
        CompiledInstruction::Nop
      } else {
        usage[dst] = pc as isize;
        usage[src] = pc as isize;
        CompiledInstruction::ISwapR { dst, src }
      }
    }

    VmOpcode::FSwapR => CompiledInstruction::FSwapR { dst },

    VmOpcode::FAddR => CompiledInstruction::FAddR {
      dst: dst % 4,
      src: src % 4,
    },

    VmOpcode::FAddM => CompiledInstruction::FAddM {
      dst: dst % 4,
      base: src,
      imm: sign_extend_u32(instr.imm32),
      mask: if instr.mod_mem() != 0 {
        SCRATCHPAD_L1_MASK
      } else {
        SCRATCHPAD_L2_MASK
      },
    },

    VmOpcode::FSubR => CompiledInstruction::FSubR {
      dst: dst % 4,
      src: src % 4,
    },

    VmOpcode::FSubM => CompiledInstruction::FSubM {
      dst: dst % 4,
      base: src,
      imm: sign_extend_u32(instr.imm32),
      mask: if instr.mod_mem() != 0 {
        SCRATCHPAD_L1_MASK
      } else {
        SCRATCHPAD_L2_MASK
      },
    },

    VmOpcode::FScalR => CompiledInstruction::FScalR { dst: dst % 4 },

    VmOpcode::FMulR => CompiledInstruction::FMulR {
      dst: dst % 4,
      src: src % 4,
    },

    VmOpcode::FDivM => CompiledInstruction::FDivM {
      dst: dst % 4,
      base: src,
      imm: sign_extend_u32(instr.imm32),
      mask: if instr.mod_mem() != 0 {
        SCRATCHPAD_L1_MASK
      } else {
        SCRATCHPAD_L2_MASK
      },
    },

    VmOpcode::FSqrtR => CompiledInstruction::FSqrtR { dst: dst % 4 },

    VmOpcode::CBranch => {
      let reg = dst;
      let target = usage[reg];
      let shift = instr.mod_cond() as u32 + CONDITION_OFFSET;

      let mut imm = sign_extend_u32(instr.imm32) | (1u64 << shift);

      if CONDITION_OFFSET > 0 || shift > 0 {
        imm &= !(1u64 << (shift - 1));
      }

      let mask = CONDITION_MASK << shift;

      for value in usage.iter_mut() {
        *value = pc as isize;
      }

      CompiledInstruction::CBranch {
        reg,
        target,
        imm,
        mask,
      }
    }

    VmOpcode::CFRound => CompiledInstruction::CFRound {
      src,
      rotate: instr.imm32 & 63,
    },

    VmOpcode::IStore => CompiledInstruction::IStore {
      base: dst,
      src,
      imm: sign_extend_u32(instr.imm32),
      mask: if instr.mod_cond() < STORE_L3_CONDITION {
        if instr.mod_mem() != 0 {
          SCRATCHPAD_L1_MASK
        } else {
          SCRATCHPAD_L2_MASK
        }
      } else {
        SCRATCHPAD_L3_MASK
      },
    },

    VmOpcode::Nop => CompiledInstruction::Nop,
  }
}

#[inline]
fn value(source: &IntSource, r: &[u64; 8]) -> u64 {
  match *source {
    IntSource::Register(index) => r[index & 7],
    IntSource::Immediate(value) => value,
  }
}

#[inline]
fn memory_address(base: &MemBase, imm: u64, mask: u64, r: &[u64; 8]) -> usize {
  let base = match *base {
    MemBase::Register(index) => r[index & 7],
    MemBase::Zero => 0,
  };

  base.wrapping_add(imm) as usize & mask as usize & SCRATCHPAD_L3_MASK as usize
}

#[inline]
fn float_memory_address(base: usize, imm: u64, mask: u64, r: &[u64; 8]) -> usize {
  r[base & 7].wrapping_add(imm) as usize & mask as usize & SCRATCHPAD_L3_MASK as usize
}

fn execute_bytecode(
  bytecode: &Bytecode,
  config: &ProgramConfiguration,
  registers: &mut RegisterFile,
  scratchpad: &mut [u8; crate::constants::SCRATCHPAD_L3],
  rounding: &mut RoundingMode,
) {
  let mut pc = 0usize;

  while pc < bytecode.instructions.len() {
    let mut next_pc = pc + 1;

    match &bytecode.instructions[pc] {
      CompiledInstruction::IAddRs {
        dst,
        src,
        shift,
        imm,
      } => {
        registers.r[*dst & 7] = registers.r[*dst & 7]
          .wrapping_add(registers.r[*src & 7].wrapping_shl(*shift))
          .wrapping_add(*imm);
      }

      CompiledInstruction::IAddM {
        dst,
        base,
        imm,
        mask,
      } => {
        let address = memory_address(base, *imm, *mask, &registers.r);

        registers.r[*dst & 7] = registers.r[*dst & 7].wrapping_add(load_u64(scratchpad, address));
      }

      CompiledInstruction::ISubR { dst, src } => {
        registers.r[*dst & 7] = registers.r[*dst & 7].wrapping_sub(value(src, &registers.r));
      }

      CompiledInstruction::ISubM {
        dst,
        base,
        imm,
        mask,
      } => {
        let address = memory_address(base, *imm, *mask, &registers.r);

        registers.r[*dst & 7] = registers.r[*dst & 7].wrapping_sub(load_u64(scratchpad, address));
      }

      CompiledInstruction::IMulR { dst, src } => {
        registers.r[*dst & 7] = registers.r[*dst & 7].wrapping_mul(value(src, &registers.r));
      }

      CompiledInstruction::IMulM {
        dst,
        base,
        imm,
        mask,
      } => {
        let address = memory_address(base, *imm, *mask, &registers.r);

        registers.r[*dst & 7] = registers.r[*dst & 7].wrapping_mul(load_u64(scratchpad, address));
      }

      CompiledInstruction::IMulhR { dst, src } => {
        registers.r[*dst & 7] = mulh(registers.r[*dst & 7], registers.r[*src & 7]);
      }

      CompiledInstruction::IMulhM {
        dst,
        base,
        imm,
        mask,
      } => {
        let address = memory_address(base, *imm, *mask, &registers.r);

        registers.r[*dst & 7] = mulh(registers.r[*dst & 7], load_u64(scratchpad, address));
      }

      CompiledInstruction::ISMulhR { dst, src } => {
        registers.r[*dst & 7] = smulh(registers.r[*dst & 7], registers.r[*src & 7]);
      }

      CompiledInstruction::ISMulhM {
        dst,
        base,
        imm,
        mask,
      } => {
        let address = memory_address(base, *imm, *mask, &registers.r);

        registers.r[*dst & 7] = smulh(registers.r[*dst & 7], load_u64(scratchpad, address));
      }

      CompiledInstruction::INegR { dst } => {
        registers.r[*dst & 7] = registers.r[*dst & 7].wrapping_neg();
      }

      CompiledInstruction::IXorR { dst, src } => {
        registers.r[*dst & 7] ^= value(src, &registers.r);
      }

      CompiledInstruction::IXorM {
        dst,
        base,
        imm,
        mask,
      } => {
        let address = memory_address(base, *imm, *mask, &registers.r);

        registers.r[*dst & 7] ^= load_u64(scratchpad, address);
      }

      CompiledInstruction::IRorR { dst, src } => {
        registers.r[*dst & 7] =
          registers.r[*dst & 7].rotate_right((value(src, &registers.r) & 63) as u32);
      }

      CompiledInstruction::IRolR { dst, src } => {
        registers.r[*dst & 7] =
          registers.r[*dst & 7].rotate_left((value(src, &registers.r) & 63) as u32);
      }

      CompiledInstruction::ISwapR { dst, src } => {
        registers.r.swap(*dst, *src);
      }

      CompiledInstruction::FSwapR { dst } => {
        if *dst < 4 {
          registers.f[*dst & 3] = fswap(registers.f[*dst & 3]);
        } else {
          let index = *dst - 4;
          registers.e[index & 3] = fswap(registers.e[index & 3]);
        }
      }

      CompiledInstruction::FAddR { dst, src } => {
        registers.f[*dst & 3] = fadd(registers.f[*dst & 3], registers.a[*src & 3], *rounding);
      }

      CompiledInstruction::FAddM {
        dst,
        base,
        imm,
        mask,
      } => {
        let address = float_memory_address(*base, *imm, *mask, &registers.r);
        let operand = from_i32_pair(&scratchpad[address..address + 8]);

        registers.f[*dst & 3] = fadd(registers.f[*dst & 3], operand, *rounding);
      }

      CompiledInstruction::FSubR { dst, src } => {
        registers.f[*dst & 3] = fsub(registers.f[*dst & 3], registers.a[*src & 3], *rounding);
      }

      CompiledInstruction::FSubM {
        dst,
        base,
        imm,
        mask,
      } => {
        let address = float_memory_address(*base, *imm, *mask, &registers.r);
        let operand = from_i32_pair(&scratchpad[address..address + 8]);

        registers.f[*dst & 3] = fsub(registers.f[*dst & 3], operand, *rounding);
      }

      CompiledInstruction::FScalR { dst } => {
        registers.f[*dst & 3] = fscale(registers.f[*dst & 3]);
      }

      CompiledInstruction::FMulR { dst, src } => {
        registers.e[*dst & 3] = fmul(registers.e[*dst & 3], registers.a[*src & 3], *rounding);
      }

      CompiledInstruction::FDivM {
        dst,
        base,
        imm,
        mask,
      } => {
        let address = float_memory_address(*base, *imm, *mask, &registers.r);

        let raw = from_i32_pair(&scratchpad[address..address + 8]);
        let operand = mask_exponent_mantissa(raw, config.e_mask);

        registers.e[*dst & 3] = fdiv(registers.e[*dst & 3], operand, *rounding);
      }

      CompiledInstruction::FSqrtR { dst } => {
        registers.e[*dst & 3] = fsqrt(registers.e[*dst & 3], *rounding);
      }

      CompiledInstruction::CBranch {
        reg,
        target,
        imm,
        mask,
      } => {
        registers.r[*reg & 7] = registers.r[*reg & 7].wrapping_add(*imm);

        if registers.r[*reg & 7] & *mask == 0 {
          next_pc = (*target + 1) as usize;
        }
      }

      CompiledInstruction::CFRound { src, rotate } => {
        *rounding = RoundingMode::from_randomx(registers.r[*src & 7].rotate_right(*rotate));
      }

      CompiledInstruction::IStore {
        base,
        src,
        imm,
        mask,
      } => {
        let address = registers.r[*base & 7].wrapping_add(*imm) as usize
          & *mask as usize
          & SCRATCHPAD_L3_MASK as usize;

        store_u64(scratchpad, address, registers.r[*src & 7]);
      }

      CompiledInstruction::Nop => {}
    }

    pc = next_pc;
  }
}
