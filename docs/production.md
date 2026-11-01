# Production operations

Rilgate remains experimental. A pass of the release checks establishes functional
coverage. It does not guarantee protection against all attacks, and it does not prove
the capacity of every host. Before you accept public traffic, use a staging deployment with the real
edge, upstream and client devices.

## Configuration and network boundary

Use `config/rilgate.production.yml` or the commented distribution template. Both use the balanced preset:
Fast difficulty 2 for application traffic, with robots.txt, favicon.ico and ACME
exceptions, and a six-hour access lifetime. Review these settings and exceptions for
your expected clients and abuse model. RandomX requires explicit policies and Stratum pool
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
a listener or contact the upstream or pool. Resolved output masks keys, pool
logins, proxy URLs and credentials.

Validate filesystem overrides with `web validate`. `--strict` also fails on [environment
check](configuration.md#environment-checks) warnings. Run it in the deployment pipeline so a warning blocks
rollout. The template uses `server.listen: ":8080"`, an IPv4 wildcard listener. Use `127.0.0.1:8080` for
a host-local edge, or a list such as `[":8080", "[::]:8080"]` to add IPv6. Compose publishes only on
host loopback.

1. Configure TLS termination at a trusted edge.
2. Restrict direct gateway access to the edge, load balancer and monitoring network.
3. Restrict the application upstream so public clients cannot bypass the gateway.

Production cookies need HTTPS.

Remote IP and ASN set feeds are fetched in the background, never on the request path.
Allow outbound HTTPS to each configured feed host, including `check.torproject.org` for Tor exits
and `raw.githubusercontent.com` for the X4BNet ASN example.
Readiness reports `asn_sets` alongside `ip_sets` and requires each remote set to load once.
ASN metrics are `rilgate_asn_set_entries`, `rilgate_asn_set_stale`,
`rilgate_asn_set_age_seconds` and `rilgate_asn_set_refresh_failures_total`, with the `set` label.
`config check` never contacts publishers, and startup does not wait for them. Allow
outbound HTTPS to the configured feed hosts. Keep DNS available. Until every remote set
loads successfully once, an instance reports not ready. A feed outage after a successful
load keeps the last valid snapshot and leaves readiness unaffected.

If you use nginx or Caddy on the same host, configure:

```yaml
server:
  trusted_proxies: ["127.0.0.1/32", "::1/128"]
```

For containers, use the actual proxy source address or a dedicated, access-controlled
proxy subnet. The default trusts no proxies. Never trust an entire public network.

The edge must overwrite incoming client headers as the examples do. An upstream that
independently trusts forwarded headers must also configure its own trust boundary.
[Client address and forwarding](configuration.md#client-address-and-forwarding) describes
how Rilgate resolves the client address and what the upstream receives.

## Web frontends and caches

An asset request without a valid `rilgate_auth` cookie can receive HTML at the asset
URL. This can occur with bots, link previews, prefetches or an expired cookie.
Module scripts then fail with a strict MIME type error, and the app loses client-side
routing. The browser receives the cookie after a successful challenge.

Allow public, immutable static asset paths before challenge rules. Presets reject
custom rules in `policy.rules`. Rules run in declaration order, and
`allow` is terminal:

```yaml
policy:
  default_action: allow
  rules:
    - !include builtin:rules/public-endpoints.yml
    - name: static-assets
      when: {path: {prefix: /assets/}}
      action: allow
    - !include builtin:rules/challenge-all.yml
```

Only allow paths that hold public files. Use `/assets/` for Vite,
`/_next/static/` for Next.js, or `/_astro/` for Astro (or its configured
`build.assets` directory).

HTML challenges default to 200 and carry `Cache-Control: no-store` and
`Vary: Accept, Content-Type, X-Requested-With, Accept-Language, Cookie`.
A CDN rule that overrides origin cache headers can store HTML under an asset URL
and serve it to visitors who passed the challenge. Cloudflare does not use
`Vary: Cookie` in its cache key. Make the rule follow origin cache headers, or
bypass the cache when `rilgate_auth` is absent. Purge affected URLs if HTML was cached.

Most caches store a 403 response only when a rule tells them to. Behind such caches,
set `site.responses.challenge_status: 403`. The default remains 200. See
[Response status codes](web.md#response-status-codes) for allowed values and JSON responses.

A Workbox `CacheFirst` runtime cache can store 200 responses without checking the
content type. Add a scripts cache plugin with `cacheWillUpdate` to require
`response.ok` and a `content-type` containing `javascript`. Use
`cachedResponseWillBeUsed` to require the same content type on existing entries.
Return `null` from either hook when its check fails.

Use a corresponding content-type check for stylesheet caches. Workbox precache entries
are fetched at install and checked only for an OK status. Keep precached public assets
on the allowed paths. Remove affected service-worker cache entries if HTML was stored.

Replace `<file>.js` with a real build output file and run without a cookie:

```bash
curl -sS -o /dev/null -w '%{http_code} %{content_type}\n' -A 'Mozilla/5.0' 'https://example.com/assets/<file>.js'
```

The response must have the asset content type, such as `application/javascript`,
and must not be `text/html`. Then complete a browser challenge and check app loading,
client-side navigation and reloads.

## Pool network access

Some networks and hosting providers block Stratum traffic or known pool hosts.
Test outbound access from the gateway host before deployment. A working DNS
lookup does not prove that the Stratum port is reachable.

Use one of these routes when direct access is blocked:

- Put the pool behind a WireGuard tunnel or an SSH local port forward. Point the
  pool address at the private tunnel endpoint.
- Configure SOCKS5, SOCKS5h or HTTP CONNECT proxies. An SSH dynamic forward can
  supply a local SOCKS5 proxy, for example `ssh -N -D 127.0.0.1:1080 tunnel-host`.
- Run the gateway or a pool relay on a separate host with permitted pool access.

Keep proxy credentials on a private network or tunnel. Proxy authentication
uses plain text on this hop. Pool TLS remains end to end through a proxy. A
public pool requires TLS in production. The gateway always verifies its
certificate. Use `tls.ca` for a private CA and `tls.server_name` when the tunnel
address differs from the pool certificate name.

If you use xmrig-proxy, select simple mode. NiceHash mode reserves nonce bytes,
which prevents the browser from using the full nonce field.

Use pooled job sessions with public pools. Per-challenge sessions open one
connection for each job and can reach the pool's connection limit. Check that
backup logins pay the intended wallets. Monitor connection, failover and
mismatch counters by configured name:

| Counter                                          | Labels                                  |
| ------------------------------------------------ | --------------------------------------- |
| `rilgate_stratum_pool_connections_total`         | `pool`, `outcome`: connected or failed  |
| `rilgate_stratum_pool_failover_total`            | `from`: pool name                       |
| `rilgate_stratum_proxy_connections_total`        | `proxy`, `outcome`: connected or failed |
| `rilgate_stratum_pool_difficulty_mismatch_total` | `pool`                                  |

Logs, metrics and readiness responses exclude pool logins, passwords, proxy URLs
and proxy credentials. Keep names free of private information.

## Proof of work deployment

### HashWX

Use [the HashWX example](../config/examples/hashwx.yml) with a custom
policy and local verification. Deploy HashWX support on all gateway instances
before changing the policy. Older builds reject the `hashwx` token variant.
Keep shared signing keys and compatible lifetimes across instances.

The gateway creates each HashWX challenge locally. It verifies one interpreted
hash on a blocking task after signature, version, lifetime, verification mode and
host checks. The normal proof API quotas and request limits stay in front of
this work. It needs no pool or dedicated verifier queue. The measured 25.9
microsecond kernel cost excludes the HTTP handler, signature checks, seed
hash and blocking-task scheduling. It is not a production latency guarantee.

Choose the production cost as described in
[HashWX configuration](configuration.md#hashwx). Three fixed solve trials have wide
variation; use hash rates and more trials to assess the expected cost. Keep explicit
policy difficulties within the intended budget.

Review edge CSP headers on worker responses. Allow worker creation and WASM
compilation as described in [browser support](web.md#hashwx). A worker policy
that blocks all WASM makes HashWX unavailable.

The [license review](proposals/hashwx-license-review.md) records the specification input,
earlier fetch and independent implementation. The dependency checks retain the
repository's LGPL exclusion.

### Remote RandomX verifier

Run `rilgate verifier serve` on a private network. Allow only gateway hosts to
reach its TLS listener. Keep the operations listener on the monitoring network.
A verifier needs no signing keys, tokens, cookies or pool connection.

Use a private CA for this channel. Configure the gateway with a CA file, client
certificate and client key. Configure the verifier with its server certificate,
server key and a client CA. The gateway checks the certificate name in
`verify.remote.server_name`, which defaults to the endpoint host. Production
requires HTTPS and all client credentials.

Trust only verifiers that you operate. A trusted verifier controls the hash
result and can cause a gateway to accept an invalid proof. TLS protects the
channel; it does not protect against a compromised verifier.

For certificate rotation, add the new CA to each trust file first. Deploy the
new certificates next. Remove the old CA after all peers use the new
certificates. Restart the processes to load changed files. Keep gateway and
verifier clocks synchronized. The protocol rejects deadlines in the past or
more than 60 seconds in the future with `503` and `{"reason":"deadline"}`.
`pow.algorithms.randomx.verify.remote.on_deadline_rejected` defaults to `fallback`.
This policy tries the next hasher. Set `reject` to return `401` without fallback.
With `[remote]` and `fallback`, a rejected deadline returns `503`.

Each verifier has one worker and at most one resident 256 MiB cache. A client
certificate can hold at most half its admission limit, with a minimum of one.
Give each gateway its own client certificate.
A closed connection or expired request drops queued work. Before it rejects a new
request, a full worker queue removes this work to free capacity. This also applies
to the local hasher of a gateway. A hash that already started finishes, but its
result is discarded. A remote timeout followed by local fallback can therefore
compute the same proof twice.

`[remote, local]` can make all gateways allocate a cache during one verifier
outage. Reserve memory for that condition. `[remote]` keeps gateway cache use at
zero and fails with `503` during an outage. A failed hash never grants access.
A cold verifier remains ready. A hash initialization failure makes it not ready
for one second, then permits another attempt. During shutdown it stays not ready,
stops new hash connections and allows up to 30 seconds for active requests to
finish. Operations have a separate connection limit. Test the first hash with a timeout long enough
for cache construction on the deployment host.

Monitor these counters without endpoint or certificate labels:

| Metric                                    | Labels                                                                                   |
| ----------------------------------------- | ---------------------------------------------------------------------------------------- |
| `rilgate_randomx_hash_total`              | `hasher`: local or remote; `outcome`: hashed, unavailable or invalid                     |
| `rilgate_randomx_hash_fallback_total`     | `from`: local or remote; `reason`: timeout, connection, overloaded, protocol or deadline |
| `rilgate_verifier_service_requests_total` | `outcome`: hashed, invalid, overloaded, expired or cancelled                             |

The gateway also exposes `rilgate_randomx_remote_duration_seconds`. The verifier
exposes its request counter only on its operations listener when
`server.metrics_enabled` is true. Readiness of a remote-only gateway probes the
verifier only when a rule selects RandomX and
`pow.algorithms.randomx.verify.mode` is `gateway`. The probe uses `server.readiness_timeout`.

## Capacity and quotas

`server.max_connections` bounds accepted TCP connections, including idle clients. Excess connections
are closed. It also sets how many idle upstream connections the gateway keeps for reuse. `server.max_requests` bounds concurrent requests, including streams and upgraded
WebSockets.

Excess requests receive `503` and `Retry-After: 1`. HTTP/2 streams per connection are
bounded by `server.h2_max_streams`. Existing body limits, deadlines, verifier queue capacity and
Stratum pool session limits remain enforced. Size OS file limits and container memory for
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

| Route          | Meaning                                                                                                                                                                                                                                                                  |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `/api/live`    | The process can serve HTTP requests                                                                                                                                                                                                                                      |
| `/api/health`  | Existing web diagnostics and cumulative pool failures                                                                                                                                                                                                                    |
| `/api/ready`   | Checks upstream TCP connectivity and an initial successful load of every configured remote IP and ASN set. When a configured challenge rule selects RandomX, at least one pool must connect through its proxies and configured TLS, unless a RandomX fallback is configured. Returns 503 when unavailable or during drain |
| `/api/metrics` | Prometheus exposition, enabled only with `server.metrics_enabled: true`                                                                                                                                                                                                  |

With a configured RandomX fallback, pool readiness does not affect `/api/ready`.
The response still includes `pool` and `pools`. See [RandomX fallback](pow.md#when-every-pool-is-unavailable).

Readiness probes have a positive timeout up to 30s (default 2s). They probe dependencies
concurrently. Only one probe runs at a time per instance.

Upstream TCP readiness does not check application HTTP responses. Pool readiness
checks proxy and TLS handshakes. It does not check Stratum login, available jobs
or proof acceptance. Each response lists pool names and results. Pool probes run
concurrently. Each pool probe has its own readiness timeout and does not change
cooldowns or failover counters. Monitor real request errors
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
cover IP set prefix counts, ASN set entry counts, age, staleness and refresh failures, and unresolved policy
decisions. For RandomX, metrics cover queue depth, active hashing, completions and
rejected work.

HTTP duration includes streaming and WebSocket lifetimes. Labels exclude paths, hosts
and client IPs to keep cardinality bounded, except a bounded configured IP or ASN set name.
Concurrent gauges and histograms are snapshots. Use `RILGATE_LOG_FORMAT=json` for JSON logs and
`RUST_LOG` for verbosity.

`deploy/monitoring/` contains a Prometheus scrape example and initial alerts. Tune thresholds
for the workload. Monitor readiness externally in addition to metrics.
Exercise alerts during staging failure tests.

`rilgate_pow_fallback_total{from="randomx",to="<fallback id>"}` counts each fallback
challenge page or JSON challenge response served by the gate.

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
must cover presentations, Fast, HashWX and RandomX challenges and receipts. Auth keys must cover
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

Image scanning does not prove safety. `.config/deny.toml` ignores no advisory.
`scripts/advisories.sh` requires an open GitHub issue with the label
`dependency-advisory` for each advisory in the lockfile. Each issue links the
upstream issues and states when to close it. CI fails when an advisory has no
open issue. A scheduled workflow checks the committed lockfile daily and opens
an issue for each new advisory. On each release, review the open
`dependency-advisory` issues.

After these checks pass, pushes to `main` publish two multi-architecture images
(`linux/amd64`, `linux/arm64`) to `ghcr.io/vigrise/rilgate`. The default image is based
on Alpine and contains the static musl build. Its tags are `main` and `sha-<commit>`,
and the same tags with the suffix `-alpine`, for example `main-alpine`. The Debian image
contains the dynamic glibc build. Each of its tags has the suffix `-debian`, for example
`main-debian`. Both images run the same configuration as user
`10001`. Use the Debian image to extend it with Debian packages. A push of a `v*` tag
releases that revision:

1. Set `version` under `[workspace.package]` in `Cargo.toml`.
2. Commit the change.
3. Create the tag `v<version>`.
4. CI repeats the checks. It builds native binaries for Linux (amd64, arm64), macOS
   (amd64, arm64) and Windows (amd64, arm64). Linux amd64 and arm64 have a dynamic glibc
   build and a static musl build. CI cross-builds static musl binaries for Linux
   ppc64le and riscv64 with `cargo-zigbuild`, and a FreeBSD amd64 binary with cross-rs.
   CI smoke tests each native binary with `--version` and `config check`, runs the
   static build in an empty image, and runs the Linux cross builds under QEMU. The
   FreeBSD binary gets no smoke test.
5. The default image gains `<version>`, `<major>.<minor>`, `<major>` (from 1.0 onward)
   and `latest` tags, and the same tags with the `-alpine` suffix. The Debian image gains
   the same tags with the `-debian` suffix, such as `latest-debian`. The tags `alpine`
   and `debian` move to the release. A tag containing `-`, such as `v1.2.0-rc.1`,
   publishes a prerelease that never moves `latest`, `latest-alpine`, `latest-debian`,
   `alpine` or `debian`.
6. A GitHub Release attaches these files, with generated release notes:
   - `rilgate-<os>-<arch>.tar.gz` (`.zip` on Windows), and
     `rilgate-linux-<arch>-static.tar.gz` for the static musl Linux build. Each archive
     holds the binary, `LICENSE` and `rilgate.yml`.
   - The bare binary of each build, used by `rilgate upgrade`.
   - `rilgate_<version>_<arch>.deb`, `rilgate-<version>-1.<arch>.rpm` and the
     systemd-sysext image `rilgate-sysext-linux-<arch>.raw` for Linux amd64, arm64,
     ppc64le and riscv64. The packages hold the static build.
   - `rilgate-windows-<arch>.msi` for Windows amd64 and arm64.
   - `rilgate-src-vendor-<version>.tar.gz`, the source with all Cargo dependencies
     vendored for offline builds.
   - `SHA256SUMS` with the checksum of each of these files.
   - `install.sh` and `install.ps1`.
   - When the `RELEASE_GPG_PRIVATE_KEY` secret is set (with the optional
     `RELEASE_GPG_PASSPHRASE`), a detached signature `<file>.asc` for each file and the
     public key `rilgate-release-key.asc`. Without the secret, the release has no
     signatures and the job shows a warning.
     `scripts/release/create-signing-key.sh "<name> <email>" <backup-dir>` creates the
     key, sets the secrets with `gh` and writes the secret key and its revocation
     certificate to the backup directory for offline storage. An empty passphrase
     leaves the backup key unencrypted and removes `RELEASE_GPG_PASSPHRASE`.

A pull request with the `release-build` label runs the release builds and keeps the
files as workflow artifacts. It publishes no image and no release. The Linux amd64 and
arm64 binaries and packages come from the image job, which runs only for tags, so a
pull request does not build them.

The release fails when the tag and the package version differ.

The dynamic Linux build comes from the image builder stage on Debian bookworm, so it
needs glibc 2.36 or newer. The static musl build runs on any Linux distribution of its
architecture. Both use mimalloc as the allocator. They had the same request throughput in
the measurements in `docs/performance.md`.
`install.sh` installs the glibc build when the system has glibc 2.36 or newer, and the
static build in other cases. `rilgate upgrade` keeps the build that runs.

All release binaries contain OpenSSL, because `pingora-openssl` builds it from source.
They do not use a system OpenSSL. A system package upgrade does not apply OpenSSL
security fixes to them, and an image scan does not report OpenSSL vulnerabilities in
them. Rebuild and release them for OpenSSL security releases. Keep the previous
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
cargo build --release --locked -p rilgate-gateway --example null-upstream
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
