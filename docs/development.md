# Development

The workspace uses the stable Rust toolchain in `rust-toolchain.toml`, including
Clippy, rustfmt and the `wasm32-unknown-unknown` target. Node.js 24 is used by CI.

Pingora uses OpenSSL for upstream TLS. Native builds require a C/C++ toolchain,
CMake, pkg-config, and OpenSSL development headers. On Debian/Ubuntu install
`build-essential cmake clang pkg-config libssl-dev`; on macOS install
`cmake pkg-config openssl@3` with Homebrew. Linux runtime images need OpenSSL
shared libraries and CA certificates.

Run commands from the repository root. Build with `scripts/build.sh` or
`scripts/build.sh --release`. The script creates the browser WASM artifact and
the generated ES5 resources before the gateway embeds them. Run `scripts/test.sh`
for the complete default checks, including WASM and web resource generation,
formatting, Rust tests, Clippy and Node tests, building incrementally in the
workspace target directory. Browser E2E tests
run with `scripts/test/browser.sh` or `npm --prefix tests/e2e run test:e2e`;
dependencies and reports stay in temporary directories.
The native RandomX reference tests below are opt-in and
allocate substantial memory.

## Browser resource generation

Edit readable sources under `web/`, then run:

```bash
cargo xtask build-web
```

The command writes `crates/rilgate-web/generated/`, which is ignored by Git and
embedded into the server binary. Run it before `cargo build` and after every
source change; `scripts/build.sh`, `scripts/test.sh`, the container build and CI
run it. A server built from stale outputs rejects the snapshot with a
`Stale generated web source` error.

The manifest records execution order, SHA-256 source digests and the pinned SWC
compiler version. The build lowers each startup member and the fast worker to
ES5 in a strict IIFE, with helpers inlined and no property mangling, and
validates the syntax of every output and of the assembled bundle. It requires
Rust and the xtask build dependencies; Node and a JavaScript compiler are not
runtime dependencies.

The fast worker source also registers the same time-sliced hashing implementation
in the page bundle for browsers without workers. It uses no Promise, so its
standalone worker output needs no Promise polyfill. The startup bundle installs
the local Apache-2.0 Promise compatibility implementation v1 only when Promise is
missing; its source digest pins it with the other members. RandomX's module loader,
solver and worker remain separate modern resources. The loader imports the
configured solver only after the classic adapter checks capabilities and consent.
Generated headers and `generated/NOTICE` retain helper license attribution.

`web init` and `web diff` use `web/` sources, never generated outputs. Operator
JavaScript overrides are concatenated as authored and must supply their own browser
compatibility. See [browser requirements](configuration.md#challenge-browser-support).

## Code organization

Typed YAML configuration and defaults live in `crates/rilgate-config/src/schema/`.
The loader parses and expands values, `assembly/resolve.rs` validates and resolves
immutable runtime settings, and `rilgate-policy` compiles request conditions.

Token signing lives in `crates/rilgate-crypto/src/token/`: `keys.rs` owns key
rotation configuration, `purpose.rs` defines protocol domains, and `signer.rs`
owns token encoding and verification. `mod.rs` keeps the public API stable;
unit tests live in `tests.rs`.

Gateway operational endpoints live under
`crates/rilgate-gateway/src/observability/`. Each endpoint has a `service.rs`;
readiness response types live in `readiness/dto.rs`, while request counters and
histograms live in `metrics/collector.rs`. The transport router dispatches to
these services.

Production verification runners live in `scripts/test/`. Browser scenarios and
Playwright configuration and npm manifests live together in `tests/e2e/`;
container proof helpers live in `tests/container/`. Dependency audit policy lives
in `.config/deny.toml`. Deployment examples and monitoring configuration live
under `deploy/`, with the operator guide in `docs/production.md`.

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

CI runs `scripts/test.sh`, real HTTPS browser E2E tests, native RandomX
reference tests, dependency checks and production container checks. See
[production operations](production.md) for the release gate and local commands.

Static response construction and body streaming can be measured separately:

```bash
cargo run --release --locked -p rilgate-web --example asset-throughput -- 200
```

This checks received byte counts and reports the median of five samples for
memory and immutable file assets at three sizes. See [asset performance](performance.md)
for measurements, design tradeoffs, and the scope of this benchmark.

The request policy and authentication gate has a separate benchmark:

```bash
cargo run --release --locked -p rilgate-gateway --example gate-throughput -- 200000
```

It reports median time for allowed and authenticated requests, plus allocation
counts collected separately from timing. It exercises the real gate and token
verification with test configuration and keys, without forwarding requests.
Run benchmarks without simultaneous builds or other load.

`scripts/perf-gate.sh` is an optional local check requiring `wrk`, Node.js,
and `curl`. It starts the bundled `scripts/perf/upstream.mjs` and generates a
lab configuration in `PERF_GATE_RUNDIR` (default `/tmp/rilgate-perf-gate`).
Gateway and upstream ports default to 8080 and 3300; override them with
`PERF_GATE_PORT` and `PERF_GATE_UPSTREAM_PORT`. The lab uses fixed test keys,
Fast difficulty 1 at `/protected`, and disabled proof rate limits. It completes
the proof and exchange protocol to obtain a real access cookie, then checks that
authenticated requests reach the upstream before and after load. It measures
health, challenge pages, authenticated proxy traffic, and allowed proxy traffic.

The script validates endpoint responses, rejects HTTP/socket errors and zero
throughput, then checks log volume and challenge p50 relative to health p50.
Absolute throughput is reported but is not a pass/fail threshold.

```bash
scripts/build.sh --release
scripts/perf-gate.sh
```

The operations listener used for health defaults to port 9090; override it with
`PERF_GATE_OPERATIONS_PORT`. Thresholds and load shape are overridable through
`PERF_GATE_MAX_LOG_BYTES`, `PERF_GATE_MAX_CHALLENGE_PCT`, `PERF_GATE_CONNECTIONS`,
`PERF_GATE_THREADS`, `PERF_GATE_DURATION` and `PERF_GATE_WARMUP`. Setting `PERF_GATE_RUST_LOG` measures a non-default log
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
