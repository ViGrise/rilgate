use alloc::vec::Vec;

use crate::{
  blake_generator::Blake2Generator,
  constants::{REGISTER_NEEDS_DISPLACEMENT, SUPERSCALAR_LATENCY, SUPERSCALAR_MAX_SIZE},
  helpers::{is_zero_or_power_of_two, mulh, sign_extend_u32, smulh},
  instruction::Instruction,
  reciprocal::reciprocal,
};

const CYCLE_MAP_SIZE: usize = SUPERSCALAR_LATENCY + 4;
const LOOK_FORWARD_CYCLES: usize = 4;
const MAX_THROWAWAY_COUNT: usize = 256;

const P0: u8 = 1;
const P1: u8 = 2;
const P5: u8 = 4;
const P01: u8 = P0 | P1;
const P05: u8 = P0 | P5;
const P015: u8 = P0 | P1 | P5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SuperscalarType {
  ISubR = 0,
  IXorR = 1,
  IAddRs = 2,
  IMulR = 3,
  IRorC = 4,
  IAddC7 = 5,
  IXorC7 = 6,
  IAddC8 = 7,
  IXorC8 = 8,
  IAddC9 = 9,
  IXorC9 = 10,
  IMulhR = 11,
  ISMulhR = 12,
  IMulRcp = 13,
  Invalid = 255,
}

impl SuperscalarType {
  fn is_multiplication(self) -> bool {
    matches!(
      self,
      Self::IMulR | Self::IMulhR | Self::ISMulhR | Self::IMulRcp
    )
  }
}

#[derive(Debug, Clone, Copy)]
struct MacroOp {
  latency: usize,
  uop1: u8,
  uop2: u8,
  dependent: bool,
}

impl MacroOp {
  const fn simple(latency: usize, uop: u8) -> Self {
    Self {
      latency,
      uop1: uop,
      uop2: 0,
      dependent: false,
    }
  }

  const fn pair(latency: usize, uop1: u8, uop2: u8) -> Self {
    Self {
      latency,
      uop1,
      uop2,
      dependent: false,
    }
  }

  const fn eliminated() -> Self {
    Self {
      latency: 0,
      uop1: 0,
      uop2: 0,
      dependent: false,
    }
  }

  const fn dependent(mut self) -> Self {
    self.dependent = true;
    self
  }
}

const SUB_RR: MacroOp = MacroOp::simple(1, P015);
const XOR_RR: MacroOp = MacroOp::simple(1, P015);
const IMUL_R: MacroOp = MacroOp::pair(4, P1, P5);
const MUL_R: MacroOp = MacroOp::pair(4, P1, P5);
const MOV_RR: MacroOp = MacroOp::eliminated();
const LEA_SIB: MacroOp = MacroOp::simple(1, P01);
const IMUL_RR: MacroOp = MacroOp::simple(3, P1);
const ROR_RI: MacroOp = MacroOp::simple(1, P05);
const ADD_RI: MacroOp = MacroOp::simple(1, P015);
const XOR_RI: MacroOp = MacroOp::simple(1, P015);
const MOV_RI64: MacroOp = MacroOp::simple(1, P015);

#[derive(Debug, Clone, Copy)]
struct InstrInfo {
  ty: SuperscalarType,
  ops: &'static [MacroOp],
  result_op: usize,
  dst_op: usize,
  src_op: isize,
}

const OPS_ISUB: [MacroOp; 1] = [SUB_RR];
const OPS_IXOR: [MacroOp; 1] = [XOR_RR];
const OPS_IADD_RS: [MacroOp; 1] = [LEA_SIB];
const OPS_IMUL: [MacroOp; 1] = [IMUL_RR];
const OPS_ROR: [MacroOp; 1] = [ROR_RI];
const OPS_ADD_C: [MacroOp; 1] = [ADD_RI];
const OPS_XOR_C: [MacroOp; 1] = [XOR_RI];
const OPS_IMULH: [MacroOp; 3] = [MOV_RR, MUL_R, MOV_RR];
const OPS_ISMULH: [MacroOp; 3] = [MOV_RR, IMUL_R, MOV_RR];
const OPS_RCP: [MacroOp; 2] = [MOV_RI64, IMUL_RR.dependent()];

