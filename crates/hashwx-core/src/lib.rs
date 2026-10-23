//! Allocation-free HashWX instance generation and interpreted hashing.
#![no_std]
#![forbid(unsafe_code)]

mod tables;

use tables::{DEEP, SHALLOW, TEMPLATES};

#[derive(Clone, Copy)]
struct Instruction {
  opcode: u8,
  dst: u8,
  src: u8,
  imm: u8,
}

impl Instruction {
  const fn control(opcode: u8) -> Self {
    Self {
      opcode,
      dst: 0,
      src: 0,
      imm: 0,
    }
  }
}

type Program = [Instruction; 11];

fn sipround(v: &mut [u64; 4]) {
  v[0] = v[0].wrapping_add(v[1]);
  v[2] = v[2].wrapping_add(v[3]);
  v[1] = v[1].rotate_right(51) ^ v[0];
  v[3] = v[3].rotate_right(48) ^ v[2];
  v[0] = v[0].rotate_right(32);
  v[2] = v[2].wrapping_add(v[1]);
  v[0] = v[0].wrapping_add(v[3]);
  v[1] = v[1].rotate_right(47) ^ v[2];
  v[3] = v[3].rotate_right(43) ^ v[0];
  v[2] = v[2].rotate_right(32);
}

fn load64(bytes: &[u8]) -> u64 {
  u64::from_le_bytes(bytes[..8].try_into().expect("eight-byte operand"))
}

struct Generator {
  key: [u64; 2],
  state: [u64; 4],
  remaining: usize,
}

impl Generator {
  fn new(key: [u64; 2], salt: u64) -> Self {
    let mut state = [
      0x736f6d6570736575 ^ key[0],
      0x646f72616e646f6d ^ key[1],
      0x6c7967656e657261 ^ key[0],
      0x7465646279746573 ^ key[1] ^ salt,
    ];
    sipround(&mut state);
    state[0] ^= salt;
    state[2] ^= 0xbb;
    for _ in 0..3 {
      sipround(&mut state);
    }
    Self {
      key,
      state,
      remaining: 4,
    }
  }

  fn next(&mut self) -> u64 {
    if self.remaining == 0 {
      for (index, value) in self.state.iter_mut().enumerate() {
        *value ^= self.key[index % 2];
      }
      for _ in 0..4 {
        sipround(&mut self.state);
      }
      self.remaining = 4;
    }
    self.remaining -= 1;
    self.state[self.remaining]
  }
}

fn generate_program(generator: &mut Generator, index: usize) -> Program {
  let gen_values: [u64; 16] = core::array::from_fn(|_| generator.next());
  let template = &TEMPLATES[(gen_values[7] % 35) as usize];
  let permutation = if index.is_multiple_of(6) {
    &DEEP[(gen_values[7] % 24) as usize]
  } else {
    &SHALLOW[(gen_values[7] % 256) as usize]
  };
  let mut destinations = [0; 8];
  for i in 1..8 {
    let j = ((gen_values[i - 1] >> 32) % (i as u64 + 1)) as usize;
    destinations[i] = destinations[j];
    destinations[j] = i as u8;
  }
  let mut program = [Instruction::control(16); 11];
  program[0] = Instruction::control(15);
  for i in 0..8 {
    let opcode = if i == 0 {
      3
    } else {
      let random = gen_values[i - 1] as u32;
      if template[i - 1] == b'M' {
        (random % 3) as u8
      } else {
        4 + (random % 9) as u8
      }
    };
    let random = gen_values[8 + i];
    let imm = match opcode {
      0..=2 => [1, 9, 33][(random % 3) as usize],
      3..=6 => 1 + (random % 63) as u8,
      _ => 1 + (random % 3) as u8,
    };
    program[i + 1] = Instruction {
      opcode,
      dst: destinations[i],
      src: if i == 0 {
        8
      } else {
        destinations[permutation[i] as usize]
      },
      imm,
    };
  }
  program[9] = Instruction::control(if index == 31 { 14 } else { 13 });
  program
}

struct Vm {
  r: [u64; 9],
  pc: usize,
  bc: u8,
  bf: bool,
  mf: bool,
  sp: usize,
  memory: [u8; 16392],
}

impl Vm {
  const fn new() -> Self {
    Self {
      r: [0; 9],
      pc: 0,
      bc: 0,
      bf: false,
      mf: false,
      sp: 0,
      memory: [0; 16392],
    }
  }

