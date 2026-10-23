# More browser proof-of-work algorithms

Status: proposed.

This proposal adds proof-of-work algorithms for browser challenges next to Fast
and RandomX. It selects Argon2id as the next algorithm and PBKDF2-SHA256 as an
optional one. It adds Equi-X, built on HashX, through a new Apache-2.0
implementation in this repository. It rejects scrypt and mining algorithms. Each
new algorithm follows the contract in
[adding an algorithm](../algorithms.md).

## 1. Problem

| Algorithm | Strength | Limit |
| --- | --- | --- |
| Fast (SHA-256) | Runs on every supported browser, including IE11 | The browser solver is pure JavaScript (`web/static/js/solvers/fast/worker.js`). A native or GPU solver computes many more hashes per second. Thus Fast costs a bot much less than a person |
| RandomX | CPU-oriented and memory-hard | It needs P2Pool for every job, about 256 MiB in the browser, and a 256 MiB cache on the verifier |

Rilgate has no memory-hard algorithm without external infrastructure. An operator
who wants a stronger challenge than Fast must operate P2Pool.

## 2. Requirements for a new algorithm

1. Server verification has a fixed and bounded cost per proof.
2. Server verification starts only after the challenge token signature, version,
   lifetime and host checks pass. `rilgate-challenge` already applies this order.
3. The algorithm needs no external service.
4. The license is in the allow list of `.config/deny.toml`: Apache-2.0, MIT, BSD,
   ISC, Zlib or a similar permissive license.
5. The browser solver runs in a worker, shows progress and supports cancellation.
6. The parameters let an operator set the solve time for a mid-range phone.

## 3. Candidates