fn info(ty: SuperscalarType) -> InstrInfo {
  match ty {
    SuperscalarType::ISubR => InstrInfo {
      ty,
      ops: &OPS_ISUB,
      result_op: 0,
      dst_op: 0,
      src_op: 0,
    },
    SuperscalarType::IXorR => InstrInfo {
      ty,
      ops: &OPS_IXOR,
      result_op: 0,
      dst_op: 0,
      src_op: 0,
    },
    SuperscalarType::IAddRs => InstrInfo {
      ty,
      ops: &OPS_IADD_RS,
      result_op: 0,
      dst_op: 0,
      src_op: 0,
    },
    SuperscalarType::IMulR => InstrInfo {
      ty,
      ops: &OPS_IMUL,
      result_op: 0,
      dst_op: 0,
      src_op: 0,
    },
    SuperscalarType::IRorC => InstrInfo {
      ty,
      ops: &OPS_ROR,
      result_op: 0,
      dst_op: 0,
      src_op: -1,
    },
    SuperscalarType::IAddC7 | SuperscalarType::IAddC8 | SuperscalarType::IAddC9 => InstrInfo {
      ty,
      ops: &OPS_ADD_C,
      result_op: 0,
      dst_op: 0,
      src_op: -1,
    },
    SuperscalarType::IXorC7 | SuperscalarType::IXorC8 | SuperscalarType::IXorC9 => InstrInfo {
      ty,
      ops: &OPS_XOR_C,
      result_op: 0,
      dst_op: 0,
      src_op: -1,
    },
    SuperscalarType::IMulhR => InstrInfo {
      ty,
      ops: &OPS_IMULH,
      result_op: 1,
      dst_op: 0,
      src_op: 1,
    },
    SuperscalarType::ISMulhR => InstrInfo {
      ty,
      ops: &OPS_ISMULH,
      result_op: 1,
      dst_op: 0,
      src_op: 1,
    },
    SuperscalarType::IMulRcp => InstrInfo {
      ty,
      ops: &OPS_RCP,
      result_op: 1,
      dst_op: 1,
      src_op: -1,
    },
    SuperscalarType::Invalid => InstrInfo {
      ty,
      ops: &[],
      result_op: 0,
      dst_op: 0,
      src_op: -1,
    },
  }
}

#[derive(Debug, Clone, Copy)]
struct RegisterInfo {
  latency: usize,
  last_group: SuperscalarType,
  last_par: i64,
}

impl Default for RegisterInfo {
  fn default() -> Self {
    Self {
      latency: 0,
      last_group: SuperscalarType::Invalid,
      last_par: -1,
    }
  }
}

#[derive(Debug, Clone)]
pub struct SuperscalarProgram {
  pub instructions: Vec<Instruction>,
  pub address_register: usize,
}

#[derive(Debug, Clone)]
struct PendingInstruction {
  info: InstrInfo,
  src: isize,
  dst: isize,
  mod_byte: u8,
  imm32: u32,
  op_group: SuperscalarType,
  op_group_par: i64,
  can_reuse: bool,
  group_par_is_source: bool,
}

impl PendingInstruction {
  fn invalid() -> Self {
    Self {
      info: info(SuperscalarType::Invalid),
      src: -1,
      dst: -1,
      mod_byte: 0,
      imm32: 0,
      op_group: SuperscalarType::Invalid,
      op_group_par: -1,
      can_reuse: false,
      group_par_is_source: false,
    }
  }

