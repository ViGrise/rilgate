# hashwx-core

An allocation-free, `no_std` HashWX instance generator and interpreter. It uses
fixed-size inline program and VM buffers, has no dependencies, and forbids unsafe
code. The crate inherits the workspace Apache-2.0 license.

## Specification and provenance

Implemented from the [HashWX specification](https://github.com/tevador/hashwx/blob/fde6d2ae34dce266497aed5c093ff7b40a933c3a/doc/specification.md),
revision `fde6d2ae34dce266497aed5c093ff7b40a933c3a`.
The implementation was written from that specification without reading, copying,
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
instances can be used concurrently. This crate implements interpreted mode only.

## Conformance

All four Appendix F vectors pass:

| Vector | Nonce | Result | Status |
| --- | --- | --- | --- |
| F.1 | 0 | `973684176f8ee362` | Pass |
| F.2 | 123456 | `401983bb07d69b07` | Pass |
| F.3 | 123456 | `4af38d834a9a8d3d` | Pass |
| F.4 | 987654321123456789 | `6a8a5514432e17a3` | Pass |

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
cargo run -p hashwx-core --release --example throughput -- 10000
```

Prints instance generation time in microseconds, elapsed hashing time, hashes per
second, and a checksum. The optional argument specifies the number of hashes.
