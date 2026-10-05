# Gateway behavior

## Internal namespace

All pages, APIs and static assets owned by the gateway live below one reserved
path configured in YAML:

```yaml
web:
  base_path: "/.well-known/rilgate"
```

With the default prefix:

```text
/.well-known/rilgate
/.well-known/rilgate/api/health
/.well-known/rilgate/api/client-config
/.well-known/rilgate/api/challenge
/.well-known/rilgate/api/verify
/.well-known/rilgate/api/exchange
/.well-known/rilgate/api/ws
/.well-known/rilgate/static/js/challenge.js
/.well-known/rilgate/static/js/solvers/fast/worker.js
/.well-known/rilgate/static/js/solvers/randomx/solver.js
/.well-known/rilgate/static/js/solvers/randomx/randomx-worker.js
/.well-known/rilgate/static/js/solvers/randomx/randomx_wasm.wasm
```

Changing `web.base_path` moves the entire gateway-owned namespace.

Only the configured subtree is reserved. With the default configuration,
paths such as:

```text
/.well-known/acme-challenge/...
/.well-known/security.txt
```

still pass through to the upstream application because they are outside
`/.well-known/rilgate`.

Unknown paths below the configured gateway prefix are **never** proxied to the
protected application.

## Reverse proxy

Configure the protected application:

```yaml
upstream:
  url: "http://127.0.0.1:3000"
  preserve_host: true
  max_body_bytes: 16777216
```

Normal application URLs stay unchanged.

For example:

```text
GET /
GET /login
GET /articles/123
```

Without a valid gateway cookie, the URL does not change:

```text
GET /articles/123
       |
       v
200 challenge HTML
served at /articles/123
```

The challenge page calls only the reserved gateway namespace:

```text
/.well-known/rilgate/api/...
/.well-known/rilgate/static/...
```

After the proof succeeds:

```text
Set-Cookie: rilgate_auth=...
       |
       v
location.reload()
       |
       v
GET /articles/123
       |
       v
upstream /articles/123
```

The protected application does not need to know about the challenge URLs, and
the browser never navigates away from the originally requested path.

Unauthenticated `GET`/`HEAD` requests receive the challenge page
in-place. Unauthenticated non-idempotent requests such as `POST` return `401`
instead of being buffered and replayed automatically.

The proxy streams request and response bodies. Requests are limited by
`upstream.max_body_bytes`. It removes standard hop-by-hop headers and headers named
by `Connection` on both sides.

## Resource limits and failure handling

The verifier uses one dedicated thread and a bounded queue. Requests beyond
`pow.verifier_queue_capacity` receive `503`; queued work whose caller disconnects
or times out is skipped. Seed changes release the old cache before allocating
another one.

P2Pool connections share `pow.p2pool.max_sessions` slots across HTTP job fetches
and WebSocket sessions. Excess requests receive `503` before upgrade or login.
WebSocket sessions expire after `pow.p2pool.session_ttl_seconds`, and pending
submissions expire after `pow.p2pool.submit_timeout_seconds`. Job rotation does
not discard an outstanding acceptance response. Shares must match the current
job and the selected difficulty before submission; access still requires the
matching P2Pool acceptance. Gateway clients send the nonce as a numeric
hexadecimal value; the Stratum adapter converts it to the little-endian byte
encoding expected by P2Pool.

```yaml
upstream:
  connect_timeout_seconds: 5
  read_timeout_seconds: 30

pow:
  verifier_queue_capacity: 8
  p2pool:
    max_sessions: 64
    session_ttl_seconds: 900
    submit_timeout_seconds: 20
```

Upstream connect/read failures produce `502`, and upstream timeouts produce
`504` before response headers are sent. Oversized request bodies produce `413`. Known oversized lengths are rejected
before upstream contact. For unknown lengths, the upstream can receive initial
chunks before the limit is exceeded; the excess chunk is rejected and the
upload ends. Upstream handlers must treat incomplete uploads as incomplete
requests. Uploads are never retried automatically.
A read timeout after streaming begins closes the body, since the status has
already been sent. PoW JSON requests are limited to 16 KiB and their responses
use `Cache-Control: no-store`; Stratum messages are limited to 64 KiB.

Startup validates policy difficulties, paths, timeouts, and queue/session
capacities. Production rejects placeholder or shorter-than-32-byte signing
keys and debug weight headers. P2Pool passwords are redacted in configuration
logs.

RandomX requires a click before solver initialization. The page discloses CPU
and memory use, and P2Pool mode explicitly discloses submitted mining work.
Workers are terminated when the challenge completes, fails, or leaves the
page. Browser API calls have a 30-second deadline, and failures show a retry
button that reloads the original URL with a fresh presentation token. Configure challenge and session lifetimes for the selected difficulty
and expected browser throughput; RandomX solve times depend on the device and difficulty.

## Proof API rate limits

Rate limiting is enabled by default for challenge issuance, verification,
receipt exchange, and WebSocket upgrades. Each request uses one unit from its
client bucket and the instance's global bucket. Rejected requests return `429`,
`Retry-After`, and `Cache-Control: no-store`. Client configuration, health,
assets, and application proxy traffic do not use this quota.

```yaml
pow:
  rate_limit:
    enabled: true
    client_burst: 10
    client_refill_per_second: 0.1
    global_burst: 64
    global_refill_per_second: 16
    max_clients: 4096
```

These defaults permit a burst of 10 proof requests per client and replenish
one request every 10 seconds. The global bucket permits a burst of 64 and
replenishes 16 requests per second. Limits are local to each gateway instance;
multiple instances have separate quotas. They do not replace the verifier's
queue or P2Pool connection limits.

Client identity comes from the TCP peer address. `Forwarded` and
`X-Forwarded-For` are ignored. IPv4-mapped IPv6 addresses share the same bucket
as their IPv4 address. Clients behind a reverse proxy or NAT share that peer's
quota; tune limits for this deployment or enforce client quotas at the trusted
edge. Embedded servers must use
`router.into_make_service_with_connect_info::<SocketAddr>()` to attach peer
addresses. Requests without connection information share an anonymous bucket.

The client table is bounded by `max_clients`. Fully replenished entries can be
removed without resetting an active quota. If the table is full of active
entries, new identities are throttled until a slot becomes available. No
background cleanup task or external store is required. Set `enabled: false`
to disable this limiter explicitly.