  fn create(ty: SuperscalarType, blake: &mut Blake2Generator) -> Self {
    let mut this = Self {
      info: info(ty),
      src: -1,
      dst: -1,
      mod_byte: 0,
      imm32: 0,
      op_group: SuperscalarType::Invalid,
      op_group_par: -1,
      can_reuse: false,
      group_par_is_source: false,
    };

    match ty {
      SuperscalarType::ISubR => {
        // Intentionally grouped with IADD_RS, matching the reference.
        this.op_group = SuperscalarType::IAddRs;
        this.group_par_is_source = true;
      }
      SuperscalarType::IXorR => {
        this.op_group = SuperscalarType::IXorR;
        this.group_par_is_source = true;
      }
      SuperscalarType::IAddRs => {
        this.mod_byte = blake.byte();
        this.op_group = SuperscalarType::IAddRs;
        this.group_par_is_source = true;
      }
      SuperscalarType::IMulR => {
        this.op_group = SuperscalarType::IMulR;
        this.group_par_is_source = true;
      }
      SuperscalarType::IRorC => {
        loop {
          this.imm32 = (blake.byte() & 63) as u32;
          if this.imm32 != 0 {
            break;
          }
        }
        this.op_group = SuperscalarType::IRorC;
      }
      SuperscalarType::IAddC7 | SuperscalarType::IAddC8 | SuperscalarType::IAddC9 => {
        this.imm32 = blake.u32();
        this.op_group = SuperscalarType::IAddC7;
      }
      SuperscalarType::IXorC7 | SuperscalarType::IXorC8 | SuperscalarType::IXorC9 => {
        this.imm32 = blake.u32();
        this.op_group = SuperscalarType::IXorC7;
      }
      SuperscalarType::IMulhR => {
        this.can_reuse = true;
        this.op_group = SuperscalarType::IMulhR;
        this.op_group_par = blake.u32() as i64;
      }
      SuperscalarType::ISMulhR => {
        this.can_reuse = true;
        this.op_group = SuperscalarType::ISMulhR;
        this.op_group_par = blake.u32() as i64;
      }
      SuperscalarType::IMulRcp => {
        loop {
          this.imm32 = blake.u32();
          if !is_zero_or_power_of_two(this.imm32) {
            break;
          }
        }
        this.op_group = SuperscalarType::IMulRcp;
      }
      SuperscalarType::Invalid => {}
    }

    this
  }

  fn create_for_slot(
    blake: &mut Blake2Generator,
    slot_size: usize,
    fetch_type: usize,
    is_last: bool,
  ) -> Self {
    let ty = match slot_size {
      3 if is_last => match blake.byte() & 3 {
        0 => SuperscalarType::ISubR,
        1 => SuperscalarType::IXorR,
        2 => SuperscalarType::IMulhR,
        _ => SuperscalarType::ISMulhR,
      },
      3 => {
        if blake.byte() & 1 == 0 {
          SuperscalarType::ISubR
        } else {
          SuperscalarType::IXorR
        }
      }
      4 if fetch_type == 4 && !is_last => SuperscalarType::IMulR,
      4 => {
        if blake.byte() & 1 == 0 {
          SuperscalarType::IRorC
        } else {
          SuperscalarType::IAddRs
        }
      }
      7 => {
        if blake.byte() & 1 == 0 {
          SuperscalarType::IXorC7
        } else {
          SuperscalarType::IAddC7
        }
      }
      8 => {
        if blake.byte() & 1 == 0 {
          SuperscalarType::IXorC8
        } else {
          SuperscalarType::IAddC8
        }
      }
      9 => {
        if blake.byte() & 1 == 0 {
          SuperscalarType::IXorC9
        } else {
          SuperscalarType::IAddC9
        }
      }
      10 => SuperscalarType::IMulRcp,
      _ => unreachable!("invalid superscalar decoder slot"),
    };

    Self::create(ty, blake)
  }

