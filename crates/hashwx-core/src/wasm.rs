//! WebAssembly 1.0 encoding of specification sections 1.4 and 3, and Appendix A.

use alloc::{vec, vec::Vec};

use crate::HashWx;

const RESULT_NONCE_OFFSET: u32 = 16408;

#[derive(Default)]
struct Code(Vec<u8>);

impl Code {
  fn op(&mut self, op: u8) {
    self.0.push(op);
  }

  fn uint(&mut self, mut value: u32) {
    loop {
      let byte = (value & 127) as u8;
      value >>= 7;
      self.op(byte | if value == 0 { 0 } else { 128 });
      if value == 0 {
        break;
      }
    }
  }

  fn int(&mut self, mut value: i64) {
    loop {
      let byte = value as u8 & 127;
      value >>= 7;
      let done = (value == 0 && byte & 64 == 0) || (value == -1 && byte & 64 != 0);
      self.op(byte | if done { 0 } else { 128 });
      if done {
        break;
      }
    }
  }

  fn c32(&mut self, value: u32) {
    self.op(0x41);
    self.int(value as i32 as i64);
  }

  fn c64(&mut self, value: u64) {
    self.op(0x42);
    self.int(value as i64);
  }

  fn get(&mut self, index: u32) {
    self.op(0x20);
    self.uint(index);
  }

  fn set(&mut self, index: u32) {
    self.op(0x21);
    self.uint(index);
  }

  fn control(&mut self, op: u8) {
    self.op(op);
    self.op(0x40);
  }

  fn mem(&mut self, op: u8, offset: u32) {
    self.op(op);
    self.uint(0); // Spec 1.4.1: memory operands permit byte-aligned accesses.
    self.uint(offset);
  }

  fn name(&mut self, name: &[u8]) {
    self.uint(name.len() as u32);
    self.0.extend_from_slice(name);
  }

  fn section(&mut self, id: u8, content: &Self) {
    self.op(id);
    self.uint(content.0.len() as u32);
    self.0.extend_from_slice(&content.0);
  }

  fn binary(&mut self, dst: u32, src: u32, op: u8) {
    self.get(dst);
    self.get(src);
    self.op(op);
    self.set(dst);
  }

  fn rotate(&mut self, dst: u32, amount: u64) {
    self.get(dst);
    self.c64(amount);
    self.op(0x8a);
    self.set(dst);
  }

  fn xor_const(&mut self, dst: u32, value: u64) {
    self.get(dst);
    self.c64(value);
    self.op(0x85);
    self.set(dst);
  }

  fn sipround(&mut self, base: u32) {
    let [a, b, c, d] = [base, base + 1, base + 2, base + 3];
    self.binary(a, b, 0x7c);
    self.binary(c, d, 0x7c);
    self.rotate(b, 51);
    self.rotate(d, 48);
    self.binary(b, a, 0x85);
    self.binary(d, c, 0x85);
    self.rotate(a, 32);
    self.binary(c, b, 0x7c);
    self.binary(a, d, 0x7c);
    self.rotate(b, 47);
    self.rotate(d, 43);
    self.binary(b, c, 0x85);
    self.binary(d, a, 0x85);
    self.rotate(c, 32);
  }

  fn halves(&mut self, lo: u32, hi: u32) {
    self.get(lo);
    self.op(0xad);
    self.get(hi);
    self.op(0xad);
    self.c64(32);
    self.op(0x86);
    self.op(0x84);
  }

  fn store(&mut self, address: u32, local: u32) {
    self.c32(address);
    self.get(local);
    self.mem(0x37, 0);
  }
}

