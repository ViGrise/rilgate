# Production operations

Rilgate remains experimental. Passing the release checks establishes functional
coverage, not a guarantee against all attacks or a capacity claim for every host.
Use a staging deployment with the real edge, upstream and client devices before
accepting public traffic.

## Configuration and network boundary

Start from `config/rilgate.production.yml`, not the development example. It
challenges every application route regardless of User-Agent, except the explicit
ACME and robots.txt allow rules. Review those exceptions for your application.
Fast difficulty 4 and a 30-minute access lifetime are starting values; measure
solve time and abuse resistance for your expected clients.

Generate two independent random signing secrets, for example with
`openssl rand -hex 32`, and inject `POW_SIGNING_KEY` and
`POW_AUTH_SIGNING_KEY` through the deployment secret store. Do not commit them.
Production requires explicit, nonempty `pow.thresholds`, strong signing keys,
and disabled debug weight headers. Unknown scalar and sequence configuration
keys are rejected. Check the resulting configuration before rollout:

```bash
./target/release/rilgate --config /etc/rilgate/rilgate.yml check-config
```

The command validates keys, limits, policy expressions and algorithm selection
without opening a listener or contacting upstream/P2Pool. Validate filesystem
web overrides separately with `rilgate web validate DIRECTORY`.

The production template binds to `127.0.0.1`; `serve.host` or `SERVE_HOST`
selects the bind IP. The container sets `SERVE_HOST=0.0.0.0` inside its network
namespace, while Compose publishes only on host loopback.

Terminate TLS at a trusted edge and restrict direct gateway access to the edge,
load balancer and monitoring network. Restrict the application upstream so public
clients cannot bypass the gateway. Production cookies require HTTPS.

When using nginx or Caddy on the same host, configure:

```yaml
serve:
  trusted_proxies: ['127.0.0.1/32', '::1/128']
```

For containers, use the actual proxy source address or a dedicated, access-controlled
proxy subnet. The default trusts no proxies. Never trust an entire public network.
`X-Forwarded-For` is read only from trusted TCP peers, from right to left until
an untrusted hop is found. Malformed trusted chains are rejected. `Forwarded` is
not used. The edge must overwrite incoming client headers as the examples do.
An upstream that independently trusts forwarded headers must also configure its
own trust boundary; Rilgate does not rewrite the application's forwarding headers.

## Capacity and quotas

`serve.max_connections` bounds accepted TCP connections, including idle clients;
excess connections are closed. `serve.max_requests` bounds concurrent requests,
including streams and upgraded WebSockets; excess requests receive `503` and
`Retry-After: 1`. HTTP/2 streams per connection are bounded by
`serve.h2_max_streams`. Existing body limits, deadlines, verifier queue capacity
and P2Pool session limits remain enforced. Size OS file limits and container
memory for these settings and the roughly 256 MiB RandomX cache.

Per-client proof quotas use the resolved client IP, but both client and global
quotas remain local to an instance. The nginx example provides shared client
quotas at one edge in front of any number of gateway instances. It also limits
ordinary requests and client connections. Multiple independent edge nodes need
a common quota service or a load-balancer policy if a strict cluster-wide quota
is required. The Caddy example does not provide shared quotas.

Drain traffic at the load balancer before stopping an instance. The gateway
stops accepting connections and allows up to 30 seconds for connection drain.
The deployment examples allow 45 seconds before forced termination. Long-lived
application WebSockets may be interrupted and clients must reconnect.

## Health, metrics and alerts

The following routes are relative to `web.base_path`:

| Route | Meaning |
| --- | --- |
| `/api/live` | The process can serve HTTP requests |
| `/api/health` | Existing web diagnostics and cumulative pool failures |
| `/api/ready` | Upstream TCP connectivity, and P2Pool TCP connectivity when a configured challenge rule selects RandomX; returns 503 when unavailable or draining |
| `/api/metrics` | Prometheus exposition, enabled only with `serve.metrics_enabled: true` |

Readiness probes have a configurable 1-30 second timeout (default 2 seconds),
probe dependencies concurrently, and allow only one probe at a time per instance.
TCP readiness does not assert successful application HTTP responses, TLS
handshakes, Stratum login, available mining jobs or proof acceptance. Monitor
real request errors and pool failures as well. Saturated request capacity can
also produce 503 for probes; operational endpoints do not reserve unlimited
resources during overload.

Keep operational endpoints private. Edge examples block their public paths;
adjust those rules when changing `web.base_path`. Scrape the gateway directly
from the monitoring network. No authentication is added to these endpoints.