  fn select_source(
    &mut self,
    cycle: usize,
    registers: &[RegisterInfo; 8],
    blake: &mut Blake2Generator,
  ) -> bool {
    let mut available = Vec::with_capacity(8);
    for (i, reg) in registers.iter().enumerate() {
      if reg.latency <= cycle {
        available.push(i);
      }
    }

    if available.len() == 2
      && self.info.ty == SuperscalarType::IAddRs
      && available.contains(&REGISTER_NEEDS_DISPLACEMENT)
    {
      self.src = REGISTER_NEEDS_DISPLACEMENT as isize;
      self.op_group_par = self.src as i64;
      return true;
    }

    let Some(src) = select_register(&available, blake) else {
      return false;
    };
    self.src = src as isize;
    if self.group_par_is_source {
      self.op_group_par = self.src as i64;
    }
    true
  }

  fn select_destination(
    &mut self,
    cycle: usize,
    allow_chained_mul: bool,
    registers: &[RegisterInfo; 8],
    blake: &mut Blake2Generator,
  ) -> bool {
    let mut available = Vec::with_capacity(8);

    for (i, reg) in registers.iter().enumerate() {
      if reg.latency > cycle {
        continue;
      }
      if !self.can_reuse && i as isize == self.src {
        continue;
      }
      if !allow_chained_mul
        && self.op_group == SuperscalarType::IMulR
        && reg.last_group == SuperscalarType::IMulR
      {
        continue;
      }
      if reg.last_group == self.op_group && reg.last_par == self.op_group_par {
        continue;
      }
      if self.info.ty == SuperscalarType::IAddRs && i == REGISTER_NEEDS_DISPLACEMENT {
        continue;
      }
      available.push(i);
    }

    let Some(dst) = select_register(&available, blake) else {
      return false;
    };

    self.dst = dst as isize;
    true
  }

  fn to_instruction(&self) -> Instruction {
    Instruction {
      opcode: self.info.ty as u8,
      dst: self.dst as u8,
      src: if self.src >= 0 {
        self.src as u8
      } else {
        self.dst as u8
      },
      mod_byte: self.mod_byte,
      imm32: self.imm32,
    }
  }
}

fn select_register(available: &[usize], blake: &mut Blake2Generator) -> Option<usize> {
  if available.is_empty() {
    return None;
  }

  let index = if available.len() == 1 {
    0
  } else {
    blake.u32() as usize % available.len()
  };

  Some(available[index])
}

const DECODER_BUFFERS: [&[usize]; 6] = [
  &[4, 8, 4],
  &[7, 3, 3, 3],
  &[3, 7, 3, 3],
  &[4, 9, 3],
  &[4, 4, 4, 4],
  &[3, 3, 10],
];

fn next_decoder(
  previous: SuperscalarType,
  cycle: usize,
  mul_count: usize,
  blake: &mut Blake2Generator,
) -> usize {
  if matches!(previous, SuperscalarType::IMulhR | SuperscalarType::ISMulhR) {
    return 5;
  }

  if mul_count < cycle + 1 {
    return 4;
  }

  if previous == SuperscalarType::IMulRcp {
    return if blake.byte() & 1 != 0 { 0 } else { 3 };
  }

  (blake.byte() & 3) as usize
}

fn schedule_uop(
  uop: u8,
  busy: &mut [[u8; 3]; CYCLE_MAP_SIZE],
  mut cycle: usize,
  commit: bool,
) -> Option<usize> {
  while cycle < CYCLE_MAP_SIZE {
    for (port, mask) in [(2usize, P5), (0, P0), (1, P1)] {
      if uop & mask != 0 && busy[cycle][port] == 0 {
        if commit {
          busy[cycle][port] = uop;
        }
        return Some(cycle);
      }
    }
    cycle += 1;
  }

  None
}