fn hash(instance: &HashWx) -> Code {
  // Locals: nonce 0, R0-R8 1..9, generator 10..13, SP/BC/iteration 14..16.
  let mut out = Code(vec![2, 13, 0x7e, 3, 0x7f]);
  let constants = [
    0x736f6d6570736575,
    0x646f72616e646f6d,
    0x6c7967656e657261,
    0x7465646279746573,
  ];
  for (i, constant) in constants.iter().enumerate() {
    out.c64(constant ^ instance.key[i % 2]);
    out.set(10 + i as u32);
  }
  out.binary(13, 0, 0x85);
  out.sipround(10);
  out.binary(10, 0, 0x85);
  out.xor_const(12, 0xbb);
  for _ in 0..3 {
    out.sipround(10);
  }
  for i in 0..4 {
    out.get(13 - i);
    out.set(1 + i);
  }
  for i in 0..4 {
    out.xor_const(10 + i, instance.key[i as usize % 2]);
  }
  for _ in 0..4 {
    out.sipround(10);
  }
  for i in 0..4 {
    out.get(13 - i);
    out.set(5 + i);
  }
  out.get(1);
  out.get(5);
  out.op(0x85);
  out.c64(!7);
  out.op(0x83);
  out.c64(3);
  out.op(0x84);
  out.set(9);
  out.store(16384, 9);
  out.c32(16384);
  out.set(14);

  for memory in [false, true] {
    out.c32(4);
    out.set(16);
    out.control(0x03);
    out.c32(32);
    out.set(15);
    for program in &instance.programs {
      out.control(0x03);
      if !memory {
        out.get(14);
        out.c32(64);
        out.op(0x6b);
        out.set(14);
        for i in 0..8 {
          out.get(14);
          out.get(8 - i);
          out.mem(0x37, i * 8);
        }
      }
      for instruction in &program[1..9] {
        let dst = u32::from(instruction.dst) + 1;
        let src = u32::from(instruction.src) + 1;
        let opcode = instruction.opcode;
        out.get(dst);
        if opcode == 3 {
          out.get(src);
          out.op(0x7e);
          out.c64(instruction.imm.into());
          out.op(0x8a);
        } else {
          out.c64(instruction.imm.into());
          out.op(match opcode {
            0 => 0x84,
            1 => 0x85,
            2 => 0x7c,
            4..=6 => 0x8a,
            7..=9 => 0x87,
            10..=12 => 0x88,
            _ => unreachable!(),
          });
          out.get(src);
          if memory {
            out.op(0xa7);
            out.c32(16383);
            out.op(0x71);
            out.mem(0x29, 0);
          }
          out.op(match opcode {
            0..=2 => 0x7e,
            4 | 7 | 10 => 0x85,
            5 | 8 | 11 => 0x7c,
            6 | 9 | 12 => 0x7d,
            _ => unreachable!(),
          });
        }
        out.set(dst);
      }
      out.get(15);
      out.op(0x45);
      out.op(0x45);
      if program[9].opcode == 13 {
        // Spec 1.4.2: the RMCG destination retains the branch flag until the branch.
        out.get(u32::from(program[1].dst) + 1);
        out.c64(32);
        out.op(0x83);
        out.op(0x50);
        out.op(0x71);
      }
      out.control(0x04);
      out.get(15);
      out.c32(1);
      out.op(0x6b);
      out.set(15);
      out.op(0x0c);
      out.uint(1);
      out.op(0x0b);
      out.op(0x0b);
    }
    out.get(16);
    out.c32(1);
    out.op(0x6b);
    out.op(0x22);
    out.uint(16);
    out.op(0x0d);
    out.uint(0);
    out.op(0x0b);
  }
  out.sipround(1);
  out.sipround(5);
  out.get(4);
  out.get(8);
  out.op(0x85);
  out.get(9);
  out.op(0x85);
  out.op(0x0b);
  out
}

fn public_hash() -> Code {
  let mut out = Code(vec![0]);
  out.c32(16400);
  out.halves(0, 1);
  out.op(0x10);
  out.uint(0);
  out.mem(0x37, 0);
  out.c32(0);
  out.op(0x0b);
  out
}

fn search() -> Code {
  let mut out = Code(vec![1, 3, 0x7e]);
  out.halves(0, 1);
  out.set(5);
  out.halves(3, 4);
  out.set(6);
  out.control(0x02);
  out.control(0x03);
  out.get(2);
  out.op(0x45);
  out.op(0x0d);
  out.uint(1);
  out.get(5);
  out.op(0x10);
  out.uint(0);
  out.op(0x22);
  out.uint(7);
  out.get(6);
  out.op(0x54);
  out.control(0x04);
  out.store(16400, 7);
  out.store(RESULT_NONCE_OFFSET, 5);
  out.c32(0);
  out.op(0x0f);
  out.op(0x0b);
  out.get(5);
  out.c64(1);
  out.op(0x7c);
  out.set(5);
  out.get(2);
  out.c32(1);
  out.op(0x6b);
  out.set(2);
  out.op(0x0c);
  out.uint(0);
  out.op(0x0b);
  out.op(0x0b);
  out.c32(u32::MAX);
  out.op(0x0b);
  out
}

pub(super) fn generate(instance: &HashWx) -> Vec<u8> {
  let mut out = Code(b"\0asm\x01\0\0\0".to_vec());
  out.section(
    1,
    &Code(vec![
      3, 0x60, 1, 0x7e, 1, 0x7e, 0x60, 2, 0x7f, 0x7f, 1, 0x7f, 0x60, 5, 0x7f, 0x7f, 0x7f, 0x7f,
      0x7f, 1, 0x7f,
    ]),
  );
  out.section(3, &Code(vec![3, 0, 1, 2]));
  out.section(5, &Code(vec![1, 1, 1, 1]));
  let mut globals = Code(vec![1, 0x7f, 0]);
  globals.c32(RESULT_NONCE_OFFSET);
  globals.op(0x0b);
  out.section(6, &globals);
  let mut exports = Code(vec![4]);
  for (name, kind, index) in [
    (b"memory".as_slice(), 2, 0),
    (b"hash", 0, 1),
    (b"search", 0, 2),
    (b"result_ptr", 3, 0),
  ] {
    exports.name(name);
    exports.op(kind);
    exports.uint(index);
  }
  out.section(7, &exports);
  let mut code = Code(vec![3]);
  for body in [hash(instance), public_hash(), search()] {
    code.uint(body.0.len() as u32);
    code.0.extend_from_slice(&body.0);
  }
  out.section(10, &code);
  out.0
}