Metrics cover HTTP status classes and duration histograms, active connections
and requests, overload rejections, pool sessions/failures, web reloads, and
RandomX queue depth, active hashing, completions and rejected work. HTTP duration
includes streaming/WebSocket lifetimes. Labels exclude paths, hosts and client
IPs to keep cardinality bounded. Concurrent gauges and histograms are snapshots.
Use `RILGATE_LOG_FORMAT=json` for JSON logs and `RUST_LOG` for verbosity.

`deploy/monitoring/` contains a Prometheus scrape example and initial alerts.
Tune thresholds for the workload. Monitor readiness externally in addition to
metrics and exercise alerts during staging failure tests.

## Key rotation and token lifetime

Keys are loaded at startup. Each proof keyring covers presentations, challenges
and receipts; the authentication keyring covers access cookies. Both use
purpose-separated signatures. IDs are included in signatures and must contain
1-32 ASCII letters, digits, underscores or hyphens. Use a new ID for every new
secret. At most eight additional verification keys are accepted per keyring.

```yaml
pow:
  signing_keys:
    active_id: proof-next
    previous:
      proof-current: '<current proof secret from the secret store>'
  auth_signing_keys:
    active_id: auth-next
    previous:
      auth-current: '<current auth secret from the secret store>'
```

`pow.signing_key` / `POW_SIGNING_KEY` and `pow.auth_signing_key` /
`POW_AUTH_SIGNING_KEY` contain the active secrets. Treat files with verification
keys as secrets too; keep them outside Git with restricted permissions.

Use a three-stage rolling deployment:

1. Keep signing with the current ID and secret; add the next ID/secret to the
   verification map on every instance. The `previous` map accepts staged future
   keys as well as historical keys.
2. After every instance accepts both keys, switch active ID/secret to the next
   pair and retain the current pair in `previous`. Roll out to every instance.
3. After the longest accepted token lifetime has elapsed since the last old
   signer stopped, remove the old verification keys. Proof keys must cover
   presentations, Fast/RandomX challenges and receipts; auth keys must cover
   the auth lifetime. Include clock skew and rollout time in the window.

Unversioned tokens can be retained with a verification entry named `legacy`.
When migrating from an older binary that cannot understand key IDs, first
upgrade every instance while keeping unversioned signing (omit `active_id`).
Then stage the next named keys as above, switch to named signing and keep the
old secrets under `legacy` until their lifetimes have elapsed. Never roll back
to a binary that cannot verify the currently issued token format.

Tokens are intentionally stateless, reusable bearer credentials within their
signed lifetime. Rechecking a challenge may issue new receipts until challenge
expiry; repeating receipt exchange preserves the original receipt issue time
and cannot extend access from that receipt. Proofs do not establish a unique
person or prevent credential sharing. Applications requiring exactly-once work,
immediate per-token revocation or account-level limits need a shared atomic
consume/revocation store or enforcement at the application/edge. Shorten challenge
and auth lifetimes to fit the abuse model.

## Release, rollout and rollback

CI gates Rust/WASM/Node checks, real HTTPS browser flows, native RandomX reference
vectors, a short performance regression workload, dependency advisories/licenses/sources, production container smoke tests,
edge config validation, and fixable high/critical image vulnerabilities. Unfixed
OS vulnerabilities need separate review; image scanning does not prove safety.
`.config/deny.toml` records two explicit maintenance-only exceptions: derivative
(RUSTSEC-2024-0388), retained by Pingora, and unsafe-libyaml
(RUSTSEC-2024-0449), retained by Pingora and the operator configuration parser.
No known vulnerability advisory is exempted. Review these dependencies on each
release and remove exceptions when their upstream dependency chains change.

Tagged `v*` revisions and manual release workflows run these checks before
producing a Linux amd64 package with checksum, source revision and toolchain
metadata as workflow artifacts. Artifact creation does not publish or deploy.
Keep the previous immutable artifact and its configuration for rollback.

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

Before rollout, run the release performance gate on a representative host,
record throughput, latency, CPU/RAM and rejection rates, then conduct a longer
soak at expected peak load with real upstream behavior. Exercise dependency loss,
recovery and termination during active traffic. There is no universal RPS target.

```bash
scripts/build.sh --release
scripts/perf-gate.sh
```

Deploy one instance, verify readiness, complete a browser proof and check alerts.
Increase traffic gradually while watching error and latency budgets. Roll back
the artifact/configuration together if budgets are exceeded. Preserve compatible
verification keyrings across both versions so rollback does not reject valid
cookies or outstanding work.
