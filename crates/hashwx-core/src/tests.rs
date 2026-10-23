use super::*;

const SEED_A: [u8; 32] = *b"This is a test seed for hashwx\0\0";
const SEED_B: [u8; 32] = *b"Lorem ipsum dolor sit amet\0\0\0\0\0\0";

#[test]
fn appendix_f_1() {
  let seed = [
    0x54, 0x68, 0x69, 0x73, 0x20, 0x69, 0x73, 0x20, 0x61, 0x20, 0x74, 0x65, 0x73, 0x74, 0x20, 0x73,
    0x65, 0x65, 0x64, 0x20, 0x66, 0x6f, 0x72, 0x20, 0x68, 0x61, 0x73, 0x68, 0x77, 0x78, 0x00, 0x00,
  ];
  assert_eq!(HashWx::new(&seed).hash(0), 0x973684176f8ee362);
}

#[test]
fn appendix_f_2() {
  let seed = [
    0x54, 0x68, 0x69, 0x73, 0x20, 0x69, 0x73, 0x20, 0x61, 0x20, 0x74, 0x65, 0x73, 0x74, 0x20, 0x73,
    0x65, 0x65, 0x64, 0x20, 0x66, 0x6f, 0x72, 0x20, 0x68, 0x61, 0x73, 0x68, 0x77, 0x78, 0x00, 0x00,
  ];
  assert_eq!(HashWx::new(&seed).hash(123456), 0x401983bb07d69b07);
}

#[test]
fn appendix_f_3() {
  let seed = [
    0x4c, 0x6f, 0x72, 0x65, 0x6d, 0x20, 0x69, 0x70, 0x73, 0x75, 0x6d, 0x20, 0x64, 0x6f, 0x6c, 0x6f,
    0x72, 0x20, 0x73, 0x69, 0x74, 0x20, 0x61, 0x6d, 0x65, 0x74, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
  ];
  assert_eq!(HashWx::new(&seed).hash(123456), 0x4af38d834a9a8d3d);
}

#[test]
fn appendix_f_4() {
  let seed = [
    0x4c, 0x6f, 0x72, 0x65, 0x6d, 0x20, 0x69, 0x70, 0x73, 0x75, 0x6d, 0x20, 0x64, 0x6f, 0x6c, 0x6f,
    0x72, 0x20, 0x73, 0x69, 0x74, 0x20, 0x61, 0x6d, 0x65, 0x74, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
  ];
  assert_eq!(
    HashWx::new(&seed).hash(987654321123456789),
    0x6a8a5514432e17a3
  );
}

fn random(state: &mut u64) -> u64 {
  *state ^= *state << 13;
  *state ^= *state >> 7;
  *state ^= *state << 17;
  *state
}

#[test]
fn reused_instances_match_fresh_instances() {
  for seed in [SEED_A, SEED_B, [0; 32], [255; 32]] {
    let mut reused = HashWx::new(&seed);
    for nonce in (0..64).chain([u64::MAX, 0, 123456, 0]) {
      assert_eq!(reused.hash(nonce), HashWx::new(&seed).hash(nonce));
      assert_eq!(reused.vm.sp, 0);
      assert_eq!(reused.vm.bc, 0);
    }
  }
}

#[test]
fn pseudorandom_seeds_and_nonces_do_not_panic() {
  let mut state = 0x123456789abcdef0;
  for _ in 0..256 {
    let mut seed = [0; 32];
    for chunk in seed.as_chunks_mut::<8>().0 {
      chunk.copy_from_slice(&random(&mut state).to_le_bytes());
    }
    let mut hasher = HashWx::new(&seed);
    for _ in 0..16 {
      hasher.hash(random(&mut state));
      assert_eq!(hasher.vm.sp, 0);
      assert_eq!(hasher.vm.bc, 0);
    }
  }
}

