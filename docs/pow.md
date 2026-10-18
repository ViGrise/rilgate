# Proof of work

## PoW algorithms

Challenge policies can choose between two algorithms.

### `fast` - lightweight SHA-256

```yaml
challenge:
  algorithm: fast
  difficulty: 2
```

`fast` uses a leading-zero-hex-digit difficulty convention: `difficulty` is the
number of leading zero hexadecimal digits required in:

```text
SHA-256(challenge || decimal_nonce)
```

Examples:

```text
difficulty 1 -> ~16 attempts on average
difficulty 2 -> ~256
difficulty 4 -> ~65,536
difficulty 6 -> ~16,777,216
```

The browser solver runs in a dedicated Web Worker and does **not** load the
RandomX WASM module. Server verification is one SHA-256 operation, so this is
appropriate for low/moderate suspicion.

The canonical YAML algorithm name is `fast`.

### `randomx` - expensive challenge

```yaml
challenge:
  algorithm: randomx
  difficulty: 8000
```

Both RandomX modes obtain work from a configured P2Pool Stratum endpoint.
Difficulty is a positive integer of at least 1000; a result must be no greater
than `floor((2^256 - 1) / difficulty)` when interpreted as a little-endian integer.

With:

```yaml
pow:
  verification: local_verify
```

the gateway verifies RandomX locally.

With:

```yaml
pow:
  verification: p2pool_accepted
```

RandomX uses the WebSocket + retained Stratum session and succeeds only after
P2Pool accepts the share.

`fast` supports only local verification. Policies selecting it with
`p2pool_accepted` are rejected. P2Pool-accepted deployments must select RandomX.

### Hybrid escalation

With local verification, a custom policy can reserve RandomX for higher scores.
Configure `pow.p2pool` explicitly for RandomX work:

```yaml
pow:
  p2pool:
    address: 127.0.0.1:3333
policy:
  mode: custom
  score:
    thresholds:
      - min: 30
        action: challenge
        challenge: {algorithm: randomx, difficulty: 8000}
      - min: 20
        action: challenge
        challenge: {algorithm: fast, difficulty: 4}
      - min: 10
        action: challenge
        challenge: {algorithm: fast, difficulty: 2}
```

If a `CHALLENGE` omits `algorithm`, `pow.default_algorithm` is used:

```yaml
pow:
  default_algorithm: randomx
```

## Enforcement modes

Configured through `pow.verification` in YAML, `--pow-verification` or
`RILGATE_POW_VERIFICATION`.

### Stateless local verification

```yaml
pow:
  verification: local_verify
```

Flow:

```text
application request
  |
  v
gateway
  |
  +--> challenge page at requested URL
          |
          v
      P2Pool-derived job
          |
          v
      RandomX in browser
          |
          v
      POST nonce to /api/verify
          |
          v
      gateway recomputes RandomX
          |
          v
      signed short-lived receipt
          |
          v
      /api/exchange
          |
          v
      HttpOnly access cookie
          |
          v
      original URL
          |
          v
      upstream
```

The challenge is self-contained and signed, so a challenge created by gateway A
can be verified by gateway B when both instances share signing keys,
enforcement mode, and compatible token lifetimes and algorithms.

No Redis, coordinator, sticky session or special load-balancer configuration is
required.

A locally accepted result proves the requested RandomX difficulty but is not
the same protocol event as P2Pool returning a Stratum `accepted`.

### P2Pool-accepted enforcement

```yaml
pow:
  verification: p2pool_accepted
```

Flow:

```text
challenge page
  |
  v
WebSocket under site.base_path
  |
  v
one gateway instance
  |
  v
matching P2Pool Stratum session
  |
  v
browser solves current job
  |
  v
gateway submits job_id + nonce + result
  |
  v
P2Pool returns accepted
  |
  v
signed receipt
  |
  v
HTTP exchange
  |
  v
shared HttpOnly access cookie
```

Only the active WebSocket challenge is temporarily tied to one gateway process.
An established WebSocket naturally stays on that instance, so the external
load balancer does not need sticky-session configuration.

After success, the normal access cookie is stateless and can be verified by any
gateway sharing the auth key.

## Browser RandomX adapter

The gateway-owned solver URL is also scoped below `site.base_path`.

Default:

```text
/.well-known/rilgate/static/js/solvers/randomx/solver.js
```

RandomX is implemented once, in `crates/randomx-core`, and reused by both the
native server verifier and the browser:

```text
crates/randomx-core   pure-Rust RandomX v1 Light mode (no C/C++ dependency)
crates/randomx-wasm   raw wasm32 ABI around randomx-core
crates/xtask          builds and stages the browser module
```

