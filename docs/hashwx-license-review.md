# HashWX license and implementation review

Review date: 2026-10-24. Scope: `hashwx-core`, `hashwx-wasm`,
`rilgate-pow-hashwx` and the HashWX browser assets.

## Inputs and earlier fetch

Upstream HashWX is LGPL-3.0. The repository license allow list excludes LGPL.
The implementation in this repository uses the workspace Apache-2.0 license.

The implementation input is the published specification at revision
`fde6d2ae34dce266497aed5c093ff7b40a933c3a`. The published design document
supplies design context and author-reported performance. Its performance
figures are separate from the local measurements in proposal section 8.

The earlier review made one upstream-source retrieval with `curl`. Its recorded
URL is:

```text
https://raw.githubusercontent.com/tevador/hashwx/fde6d2ae34dce266497aed5c093ff7b40a933c3a/doc/specification.md
```

The fetched artifact was the specification document, stored outside the
repository. This is the single earlier upstream fetch recorded for this task.
No upstream implementation source was read or fetched during the continuation
that completed steps 3 to 5.

## Independent implementation

No upstream implementation code was copied, translated or adapted into this
repository. The implementation was written from the specification:

| Area | Input and implementation |
| --- | --- |
| Instance generator | Specification section 2 and Appendices A to D; Rust generator and fixed program buffers |
| Interpreter | Specification sections 1 and 3 and Appendix E; fixed VM memory and Rust integer operations |
| WASM generator | The VM operations above, encoded directly as WebAssembly instructions by the repository's binary encoder |
| Conformance | The four data vectors in specification Appendix F; native and actual WASM checks |
| Raw WASM ABI | Repository-owned handle, buffer and u32-half interfaces |
| Browser solver | Repository-owned worker scheduling, seed batches, progress, cancellation and fallback |
| Backend and protocol | The Rilgate proposal's signed challenge, nonce encoding, seed derivation and difficulty rule |

Opcode templates and source permutations are specification data from
Appendices B and D. Appendix F vectors are used for conformance checks.
The implementation includes no upstream native compiler, interpreter, WASM
emitter, source file or binary.

The core crate forbids unsafe Rust and has no third-party dependencies.
The raw ABI uses unsafe pointer operations with documented caller contracts.
The WebAssembly 1.0 build uses `no_std` and the `dlmalloc` allocator.
A development-only `wasmparser` validator checks the staged artifact.
Seed hashing uses the workspace `sha2` dependency. The backend uses the local
core, shared Rilgate contracts, `sha2`, `getrandom`, `uuid`, `hex`, `tokio` and
`anyhow`. No upstream HashWX crate or LGPL component is added to this graph.
The generated WASM comes from this implementation.

## Review evidence

- The core README pins the specification and records all four Appendix F vectors.
- Native, WASM interpreter and generated WASM conformance tests pass.
- Differential tests compare 4096 nonces across 64 seeds in an actual WASM runtime.
- Worker tests check the signed SHA-256 batch seed against an independent digest.
- Chromium, Firefox and WebKit solve through the gateway and the native verifier.
- Workspace license declarations for all three HashWX crates are Apache-2.0.
- `cargo deny --locked --config .config/deny.toml check licenses sources bans`
  checks the resolved dependency graph. Its final result is recorded with the
  delivery checks. The existing LGPL exclusion is retained.

The implementation provenance rule remains in force for later changes: use
the specification and published design document. Preserve the input revision
and conformance record when the specification changes.

## Delivery checks

Checks completed on 2026-10-24:

- `cargo xtask build-wasm && cargo xtask build-web`: pass. The HashWX artifact
  is built for `wasm32v1-none` and passes strict WebAssembly 1.0 validation.
- `scripts/test.sh`: pass, including format, workspace tests, Clippy with
  `-D warnings` and all 63 Node tests.
- Playwright: all 45 cases pass in Chromium, Firefox and WebKit. This includes
  12 HashWX cases for generated code, interpreter fallback, worker CSP and the
  gateway authentication flow. The first full run had one Chromium gateway
  readiness timeout during the build; the complete repeat passed in 9.4 seconds.
- `cargo deny --locked --config .config/deny.toml check licenses sources bans`:
  pass. No LGPL dependency is added.
- `scripts/advisories.sh`: pass. No new advisory is reported. Existing tracked
  advisories are `RUSTSEC-2024-0388` (`derivative`, issue 15) and
  `RUSTSEC-2024-0449` (`unsafe-libyaml`, issue 16). No ignore was added.

The mid-range phone measurement remains open. The iPhone 15 Pro Max is a
high-end device, so the difficulty defaults remain placeholders.
