//! Seed-specialized WASM code generation for Light-mode dataset items.

use alloc::{vec, vec::Vec};

use crate::{
  constants::{CACHE_LINE_SIZE, CACHE_SIZE, DATASET_ADD, DATASET_MUL},
  helpers::sign_extend_u32,
  reciprocal::reciprocal,
  superscalar::{SuperscalarProgram, SuperscalarType},
};

fn unsigned(out: &mut Vec<u8>, mut value: u32) {
  loop {
    let byte = (value & 127) as u8;
    value >>= 7;
    out.push(byte | if value != 0 { 128 } else { 0 });
    if value == 0 {
      break;
    }
  }
}

fn signed(out: &mut Vec<u8>, mut value: i64) {
  loop {
    let byte = (value as u8) & 127;
    value >>= 7;
    let done = (value == 0 && byte & 64 == 0) || (value == -1 && byte & 64 != 0);
    out.push(byte | if done { 0 } else { 128 });
    if done {
      break;
    }
  }
}

fn section(out: &mut Vec<u8>, kind: u8, bytes: &[u8]) {
  out.push(kind);
  unsigned(out, bytes.len() as u32);
  out.extend_from_slice(bytes);
}

fn name(out: &mut Vec<u8>, value: &[u8]) {
  unsigned(out, value.len() as u32);
  out.extend_from_slice(value);
}

fn local_get(out: &mut Vec<u8>, index: u32) {
  out.push(0x20);
  unsigned(out, index);
}
fn local_set(out: &mut Vec<u8>, index: u32) {
  out.push(0x21);
  unsigned(out, index);
}
fn i64_const(out: &mut Vec<u8>, value: u64) {
  out.push(0x42);
  signed(out, value as i64);
}
fn i32_const(out: &mut Vec<u8>, value: u32) {
  out.push(0x41);
  signed(out, value as i32 as i64);
}
fn limb(out: &mut Vec<u8>, index: u32, high: bool, signed: bool) {
  local_get(out, index);
  i64_const(out, if high { 32 } else { 0xffff_ffff });
  out.push(if high {
    if signed { 0x87 } else { 0x88 }
  } else {
    0x83
  });
}

fn multiply_high(out: &mut Vec<u8>, a: u32, b: u32, signed: bool, temp: u32) {
  // The low product's carry is unsigned even when the upper limbs are signed.
  limb(out, a, true, signed);
  limb(out, b, false, signed);
  out.push(0x7e);
  limb(out, a, false, signed);
  limb(out, b, false, signed);
  out.push(0x7e);
  i64_const(out, 32);
  out.extend_from_slice(&[0x88, 0x7c]);
  local_set(out, temp);
  limb(out, a, a == b, signed);
  limb(out, b, a != b, signed);
  out.push(0x7e);
  limb(out, temp, false, signed);
  out.push(0x7c);
  i64_const(out, 32);
  out.push(if signed { 0x87 } else { 0x88 });
  limb(out, a, true, signed);
  limb(out, b, true, signed);
  out.extend_from_slice(&[0x7e, 0x7c]);
  limb(out, temp, true, signed);
  out.push(0x7c);
}

struct Layout {
  item: u32,
  output: u32,
  r: u32,
  line: u32,
  temp: u32,
  first: u32,
}

