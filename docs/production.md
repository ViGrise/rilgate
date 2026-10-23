# Production operations

Rilgate remains experimental. A pass of the release checks establishes functional
coverage. It does not guarantee protection against all attacks, and it does not prove
the capacity of every host. Before you accept public traffic, use a staging deployment with the real
edge, upstream and client devices.

## Configuration and network boundary

Use `config/rilgate.production.yml` or the commented distribution template. Both use the balanced preset:
Fast difficulty 2 for application traffic, with robots.txt, favicon.ico and ACME
exceptions, and a six-hour access lifetime. Review these settings and exceptions for
your expected clients and abuse model. RandomX requires explicit policies and P2Pool
configuration.

1. Generate a random secret with `openssl rand -hex 32`.
2. Inject `RILGATE_SECURITY_SECRET` through the deployment secret store.
3. Do not commit the secret.

Rilgate uses HKDF-SHA256 to derive independent challenge and authentication signing
keys. Explicit `security.keys` are available for independent rotation. All environments
reject unknown fields and conflicting configuration. Production forbids debug scores.

```bash
./target/release/rilgate --config /etc/rilgate/rilgate.yml config check --strict
./target/release/rilgate --config /etc/rilgate/rilgate.yml config show --defaults
```

These commands validate keys, limits, policies and algorithm selection. They do not open
a listener or contact the upstream or P2Pool. Resolved output redacts all keys and pool
passwords.

