# Development

The workspace uses the stable Rust toolchain in `rust-toolchain.toml`, including
Clippy, rustfmt and the `wasm32-unknown-unknown` target. Node.js 24 is used by CI.

Run commands from the repository root. Build with `scripts/build.sh` or `scripts/build.sh --release`. It creates the
browser WASM artifact before the gateway embeds it. Run `scripts/test.sh` for
the complete default checks, including WASM generation, formatting, Rust tests,
Clippy and Node tests. The native RandomX reference tests below are opt-in and
allocate substantial memory.

## Verification

Run the gateway integration tests, worker lifecycle tests, and static checks:

```bash
scripts/test.sh
```

The Node tests exercise worker messaging with test doubles and check the built
WASM module against a reference hash. They require Node.js with ES module,
WebAssembly, fetch, and structured-clone support. The WASM test allocates a
256 MiB cache.

Gateway tests cover cross-instance proofs, token purpose, expiry, in-place
challenges, overload, proxy failures, and response streaming/header filtering.
Mock P2Pool tests cover share acceptance, job rotation, and submission timeouts;
reader tests cover interrupted and oversized Stratum frames.
Signatures bind tokens to presentation, challenge, receipt, or authentication
purpose, even when signing keys are shared. Signatures from other token formats are rejected.

RandomX correctness is checked against the official reference vectors:

```bash
# cache, dataset items, AES generator, reciprocal, superscalar and full hashes
cargo test-randomx

# server-side verifier recomputing a reference vector
cargo test -p rilgate-pow-randomx --release --lib -- --ignored --test-threads=1
```
