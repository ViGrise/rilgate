use crate::constants::FREQS;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Instruction {
  pub opcode: u8,
  pub dst: u8,
  pub src: u8,
  pub mod_byte: u8,
  pub imm32: u32,
}

impl Instruction {
  #[inline]
  pub fn mod_mem(self) -> u8 {
    self.mod_byte & 3
  }

  #[inline]
  pub fn mod_shift(self) -> u8 {
    (self.mod_byte >> 2) & 3
  }

  #[inline]
  pub fn mod_cond(self) -> u8 {
    self.mod_byte >> 4
  }

  #[inline]
  pub fn to_bytes(self) -> [u8; 8] {
    let mut out = [0u8; 8];
    out[0] = self.opcode;
    out[1] = self.dst;
    out[2] = self.src;
    out[3] = self.mod_byte;
    out[4..8].copy_from_slice(&self.imm32.to_le_bytes());
    out
  }

  #[inline]
  pub fn from_bytes(bytes: &[u8]) -> Self {
    Self {
      opcode: bytes[0],
      dst: bytes[1],
      src: bytes[2],
      mod_byte: bytes[3],
      imm32: u32::from_le_bytes(bytes[4..8].try_into().expect("8 byte instruction")),
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmOpcode {
  IAddRs,
  IAddM,
  ISubR,
  ISubM,
  IMulR,
  IMulM,
  IMulhR,
  IMulhM,
  ISMulhR,
  ISMulhM,
  IMulRcp,
  INegR,
  IXorR,
  IXorM,
  IRorR,
  IRolR,
  ISwapR,
  FSwapR,
  FAddR,
  FAddM,
  FSubR,
  FSubM,
  FScalR,
  FMulR,
  FDivM,
  FSqrtR,
  CBranch,
  CFRound,
  IStore,
  Nop,
}

#[inline]
pub fn decode_vm_opcode(opcode: u8) -> VmOpcode {
  let mut ceiling = 0u16;
  for (index, freq) in FREQS.iter().copied().enumerate() {
    ceiling += freq as u16;
    if (opcode as u16) < ceiling {
      return match index {
        0 => VmOpcode::IAddRs,
        1 => VmOpcode::IAddM,
        2 => VmOpcode::ISubR,
        3 => VmOpcode::ISubM,
        4 => VmOpcode::IMulR,
        5 => VmOpcode::IMulM,
        6 => VmOpcode::IMulhR,
        7 => VmOpcode::IMulhM,
        8 => VmOpcode::ISMulhR,
        9 => VmOpcode::ISMulhM,
        10 => VmOpcode::IMulRcp,
        11 => VmOpcode::INegR,
        12 => VmOpcode::IXorR,
        13 => VmOpcode::IXorM,
        14 => VmOpcode::IRorR,
        15 => VmOpcode::IRolR,
        16 => VmOpcode::ISwapR,
        17 => VmOpcode::FSwapR,
        18 => VmOpcode::FAddR,
        19 => VmOpcode::FAddM,
        20 => VmOpcode::FSubR,
        21 => VmOpcode::FSubM,
        22 => VmOpcode::FScalR,
        23 => VmOpcode::FMulR,
        24 => VmOpcode::FDivM,
        25 => VmOpcode::FSqrtR,
        26 => VmOpcode::CBranch,
        27 => VmOpcode::CFRound,
        28 => VmOpcode::IStore,
        _ => VmOpcode::Nop,
      };
    }
  }
  VmOpcode::Nop
}
