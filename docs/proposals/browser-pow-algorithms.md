# More browser proof-of-work algorithms

Status: HashWX steps 1 to 5 are implemented. Difficulty defaults await a
mid-range phone measurement. Optional algorithms and fallback remain proposed.

This proposal adds proof-of-work algorithms for browser challenges next to Fast
and RandomX. It selects HashWX as the next algorithm, through a new Apache-2.0
implementation in this repository. Argon2id and PBKDF2-SHA256 are optional. It
rejects HashX, Equi-X, scrypt and mining algorithms. Each new algorithm follows
the contract in [adding an algorithm](../algorithms.md).

## 1. Problem

| Algorithm | Strength | Limit |
| --- | --- | --- |
| Fast (SHA-256) | Runs on every supported browser, including IE11 | The browser solver is pure JavaScript (`web/static/js/solvers/fast/worker.js`). A native or GPU solver computes many more hashes per second. Thus Fast costs a bot much less than a person |
| RandomX | CPU-oriented and memory-hard | It needs a Stratum pool for every job, about 256 MiB in the browser, and a 256 MiB cache on the verifier |

Rilgate has no GPU-resistant algorithm without external infrastructure. An
operator who wants a stronger challenge than Fast must configure a Stratum pool.

## 2. Requirements for a new algorithm

1. Server verification has a fixed and bounded cost per proof.
2. Server verification starts only after the challenge token signature, version,
   lifetime and host checks pass. `rilgate-challenge` already applies this order.
3. The algorithm needs no external service.
4. The license is in the allow list of `.config/deny.toml`: Apache-2.0, MIT, BSD,
   ISC, Zlib or a similar permissive license.
5. The browser solver runs in a worker, shows progress and supports cancellation.
6. The parameters let an operator set the solve time for a mid-range phone.
   Default calibration must use a mid-range phone.

## 3. Candidates

