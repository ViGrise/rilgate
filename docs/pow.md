# Proof of work

## PoW algorithms

Challenge policies can select Fast, HashWX or RandomX.

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

The browser solver runs in a dedicated Web Worker and does **not** load the RandomX WASM
module. Server verification is one SHA-256 operation, so this is appropriate for low or
moderate suspicion.

The canonical YAML algorithm name is `fast`.

### `hashwx` - local CPU work

```yaml
challenge:
  algorithm: hashwx
  difficulty: 50000
```

HashWX generates a hash function from a seed. Each function has 32 programs
and uses 16392 bytes of VM memory. The browser generates and compiles a WASM
module for each seed batch. The server generates one instance and computes
one interpreted hash. It uses a blocking task after the shared token checks.
HashWX needs no pool, remote verifier or mining transport.

The signed challenge contains a 32-byte value in lower-case hex,
`nonces_per_seed`, `difficulty`, host, version and lifetime. The proof is a
canonical decimal `u64` nonce, from `0` to `18446744073709551615`:

```text
batch  = nonce / nonces_per_seed
seed   = SHA-256(raw_challenge_bytes || batch_as_u64_little_endian)
hash   = HashWX(seed)(nonce)
valid  = hash < floor((2^64 - 1) / difficulty)
```

Difficulty is an expected hash count. It is between 1 and 2^40. It is distinct
from Fast's leading-zero hexadecimal digit count. Without an explicit policy difficulty,
`base * 2^(max(score, 0) / score_step)` is capped at `max`, with integer
division and saturation. An explicit policy difficulty bypasses this mapping.

