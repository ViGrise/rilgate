# Proof of work

## PoW algorithms

Challenge policies can select Fast, HashWX, RandomX or Refresh.

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

HashWX always uses gateway verification. Its challenge lifetime is `pow.challenge.ttl`.
The browser starts automatically, uses at most two workers by default, and
checks expiry during range searches. It terminates all workers on completion,
error, cancellation or page exit. It keeps 64-bit values as u32 halves and
needs no BigInt or SIMD. A failure to compile a generated module retains the
WASM interpreter. A worker policy that denies all WASM blocks both engines.
See [browser requirements](web.md#hashwx).

The shipped difficulty defaults are placeholders. The iPhone 15 Pro Max
measurement is from a high-end device. A mid-range phone measurement is still
required before the defaults can target 1 to 3 seconds. See
[measurements](performance.md#hashwx-measurements-2026-10-23) and the
[complete example](../config/examples/hashwx.yml).

HashWX tokens use version 1 with an explicit `hashwx` algorithm tag. Older
builds reject that tag. Deploy HashWX support to every gateway instance before
selecting it in policy. Shared signing keys, token lifetimes and verification mode
must also match. Receipts and access cookies retain the algorithm and difficulty.

### `refresh` - delay without JavaScript

```yaml
challenge:
  algorithm: refresh
  difficulty: 5
```

The challenge page uses an HTML meta refresh and runs no script. Difficulty is a
delay of 1 to 10 seconds. After the delay, the browser opens
`{site.base_path}/api/refresh`. That URL sets the access cookie and redirects to the
original request path on the same host. If the browser opens the URL early, it gets the
page again with the remaining delay.

The delay proves no work, so use it only as a speed bump for clients that do not
run JavaScript. A refresh rule cannot be `pow.default_algorithm`, and it always
uses gateway verification. A cookie from any other algorithm satisfies a refresh
rule. A refresh cookie satisfies a rule when its difficulty meets the requirement.

### `randomx` - expensive challenge

```yaml
challenge:
  algorithm: randomx
  difficulty: 8000
```

Both RandomX modes get work from a configured Stratum endpoint. Difficulty is a
positive integer of at least 1000. A result must be no greater than `floor((2^256 - 1) / difficulty)` when
interpreted as a little-endian integer.

Select how Rilgate accepts RandomX proofs with
[`pow.algorithms.randomx.verify.mode`](#verification-modes).

### When every pool is unavailable

`pow.algorithms.randomx.fallback` is optional and absent by default. It works in
both verification modes. Configure both fields to serve another challenge
while every Stratum pool is in its failure cooldown:

```yaml
pow:
  algorithms:
    randomx:
      fallback:
        algorithm: fast
        difficulty: 4
```

The algorithm must be `fast` or an enabled `hashwx`. Its backend validates the
difficulty. Both flags or environment variables must be supplied together:
`--pow-algorithms-randomx-fallback-algorithm` /
`RILGATE_POW_ALGORITHMS_RANDOMX_FALLBACK_ALGORITHM` and
`--pow-algorithms-randomx-fallback-difficulty` /
`RILGATE_POW_ALGORITHMS_RANDOMX_FALLBACK_DIFFICULTY`.

The gate selects the solver before rendering the page or JSON challenge response.
A cookie satisfying the RandomX policy still passes during an outage. A fallback
cookie passes only while fallback is active, with the same verification mode, host and
binding checks and at least the configured fallback difficulty. When a pool becomes
eligible again, that cookie receives a RandomX challenge.

A RandomX page rendered just before failure can receive HTTP 503 with
`{"code":"work_unavailable"}`, `Retry-After: 1` and `Cache-Control: no-store`
from `/api/challenge`. A pool WebSocket connection failure returns an error message
with `code: "work_unavailable"`. On either path, the adapter reloads once per page
URL to select the fallback. A second failure shows the `unavailable` state. A
successful challenge clears the reload guard. Without `fallback`, RandomX work
fails with `work_unavailable` on both paths while every pool is unavailable.

### Pool selection

Configure a pool list with ordered backups. A pooled job session stays on the
pool that supplied its jobs. With `verify.mode: pool_share`, the browser
WebSocket submits shares on that same pool connection. A new session can use a
backup pool. With `verify.mode: gateway`, verification does not contact the pool and
submits no shares. This mode earns no mining reward.

A pool can change the requested difficulty. Rilgate keeps the target from the
pool job. It reports one warning and one mismatch count per connection. The
warning contains the pool name and requested and received difficulty.

Use pools that you trust with `verify.mode: pool_share`. A pool that accepts every
share can grant access to every client. Each failover pool can pay a different
wallet. See [pool configuration](configuration.md#stratum-pools-and-proxies).

P2Pool sets the payout wallet on the node with `--wallet` and ignores a login
wallet. Use `x+{difficulty}` with P2Pool. Other pools can require a wallet in
`login.user`. Copy the login format from that pool's setup instructions.

### Hybrid escalation

A custom policy can reserve RandomX for higher scores in both verification modes.
Configure `pow.stratum_pool` explicitly for RandomX work:

```yaml
pow:
  stratum_pool:
    pools:
      - name: local
        address: 127.0.0.1:3333
policy:
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

## Verification modes

`pow.algorithms.randomx.verify.mode` selects `gateway` (default) or `pool_share`
for RandomX. Use `--pow-algorithms-randomx-verify-mode` or
`RILGATE_POW_ALGORITHMS_RANDOMX_VERIFY_MODE` for the same setting.
Fast, HashWX and refresh always use `gateway`. They can work in the same policy
as RandomX with `pool_share`.

### Gateway verification

With `gateway`, Rilgate checks the proof. For RandomX, it computes the hash
with the hashers in `pow.algorithms.randomx.verify.order`.

```yaml
pow:
  algorithms:
    randomx:
      verify:
        mode: gateway
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
      POST nonce + set_cookie: true to /api/verify
          |
          v
      gateway checks the RandomX hash
          |
          v
      signed short-lived receipt + cookie_set: true
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

- Signing keys and verification mode.
- Compatible token lifetimes and algorithms.

No Redis, coordinator, sticky session or special load-balancer configuration is
required.

A result accepted by the gateway proves the requested RandomX difficulty. It is a different
protocol event from a Stratum `accepted` response.

### Pool share verification

With `pool_share`, a RandomX proof is valid only when a Stratum pool accepts
the share. Shares for the last 3 jobs go to the pool, which decides if each job is
still valid. The browser keeps its workers and cache across jobs with the same seed.
A job with the same seed keeps the hashing state and the total attempt count; a
different seed shows the setup state.

```yaml
pow:
  algorithms:
    randomx:
      verify:
        mode: pool_share
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
does not need sticky-session configuration. After a closed WebSocket, the page opens
a pool session by itself. If 5 sessions in a row end before a job arrives, the page
reloads one time and asks for consent again.

After success, the normal access cookie is stateless and can be verified by any gateway
that shares the auth key.

HTTP proof clients may send the optional boolean `set_cookie` to `/api/verify`.
It defaults to `false`: verification returns only `receipt` and sets no cookie.
With `set_cookie: true`, successful verification also runs receipt exchange with the
same host and client binding checks and returns `receipt` plus `cookie_set: true`,
with the HttpOnly access cookie in the same response. Verification errors set no
cookie; exchange errors use the same response as `/api/exchange`. Each verify
request still consumes one proof quota unit.

The browser requests this option for Fast, HashWX and local RandomX proofs. If
`cookie_set: true` is absent, it calls `/api/exchange` with the returned receipt
for compatibility with older gateways. Pool mode uses its WebSocket receipt and
`/api/exchange`. The refresh challenge uses its existing redemption flow.

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
static/js/solvers/loader.js          ES module importing the configured solver
static/js/solvers/randomx/solver.js          ES module exporting solve(job)
static/js/solvers/randomx/randomx-worker.js  Web Worker owning the WASM instance
static/js/solvers/randomx/randomx_wasm.wasm  randomx-core compiled to wasm32 with SIMD
static/js/solvers/randomx/randomx_wasm_scalar.wasm  module for browsers without SIMD
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

Light mode keeps roughly 256 MiB of Argon2d cache in each worker. It derives dataset
items on demand. It does not materialize the ~2 GiB dataset.

### Proof scope and lifetime

Presentation and challenge tokens bind the request host. Receipts and access cookies
retain that binding, including the port. HTTP/2 authority is supported when the Host
header is absent.

Missing hosts cannot bypass a nonempty binding. Old receipts and access tokens without a
host claim need a new proof. Fast challenges also retain their verification mode and
cannot cross mode changes. Older fast challenge tokens without that claim need a new
challenge. All token validators reject unsupported versions, future issue times, invalid
lifetimes, and expiration at the current second.

Access cookies apply across paths on their bound host. A `CHALLENGE` policy accepts a
cookie only when its algorithm and verification mode match and its difficulty is at least
the policy's requirement. `DENY` rules still deny access with a valid cookie.

Local RandomX work appends a domain marker and a random 16-byte challenge ID to the
Pool-derived blob. Identical pool jobs therefore yield distinct proofs without a
rebuild of the seed cache. Pool blobs must fit in 233 bytes before the 23-byte suffix.
Larger jobs return 503. This local work cannot be submitted as a Stratum pool share. The
`pool_share` mode uses the original pool blob unchanged.

`pow.challenge.ttl` controls presentations, Fast challenges and HashWX challenges. `pow.algorithms.randomx.challenge_ttl` controls local
RandomX solving and defaults to 3600 seconds. Tune it for browser hash rates and policy
difficulty.

HashWX and RandomX HTTP responses include `expires_in`, the seconds remaining
at issuance, and retain `expires_at` in Unix seconds for compatibility. The client
records the local response time. Each solver derives a local deadline from that
time and the relative lifetime, so server and client clocks can differ. RandomX
checks the local deadline before initialization and before and after each hash.
Native verification uses the signed server lifetime. RandomX abandons queued
work at its server deadline. The challenge service checks lifetime before and
after backend verification. An already running native hash finishes on its verifier thread.

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
comparisons. The SIMD WASM core uses two-lane SIMD and normalized Dekker residuals, and
retains software arithmetic for exceptional values. All four RandomX rounding modes
remain exact. Directed division and square root reuse the nearest-rounded result when
they compute residuals. The `no_std` build retains software square root. WASM assets
include `randomx_wasm_scalar.wasm` for browsers without SIMD128 support.

Rust generates seed-specialized SuperscalarHash kernels for one, two or three dataset
items. Integer high multiplication is inlined, and cache-line reads begin before the
independent arithmetic. The worker compiles the module against the existing cache,
reuses three table slots across seed changes, and hashes groups of three consecutive
nonces. Worker `i` of `N` takes group indexes `i, i + N, i + 2N` and so on. The first
solution stops hashing. Workers stay alive across jobs and stop when the check ends.
Progress sums attempts and rates across workers.

Each nonce has independent VM state. The extra scratchpad costs about 4 MiB relative to
a single VM alongside each worker's 256 MiB cache. Kernel installation failure retains
interpretation within the assigned nonce groups. No C++ or Emscripten is required.

The worker count is `max(1, min(byCores, byMemory, max_workers))`, where
`byCores = max(1, (cores || 2) - 1)` and
`byMemory = deviceMemory ? floor(deviceMemory / 2) : 1`. A missing core report counts as 2 cores. A
browser without a memory report, such as Firefox, Safari and every iOS browser, gets one
worker, so the cache needs 256 MiB. Set `pow.algorithms.randomx.browser.max_workers`
to an integer from 1 to 8 (default 8) to cap the count, including the development
`rx_workers` override. CPU and memory limits can select fewer workers.

Set `pow.algorithms.randomx.browser.store_cache` to `false` to disable IndexedDB
reads and writes. Workers build the cache when the seed changes and keep it across
jobs with the same seed. The default is `true`.

The `pow.algorithms.randomx.browser.job_details` setting defaults to `true`. The page
shows the job, the block and the seed under the attempt count while RandomX runs.
A different seed shows the setup state with the notice "new seed, preparing again"
and keeps the attempt count until hashing resumes. Set it to `false` to hide the line.

Each worker owns its cache. IndexedDB retains
one seed cache in `rilgate-randomx` / `caches`, with a SHA-256 digest checked before
import. Storage failures are ignored and workers build the cache directly. Memory use
is up to `N x 256 MiB` plus scratchpads and runtime memory. The browser requires
Worker, WebAssembly and BigInt. SIMD is optional. See the
[development browser checks](development.md#browser-checks) for measurement overrides.

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

The [verification mode](#verification-modes) sets the proof requirement. With
`gateway`, the hash can come from a local worker or a remote verifier. Both
modes need `pow.stratum_pool.pools[].address` to get work. With `pool_share`,
`verify.order` must be `[local]`. A `verify.remote.endpoint` or any
`verify.remote.tls` file is a configuration error.

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
  default_algorithm: randomx
  stratum_pool:
    pools:
      - name: internal
        address: pool.internal:3333
        tls: {enabled: true}
policy:
  default_action: challenge
```

For a dedicated verifier, see [configuration](configuration.md#dedicated-randomx-verifier)
and [production operations](production.md#remote-randomx-verifier).

HashWX reserves one verification slot per available CPU. When all slots are
reserved, verification returns the capacity error (HTTP 503). A slot stays
reserved until its blocking task completes, including after request cancellation.