#[test]
fn generation_invariants() {
  for seed in [SEED_A, SEED_B, [0; 32], [255; 32]] {
    let instance = HashWx::new(&seed);
    for (index, program) in instance.programs.iter().enumerate() {
      assert_eq!(program[0].opcode, 15);
      assert_eq!(program[1].opcode, 3);
      assert_eq!(program[1].src, 8);
      assert_eq!(program[9].opcode, if index == 31 { 14 } else { 13 });
      assert_eq!(program[10].opcode, 16);
      let mut seen = [false; 8];
      for instruction in &program[1..9] {
        assert!(!seen[instruction.dst as usize]);
        seen[instruction.dst as usize] = true;
        let valid = match instruction.opcode {
          0..=2 => [1, 9, 33].contains(&instruction.imm),
          3..=6 => (1..=63).contains(&instruction.imm),
          7..=12 => (1..=3).contains(&instruction.imm),
          _ => false,
        };
        assert!(valid);
      }
      assert_eq!(program[2..9].iter().filter(|i| i.opcode <= 2).count(), 3);
      for instruction in &program[2..9] {
        assert!(instruction.src < 8);
        assert_ne!(instruction.dst, instruction.src);
      }
    }
  }
  for permutation in SHALLOW.iter().chain(DEEP.iter()) {
    let mut sorted = *permutation;
    sorted.sort_unstable();
    assert_eq!(sorted, [0, 1, 2, 3, 4, 5, 6, 7]);
    for (index, &value) in permutation.iter().enumerate().skip(1) {
      assert_ne!(index, value as usize);
    }
  }
  for template in TEMPLATES {
    assert_eq!(template.iter().filter(|&&op| op == b'M').count(), 3);
  }
}

#[test]
fn arithmetic_opcodes_in_both_operand_modes() {
  let expected = [
    0xe38e38e38e38e390,
    0xe4b17e4b17e4b180,
    0xe26af37c048d15a0,
    0xe00000000000000c,
    0x9edcba9876543214,
    0x5edcba9876543214,
    0x6123456789abcdf4,
    0x1edcba9876543214,
    0xdedcba9876543214,
    0xe123456789abcdf4,
    0xdedcba9876543214,
    0x1edcba9876543214,
    0x2123456789abcdf4,
  ];
  for mf in [false, true] {
    for (opcode, &expected) in expected.iter().enumerate() {
      let mut vm = Vm::new();
      vm.r[0] = 0x8000000000000011;
      vm.r[1] = if mf { u64::MAX } else { 0xfedcba9876543210 };
      vm.r[8] = 3;
      vm.mf = mf;
      vm.memory[16383..16391].copy_from_slice(&0xfedcba9876543210u64.to_le_bytes());
      let mut program = [Instruction::control(16); 11];
      program[0] = Instruction {
        opcode: opcode as u8,
        dst: 0,
        src: if opcode == 3 { 8 } else { 1 },
        imm: if opcode <= 2 { 9 } else { 2 },
      };
      vm.execute(&program);
      assert_eq!(vm.r[0], expected, "opcode={opcode}, MF={mf}");
      assert_eq!(vm.bf, opcode == 3 && expected & 32 != 0);
      assert_eq!(vm.r[8], 3);
      assert_eq!(vm.pc, 2);
    }
  }
}

#[test]
fn branches_store_and_halt() {
  for opcode in [13, 14] {
    for bf in [false, true] {
      let mut vm = Vm::new();
      vm.r = [0x0807060504030201, 2, 3, 4, 5, 6, 7, 8, 9];
      vm.sp = 192;
      vm.bc = 2;
      vm.bf = bf;
      let mut program = [Instruction::control(16); 11];
      program[0] = Instruction::control(15);
      program[1] = Instruction::control(opcode);
      vm.execute(&program);
      let branched = opcode == 14 || !bf;
      assert_eq!(vm.sp, if branched { 0 } else { 128 });
      assert_eq!(vm.bc, if branched { 0 } else { 2 });
      assert_eq!(vm.pc, 3);
      for i in 0..8 {
        assert_eq!(load64(&vm.memory[vm.sp + 8 * i..]), vm.r[7 - i]);
      }
      assert_eq!(
        &vm.memory[vm.sp + 56..vm.sp + 64],
        &[1, 2, 3, 4, 5, 6, 7, 8]
      );
      let before = vm.memory;
      vm.mf = true;
      vm.execute(&program);
      assert_eq!(vm.memory, before);
      assert_eq!(vm.sp, if branched { 0 } else { 128 });
    }
  }
}

#[test]
fn write_phase_fills_memory_and_preserves_r8_tail() {
  let mut instance = HashWx::new(&SEED_A);
  instance.vm.r = [1, 2, 3, 4, 5, 6, 7, 8, 0x0123456789abcdef];
  instance.vm.sp = 16384;
  instance.vm.memory[16384..].copy_from_slice(&instance.vm.r[8].to_le_bytes());
  instance.vm.phase(&instance.programs);
  assert_eq!(instance.vm.sp, 0);
  assert_eq!(instance.vm.bc, 0);
  assert_eq!(load64(&instance.vm.memory[16384..]), instance.vm.r[8]);
  let memory = instance.vm.memory;
  instance.vm.mf = true;
  instance.vm.phase(&instance.programs);
  assert_eq!(instance.vm.memory, memory);
  assert_eq!(instance.vm.sp, 0);
}
