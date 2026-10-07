# Development

The workspace uses the stable Rust toolchain in `rust-toolchain.toml`, including
Clippy, rustfmt and the `wasm32-unknown-unknown` target. Node.js 24 is used by CI.

Pingora uses OpenSSL for upstream TLS. Native builds require a C/C++ toolchain,
CMake, pkg-config, and OpenSSL development headers. On Debian/Ubuntu install
`build-essential cmake clang pkg-config libssl-dev`; on macOS install
`cmake pkg-config openssl@3` with Homebrew. Linux runtime images need OpenSSL
shared libraries and CA certificates.

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

Gateway integration tests send requests through the real Pingora TCP listener,
including HTTP/2, proof and application WebSockets, streaming uploads, peer-IP
rate limiting, and graceful shutdown. Axum and Reqwest are test-only dependencies
used for mock origins and clients.

Gateway tests cover cross-instance proofs, token purpose, expiry, in-place
challenges, path normalization, overload, proxy failures, and response
streaming/header filtering. Mock P2Pool tests cover share acceptance, job
rotation, submission timeouts, connection reuse, unpooled fetches, and idle
session expiry; reader tests cover interrupted and oversized Stratum frames.
Signatures bind tokens to presentation, challenge, receipt, or authentication
purpose, even when signing keys are shared. Signatures from other token formats are rejected.

## Performance

CI runs `scripts/test.sh`; it does not run performance tests.

`scripts/perf-gate.sh` is an optional local check requiring `wrk`, Node.js,
and `curl`. It starts the bundled `scripts/perf/upstream.mjs` and generates a
lab configuration in `PERF_GATE_RUNDIR` (default `/tmp/rilgate-perf-gate`).
Gateway and upstream ports default to 8080 and 3300; override them with
`PERF_GATE_PORT` and `PERF_GATE_UPSTREAM_PORT`. The lab uses fixed test keys,
Fast difficulty 1 at `/protected`, and disabled proof rate limits.

The script validates endpoint responses, rejects HTTP/socket errors and zero
throughput, then checks log volume and challenge p50 relative to health p50.
Absolute throughput is reported but is not a pass/fail threshold.

```bash
scripts/build.sh --release
scripts/perf-gate.sh
```

Thresholds are overridable through `PERF_GATE_MAX_LOG_BYTES`,
`PERF_GATE_MAX_CHALLENGE_PCT`, `PERF_GATE_CONNECTIONS` and
`PERF_GATE_DURATION`. Setting `PERF_GATE_RUST_LOG` measures a non-default log
filter instead of the shipping one. Set `PERF_GATE_BINARY` to test a binary
from another Cargo target directory.

Tokio worker count can affect throughput and CPU per request. Compare worker
counts with the same load before choosing a value for a deployment:

```bash
TOKIO_WORKER_THREADS=4 scripts/perf-gate.sh
```

Set `TOKIO_WORKER_THREADS` in the process environment before starting Rilgate.
The runtime is created before `.env` is loaded, so setting this variable only
inside `.env` does not configure its workers. The default remains Tokio's CPU
count based setting; an optimal value on a development machine does not imply
an optimal value on a Linux deployment.

## Native RandomX reference checks

RandomX correctness is checked against the official reference vectors:

```bash
# cache, dataset items, AES generator, reciprocal, superscalar and full hashes
cargo test-randomx

# server-side verifier recomputing a reference vector
cargo test -p rilgate-pow-randomx --release --lib -- --ignored --test-threads=1
```