  fn execute(&mut self, program: &Program) {
    self.pc = 0;
    loop {
      let instruction = program[self.pc];
      self.pc += 1;
      let Instruction {
        opcode,
        dst,
        src,
        imm,
      } = instruction;
      match opcode {
        0..=12 => {
          let dst = dst as usize;
          let value = self.r[dst];
          // Spec 1.4.2: RMCG always reads R8 directly, regardless of MF.
          let source = if self.mf && opcode != 3 {
            load64(&self.memory[(self.r[src as usize] & 16383) as usize..])
          } else {
            self.r[src as usize]
          };
          let shift = imm as u32;
          self.r[dst] = match opcode {
            0 => (value | imm as u64).wrapping_mul(source),
            1 => (value ^ imm as u64).wrapping_mul(source),
            2 => value.wrapping_add(imm as u64).wrapping_mul(source),
            3 => value.wrapping_mul(source).rotate_right(shift),
            4 => value.rotate_right(shift) ^ source,
            5 => value.rotate_right(shift).wrapping_add(source),
            6 => value.rotate_right(shift).wrapping_sub(source),
            7 => ((value as i64 >> shift) as u64) ^ source,
            8 => ((value as i64 >> shift) as u64).wrapping_add(source),
            9 => ((value as i64 >> shift) as u64).wrapping_sub(source),
            10 => (value >> shift) ^ source,
            11 => (value >> shift).wrapping_add(source),
            12 => (value >> shift).wrapping_sub(source),
            _ => unreachable!(),
          };
          if opcode == 3 {
            self.bf = self.r[dst] & 32 != 0;
          }
        }
        13 | 14 => {
          if self.bc != 0 && (opcode == 14 || !self.bf) {
            self.bc -= 1;
            self.pc = 0;
          }
        }
        15 => {
          if !self.mf {
            self.sp -= 64;
            for i in 0..8 {
              let address = self.sp + i * 8;
              self.memory[address..address + 8].copy_from_slice(&self.r[7 - i].to_le_bytes());
            }
          }
        }
        16 => break,
        _ => unreachable!(),
      }
    }
  }

  fn phase(&mut self, programs: &[Program; 32]) {
    for _ in 0..4 {
      self.bc = 32;
      for program in programs {
        self.execute(program);
      }
      debug_assert_eq!(self.bc, 0);
    }
  }
}

/// A seeded HashWX interpreter with reusable program and VM storage.
///
/// Construction and hashing use fixed-size inline buffers without allocation.
/// Hashing takes exclusive access to reuse the 16392-byte VM memory.
/// Separate instances can be used concurrently.
pub struct HashWx {
  programs: [Program; 32],
  key: [u64; 2],
  vm: Vm,
}

impl HashWx {
  /// Generates the 32 programs and Siphash key from a 32-byte seed.
  pub fn new(seed: &[u8; 32]) -> Self {
    let mut generator = Generator::new([load64(seed), load64(&seed[8..])], u64::MAX);
    Self {
      programs: core::array::from_fn(|index| generate_program(&mut generator, index)),
      key: [load64(&seed[16..]), load64(&seed[24..])],
      vm: Vm::new(),
    }
  }

  /// Calculates the 64-bit hash of a nonce, reusing this instance's VM storage.
  pub fn hash(&mut self, nonce: u64) -> u64 {
    let mut generator = Generator::new(self.key, nonce);
    for value in &mut self.vm.r[..8] {
      *value = generator.next();
    }
    self.vm.r[8] = ((self.vm.r[0] ^ self.vm.r[4]) & !7) | 3;
    self.vm.bf = false;
    self.vm.mf = false;
    self.vm.sp = 16384;
    self.vm.memory[16384..].copy_from_slice(&self.vm.r[8].to_le_bytes());
    self.vm.phase(&self.programs);
    // Spec 3.2: 128 initial STOREs plus 128 branch repeats fill exactly 16 KiB.
    debug_assert_eq!(self.vm.sp, 0);
    self.vm.mf = true;
    self.vm.phase(&self.programs);
    // Spec 3.4 and Appendix E: one SipRound for each four-register group.
    let mut low = self.vm.r[..4].try_into().expect("four registers");
    let mut high = self.vm.r[4..8].try_into().expect("four registers");
    sipround(&mut low);
    sipround(&mut high);
    low[3] ^ high[3] ^ self.vm.r[8]
  }
}

#[cfg(test)]
mod tests;
