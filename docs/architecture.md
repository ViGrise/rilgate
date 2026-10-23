# Workspace architecture

The workspace root is virtual; its default package is `rilgate`. The application
composes implementations and the gateway receives ready services. Public modules
are grouped by responsibility instead of collecting every subsystem in `src/`.

## Dependency direction

```text
rilgate -> config, gateway, challenge, pow implementations, pool, web, telemetry, ip-feeds
config -> domain configuration types (challenge, policy, pool, pow, web), protocol, crypto
gateway -> config, challenge, policy, pool, pow API, protocol, crypto, web, core
challenge -> pow API, policy, protocol, crypto, core
ip-feeds -> policy
pow-randomx -> pow API, pool provider contract, protocol, core, randomx-core
pow-hash -> pow API, core
pool -> stratum, protocol, crypto, core
stratum -> protocol, crypto
pow API -> protocol, core
policy -> protocol
web -> its own configuration and resource/runtime modules
```

`rilgate-test-utils` is a development dependency. It shares fixtures and explicit
service composition for gateway tests; production bootstrap remains in the app.
Domain crates do not depend on the configuration loader or application state.
The typed YAML schema groups settings by operator responsibility. Defaults live
in the schema, and assembly resolves immutable domain settings without exposing
the serialization structure to gateway execution. Unknown legacy fields fail.

## Source organization

```text
crates/rilgate-config/src/
  schema/           Typed YAML groups, defaults, durations and sizes
  loader/           YAML parsing, bounded environment expansion and overrides
  assembly/         Validation and immutable runtime resolution
  model/            Domain PoW and quota settings
  runtime/          Listener and upstream settings
crates/rilgate-policy/src/
  policy/schema.rs  Rules, actions, thresholds and condition representation
  policy/presets.rs Built-in policy definitions
  policy/evaluator.rs Ordered rule and threshold execution
  conditions/       Shared bounded matcher compiler and evaluator
  ip_sets/          Address range index, set state and the set registry
crates/rilgate-ip-feeds/src/
  parse.rs          Published IP prefix document parsing
  fetch.rs          Conditional, bounded and redirect-checked HTTP requests
  refresh.rs        Background refresh loop with backoff and jitter
crates/rilgate-crypto/src/
  derivation.rs     Purpose-specific HKDF signing keys
  secret/           Redacted secret representation
  token/            Keyrings, protocol domains, signing and verification
crates/rilgate/src/
  cli/              config check/show, web management and serving
  app/              Algorithm registration and service composition
crates/rilgate-gateway/src/
  transport/        Pingora lifecycle, client IPs and proof quotas
  proxy/            Application gate, forwarding and upstream connections
  auth/             Access cookie parsing and Set-Cookie construction
  web/              Challenge, denied and error pages, assets and response negotiation
  observability/    Health, dependency readiness and Prometheus exposition
  proof/            HTTP and connection-affine WebSocket proof APIs
  runtime/          Shared application state and resources
crates/xtask/src/   Browser WASM staging and ES5 resource generation
scripts/test/       Production browser and container verification runners
tests/e2e/          Playwright configuration, npm manifests and browser tests
tests/container/    Container protocol helpers
config/             Production, commented, custom and RandomX configuration examples
deploy/             Docker, systemd, edge proxy and monitoring examples
```

Other domain crates own algorithms, shared work/claim models, P2Pool sessions,
Stratum framing, web resources and telemetry. Integration tests live with their
owning crate; logs and temporary build/test artifacts live outside the project.

## Ownership and extension

The PoW API accepts `WorkRequest`, `PreparedWork` and `Proof`; it has no signing
keys or HTTP handlers. Signed lifecycle validation belongs to `rilgate-challenge`.
HTTP cookies, error mapping and reverse proxy behavior belong to `rilgate-gateway`.
Algorithm registration belongs to application bootstrap.

New algorithms get real implementation crates when their backend and protocol
are available. See [algorithm development](algorithms.md). Unimplemented algorithms,
replay storage, vardiff and fuzz targets
are not represented by empty crates or placeholder modules.

Canonical embedded resources are in root `web/`. The archived
[web subsystem proposal](proposals/web.md) records the original design goals;
[web resources](web.md) describes the implemented behavior and the tree above
gives the implemented paths. Operator resources are seeded and loaded
from the configured runtime root, independently of repository source paths.

Rust integration tests belong to their owning crate. Node browser tests remain
in root `tests/`; native RandomX tests requiring substantial memory are explicitly
ignored by default.

## Runtime invariants

Challenge pages stay at the requested application URL. Their APIs and assets
resolve beneath the configured `site.base_path`. Policy rules choose algorithms
and difficulty independently of the global default.

Fast and RandomX local proofs can cross gateway instances sharing signing keys
and compatible configuration. P2Pool acceptance remains attached to one browser
WebSocket and its retained gateway Stratum session. RandomX work requires the
configured disclosure and explicit browser consent.

Each challenge service prepares keyed HMAC states for presentation, challenge,
receipt and access tokens. Requests clone these states and use separate token
buffers; signatures retain the purpose-bound HMAC-SHA256 wire format and
constant-time verification. Prepared states belong to the service configuration
and are never shared across signing keys. Single-secret configuration derives
independent challenge and authentication keys with HKDF before service construction.

Native and browser RandomX share `randomx-core` and reference vectors. Light mode
uses a 256 MiB cache and derives dataset items on demand. Native verification
reuses the seed cache on a dedicated thread, keeping hashing off Tokio workers.

Embedded challenge HTML is rendered once per locale and snapshot, with a small
JSON-safe bootstrap inserted per response. Custom templates retain dynamic
rendering. Pooled HTTP RandomX issuance shares one rotating job snapshot per
difficulty; session budgets and idle deadlines remain bounded. Native dataset
execution precomputes seed-bound superscalar operands, including reciprocals,
when constructing the cache; original programs remain available to the WASM
kernel generator and reference checks.

## HTTP transport

`rilgate-gateway::transport::server::Gateway` runs Pingora's HTTP proxy application
on the existing Tokio runtime and TCP accept loop. Pingora handles HTTP/1.1,
HTTP/2, upstream pooling, TLS, streaming, and application WebSocket tunnels.
Local API, health, assets, and policy-generated pages write directly to the
downstream session. The proof WebSocket uses a bounded in-memory duplex adapter
with tungstenite framing and the shared Stratum session logic. There is no
internal HTTP listener or Unix socket.

Application state owns the policy/challenge services, P2Pool provider, limiter,
web snapshot manager, and reload watcher handles. Shutdown stops acceptance,
cancels proof WebSockets, notifies Pingora, and drains connections for up to
30 seconds. The socket digest supplies peer addresses for rate limiting;
forwarded chains influence client IP only when the socket peer belongs to an
explicit trusted proxy network. That resolved IP is shared by policies and quotas.
