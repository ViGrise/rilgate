# More browser proof-of-work algorithms

Status: proposed.

This proposal adds proof-of-work algorithms for browser challenges next to Fast
and RandomX. It selects HashWX as the next algorithm, through a new Apache-2.0
implementation in this repository. Argon2id and PBKDF2-SHA256 are optional. It
rejects HashX, Equi-X, scrypt and mining algorithms. Each new algorithm follows
the contract in [adding an algorithm](../algorithms.md).

## 1. Problem

| Algorithm | Strength | Limit |
| --- | --- | --- |
| Fast (SHA-256) | Runs on every supported browser, including IE11 | The browser solver is pure JavaScript (`web/static/js/solvers/fast/worker.js`). A native or GPU solver computes many more hashes per second. Thus Fast costs a bot much less than a person |
| RandomX | CPU-oriented and memory-hard | It needs P2Pool for every job, about 256 MiB in the browser, and a 256 MiB cache on the verifier |

Rilgate has no GPU-resistant algorithm without external infrastructure. An
operator who wants a stronger challenge than Fast must operate P2Pool.

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
  README. Get a license review before the first release that includes the crate.

### Crates

| Crate | Role |
| --- | --- |
| `crates/hashwx-core` | `no_std` instance generator, interpreter and WASM code generator |
| `crates/hashwx-wasm` | Raw `wasm32-unknown-unknown` ABI for the browser solver, staged by `cargo xtask build-wasm` |
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

Without an explicit difficulty, the score maps to a difficulty with the same
`base`, `score_step` and `max` rule as RandomX. The default values are
placeholders until the measurements in section 8.

Verification is cheap, so HashWX needs no dedicated verifier pool. It runs on a
blocking task with the proof API rate limits in front.

### Browser

- The solver needs WebAssembly and workers. The classic adapter in the startup
  bundle checks these capabilities. On an incapable browser, it shows the
  localized failure and retry controls. The client never switches algorithms.
- For each batch, `hashwx-core` generates the 32 programs and emits a WASM module.
  The worker compiles the module and hashes the batch. `randomx-core` uses the
  same technique for its SuperscalarHash kernels.
- If the compilation of a generated module fails, the worker uses the interpreter.
- The solver can use several workers, one batch for each worker. It reports
  progress after each batch and supports cancellation.

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

The values in sections 3 and 4 are from the HashWX authors. Before the defaults
are fixed, measure these values with the Rilgate implementation. Use a mid-range
phone, a desktop browser, and a native solver on a server CPU:

- HashWX hashes per second with generated WASM, with the WASM interpreter, and
  natively, for `nonces_per_seed` of 463 and 65536.
- HashWX server verification time with the interpreter.
- Fast hashes per second, as the baseline.
- If Argon2id is added: evaluations per second for `memory` of 4, 16 and 64 MiB,
  and server verification time and peak memory.

Set the default difficulty so that the median solve on the phone takes 1 to 3
seconds. Record the ratio between the native solver and the phone for each
algorithm in `docs/performance.md`.

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