fn schedule_mop(
  mop: MacroOp,
  busy: &mut [[u8; 3]; CYCLE_MAP_SIZE],
  mut cycle: usize,
  dep_cycle: usize,
  commit: bool,
) -> Option<usize> {
  if mop.dependent {
    cycle = cycle.max(dep_cycle);
  }

  if mop.uop1 == 0 {
    return Some(cycle);
  }

  if mop.uop2 == 0 {
    return schedule_uop(mop.uop1, busy, cycle, commit);
  }

  while cycle < CYCLE_MAP_SIZE {
    let mut scratch = *busy;
    let c1 = schedule_uop(mop.uop1, &mut scratch, cycle, false);
    let c2 = schedule_uop(mop.uop2, &mut scratch, cycle, false);

    if let Some(resolved) = c1.filter(|cycle| *cycle == c2.unwrap_or(usize::MAX)) {
      if commit {
        schedule_uop(mop.uop1, busy, resolved, true);
        schedule_uop(mop.uop2, busy, resolved, true);
      }
      return Some(resolved);
    }

    cycle += 1;
  }

  None
}

pub fn generate_superscalar(blake: &mut Blake2Generator) -> SuperscalarProgram {
  let mut busy = [[0u8; 3]; CYCLE_MAP_SIZE];
  let mut registers = [RegisterInfo::default(); 8];

  let mut decoder_index: Option<usize> = None;
  let mut current = PendingInstruction::invalid();
  let mut macro_index = 0usize;
  let mut cycle = 0usize;
  let mut dep_cycle = 0usize;
  let mut retire_cycle = 0usize;
  let mut saturated = false;
  let mut mul_count = 0usize;
  let mut throw_away = 0usize;
  let mut program = Vec::with_capacity(SUPERSCALAR_MAX_SIZE);

  let mut decode_cycle = 0usize;
  while decode_cycle < SUPERSCALAR_LATENCY && !saturated && program.len() < SUPERSCALAR_MAX_SIZE {
    let next = next_decoder(current.info.ty, decode_cycle, mul_count, blake);
    decoder_index = Some(next);
    let decoder = DECODER_BUFFERS[next];

    let mut buffer_index = 0usize;
    while buffer_index < decoder.len() {
      let top_cycle = cycle;

      if macro_index >= current.info.ops.len() {
        if saturated || program.len() >= SUPERSCALAR_MAX_SIZE {
          break;
        }

        current = PendingInstruction::create_for_slot(
          blake,
          decoder[buffer_index],
          next,
          buffer_index + 1 == decoder.len(),
        );
        macro_index = 0;
      }

      let mop = current.info.ops[macro_index];
      let Some(mut schedule_cycle) = schedule_mop(mop, &mut busy, cycle, dep_cycle, false) else {
        saturated = true;
        break;
      };

      if macro_index as isize == current.info.src_op {
        let mut forward = 0usize;
        while forward < LOOK_FORWARD_CYCLES
          && !current.select_source(schedule_cycle, &registers, blake)
        {
          schedule_cycle += 1;
          cycle += 1;
          forward += 1;
        }

        if forward == LOOK_FORWARD_CYCLES {
          if throw_away < MAX_THROWAWAY_COUNT {
            throw_away += 1;
            macro_index = current.info.ops.len();
            continue;
          }
          current = PendingInstruction::invalid();
          break;
        }
      }

      if macro_index == current.info.dst_op {
        let mut forward = 0usize;
        while forward < LOOK_FORWARD_CYCLES
          && !current.select_destination(schedule_cycle, throw_away > 0, &registers, blake)
        {
          schedule_cycle += 1;
          cycle += 1;
          forward += 1;
        }

        if forward == LOOK_FORWARD_CYCLES {
          if throw_away < MAX_THROWAWAY_COUNT {
            throw_away += 1;
            macro_index = current.info.ops.len();
            continue;
          }
          current = PendingInstruction::invalid();
          break;
        }
      }

      throw_away = 0;

      let Some(schedule_cycle) = schedule_mop(mop, &mut busy, schedule_cycle, schedule_cycle, true)
      else {
        saturated = true;
        break;
      };

      dep_cycle = schedule_cycle + mop.latency;

      if macro_index == current.info.result_op {
        let dst = current.dst as usize;
        retire_cycle = dep_cycle;
        registers[dst].latency = retire_cycle;
        registers[dst].last_group = current.op_group;
        registers[dst].last_par = current.op_group_par;
      }

      buffer_index += 1;
      macro_index += 1;

      if schedule_cycle >= SUPERSCALAR_LATENCY {
        saturated = true;
      }

      cycle = top_cycle;

      if macro_index >= current.info.ops.len() {
        program.push(current.to_instruction());
        if current.info.ty.is_multiplication() {
          mul_count += 1;
        }
      }
    }

    cycle += 1;
    decode_cycle += 1;
  }

  let mut asic = [0usize; 8];
  for instr in &program {
    let dst = instr.dst as usize;
    let src = instr.src as usize;
    let dst_latency = asic[dst] + 1;
    let src_latency = if dst != src { asic[src] + 1 } else { 0 };
    asic[dst] = dst_latency.max(src_latency);
  }

  let mut address_register = 0usize;
  let mut max_latency = 0usize;
  for (i, latency) in asic.into_iter().enumerate() {
    if latency > max_latency {
      max_latency = latency;
      address_register = i;
    }
  }

  let _ = retire_cycle;
  let _ = decoder_index;

  SuperscalarProgram {
    instructions: program,
    address_register,
  }
}

