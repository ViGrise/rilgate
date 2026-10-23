# Gateway behavior

## Internal namespace

Gateway APIs and static assets live below one reserved path configured in YAML.
Challenge, denied, and upstream error pages are rendered at the requested application
URL:

```yaml
site:
  base_path: "/.well-known/rilgate"
```

With the default prefix:

```text
/.well-known/rilgate/api/health
/.well-known/rilgate/api/live
/.well-known/rilgate/api/ready
/.well-known/rilgate/api/metrics
/.well-known/rilgate/api/client-config
/.well-known/rilgate/api/challenge
/.well-known/rilgate/api/verify
/.well-known/rilgate/api/exchange
/.well-known/rilgate/api/ws
/.well-known/rilgate/static/js/challenge.bundle.js
/.well-known/rilgate/static/js/challenge.js
/.well-known/rilgate/static/js/solvers/fast/worker.js
/.well-known/rilgate/static/js/solvers/randomx/solver.js
/.well-known/rilgate/static/js/solvers/randomx/randomx-worker.js
/.well-known/rilgate/static/js/solvers/randomx/randomx_wasm.wasm
```

The challenge page loads the assembled classic `challenge.bundle.js`. Individual script routes
remain available. `/api/health`, `/api/live`, `/api/ready` and `/api/metrics` are served
only on operations listeners.

Public listeners return 404 for them. Operations listeners return 404 for every other
route. With the default empty `server.operations.listen` list, operational routes are disabled and are
served nowhere. Metrics still need `server.metrics_enabled: true`.

A change to `site.base_path` moves the APIs and assets. The prefix itself has no index page
and returns `404`.

Only the configured subtree is reserved. With the default configuration, paths such as:

```text
/.well-known/acme-challenge/...
/.well-known/security.txt
```

remain application routes governed by the configured policy because they are outside
`/.well-known/rilgate`. The balanced preset allows ACME paths. Unless a policy explicitly allows
them, other application paths need a valid proof.

Unknown paths below the configured gateway prefix are **never** proxied to the protected
application.

## Reverse proxy

Pingora serves API endpoints, static assets, proof WebSockets, challenge pages, and
application forwarding on one listener. No internal HTTP service or Unix socket is
required. Plaintext HTTP/1.1 and HTTP/2 prior knowledge are supported. Public TLS
termination can continue at the deployment reverse proxy.

Configure the protected application:

```yaml
server:
  upstream:
    url: "http://127.0.0.1:3000"
    preserve_host: true
    max_body_size: 16MiB
```

Normal application URLs stay unchanged.

Paths that need URL normalization, including `.` or `..` segments and
their percent-encoded forms, return `400`. Policy matching and upstream
forwarding use the same path representation. Query strings do not affect this check.

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

For browser HTML navigation, the application does not need to know about the challenge
URLs and the browser stays at the originally requested path.

For a `CHALLENGE` policy decision, unauthenticated HTML `GET` and `HEAD`
requests receive the challenge page in-place. Other methods return a plain `401`
when HTML is selected. JSON responses use `403` and identify the challenge with
notification headers and a configurable body. Automatic selection considers
`Accept`, JSON request `Content-Type`, `X-Requested-With`, and configured API path prefixes.
See [AJAX and API integration](api-integration.md) for JSON templates and the same-origin iframe
completion protocol. Intercepted bodies are never buffered for automatic replay.

Application WebSocket upgrades receive the same policy and cookie checks as ordinary
requests. The dedicated proof WebSocket is handled locally.

The proxy streams request and response bodies. Requests are limited by `server.upstream.max_body_size`. It
removes standard hop-by-hop headers and headers named by `Connection` on both sides, and
retains the protocol fields required for accepted WebSocket upgrades.

Redirects are returned to the client. Upstream HTTP requests are never replayed. On
connection failure, another resolved address can be tried before the request is sent.
Literal IPv4 and IPv6 upstreams use their configured address directly. Hostname DNS
results are refreshed after 60 seconds. HTTPS upstreams use certificate and hostname
verification with upstream-host SNI, independently of `preserve_host`.

## Resource limits and failure handling

The native RandomX verifier uses one dedicated thread and a bounded queue. Requests
beyond `pow.algorithms.randomx.verifier_queue` receive `503`. Queued work whose caller disconnects or
exceeds its deadline is skipped. Seed changes release the old cache before allocation of
another one.

P2Pool connections share `pow.p2pool.sessions.max` slots across HTTP job fetches and WebSocket
sessions. Excess requests receive `503` before upgrade or login. WebSocket
sessions expire after `pow.p2pool.sessions.ttl`, and pending submissions expire after `pow.p2pool.timeout.submit`.

Job rotation does not discard an outstanding acceptance response. Shares must match the
current job and the selected difficulty before submission. Access still requires the
matching P2Pool acceptance. Gateway clients send the nonce as a numeric hexadecimal
value. The Stratum adapter converts it to the little-endian byte encoding expected by
P2Pool.

Challenge issuance keeps its Stratum sessions open in a pool of at most `pow.p2pool.job_sessions.max`. A
task owns each session and tracks job rotation while idle. Challenge issuance thus
reuses a connection.

It does not open a new one. Those sessions take permits from `sessions.max`, so
`job_sessions.max` must not exceed it and the remainder stays available to WebSocket sessions.
A session unused for `pow.p2pool.job_sessions.idle` is closed.

Incoming job notifications do not extend that deadline. The idle clock starts when the
initial job becomes available, so connection and login time does not consume it. With
`pow.p2pool.job_sessions.mode: per_challenge`, each challenge uses one connection within the shared `sessions.max` budget.
Zero is not a mode selector.