Validate filesystem overrides with `web validate`. `--strict` also fails on [environment
check](configuration.md#environment-checks) warnings. Run it in the deployment pipeline so a warning blocks
rollout. The template uses `server.listen: ":8080"`, an IPv4 wildcard listener. Use `127.0.0.1:8080` for
a host-local edge, or a list such as `[":8080", "[::]:8080"]` to add IPv6. Compose publishes only on
host loopback.

1. Configure TLS termination at a trusted edge.
2. Restrict direct gateway access to the edge, load balancer and monitoring network.
3. Restrict the application upstream so public clients cannot bypass the gateway.

Production cookies need HTTPS.

Remote IP set feeds are fetched in the background, never on the request path.
`config check` never contacts publishers, and startup does not wait for them. Allow
outbound HTTPS to the configured feed hosts. Keep DNS available. Until every remote set
loads successfully once, an instance reports not ready. A feed outage after a successful
load keeps the last valid snapshot and leaves readiness unaffected.

If you use nginx or Caddy on the same host, configure:

```yaml
server:
  trusted_proxies: ['127.0.0.1/32', '::1/128']
```

For containers, use the actual proxy source address or a dedicated, access-controlled
proxy subnet. The default trusts no proxies. Never trust an entire public network.

`X-Forwarded-For` is read only from trusted TCP peers, from right to left until an untrusted
hop is found. Malformed trusted chains are rejected. `Forwarded` is not used. The edge
must overwrite incoming client headers as the examples do. An upstream that
independently trusts forwarded headers must also configure its own trust boundary.
Rilgate does not rewrite the application's forwarding headers.

## Capacity and quotas

`server.max_connections` bounds accepted TCP connections, including idle clients. Excess connections
are closed. `server.max_requests` bounds concurrent requests, including streams and upgraded
WebSockets.

Excess requests receive `503` and `Retry-After: 1`. HTTP/2 streams per connection are
bounded by `server.h2_max_streams`. Existing body limits, deadlines, verifier queue capacity and
P2Pool session limits remain enforced. Size OS file limits and container memory for
these settings and the roughly 256 MiB RandomX cache.

Per-client proof quotas use the resolved client IP, but both client and global quotas
remain local to an instance. The nginx example provides shared client quotas at one edge
in front of any number of gateway instances. It also limits ordinary requests and client
connections. If a strict cluster-wide quota is required, multiple independent edge nodes
need a common quota service or a load-balancer policy. The Caddy example does not
provide shared quotas.

Before you stop an instance, drain traffic at the load balancer. The gateway accepts no new connections. It allows up to 30 seconds for connection drain. The deployment
examples allow 45 seconds before forced termination. Long-lived application WebSockets
can be interrupted and clients must reconnect.

## Health, metrics and alerts

The following routes are relative to `site.base_path`:

| Route | Meaning |
| --- | --- |
| `/api/live` | The process can serve HTTP requests |
| `/api/health` | Existing web diagnostics and cumulative pool failures |
| `/api/ready` | Checks upstream TCP connectivity and an initial successful load of every configured remote IP set. When a configured challenge rule selects RandomX, it also checks P2Pool TCP connectivity. Returns 503 when unavailable or during drain |
| `/api/metrics` | Prometheus exposition, enabled only with `server.metrics_enabled: true` |

Readiness probes have a positive timeout up to 30s (default 2s). They probe dependencies
concurrently. Only one probe runs at a time per instance.

TCP readiness does not assert successful application HTTP responses, TLS handshakes,
Stratum login, available mining jobs or proof acceptance. Monitor real request errors
and pool failures as well. Saturated request capacity can also produce 503 for probes.
Operational endpoints do not reserve unlimited resources during overload.

Keep operational endpoints private. Set `server.operations.listen` to a loopback address such as
`127.0.0.1:9090`, or an address on a private monitoring network. Direct probes and
Prometheus to that listener.

Public listeners always return 404 for these routes. Operations listeners serve only
these four routes and share connection and request capacity with public traffic. The
default empty list disables operational routes entirely. `config/rilgate.production.yml` sets `127.0.0.1:9090`
and enables metrics. Container orchestrator probes need an operations address reachable
by the probe, such as the pod IP. Loopback is reachable only within the container
network namespace.

Edge examples retain rules that block their public paths as defense in depth. If you
change `site.base_path`, adjust those rules. These endpoints have no authentication.

Metrics cover HTTP status classes and duration histograms, active connections and
requests, overload rejections, pool sessions and failures, and web reloads. They also
cover IP set prefix counts, age, staleness and refresh failures, and unresolved policy
decisions. For RandomX, metrics cover queue depth, active hashing, completions and
rejected work.

HTTP duration includes streaming and WebSocket lifetimes. Labels exclude paths, hosts
and client IPs to keep cardinality bounded, except a bounded configured IP set name.
Concurrent gauges and histograms are snapshots. Use `RILGATE_LOG_FORMAT=json` for JSON logs and
`RUST_LOG` for verbosity.

`deploy/monitoring/` contains a Prometheus scrape example and initial alerts. Tune thresholds
for the workload. Monitor readiness externally in addition to metrics.
Exercise alerts during staging failure tests.

## Key rotation and token lifetime

Keys are loaded at startup. Each proof keyring covers presentations, challenges and
receipts. The authentication keyring covers access cookies.

Both use purpose-separated signatures. IDs are included in signatures and must contain 1
to 32 ASCII letters, digits, underscores or hyphens. Use a new ID for every new secret.
At most eight extra verification keys are accepted per keyring.

```yaml
security:
  keys:
    challenge: "${RILGATE_SECURITY_KEYS_CHALLENGE}"
    auth: "${RILGATE_SECURITY_KEYS_AUTH}"
    challenge_rotation:
      active_id: proof-next
      previous:
        proof-current: "${RILGATE_SECURITY_KEYS_CHALLENGE_PREVIOUS}"
    auth_rotation:
      active_id: auth-next
      previous:
        auth-current: "${RILGATE_SECURITY_KEYS_AUTH_PREVIOUS}"
```

When you select explicit keys, remove `security.secret`. Treat all current and verification
keys as secrets. Keep secret-bearing files outside Git with restricted permissions.
Single-secret mode derives unversioned signing keys. If named-key rotation is required,
use explicit-key mode.

Use a three-stage rolling deployment.

First stage:

1. Continue to sign with the current ID and secret.
2. Add the next ID and secret to the verification map on every instance.

Second stage:

1. After every instance accepts both keys, switch the active ID and secret to the next pair.
2. Retain the current pair in `previous`.
3. Deploy to every instance.

Third stage:

1. After the longest accepted token lifetime elapses since the last old signer stopped, remove the old verification keys.

The `previous` map accepts staged future keys as well as historical keys. Proof keys
must cover presentations, Fast and RandomX challenges and receipts. Auth keys must cover
the auth lifetime. Include clock skew and rollout time in the window.

Unversioned tokens can be retained with a verification entry named `legacy`. Before
you switch active signers, stage named verification keys on all instances. Keep the old
unversioned signing key under `legacy` for the overlap window.

Tokens are intentionally stateless, reusable bearer credentials within their signed
lifetime. Authentication cookies are also portable between machines unless `security.auth.bind`
is configured. Binding ties a cookie to the client network, browser family and chosen
headers, so a copied cookie fails from a different network or browser. It does not stop
cookie sharing between clients that match the same binding. It does not identify a
person or revoke a cookie. See [configuration](configuration.md#authentication-cookie-binding).

A new check of a challenge can issue new receipts until challenge expiry. Repeated
receipt exchange preserves the original receipt issue time and cannot extend access from
that receipt. Proofs do not establish a unique person or prevent credential sharing.
Applications can need exactly-once work, immediate per-token revocation or account-level
limits. These requirements need a shared atomic store for consumption or revocation, or
enforcement at the application or edge. Shorten challenge and auth lifetimes to fit the
abuse model.

## Release, rollout and rollback

CI gates Rust, WASM and Node checks, real HTTPS browser flows, native RandomX reference
vectors, and a short performance regression workload. It also gates dependency
advisories, licenses and sources, production container smoke tests, edge config
validation, and fixable high or critical image vulnerabilities. Unfixed OS
vulnerabilities need separate review.

Image scanning does not prove safety. `.config/deny.toml` records two explicit maintenance-only
exceptions: derivative (RUSTSEC-2024-0388), retained by Pingora, and unsafe-libyaml
(RUSTSEC-2024-0449), retained by Pingora and the operator configuration parser. No known
vulnerability advisory is exempted. On each release, review these dependencies.
When their upstream dependency chains change, remove the exceptions. A scheduled workflow also
checks advisories against the committed lockfile daily, so new advisories surface
between pushes.

After these checks pass, pushes to `main` publish a multi-architecture image
(`linux/amd64`, `linux/arm64`) to `ghcr.io/vigrise/rilgate`. Its tags are `main` and
`sha-<commit>`. A push of a `v*` tag releases that revision:

1. Set `version` under `[workspace.package]` in `Cargo.toml`.
2. Commit the change.
3. Create the tag `v<version>`.
4. CI repeats the checks. It builds native binaries for Linux (amd64, arm64), macOS
   (amd64, arm64) and Windows (amd64, arm64). It smoke tests each binary with
   `--version` and `config check`.
5. The image gains `<version>`, `<major>.<minor>`, `<major>` (from 1.0 onward) and
   `latest` tags. A tag containing `-`, such as `v1.2.0-rc.1`, publishes a
   prerelease that never moves `latest`.
6. A GitHub Release attaches `rilgate-<os>-<arch>.tar.gz` (`.zip` on Windows). Each archive holds
   the binary, `LICENSE` and `rilgate.production.yml`. The release also attaches the bare binary
   `rilgate-<os>-<arch>` (`.exe` on Windows), used by `rilgate upgrade`. It attaches
   `SHA256SUMS` for both, and `install.sh`, with generated release notes.

The release fails when the tag and the package version differ.

Linux binaries come from the image builder stage, so they need glibc 2.36 or newer and
OpenSSL 3 (`libssl3`). macOS and Windows binaries link OpenSSL statically through the
`vendored-openssl` feature. Rebuild them for OpenSSL security releases. Keep the previous
immutable image digest or archive and its configuration for rollback.

Run locally with the prerequisites in `docs/development.md`:

```bash
scripts/test.sh
cargo test -p randomx-core --release --locked --test vectors -- --ignored --test-threads=1
cargo test -p rilgate-pow-randomx --release --locked --lib -- --ignored --test-threads=1
cargo build --locked
scripts/test/browser.sh
cargo deny --locked --config .config/deny.toml check advisories licenses sources bans
docker build -f deploy/docker/Dockerfile -t rilgate:production-check .
scripts/test/container.sh
```

Before rollout:

1. Run the release performance gate on a representative host.
2. Record throughput, latency, CPU, RAM and rejection rates.
3. Conduct a longer soak at expected peak load with real upstream behavior.
4. Exercise dependency loss, recovery and termination during active traffic.

There is no universal RPS target.

```bash
scripts/build.sh --release
scripts/perf-gate.sh
```

1. Deploy one instance.
2. Check readiness.
3. Complete a browser proof.
4. Check alerts.
5. Increase traffic gradually as you monitor error and latency budgets.
6. If budgets are exceeded, restore the artifact and configuration together.
7. Preserve compatible verification keyrings across both versions so rollback does not
   reject valid cookies or outstanding work.