pub fn execute_superscalar(registers: &mut [u64; 8], program: &SuperscalarProgram) {
  const ISUB_R: u8 = SuperscalarType::ISubR as u8;
  const IXOR_R: u8 = SuperscalarType::IXorR as u8;
  const IADD_RS: u8 = SuperscalarType::IAddRs as u8;
  const IMUL_R: u8 = SuperscalarType::IMulR as u8;
  const IROR_C: u8 = SuperscalarType::IRorC as u8;
  const IADD_C7: u8 = SuperscalarType::IAddC7 as u8;
  const IADD_C8: u8 = SuperscalarType::IAddC8 as u8;
  const IADD_C9: u8 = SuperscalarType::IAddC9 as u8;
  const IXOR_C7: u8 = SuperscalarType::IXorC7 as u8;
  const IXOR_C8: u8 = SuperscalarType::IXorC8 as u8;
  const IXOR_C9: u8 = SuperscalarType::IXorC9 as u8;
  const IMULH_R: u8 = SuperscalarType::IMulhR as u8;
  const ISMULH_R: u8 = SuperscalarType::ISMulhR as u8;
  const IMUL_RCP: u8 = SuperscalarType::IMulRcp as u8;

  for instr in &program.instructions {
    let dst = instr.dst as usize;
    let src = instr.src as usize;

    match instr.opcode {
      ISUB_R => {
        registers[dst] = registers[dst].wrapping_sub(registers[src]);
      }
      IXOR_R => {
        registers[dst] ^= registers[src];
      }
      IADD_RS => {
        registers[dst] =
          registers[dst].wrapping_add(registers[src].wrapping_shl(instr.mod_shift() as u32));
      }
      IMUL_R => {
        registers[dst] = registers[dst].wrapping_mul(registers[src]);
      }
      IROR_C => {
        registers[dst] = registers[dst].rotate_right(instr.imm32 & 63);
      }
      IADD_C7 | IADD_C8 | IADD_C9 => {
        registers[dst] = registers[dst].wrapping_add(sign_extend_u32(instr.imm32));
      }
      IXOR_C7 | IXOR_C8 | IXOR_C9 => {
        registers[dst] ^= sign_extend_u32(instr.imm32);
      }
      IMULH_R => {
        registers[dst] = mulh(registers[dst], registers[src]);
      }
      ISMULH_R => {
        registers[dst] = smulh(registers[dst], registers[src]);
      }
      IMUL_RCP => {
        registers[dst] = registers[dst].wrapping_mul(reciprocal(instr.imm32));
      }
      opcode => unreachable!("invalid superscalar opcode {opcode}"),
    }
  }
}
