# hashwx-core

A `no_std` HashWX instance generator, interpreter, and WASM code generator.
Instance construction and interpretation use fixed-size inline buffers without
allocation. Code generation uses `alloc::Vec`. The crate has no dependencies and
forbids unsafe code. It inherits the workspace Apache-2.0 license.

## Specification and provenance

Implemented from the [HashWX specification](https://github.com/tevador/hashwx/blob/fde6d2ae34dce266497aed5c093ff7b40a933c3a/doc/specification.md),
revision `fde6d2ae34dce266497aed5c093ff7b40a933c3a`.
The implementation was written from that specification without copying,
translating, or adapting the upstream LGPL code. Opcode templates and source
permutation tables come from specification Appendices B and D.

## API

```rust
use hashwx_core::HashWx;

let mut instance = HashWx::new(&[0; 32]);
let first = instance.hash(0);
let second = instance.hash(1);
assert_eq!(first, instance.hash(0));
```

`HashWx::new(seed: &[u8; 32]) -> HashWx` generates the 32 programs and key.
`HashWx::hash(&mut self, nonce: u64) -> u64` reuses the instance's 16392-byte VM
memory. Neither operation allocates. Hashing needs exclusive access; independent
instances can be used concurrently.

## WASM code generation

`HashWx::wasm_module(&self) -> Vec<u8>` emits deterministic WebAssembly 1.0 bytes
specialized to the instance's 32 programs and key. It uses a small internal binary
encoder, with no runtime dependency. The emitted module has no imports, SIMD,
BigInt boundary values, or post-MVP instructions. Its size is below 32 KiB.

Each instantiated module owns one fixed 64 KiB memory page. Bytes 0..16392 hold
VM memory (specification section 1.3), 16400..16408 hold the last successful hash,
and 16408..16416 hold the last matching nonce. Outputs are little endian.
The remaining bytes are unused. Exports:

| Export | Arguments | Result |
| --- | --- | --- |
| `memory` | - | Private linear memory |
| `hash` | `nonce_lo, nonce_hi` | 0; writes hash at 16400 |
| `search` | `nonce_lo, nonce_hi, count, target_lo, target_hi` | 0 for first hash < target; -1 if exhausted |

All function parameters are unsigned u32 values carried in WASM i32 slots.
Search advances the nonce modulo 2^64. A zero count searches nothing; a zero
target exhausts the range. Search writes hash and nonce only on success.
Repeated calls reuse VM memory and fully overwrite it during the write phase.
Each module supports sequential calls; instantiate separate modules for workers.

The generator emits separate register-source and memory-source bodies, with
eight VM arithmetic registers and R8 in i64 locals. Each program becomes a
structured loop sharing the phase's branch budget. Destination permutation
uniqueness (section 2.3.3) preserves the RMCG destination through the branch,
allowing its bit 5 to serve as BF without a separate flag local. The key and
instruction operands are constants. Siphash initialization and finalization are
emitted directly from Appendix A and section 3.4.

The raw interpreter ABI and build instructions are in
[`hashwx-wasm`](../hashwx-wasm/README.md).

## Conformance

All four Appendix F vectors pass:

| Vector | Nonce | Result | Status |
| --- | --- | --- | --- |
| F.1 | 0 | `973684176f8ee362` | Pass |
| F.2 | 123456 | `401983bb07d69b07` | Pass |
| F.3 | 123456 | `4af38d834a9a8d3d` | Pass |
| F.4 | 987654321123456789 | `6a8a5514432e17a3` | Pass |

`cargo test -p hashwx-core --locked` also validates modules for 32 seeds using
Node's WebAssembly engine and checks deterministic emission across fresh and
reused instances. Node must be on PATH; no validator crate is required.
After `cargo xtask build-wasm`, `node --test tests/hashwx-wasm.test.mjs` checks
each Appendix F vector through the interpreted ABI and generated module, compares
4096 nonces across 64 pseudorandom seeds, and exercises strict range searches,
zero counts, nonce carries, wraparound, invalid handles, and instance isolation.
The browser-shaped test harness uses u32 halves throughout and requires no BigInt.

Additional tests cover deterministic reuse against fresh instances, 4096 hashes
across 256 pseudorandom seeds, every arithmetic opcode in both operand modes,
branches, little-endian STORE layout, unaligned memory loads at the upper address
boundary, program generation invariants, and write/read phase invariants. Debug
assertions check exhaustion of each phase's branch budget and that the write
phase's 256 STOREs end at SP = 0.

Section 3.4's wording "two SipRounds" is interpreted as two rounds total: one
on R0-R3 and one on R4-R7, as explicitly shown in Appendix E. This matches all
Appendix F vectors. BF starts cleared; each program overwrites BF through RMCG
before evaluating its branch, so the initial value does not affect the hash.

## Benchmark

```sh
cargo run -p hashwx-core --release --example hashwx-throughput -- 10000
```

Prints JSON with mean generation time, hash throughput, generation plus one
hash cost, a checksum, and individual solve times at difficulties 1000, 10000
and 50000. The optional argument sets the nonces per seed. Throughput averages
three seeds. Solve measurements use three deterministic trials per difficulty.

`node scripts/bench-hashwx-wasm.mjs` measures single-thread WASM interpreter and
generated-module throughput for 463 and 65536 nonces per seed. It reports setup,
module compilation/instantiation, hash-only throughput, and total throughput
including per-seed setup and compilation. Each row averages three distinct seeds.

`node scripts/bench-hashwx-browser.mjs` starts the LAN prototype benchmark on a
free port. Open its printed LAN URL on the device and select Start benchmark.
The page reports progress and saves each completed result to
`target/hashwx-bench/`. See proposal section 8 for the measured values and limits.