| Candidate | Memory-hard | Browser | Rust crate and license | Decision |
| --- | --- | --- | --- | --- |
| Argon2id ([RFC 9106](https://datatracker.ietf.org/doc/rfc9106/)) | Yes | WASM | `argon2` (RustCrypto), MIT or Apache-2.0 | P1: add |
| PBKDF2-SHA256 | No | Native WebCrypto | `pbkdf2` (RustCrypto), MIT or Apache-2.0 | P2: optional |
| scrypt | Yes | WASM | `scrypt` (RustCrypto), MIT or Apache-2.0 | Rejected: Argon2id covers the same role |
| Equi-X on HashX ([Tor spec](https://spec.torproject.org/hspow-spec/v1-equix.html)) | Partly | WASM | `hashx` and `equix` from Arti are LGPL-3.0-only. A new Apache-2.0 implementation | P1: add, own implementation (section 7) |
| SHA-256d, Equihash, KawPoW, Etchash, kHeavyHash | Varies | Poor | Varies | Rejected: designed for mining hardware, and solve times do not suit web access |

Other projects use similar choices. For example, the
[ALTCHA documentation](https://altcha.org/docs/integration/proof-of-work-captcha/)
describes PBKDF2, scrypt and Argon2id variants.

## 4. Argon2id

### Work and proof

```text
salt     = challenge             (32 random bytes, hex in the protocol)
password = nonce                 (decimal string)
hash     = Argon2id(password, salt, m, t, p, length = 32)
valid    = hash has at least `difficulty` leading zero bits
```

- Difficulty uses leading zero bits, which gives finer steps than the hexadecimal
  digits of Fast.
- The expected number of evaluations is `2^difficulty`. Each evaluation costs `m` KiB of
  memory and `t` passes over that memory.
- The challenge claims sign `m`, `t`, `p` and `difficulty`. A client cannot change
  them.

### Configuration

```yaml
pow:
  algorithms:
    argon2id:
      memory: 16MiB
      iterations: 1
      parallelism: 1
      verifier_workers: 2
      verifier_queue: 32
      difficulty:
        base: 4
        score_step: 10
        max: 12
```

| Setting | Default | Range |
| --- | --- | --- |
| `memory` | `16MiB` | `1MiB` to `64MiB` |
| `iterations` | `1` | `1` to `8` |
| `parallelism` | `1` | `1` to `4` |
| `verifier_workers` | `2` | `1` to `64` |
| `verifier_queue` | `32` | `1` to `1024` |
| `difficulty.base`, `.max` | `4`, `12` | `1` to `24` leading zero bits |

A policy selects it with `challenge: {algorithm: argon2id, difficulty: 6}`. Without
an explicit difficulty, the score maps to bits with the same `base`, `score_step`
and `max` rule as RandomX, on a logarithmic scale.

### Server cost and denial of service

- One verification costs one Argon2id evaluation with the signed parameters.
- Verification runs on `verifier_workers` dedicated threads, off Tokio workers. The
  peak memory is `verifier_workers × memory`, which is 32 MiB with the defaults.
- A full queue returns `503` as `CapacityExceeded`.
- The proof API rate limits apply before verification.
- The `64MiB` memory limit and the worker count bound the memory that a burst of
  proofs can use.
- A new environment check warns in production when `verifier_workers × memory`
  exceeds `512MiB`.

### Browser

- A new `argon2-wasm` crate builds the RustCrypto `argon2` crate for
  `wasm32-unknown-unknown`. `cargo xtask build-wasm` stages it next to the RandomX
  module.
- The solver runs in a worker, as the RandomX solver does. It reports progress
  after each evaluation and supports cancellation.
- Argon2id needs WebAssembly and workers. The classic adapter in the startup bundle
  checks these capabilities. On an incapable browser, it shows the localized
  failure and retry controls. The client never switches algorithms.
- Memory per evaluation is `memory`. The default of 16 MiB suits older phones.

### Variance

With a small `2^difficulty`, the solve time varies widely. For example, with an
expected 32 evaluations, about 5 percent of solves take more than three times the
mean. An optional `solutions` setting (`1` to `8`, default `1`) asks for several
independent nonces at a lower difficulty each. More solutions reduce the variance
and multiply the server cost by the same number.

### Native and browser gap

A memory-hard function reduces the advantage of GPU and ASIC solvers, because
memory bandwidth limits each evaluation. A native CPU solver stays faster than WASM.
The gap is much smaller than for pure JavaScript SHA-256. Measure both before you
set the defaults (section 8).

## 5. PBKDF2-SHA256 (optional)

```text
hash  = PBKDF2-HMAC-SHA256(password = nonce, salt = challenge, iterations, length = 32)
valid = hash has at least `difficulty` leading zero bits
```

- The browser uses `crypto.subtle.deriveBits`, which runs native code. This reduces
  the CPU gap between a browser and a native solver for the SHA-256 family. The GPU
  gap stays, because PBKDF2 is not memory-hard.
- WebCrypto exists only in secure contexts. The challenge needs HTTPS or
  `localhost`, and production already requires HTTPS.
- IE11 has only the prefixed, event-based `msCrypto` API. PBKDF2 is a modern-only
  algorithm and cannot replace Fast as the default.
- Server cost is `iterations` HMAC-SHA256 operations per proof. A limit of `100000`
  iterations keeps one verification in the low milliseconds.

PBKDF2 is P2. Add it only when operators ask for a challenge between Fast and
Argon2id that needs no WASM.

## 6. Capability fallback (optional)

A browser that cannot run a WASM algorithm cannot pass its challenge. An operator
can allow an explicit fallback:

```yaml
challenge:
  algorithm: argon2id
  difficulty: 6
  fallback:
    algorithm: fast
    difficulty: 6
```

- The presentation token signs both the primary and the fallback challenge.
- An incapable browser requests the fallback. The server issues it only when the
  presentation token allows it.
- The cookie records the algorithm and difficulty that the browser solved. A
  later policy check applies the current rules to that record.
- **Security cost:** a bot can claim that it is incapable and solve the cheaper
  fallback. The fallback lowers the protection of the rule to the fallback level.
  Thus the default is no fallback, and `config check` reports a warning for each
  rule with a fallback.

## 7. Equi-X and HashX

### Why Equi-X

Equi-X is the puzzle of the Tor onion service proof-of-work. It suits Rilgate for
these reasons:

- Verification is cheap. Per the Tor specification, one check generates one HashX
  program and evaluates it eight times, then computes one Blake2b hash. Argon2id
  verification costs one full memory-hard evaluation.
- A solve uses a small, fixed amount of memory, about 2 MiB per the specification.
- HashX generates a new random program for each seed. This favors general-purpose
  CPUs over fixed hardware.

### License and implementation rule

The Rust `hashx` and `equix` crates from Arti are LGPL-3.0-only. Rilgate ships one
static binary under Apache-2.0, and `.config/deny.toml` does not allow LGPL. Thus
Rilgate gets its own implementation under Apache-2.0:

- Write the code from the published specifications and algorithm descriptions.
- Do not copy, translate or adapt code from the LGPL implementations.
- Use published test vectors only to check conformance. Test vectors are data, and
  the implementation does not include LGPL code.
- Record the specification sources and the conformance results in each crate
  README. Get a license review before the first release that includes the crates.

HashX is a lighter variant of SuperscalarHash. `randomx-core` already contains an
Apache-2.0 SuperscalarHash generator and seed-specialized WASM kernels. The new
crates reuse these techniques and keep their own code paths, because HashX defines
its own instruction set and program rules.

### Crates

| Crate | Role |
| --- | --- |
| `crates/hashx-core` | `no_std` program generator and interpreter. Optional native code generation later |
| `crates/equix-core` | `no_std` Equi-X solver and verifier on `hashx-core` |
| `crates/equix-wasm` | Raw `wasm32-unknown-unknown` ABI for the browser solver, staged by `cargo xtask build-wasm` |
| `crates/rilgate-pow-equix` | `rilgate_pow::Algorithm` backend |

### Work and proof

The scheme follows Tor v1 so that published vectors apply:

```text
challenge  = 32 random bytes, signed in the challenge claims
seed       = challenge || nonce
solution   = 8 indices from Equi-X(seed), 16 bytes
valid      = Equi-X verifies (seed, solution)
             and Blake2b-32(challenge || nonce || solution) × effort ≤ 2^32 − 1
```

- `effort` is the difficulty. The expected number of Equi-X solves is about
  `effort`.
- The proof is the nonce and the solution. The server never solves.
- Policy selects it with `challenge: {algorithm: equix, difficulty: 64}`.

### Configuration

```yaml
pow:
  algorithms:
    equix:
      difficulty:
        base: 16
        score_step: 10
        max: 4096
```

Verification is cheap, so Equi-X needs no dedicated verifier pool. It runs on a
blocking task with the proof API rate limits in front.

### Browser

- The solver needs WebAssembly and workers. The classic adapter checks these
  capabilities, as for Argon2id.
- HashX needs a new program for each nonce. The WASM solver interprets the program
  first. Code generation of a WASM function for each program is an optimization to
  measure. `randomx-core` uses this technique for SuperscalarHash.
- A native solver can compile each program to machine code, so it is faster than a
  WASM interpreter. Section 8 measures this gap.

### Conformance

- `hashx-core` matches the published HashX test vectors.
- `equix-core` matches the published Equi-X vectors for solutions and verification.
- The full proof check matches the Tor v1 vectors, including the Blake2b effort
  check.
- Native and WASM builds give the same results for the same vectors.
- A fuzz target checks that the verifier rejects random solutions and never panics.

## 8. Measurement before defaults

Before the defaults are fixed, measure these values on a mid-range phone, a
desktop browser, and a native solver on a server CPU:

- Argon2id evaluations per second for `memory` of 4, 16 and 64 MiB.
- Equi-X solves per second with the WASM interpreter, with WASM code generation, and
  with a native solver.
- PBKDF2 evaluations per second for 10000 and 100000 iterations.
- Fast evaluations per second, as the baseline.
- Server verification time and peak memory for each algorithm.

Set the default difficulty so that the median solve on the phone takes 1 to 3
seconds. Record the ratio between the native solver and the phone for each
algorithm in `docs/performance.md`.

## 9. Integration

Each algorithm follows [adding an algorithm](../algorithms.md):

| Part | Argon2id | Equi-X |
| --- | --- | --- |
| Backend crate | `crates/rilgate-pow-argon2` with a bounded verifier pool | `crates/rilgate-pow-equix` on `equix-core` |
| Protocol | `PowAlgorithm::Argon2id` with `m`, `t`, `p` and `difficulty` | `PowAlgorithm::Equix` with `challenge` and `effort`, and a proof with a nonce and a solution |
| Mapping | `rilgate-challenge/src/challenge/work.rs` | Same |
| Registration | `crates/rilgate/src/app/bootstrap.rs` | Same |
| Configuration | `pow.algorithms.argon2id` | `pow.algorithms.equix` |
| Browser | `web/static/js/solvers/argon2id/` with a classic adapter in `STARTUP`, a modern solver and worker, and the WASM module | `web/static/js/solvers/equix/` with the same structure |
| Required resources | The served files in `crates/rilgate-web/src/runtime/snapshot.rs` |
| Metrics | The `rilgate_verifier_*` diagnostics with an `algorithm` label |
| Documentation | `docs/pow.md`, `docs/policy.md`, `docs/configuration.md` and `docs/performance.md` |

Cookie rules do not change. A `CHALLENGE` rule accepts a cookie only when its
algorithm and enforcement match and its difficulty is at least the required one.

## 10. Verification

| Area | Tests |
| --- | --- |
| Conformance | Argon2id matches the RFC 9106 test vectors in native Rust and in the WASM module. Equi-X conformance is in section 7 |
| Proof | Valid proofs pass. Changed parameters, a short hash, a lower difficulty and an expired challenge fail |
| Order of checks | An invalid token signature fails without an Argon2id evaluation |
| Capacity | A full verifier queue returns `503`. Peak memory stays within `verifier_workers × memory` |
| Browser | The Playwright suite solves an Argon2id challenge in Chromium, Firefox and WebKit. An incapable browser shows the failure controls |
| Fallback | A presentation token without a fallback cannot get a fallback challenge |

## 11. Delivery sequence

1. Measurements in section 8 with prototype solvers.
2. Argon2id backend, protocol variants, configuration and verifier pool.
3. Argon2id browser solver, WASM build and capability checks.
4. `hashx-core` with conformance tests, then `equix-core` with conformance and fuzz
   tests.
5. Equi-X backend, protocol variants, `equix-wasm` and the browser solver.
6. Documentation and the environment check for verifier memory.
7. Optional: PBKDF2-SHA256, when operators ask for it.
8. Optional: capability fallback, when operators ask for it.

Argon2id comes first because a maintained permissive crate exists. Equi-X needs
the new HashX and Equi-X implementations and their conformance work.
