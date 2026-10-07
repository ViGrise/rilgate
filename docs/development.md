# Development

The workspace uses the stable Rust toolchain in `rust-toolchain.toml`, including
Clippy, rustfmt and the `wasm32-unknown-unknown` target. Node.js 24 is used by CI.

Run commands from the repository root. Build with `scripts/build.sh` or
`scripts/build.sh --release`. The script creates the browser WASM artifact
before the gateway embeds it. Run `scripts/test.sh` for
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
challenges, path normalization, overload, proxy failures, and response
streaming/header filtering. Mock P2Pool tests cover share acceptance, job
rotation, submission timeouts, connection reuse, unpooled fetches, and idle
session expiry; reader tests cover interrupted and oversized Stratum frames.
Signatures bind tokens to presentation, challenge, receipt, or authentication
purpose, even when signing keys are shared. Signatures from other token formats are rejected.

## Performance

CI runs `scripts/test.sh`; it does not run performance tests.

`scripts/perf-gate.sh` is an optional local check. It requires `wrk`, Node.js,
`curl`, and two local fixtures: `loadtest/upstream.mjs` and
`loadtest/rilgate.loadtest.yml`. The `loadtest/` directory is not distributed in
Git, so this script cannot run from a fresh clone without those fixtures.
The configuration must start a Fast challenge at `/protected`, serve health
under the default namespace, and use the same ports as the script (gateway
8080 and upstream 3300 by default).

With those fixtures in place, the script starts the release gateway with its
default log filter and drives the health and challenge endpoints. It checks
total log bytes during the load run and challenge p50 latency relative to
health p50. Absolute throughput is reported but is not a pass/fail threshold.

```bash
scripts/build.sh --release
scripts/perf-gate.sh
```

Thresholds are overridable through `PERF_GATE_MAX_LOG_BYTES`,
`PERF_GATE_MAX_CHALLENGE_PCT`, `PERF_GATE_CONNECTIONS` and
`PERF_GATE_DURATION`. Setting `PERF_GATE_RUST_LOG` measures a non-default log
filter instead of the shipping one.

## Native RandomX reference checks

RandomX correctness is checked against the official reference vectors:

```bash
# cache, dataset items, AES generator, reciprocal, superscalar and full hashes
cargo test-randomx

# server-side verifier recomputing a reference vector
cargo test -p rilgate-pow-randomx --release --lib -- --ignored --test-threads=1
```