The GPU advantage column comes from the
[HashWX design document](https://github.com/tevador/hashwx/blob/master/doc/design.md),
which compares a 16-thread Ryzen 3700X with WASM or JavaScript solvers to an RTX
5060 Ti. Rilgate did not measure these values (section 8).

| Candidate | GPU advantage | Server verification | Rust code and license | Decision |
| --- | --- | --- | --- | --- |
| [HashWX](https://github.com/tevador/hashwx) | about 2x | Under 10 µs per the design | Upstream code is LGPL-3.0. A new Apache-2.0 implementation | P1: add, own implementation (section 4) |
| Argon2id ([RFC 9106](https://datatracker.ietf.org/doc/rfc9106/)) | about 20x | One memory-hard evaluation | `argon2` (RustCrypto), MIT or Apache-2.0 | P2: optional (section 5) |
| PBKDF2-SHA256 | High, not memory-hard | `iterations` HMAC operations | `pbkdf2` (RustCrypto), MIT or Apache-2.0 | P3: optional (section 6) |
| HashX and Equi-X | about 400x for HashX | Cheap | `hashx` and `equix` from Arti are LGPL-3.0-only | Rejected: HashWX supersedes HashX |
| scrypt | Not measured | One memory-hard evaluation | `scrypt` (RustCrypto), MIT or Apache-2.0 | Rejected: Argon2id covers the same role |
| SHA-256d, Equihash, KawPoW, Etchash, kHeavyHash | Varies | Varies | Varies | Rejected: designed for mining hardware, and solve times do not suit web access |

The HashWX design document lists these problems of HashX:

- Some HashX instances are weak and return hashes that almost do not depend on the input.
- HashX has too few branches to slow JIT-compiled GPU solvers.
- HashX uses no memory, so GPU interpreters reach full occupancy.
- The instruction set suits x86. WASM performance is poor because of 128-bit multiplications.
- Program generation can fail.

## 4. HashWX

### Properties

These values come from the HashWX README and design document:

- Each seed generates a unique function of 32 random VM programs, with integer
  operations and branches.
- The VM has 16392 bytes of memory. A hash writes all of it and then reads it.
- Generation of a function takes about 20000 CPU cycles. A hash takes under 10000
  cycles in compiled mode. Interpreted mode is about 5 times slower.
- WebAssembly with a JIT reaches about 60 percent of native performance.
- The output is a uniformly distributed 64-bit value with 64 bits of preimage
  resistance. HashWX is not a general hash function, and collision resistance is
  not a design goal.

### License and implementation rule

The upstream code is LGPL-3.0. Rilgate ships one static binary under Apache-2.0, and
`.config/deny.toml` does not allow LGPL. Thus Rilgate gets its own implementation
under Apache-2.0:

- Write the code from the published
  [specification](https://github.com/tevador/hashwx/blob/master/doc/specification.md).
- Do not copy, translate or adapt code from the upstream implementation.
- Use the test vectors in Appendix F of the specification only to check
  conformance.
- Record the specification revision and the conformance results in the crate
  README. The [license review](../hashwx-license-review.md) records the inputs
  and the earlier upstream fetch. No upstream implementation code was copied.

### Crates

| Crate | Role |
| --- | --- |
| `crates/hashwx-core` | `no_std` instance generator, interpreter and WASM code generator |
| `crates/hashwx-wasm` | Raw `wasm32v1-none` ABI for the browser solver, staged by `cargo xtask build-wasm` |
| `crates/rilgate-pow-hashwx` | `rilgate_pow::Algorithm` backend |

The server verifies with the interpreter. At about 5 times the compiled cost, one
verification stays in the tens of microseconds. A native code generator is not
necessary on the server.

### Work and proof

The scheme follows the minimal protocol in the HashWX README:

```text
challenge  = 32 random bytes, signed in the challenge claims
batch      = nonce / nonces_per_seed            (integer division)
seed       = SHA-256(challenge || batch as u64 little-endian)
hash       = HashWX(seed)(nonce)               (64-bit result)
valid      = hash < floor((2^64 - 1) / difficulty)
```

- The README does not fix the byte encoding of the batch number. The
  little-endian `u64` encoding above is the Rilgate definition.
- The expected number of hashes is `difficulty`.
- `nonces_per_seed` is signed in the claims. Its default is `65536`, the README
  value for browsers. The browser compiles one function for each batch of nonces.
- The proof is the 64-bit nonce. The server generates one function and computes one
  hash.
- Policy selects it with `challenge: {algorithm: hashwx, difficulty: 200000}`.

### Configuration

```yaml
pow:
  algorithms:
    hashwx:
      nonces_per_seed: 65536
      difficulty:
        base: 50000
        score_step: 10
        max: 5000000
```

| Setting | Default | Range |
| --- | --- | --- |
| `nonces_per_seed` | `65536` | `463` to `1048576` |
| `difficulty.base` | `50000` | `1` to `2^40` |
| `difficulty.max` | `5000000` | `difficulty.base` to `2^40` |
| `difficulty.score_step` | `10` | Positive i32 |

Without an explicit difficulty, the score maps to a difficulty with the same
`base`, `score_step` and `max` rule as RandomX. The default values are
placeholders until a mid-range phone measurement is recorded in section 8.
Explicit rule difficulties override this mapping, including its max cap.

Verification is cheap, so HashWX needs no dedicated verifier pool. It runs on a
blocking task with the proof API rate limits in front.

### Browser solver

- For each batch, `hashwx-core` generates the 32 programs and emits a WASM module.
  The worker compiles the module with `WebAssembly.instantiate` and hashes the
  batch. The RandomX worker already instantiates generated modules in the same way
  (`web/static/js/solvers/randomx/randomx-worker.js`).
- If the compilation of a generated module fails, the worker uses the interpreter.
- The solver uses up to two workers by default, with distinct batches. It reports
  progress after each batch and supports cancellation.
- HashWX starts without a consent step. Each VM uses 16 KiB, and the default
  difficulty is a placeholder for a future mid-range target of 1 to 3 seconds. RandomX keeps its consent step,
  because it allocates about 256 MiB and can run for much longer.

### Browser requirements

| Requirement | Rule |
| --- | --- |
| WebAssembly 1.0 | Required. SIMD is not required. The classic adapter checks `WebAssembly.instantiate` before it loads the solver |
| Workers | Required. The adapter checks `Worker` |
| JavaScript modules | Required. Solver assets use ES2020 syntax and load after the classic adapter |
| BigInt | Not required. The `hashwx-wasm` ABI passes each 64-bit nonce and hash as two 32-bit values |
| Memory | 16 KiB per VM, plus the WASM module and the generated code |
| Runtime compilation | Required for full speed. Rilgate sends only `frame-ancestors 'self'` as its content security policy, so it does not block generated modules |

On an incapable browser, the adapter shows the localized failure and retry
controls. The client never switches algorithms. Section 7 describes the optional
fallback.

Known limits:

- IE11 and other browsers without WebAssembly cannot solve HashWX challenges.
- Safari Lockdown Mode disables WebAssembly, so it cannot solve HashWX challenges.
- An edge CSP must permit worker creation and WASM compilation in the worker's
  own policy. If a generated module fails after the interpreter loads, the
  worker uses that interpreter. A worker policy denying all WASM also blocks
  the interpreter. See `docs/web.md` and `docs/production.md`.
- Phones have fewer cores and lower speed per core than the desktop in the
  published benchmark. Section 8 records a high-end phone. A mid-range phone is still required
  to set the default difficulty.
- Module compilation costs time for each batch. The default `nonces_per_seed` of
  65536 spreads this cost over many hashes.
- Long solves use battery and heat on phones. `difficulty.max` caps mapped work. Explicit policy difficulties
  override that mapping.

### Conformance

- `hashwx-core` matches every test vector in Appendix F of the specification, in
  the interpreter and in the generated WASM.
- Native and WASM builds give the same hash for the same seed and nonce.
- A property test compares the interpreter and the generated WASM for random seeds
  and nonces.
- A fuzz target checks that generation and hashing never panic for any seed.

## 5. Argon2id (optional)

Argon2id is memory-hard, and a maintained permissive crate exists. Its GPU
advantage is about 20x in the HashWX benchmark, and server verification costs one
full evaluation. Add it when operators ask for an algorithm from a standard.

```text
hash  = Argon2id(password = nonce, salt = challenge, m, t, p, length = 32)
valid = hash has at least `difficulty` leading zero bits
```

| Setting | Default | Range |
| --- | --- | --- |
| `memory` | `16MiB` | `1MiB` to `64MiB` |
| `iterations` | `1` | `1` to `8` |
| `parallelism` | `1` | `1` to `4` |
| `verifier_workers` | `2` | `1` to `64` |
| `verifier_queue` | `32` | `1` to `1024` |
| `difficulty.base`, `.max` | `4`, `12` | `1` to `24` leading zero bits |

- The challenge claims sign `m`, `t`, `p` and `difficulty`.
- Verification runs on `verifier_workers` dedicated threads. The peak memory is
  `verifier_workers × memory`. A full queue returns `503` as `CapacityExceeded`.
- A new environment check warns in production when `verifier_workers × memory`
  exceeds `512MiB`.
- With an expected 32 evaluations, about 5 percent of solves take more than three
  times the mean. An optional `solutions` setting (`1` to `8`) asks for several
  nonces at a lower difficulty each. It reduces the variance and multiplies the
  server cost by the same number.
- The browser module comes from the RustCrypto `argon2` crate, built for
  `wasm32-unknown-unknown`.

## 6. PBKDF2-SHA256 (optional)

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

Add it only when operators ask for a challenge without WASM between Fast and
HashWX.

## 7. Capability fallback (optional)

A browser that cannot run a WASM algorithm cannot pass its challenge. An operator
can allow an explicit fallback:

```yaml
challenge:
  algorithm: hashwx
  difficulty: 200000
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

## 8. Measurement before defaults

### Desktop measurements, 2026-10-23

Measured on an Apple M4 Pro Mac, macOS 26.4.1 (25E253), arm64. The browser
runs used one worker. Chrome was 155.0.8059.86 in headless mode, driven by
Playwright. Safari was 26.4 (21624.1.16.11.4), run in the Safari application.
Their reported `navigator.hardwareConcurrency` values were 12 and 8,
respectively. These values are browser reports, not physical core counts.
The native run used Rust 1.98.0 (88d9e12ae), with the release profile and one
thread. All builds used this worktree's `target` directory.

The prototype uses deterministic xorshift32 seeds. A seed produces one HashWX
instance. The native and browser runs use the same seeds, nonce ranges and
strict `hash < floor((2^64 - 1) / difficulty)` target. Every browser solve
matched the native attempt count and batch count. This measures calculation,
instance generation and compilation. It excludes SHA-256 challenge seed
derivation, challenge issuance, proof submission and token checks. It is not
a measurement of the complete server challenge protocol.

Each throughput row averages three seeds after one browser warmup seed.
The zero target forces each search to exhaust its nonce range. Native hashing
also has a warmup. Hash-only rates exclude setup. Total rates include instance
setup and, for generated WASM, compilation and instantiation. The browser
interpreter setup also emits the generated module. Initial artifact download
and instantiation are separate in the JSON results. The staged artifact was
43550 bytes. Browser clock precision can round short setup times to zero.
The recorded Safari run used a raised benchmark window. Another Safari run,
whose page focus was not recorded, gave 112155 H/s for generated WASM at
65536 nonces per seed and 82228 H/s for Fast. The variation is a reason to
keep the benchmark page in the foreground and repeat measurements before
selecting defaults.

| Runtime | Nonces per seed | Hash-only H/s | Total H/s | Compile and instantiate ms per seed |
| --- | ---: | ---: | ---: | ---: |
| Chrome, WASM interpreter | 463 | 64009 | 63716 | 0.000 |
| Chrome, generated WASM | 463 | 163412 | 154333 | 0.133 |
| Chrome, WASM interpreter | 65536 | 65001 | 64997 | 0.000 |
| Chrome, generated WASM | 65536 | 323901 | 323475 | 0.133 |
| Safari, WASM interpreter | 463 | 66143 | 66143 | 0.000 |
| Safari, generated WASM | 463 | 106846 | 92600 | 0.667 |
| Safari, WASM interpreter | 65536 | 69917 | 69917 | 0.000 |
| Safari, generated WASM | 65536 | 307681 | 306721 | 0.667 |
| Native interpreter | 463 | 71608 | 71576 | Not applicable |
| Native interpreter | 65536 | 71225 | 71224 | Not applicable |
| iPhone 15 Pro Max, WASM interpreter | 463 | 55560 | 55560 | 0.000 |
| iPhone 15 Pro Max, generated WASM | 463 | 69450 | 66143 | 0.333 |
| iPhone 15 Pro Max, WASM interpreter | 65536 | 62814 | 62814 | 0.000 |
| iPhone 15 Pro Max, generated WASM | 65536 | 387024 | 386263 | 0.333 |

For the Fast baseline, the browser worker ran the existing JavaScript SHA-256
function on `benchmark` plus a decimal nonce. After 1000 warmup hashes, 65536
hashes gave 502192 H/s in Chrome and 352344 H/s in Safari.
The results include a checksum of the computed hashes.

A native loop generated an instance and computed one hash for each of 10000
seed/nonce pairs. Its mean was 25.903 microseconds in the 463 run and 25.885
microseconds in the 65536 run. This excludes challenge seed derivation and the
proof API. It is the measured interpreter verification kernel cost, not a
server latency claim. No separate server CPU or native code generator was
measured.

### Solve times

Each cell below is the median elapsed time in milliseconds for three fixed
trials. The JSON files contain each trial's time, attempt count and batch
count. Times include generation, module compilation and range search. Browser
search calls cover at most 256 nonces. The initial artifact load is excluded.
The prototype uses the same deterministic trials on each runtime, so it is
useful for comparisons. Three trials do not establish a population median;
random proof lengths can make a higher difficulty finish sooner in this sample.

| Runtime | Nonces per seed | Difficulty 1000 | Difficulty 10000 | Difficulty 50000 |
| --- | ---: | ---: | ---: | ---: |
| Chrome, WASM interpreter | 463 | 7.7 | 676.7 | 527.2 |
| Chrome, generated WASM | 463 | 3.5 | 275.2 | 219.2 |
| Chrome, WASM interpreter | 65536 | 16.4 | 118.6 | 594.4 |
| Chrome, generated WASM | 65536 | 6.0 | 25.4 | 122.6 |
| Safari, WASM interpreter | 463 | 8.0 | 638.0 | 488.0 |
| Safari, generated WASM | 463 | 6.0 | 403.0 | 312.0 |
| Safari, WASM interpreter | 65536 | 15.0 | 110.0 | 557.0 |
| Safari, generated WASM | 65536 | 10.0 | 31.0 | 131.0 |
| Native interpreter | 463 | 6.895 | 615.497 | 480.221 |
| Native interpreter | 65536 | 14.985 | 108.836 | 547.617 |
| iPhone 15 Pro Max, WASM interpreter | 463 | 8.0 | 688.0 | 536.0 |
| iPhone 15 Pro Max, generated WASM | 463 | 9.0 | 666.0 | 589.0 |
| iPhone 15 Pro Max, WASM interpreter | 65536 | 17.0 | 125.0 | 624.0 |
| iPhone 15 Pro Max, generated WASM | 65536 | 6.0 | 29.0 | 107.0 |

The iPhone run was recorded at 2026-10-23T17:07:17Z on an iPhone 15 Pro Max.
Its user agent reports Safari 17.5 and iOS 17.5.1. It reports
`hardwareConcurrency: 4`. The Fast baseline was 448877 H/s.
The source file is `target/hashwx-bench/2026-10-23T17-07-16.766Z-204e5542-2fbf-45d5-8e46-b7f30cf0b3a2.json`.

The iPhone 15 Pro Max is a high-end device. It does not meet the mid-range
phone requirement in sections 2 and 8. The section 4 defaults stay
placeholders until a mid-range phone is measured. Three trials per difficulty
have a wide solve-time spread. The hash rates are the reliable figures for
these comparisons. The solve medians describe these fixed samples only.
Use a mid-range phone to select the target of 1 to 3 seconds. Record that
phone's native-to-browser ratio in `docs/performance.md`.

### Run the prototype on the LAN

From the worktree root:

```sh
export CARGO_BUILD_BUILD_DIR="$PWD/target" CARGO_TARGET_DIR="$PWD/target"
cargo xtask build-wasm && cargo xtask build-web
node scripts/bench-hashwx-browser.mjs
```

The server binds `0.0.0.0` and selects a free port. It prints localhost and LAN
URLs. `RILGATE_HASHWX_BENCH_PORT=8787 node scripts/bench-hashwx-browser.mjs` selects a fixed port
when that port is free. The page path is `/`; `/?autorun=1` starts a run when
the page loads. The server serves only the benchmark page, its worker, the
HashWX artifact and the Fast worker. It accepts result JSON at `POST /results`.

On the iPhone, connect to the Mac's LAN, open the printed LAN URL in Safari,
and select **Start benchmark**. Keep Safari in the foreground and the screen
awake until the page shows **Complete. Saved to ...**. The page shows progress
and has a Cancel button. It stops after ten minutes. The default run uses
both engines, both batch sizes, three trials per difficulty, and difficulties
1000, 10000 and 50000. Leave those values unchanged for the first comparison.
The user agent, reported hardware concurrency, parameters, throughput and
individual solve times are saved automatically on the Mac. If upload fails,
the page retains the JSON for manual recovery.

Results are written to the ignored `target/hashwx-bench/` directory with a
server-generated timestamp and random file name. Native commands are:

```sh
cargo run -p hashwx-core --release --example hashwx-throughput -- 463
cargo run -p hashwx-core --release --example hashwx-throughput -- 65536
```

The recorded files for the tables above are:

- Chrome: `target/hashwx-bench/2026-10-23T16-54-27.780Z-f8e32828-5d55-46a3-9dab-3d07cff04b87.json`
- Safari: `target/hashwx-bench/2026-10-23T16-57-44.811Z-cfd4da1d-e2fc-40b3-8e46-94774d5e938a.json`
- Native: `target/hashwx-bench/native-463.json` and `target/hashwx-bench/native-65536.json`

### License review record

The [step 5 license review](../hashwx-license-review.md) records the single
earlier upstream fetch and the pinned specification input. No upstream
implementation code was copied, translated or adapted. No upstream source was
read or fetched during the continuation that completed steps 3 to 5.

## 9. Integration

Each algorithm follows [adding an algorithm](../algorithms.md):

| Part | HashWX |
| --- | --- |
| Backend crate | `crates/rilgate-pow-hashwx` on `hashwx-core` |
| Protocol | `PowAlgorithm::Hashwx`, plus work, claims and challenge response variants with `challenge`, `nonces_per_seed` and `difficulty`. The proof is a 64-bit nonce |
| Mapping | `rilgate-challenge/src/challenge/work.rs` |
| Registration | `crates/rilgate/src/app/bootstrap.rs` |
| Configuration | `pow.algorithms.hashwx` with flags, variables and validation |
| Browser | `web/static/js/solvers/hashwx/` with a classic adapter in `STARTUP`, a modern solver and worker, and the WASM module |
| Required resources | The served files in `crates/rilgate-web/src/runtime/snapshot.rs` |
| Documentation | `docs/pow.md`, `docs/policy.md`, `docs/configuration.md` and `docs/performance.md` |

Argon2id and PBKDF2 use the same parts with their own crates
(`rilgate-pow-argon2`, `rilgate-pow-pbkdf2`) and settings.

Cookie rules do not change. A `CHALLENGE` rule accepts a cookie only when its
algorithm and enforcement match and its difficulty is at least the required one.

## 10. Verification

| Area | Tests |
| --- | --- |
| Conformance | Section 4 for HashWX. RFC 9106 test vectors for Argon2id, in native Rust and in the WASM module |
| Proof | Valid proofs pass. A changed challenge, `nonces_per_seed` or difficulty fails. An expired challenge fails |
| Order of checks | An invalid token signature fails without a hash computation |
| Capacity | For Argon2id, a full verifier queue returns `503`, and peak memory stays within `verifier_workers × memory` |
| Browser | The Playwright suite solves a HashWX challenge in Chromium, Firefox and WebKit. An incapable browser shows the failure controls |
| Browser limits | Generated-module compilation failure retains the interpreter. Both engines run without `BigInt`. A worker CSP denying all WASM rejects calculation |
| Fallback | A presentation token without a fallback cannot get a fallback challenge |

## 11. Delivery sequence

1. `hashwx-core` interpreter with the Appendix F vectors, property tests and fuzz
   target.
2. Measurements in section 8 with a prototype browser solver.
3. HashWX WASM code generation, `hashwx-wasm` and the browser solver.
4. HashWX backend, protocol variants and configuration.
5. Documentation and the license review.
6. Optional: Argon2id, when operators ask for it.
7. Optional: PBKDF2-SHA256, when operators ask for it.
8. Optional: capability fallback, when operators ask for it.