The browser gets three assets:

```text
static/js/solvers/randomx/solver.js          ES module exporting solve(job)
static/js/solvers/randomx/randomx-worker.js  Web Worker owning the WASM instance
static/js/solvers/randomx/randomx_wasm.wasm  randomx-core compiled to wasm32
```

`solver.js` exports:

```js
export async function solve(job) {
  return {
    nonce: "00000000",
    result: "64 hexadecimal characters",
  };
}
```

The module is embedded with the rest of the web bundle into the server binary,
so deployment stays a single file. Rebuild it and the gateway after changing
`crates/randomx-core` or `crates/randomx-wasm`:

```bash
rustup target add wasm32-unknown-unknown
cargo xtask build-wasm
cargo build --release
```

Light mode keeps roughly 256 MiB of Argon2d cache in the worker and derives
dataset items on demand instead of materializing the ~2 GiB dataset.

### Proof scope and lifetime

Presentation and challenge tokens bind the request host. Receipts and access
cookies retain that binding, including the port. HTTP/2 authority is supported
when the Host header is absent. Missing hosts cannot bypass a nonempty binding.
Old receipts and access tokens without a host claim require a new proof. Fast
challenges also retain their enforcement mode and cannot cross mode changes;
older fast challenge tokens without that claim require a new challenge.
All token validators reject unsupported versions, future issue times, invalid
lifetimes, and expiration at the current second.

Access cookies apply across paths on their bound host. A `CHALLENGE` policy
accepts a cookie only when its algorithm and enforcement mode match and its
difficulty is at least the policy's requirement. `DENY` rules still deny access
with a valid cookie.

Local RandomX work appends a domain marker and a random 16-byte challenge ID to
the P2Pool-derived blob. Identical pool jobs therefore yield distinct proofs
without rebuilding the seed cache. Pool blobs must fit in 233 bytes before the
23-byte suffix; larger jobs return 503. This local work cannot be submitted as a
P2Pool share. The `p2pool_accepted` mode uses the original pool blob unchanged.

`pow.challenge.ttl` controls presentations and fast challenges.
`pow.algorithms.randomx.challenge_ttl` controls local RandomX solving and defaults
to 3600 seconds. Tune it for browser hash rates and policy difficulty. The HTTP
RandomX response includes `expires_at` in Unix seconds; the browser checks it
before initialization and before and after each hash. Native verification times
out at the deadline, abandons queued work, and rechecks expiration before
issuing a receipt. An already running native hash finishes on its verifier thread.

RandomX difficulty remains a JSON number through JavaScript's largest safe
integer and becomes a decimal string above that limit, preserving all u64 values
in HTTP and WebSocket job messages. Custom solvers should accept both forms.

Exchanging a receipt preserves its issue time when calculating access expiry.
Repeated exchange cannot extend access. Tokens remain reusable bearer credentials
within their signed lifetime; rejecting every replay across instances requires
shared state or a different protocol. Re-verifying the same challenge can still
issue receipts until that challenge expires.

### RandomX throughput

The native core corrects binary64 rounding with exact residuals or integer
significand comparisons. The WASM core uses two-lane SIMD and normalized Dekker
residuals, retaining software arithmetic for exceptional values. All four
RandomX rounding modes remain exact. Directed division and square root reuse
the nearest-rounded result when computing residuals. The `no_std` build retains
software square root. WASM assets require SIMD128 support.

Rust generates seed-specialized SuperscalarHash kernels for one, two or three
dataset items. Integer high multiplication is inlined, and cache-line reads begin before
the independent arithmetic. The worker compiles the module against the existing
cache, reuses three table slots across seed changes, and hashes three consecutive
nonces in one worker. Each nonce has independent VM state; the extra scratchpad
costs about 4 MiB relative to a single VM alongside the shared 256 MiB cache.
Kernel installation failure retains single-hash interpretation. No C++, Emscripten or additional workers are
required.

Native throughput can be measured with:

```bash
cargo run -p randomx-core --release --example throughput -- 8 nonce
```

Arithmetic validation covers IEEE boundaries and 200,000 random pairs against
the software oracle in the actual WASM runtime. AES table rounds are compared
with the portable AES implementation over 4,096 state/key pairs. Dataset tests
compare single, paired and triple kernels with interpretation over three seeds
and 411 boundary/random items. Full hash tests check paired and triple versus
individual execution and official reference vectors.

```bash
cargo build -p randomx-wasm --release --target wasm32-unknown-unknown --features validation
node scripts/validate-randomx-wasm.mjs
```

The validation build stays under `target/`; `cargo xtask build-wasm` stages the
production artifact without the validation export.