HashWX supports `local_verify`. Its challenge lifetime is `pow.challenge.ttl`.
The browser starts automatically, uses at most two workers by default, and
checks expiry during range searches. It terminates all workers on completion,
error, cancellation or page exit. It keeps 64-bit values as u32 halves and
needs no BigInt or SIMD. A failure to compile a generated module retains the
WASM interpreter. A worker policy that denies all WASM blocks both engines.
See [browser requirements](web.md#hashwx-browser-requirements).

The shipped difficulty defaults are placeholders. The iPhone 15 Pro Max
measurement is from a high-end device. A mid-range phone measurement is still
required before the defaults can target 1 to 3 seconds. See
[measurements](performance.md#hashwx-measurements) and the
[complete example](../config/rilgate.hashwx.example.yml).

HashWX tokens use version 1 with an explicit `hashwx` algorithm tag. Older
builds reject that tag. Deploy HashWX support to every gateway instance before
selecting it in policy. Shared signing keys, token lifetimes and enforcement
must also match. Receipts and access cookies retain the algorithm and difficulty.

### `randomx` - expensive challenge

```yaml
challenge:
  algorithm: randomx
  difficulty: 8000
```

Both RandomX modes get work from a configured Stratum endpoint. Difficulty is a
positive integer of at least 1000. A result must be no greater than `floor((2^256 - 1) / difficulty)` when
interpreted as a little-endian integer.

With:

```yaml
pow:
  verification: local_verify
```

the gateway verifies the RandomX hash with its configured hashers.

With:

```yaml
pow:
  verification: stratum_pool_accepted
```

RandomX uses the WebSocket + retained Stratum session and succeeds only after Stratum pool
accepts the share.

`fast` and `hashwx` support local verification. Policies that select either with `stratum_pool_accepted`
are rejected. Pool-accepted deployments must select RandomX.

### Pool selection

Configure a pool list with ordered backups. A pooled job session stays on the
pool that supplied its jobs. In `stratum_pool_accepted` mode, the browser
WebSocket submits shares on that same pool connection. A new session can use a
backup pool. With `local_verify`, verification does not contact the pool and
submits no shares. This mode earns no mining reward.

A pool can change the requested difficulty. Rilgate keeps the target from the
pool job. It reports one warning and one mismatch count per connection. The
warning contains the pool name and requested and received difficulty.

Use pools that you trust for `stratum_pool_accepted`. A pool that accepts every
share can grant access to every client. Each failover pool can pay a different
wallet. See [pool configuration](configuration.md#stratum-pools-and-proxies).

P2Pool sets the payout wallet on the node with `--wallet` and ignores a login
wallet. Use `x+{difficulty}` with P2Pool. Other pools can require a wallet in
`login.user`. Copy the login format from that pool's setup instructions.

### Hybrid escalation

With local verification, a custom policy can reserve RandomX for higher scores.
Configure `pow.stratum_pool` explicitly for RandomX work:

```yaml
pow:
  stratum_pool:
    pools:
      - name: local
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

Configured through `pow.verification` in YAML, `--pow-verification` or `RILGATE_POW_VERIFICATION`.

### Stateless local verification

```yaml
pow:
  verification: local_verify
```

RandomX flow:

```text
application request
  |
  v
gateway
  |
  +--> challenge page at requested URL
          |
          v
      Pool-derived job
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

The challenge is self-contained and signed. Gateway B can validate a challenge created
by gateway A if both instances share:

- Signing keys and enforcement mode.
- Compatible token lifetimes and algorithms.

No Redis, coordinator, sticky session or special load-balancer configuration is
required.

A locally accepted result proves the requested RandomX difficulty. It is a different
protocol event from a Stratum `accepted` response.

### Pool-accepted enforcement

```yaml
pow:
  verification: stratum_pool_accepted
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
matching Stratum pool session
  |
  v
browser solves current job
  |
  v
gateway submits job_id + nonce + result
  |
  v
Stratum pool returns accepted
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

Only the active WebSocket challenge is temporarily tied to one gateway process. An
established WebSocket naturally stays on that instance, so the external load balancer
does not need sticky-session configuration.

After success, the normal access cookie is stateless and can be verified by any gateway
that shares the auth key.

## Browser RandomX adapter

The gateway-owned solver URL is also scoped below `site.base_path`.

Default:

```text
/.well-known/rilgate/static/js/solvers/randomx/solver.js
```

RandomX is implemented once, in `crates/randomx-core`, and reused by both the native server
verifier and the browser:

```text
crates/randomx-core   pure-Rust RandomX v1 Light mode (no C/C++ dependency)
crates/randomx-wasm   raw wasm32 ABI around randomx-core
crates/xtask          builds and stages the browser module
```

The browser gets these modern assets, loaded only after the classic adapter in the
startup bundle confirms browser capabilities and consent:

```text
static/js/solvers/randomx/loader.js          ES module importing the configured solver
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

The module is embedded with the rest of the web bundle into the server binary, so
deployment stays a single file. After you change `crates/randomx-core` or `crates/randomx-wasm`, rebuild
the module and gateway. The build script runs `cargo xtask build-wasm` and `cargo xtask build-web` before
`cargo build`:

```bash
scripts/build.sh --release
```

Light mode keeps roughly 256 MiB of Argon2d cache in the worker. It derives dataset
items on demand. It does not materialize the ~2 GiB dataset.

### Proof scope and lifetime

Presentation and challenge tokens bind the request host. Receipts and access cookies
retain that binding, including the port. HTTP/2 authority is supported when the Host
header is absent.

Missing hosts cannot bypass a nonempty binding. Old receipts and access tokens without a
host claim need a new proof. Fast challenges also retain their enforcement mode and
cannot cross mode changes. Older fast challenge tokens without that claim need a new
challenge. All token validators reject unsupported versions, future issue times, invalid
lifetimes, and expiration at the current second.

Access cookies apply across paths on their bound host. A `CHALLENGE` policy accepts a
cookie only when its algorithm and enforcement mode match and its difficulty is at least
the policy's requirement. `DENY` rules still deny access with a valid cookie.

Local RandomX work appends a domain marker and a random 16-byte challenge ID to the
Pool-derived blob. Identical pool jobs therefore yield distinct proofs without a
rebuild of the seed cache. Pool blobs must fit in 233 bytes before the 23-byte suffix.
Larger jobs return 503. This local work cannot be submitted as a Stratum pool share. The
`stratum_pool_accepted` mode uses the original pool blob unchanged.

`pow.challenge.ttl` controls presentations, Fast challenges and HashWX challenges. `pow.algorithms.randomx.challenge_ttl` controls local
RandomX solving and defaults to 3600 seconds. Tune it for browser hash rates and policy
difficulty.

The HTTP RandomX response includes `expires_at` in Unix seconds. The browser checks it
before initialization and before and after each hash. Native verification reaches its
timeout at the deadline and abandons queued work. It rechecks expiration before it
issues a receipt. An already running native hash finishes on its verifier thread.

RandomX difficulty remains a JSON number through JavaScript's largest safe integer.
Above that limit, it becomes a decimal string. This preserves all u64 values in HTTP and
WebSocket job messages. Custom solvers should accept both forms.

Receipt exchange preserves the receipt issue time to calculate access expiry. Repeated
exchange cannot extend access. Tokens remain reusable bearer credentials within their
signed lifetime.

Unless `security.auth.bind` ties authentication cookies to the client network, browser family or
chosen headers, they work from any machine. Binding limits where a copied cookie works.
It does not stop reuse by a client that matches the binding. Rejection of every replay
across instances requires shared state or a different protocol. Repeated verification of the
same challenge can still issue receipts until that challenge expires.

### RandomX throughput

The native core corrects binary64 rounding with exact residuals or integer significand
comparisons. The WASM core uses two-lane SIMD and normalized Dekker residuals, and
retains software arithmetic for exceptional values. All four RandomX rounding modes
remain exact. Directed division and square root reuse the nearest-rounded result when
they compute residuals. The `no_std` build retains software square root. WASM assets
need SIMD128 support.

Rust generates seed-specialized SuperscalarHash kernels for one, two or three dataset
items. Integer high multiplication is inlined, and cache-line reads begin before the
independent arithmetic. The worker compiles the module against the existing cache,
reuses three table slots across seed changes, and hashes three consecutive nonces in one
worker.

Each nonce has independent VM state. The extra scratchpad costs about 4 MiB relative to
a single VM alongside the shared 256 MiB cache. Kernel installation failure retains
single-hash interpretation. No C++, Emscripten or extra workers are required.

Native throughput can be measured with:

```bash
cargo run -p randomx-core --release --example throughput -- 8 nonce
```

Arithmetic validation covers IEEE boundaries and 200,000 random pairs against the
software oracle in the actual WASM runtime. AES table rounds are compared with the
portable AES implementation over 4,096 state and key pairs. Dataset tests compare
single, paired and triple kernels with interpretation over three seeds and 411 boundary
and random items. Full hash tests check paired and triple versus individual execution
and official reference vectors.

```bash
cargo build -p randomx-wasm --release --target wasm32-unknown-unknown --features validation
node scripts/validate-randomx-wasm.mjs
```

The validation build stays under `target/`. `cargo xtask build-wasm` stages the production
artifact without the validation export.

## RandomX hash location

`pow.verification` sets the proof requirement. With `local_verify`, the gateway
checks a hash against the difficulty target. The hash can come from a local
worker or a remote verifier. With `stratum_pool_accepted`, the gateway waits for Stratum pool
to accept the share. A remote verifier does not replace pool acceptance. Both
modes still need `pow.stratum_pool.pools[].address` to get work.

Set `pow.algorithms.randomx.verify.order` to select the hash location:

| Order | Operation |
| --- | --- |
| `[local]` | Use the local worker. This is the default. |
| `[remote]` | Use the remote verifier. The gateway allocates no RandomX cache. |
| `[remote, local]` | Try remote first. Use local if remote is unavailable. |
| `[local, remote]` | Try local first. Use remote if local is unavailable. |

A returned hash is final. A hash that misses the target receives `401` and does
not start a fallback. An invalid request also stops the sequence. If every
hasher is unavailable, the gateway returns `503`. All attempts share the
challenge expiry. Each remote attempt also has its configured timeout.

A verifier returns `503` with `{"reason":"deadline"}` when the request deadline
has passed or is more than 60 seconds ahead. Set
`pow.algorithms.randomx.verify.remote.on_deadline_rejected` to `fallback` (default)
to try the next hasher. With `[remote]`, this returns `503`. Set `reject` to stop
the sequence and return `401`. Malformed seed, input or body data remains invalid.

The local worker creates its 256 MiB cache on the first hash. A cold cache build
waits within the request deadline. The cache is released after
`pow.algorithms.randomx.cache.idle_timeout`, which defaults to `30m`. Set `0s` to
keep it until the process stops. A seed change releases the old cache before it
builds the new cache.

Use a custom policy to select RandomX. Presets select Fast explicitly, so
`pow.default_algorithm: randomx` alone does not change a preset challenge.

```yaml
pow:
  verification: local_verify
  default_algorithm: randomx
  stratum_pool:
    pools:
      - name: internal
        address: pool.internal:3333
        tls: {enabled: true}
policy:
  mode: custom
  default_action: challenge
```

For a dedicated verifier, see [configuration](configuration.md#dedicated-randomx-verifier)
and [production operations](production.md#remote-randomx-verification).