fn dataset(programs: &[SuperscalarProgram], cache_base: u32, count: usize) -> Vec<u8> {
  let layouts = if count == 3 {
    (0..3)
      .map(|i| Layout {
        item: i,
        output: i,
        r: 2 + i * 8,
        line: 26 + i,
        temp: 29 + i * 2,
        first: 30 + i * 2,
      })
      .collect()
  } else if count == 2 {
    vec![
      Layout {
        item: 0,
        output: 2,
        r: 4,
        line: 20,
        temp: 22,
        first: 23,
      },
      Layout {
        item: 1,
        output: 3,
        r: 12,
        line: 21,
        temp: 24,
        first: 25,
      },
    ]
  } else {
    vec![Layout {
      item: 0,
      output: 1,
      r: 2,
      line: 10,
      temp: 11,
      first: 12,
    }]
  };
  let mut out = if count == 3 {
    vec![3, 24, 0x7e, 3, 0x7f, 6, 0x7e]
  } else if count == 2 {
    vec![3, 16, 0x7e, 2, 0x7f, 4, 0x7e]
  } else {
    vec![3, 8, 0x7e, 1, 0x7f, 2, 0x7e]
  };
  for layout in &layouts {
    if count == 3 {
      local_get(&mut out, 0);
      out.push(0x29);
      unsigned(&mut out, 3);
      unsigned(&mut out, layout.item * 8);
    } else {
      local_get(&mut out, layout.item);
    }
    i64_const(&mut out, 1);
    out.push(0x7c);
    i64_const(&mut out, DATASET_MUL);
    out.push(0x7e);
    local_set(&mut out, layout.r);
    for (index, value) in DATASET_ADD.iter().enumerate().skip(1) {
      local_get(&mut out, layout.r);
      i64_const(&mut out, *value);
      out.push(0x85);
      local_set(&mut out, layout.r + index as u32);
    }
  }
  let mut addresses = layouts.iter().map(|layout| layout.item).collect::<Vec<_>>();
  for (program_index, program) in programs.iter().enumerate() {
    for (layout, address) in layouts.iter().zip(&addresses) {
      if count == 3 && program_index == 0 {
        local_get(&mut out, 0);
        out.push(0x29);
        unsigned(&mut out, 3);
        unsigned(&mut out, layout.item * 8);
      } else {
        local_get(&mut out, *address);
      }
      out.push(0xa7);
      i32_const(&mut out, (CACHE_SIZE / CACHE_LINE_SIZE - 1) as u32);
      out.push(0x71);
      i32_const(&mut out, 6);
      out.push(0x74);
      i32_const(&mut out, cache_base);
      out.push(0x6a);
      local_set(&mut out, layout.line);
      // Independent cache lines can arrive while both arithmetic chains execute.
      local_get(&mut out, layout.line);
      out.extend_from_slice(&[0x29, 3, 0]);
      local_set(&mut out, layout.first);
    }
    for instr in &program.instructions {
      for layout in &layouts {
        let dst = instr.dst as u32 + layout.r;
        let src = instr.src as u32 + layout.r;
        if instr.opcode != SuperscalarType::IMulhR as u8
          && instr.opcode != SuperscalarType::ISMulhR as u8
        {
          local_get(&mut out, dst);
        }
        match instr.opcode {
          x if x == SuperscalarType::ISubR as u8 => {
            local_get(&mut out, src);
            out.push(0x7d);
          }
          x if x == SuperscalarType::IXorR as u8 => {
            local_get(&mut out, src);
            out.push(0x85);
          }
          x if x == SuperscalarType::IAddRs as u8 => {
            local_get(&mut out, src);
            i64_const(&mut out, instr.mod_shift() as u64);
            out.extend_from_slice(&[0x86, 0x7c]);
          }
          x if x == SuperscalarType::IMulR as u8 => {
            local_get(&mut out, src);
            out.push(0x7e);
          }
          x if x == SuperscalarType::IRorC as u8 => {
            i64_const(&mut out, (instr.imm32 & 63) as u64);
            out.push(0x8a);
          }
          x if [
            SuperscalarType::IAddC7 as u8,
            SuperscalarType::IAddC8 as u8,
            SuperscalarType::IAddC9 as u8,
          ]
          .contains(&x) =>
          {
            i64_const(&mut out, sign_extend_u32(instr.imm32));
            out.push(0x7c);
          }
          x if [
            SuperscalarType::IXorC7 as u8,
            SuperscalarType::IXorC8 as u8,
            SuperscalarType::IXorC9 as u8,
          ]
          .contains(&x) =>
          {
            i64_const(&mut out, sign_extend_u32(instr.imm32));
            out.push(0x85);
          }
          x if x == SuperscalarType::IMulhR as u8 => {
            multiply_high(&mut out, dst, src, false, layout.temp);
          }
          x if x == SuperscalarType::ISMulhR as u8 => {
            multiply_high(&mut out, dst, src, true, layout.temp);
          }
          x if x == SuperscalarType::IMulRcp as u8 => {
            i64_const(&mut out, reciprocal(instr.imm32));
            out.push(0x7e);
          }
          opcode => unreachable!("invalid superscalar opcode {opcode}"),
        }
        local_set(&mut out, dst);
      }
    }
    for index in 0..8 {
      for layout in &layouts {
        local_get(&mut out, layout.r + index);
        if index == 0 {
          local_get(&mut out, layout.first);
        } else {
          local_get(&mut out, layout.line);
          out.push(0x29);
          unsigned(&mut out, 3);
          unsigned(&mut out, index * 8);
        }
        out.push(0x85);
        local_set(&mut out, layout.r + index);
      }
    }
    for (address, layout) in addresses.iter_mut().zip(&layouts) {
      *address = layout.r + program.address_register as u32;
    }
  }
  for layout in &layouts {
    for index in 0..8 {
      local_get(&mut out, if count == 3 { 1 } else { layout.output });
      local_get(&mut out, layout.r + index);
      out.push(0x37);
      unsigned(&mut out, 3);
      unsigned(
        &mut out,
        index * 8 + if count == 3 { layout.output * 64 } else { 0 },
      );
    }
  }
  out.push(0x0b);
  out
}

pub(crate) fn generate(programs: &[SuperscalarProgram], cache_base: u32) -> Vec<u8> {
  let mut out = b"\0asm\x01\0\0\0".to_vec();
  section(
    &mut out,
    1,
    &[
      3, 0x60, 2, 0x7e, 0x7f, 0, 0x60, 4, 0x7e, 0x7e, 0x7f, 0x7f, 0, 0x60, 2, 0x7f, 0x7f, 0,
    ],
  );
  let mut imports = vec![1];
  name(&mut imports, b"env");
  name(&mut imports, b"memory");
  imports.extend_from_slice(&[2, 0, 0]);
  section(&mut out, 2, &imports);
  section(&mut out, 3, &[3, 0, 1, 2]);
  let mut exports = vec![3];
  name(&mut exports, b"dataset_item");
  exports.extend_from_slice(&[0, 0]);
  name(&mut exports, b"dataset_pair");
  exports.extend_from_slice(&[0, 1]);
  name(&mut exports, b"dataset_triple");
  exports.extend_from_slice(&[0, 2]);
  section(&mut out, 7, &exports);
  let mut code = vec![3];
  for count in 1..=3 {
    let body = dataset(programs, cache_base, count);
    unsigned(&mut code, body.len() as u32);
    code.extend_from_slice(&body);
  }
  section(&mut out, 10, &code);
  out
}