One pooled session per difficulty publishes its latest job to all matching issuance
requests. Concurrent requests share the initial connection and login. A read of an
initialized job does not reserve a session or need a task message. The background task
continuously tracks job rotation and reconnects on demand after a connection fails.
WebSocket acceptance sessions remain independent.

Size `job_sessions.max` for the number of active difficulties. Idle sessions for one
difficulty still occupy capacity that another difficulty can need. Requests that cannot
reuse a matching session or open another one return `503`. The health endpoint
exposes `pool.fetch_failures_total` and `pool.capacity_failures_total`. HTTP job-fetch warnings include these cumulative
counters and are emitted at most once per ten seconds per provider.

```yaml
server:
  upstream:
    timeout:
      connect: 5s
      read: 30s
pow:
  algorithms:
    randomx:
      verifier_queue: 8
  p2pool:
    address: 127.0.0.1:3333
    timeout:
      submit: 20s
    sessions:
      max: 64
      ttl: 15m
    job_sessions:
      mode: pooled
      max: 16
      idle: 1m
```

Upstream connect or read failures produce `502`, and upstream timeouts produce
`504` before response headers are sent. Oversized request bodies produce
`413`. Known oversized lengths are rejected before upstream contact.

For unknown lengths, the upstream can receive initial chunks before the limit is
exceeded. The excess chunk is rejected and the upload ends. Upstream handlers must treat
incomplete uploads as incomplete requests.

Uploads are never retried automatically. After streaming begins, a read timeout closes
the body because the status was already sent. PoW JSON requests are limited to 16 KiB
and their responses use `Cache-Control: no-store`. Stratum messages are limited to 64 KiB.

Local HTTP endpoints have a fixed 30-second deadline and return `408` on expiry.
Proxy connection attempts share a 30-second budget, and uploads have a 30-second budget
with a 30-second downstream read timeout. Upstream reads and writes use `server.upstream.timeout.read`.
This is an inactivity limit, not a deadline for the complete response. Streaming
responses and upgraded WebSocket sessions can outlive these request budgets.

Startup validates policy difficulties, paths, timeouts, and queue and session
capacities. Every environment rejects placeholder or shorter-than-32-byte signing keys.
Production also rejects debug score headers. P2Pool passwords are redacted in
configuration logs.

RandomX requires a click before solver initialization. The page discloses CPU and memory
use, and P2Pool mode explicitly discloses submitted mining work. Workers are terminated
when the challenge completes, fails, or leaves the page. Browser API calls have a
30-second deadline, and failures show a retry button that reloads the original URL with
a fresh presentation token. Configure challenge and session lifetimes for the selected
difficulty and expected browser throughput. RandomX solve times depend on the device and
difficulty.

## Proof API rate limits

Rate limiting is enabled by default for challenge issuance, verification, receipt
exchange, and WebSocket upgrades. Each request uses one unit from its client bucket and
the instance's global bucket. Rejected requests return `429`, `Retry-After`, and
`Cache-Control: no-store`. Client configuration, operational endpoints, assets, and application proxy
traffic do not use this proof quota. All requests remain subject to connection and
request concurrency limits.

```yaml
pow:
  rate_limit:
    enabled: true
    client:
      burst: 10
      refill_per_second: 0.1
    global:
      burst: 64
      refill_per_second: 16
    max_clients: 4096
```

These defaults permit a burst of 10 proof requests per client and replenish one request
every 10 seconds. The global bucket permits a burst of 64 and replenishes 16 requests
per second. Limits are local to each gateway instance. Multiple instances have separate
quotas. They do not replace the verifier's queue or P2Pool connection limits.

Client identity defaults to the TCP peer address. `Forwarded` is ignored. When
`server.trusted_proxies` contains the peer, `X-Forwarded-For` is resolved from right to left through
trusted hops.

Untrusted peers cannot override their IP. IPv4-mapped IPv6 addresses share the same
bucket as their IPv4 address. Clients behind an untrusted reverse proxy or NAT share
that peer's quota. Configure trusted proxies or enforce client quotas at the edge.
`Gateway::serve` gets peer addresses from accepted TCP sockets. Requests without connection
information share an anonymous bucket.

The client table is bounded by `max_clients`. Fully replenished entries can be removed
without a reset of an active quota. If the table is full of active entries, new
identities are throttled until a slot becomes available. No background cleanup task or
external store is required. Set `enabled: false` to disable this limiter explicitly.

## Logging

`RUST_LOG` accepts a `tracing` filter and defaults to `rilgate=info,pingora=error`.

Generated request IDs are UUIDv7 with ordering within each worker. A valid
client-provided `x-request-id` is propagated through the gateway.

Per-request diagnostics (the HTTP completion event, per-request policy decisions,
presented challenges and missing translations) are `DEBUG`, so the default does not
emit them. If you enable them, they add log output for each request and can affect
throughput. Raise them for a specific subsystem:

```bash
RUST_LOG=rilgate_gateway::transport::server=debug   # HTTP completion events
RUST_LOG=rilgate_gateway::proxy::gate=debug          # policy decisions
```

Startup and successful reload events use `INFO`. WebSocket P2Pool session loss
uses `WARN`. Upstream errors are included in HTTP completion events.

HTTP job-fetch failures use rate-limited `WARN` events with cumulative counters.
Pooled connection details use `DEBUG`. HTTP completion events, including individual
5xx responses, use `DEBUG`. Internal application errors retain their explicit
`ERROR` logs. Failed web snapshot builds and reloads use `ERROR`.

See [production operations](production.md) for connection and request limits, readiness
behavior, Prometheus metrics and deployment trust boundaries.
