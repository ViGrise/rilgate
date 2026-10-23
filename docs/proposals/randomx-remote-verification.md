# RandomX remote verification

Status: proposed.

This proposal lets one Rilgate binary compute RandomX verification hashes in its
own process, on a remote verifier, or on both in a configured order. It also lets
a process serve its hasher to other gateways. The goal is to remove the 256 MiB
RandomX cache from gateways that do not need it. The challenge protocol, tokens,
cookies and `pow.verification` do not change.

## 1. Current implementation

| Item | Current behavior |
| --- | --- |
| Hash function | `RandomXVerifier::hash(seed, blob)` returns 32 bytes. It accepts a seed and a blob of 1 to 256 bytes each (`workers.rs:110-114`) |
| Work dimensions | `RandomXAlgorithm::verify` and the Stratum job parser need a 32-byte seed. The blob is at most 256 bytes |
| Target check | `RandomXAlgorithm::verify` compares the hash with the difficulty target after the hash returns (`randomx/mod.rs:170-173`) |
| Deadline | One timeout wraps the hash wait and ends at the challenge expiry. Issuance checks the lifetime again before it issues a receipt |
| Thread model | One dedicated thread with a bounded queue (`pow.algorithms.randomx.verifier_queue`, default 8, range 1 to 1024) |
| Cancellation | A request that the caller drops before it starts is skipped. A hash that started finishes |
| Cache allocation | The verifier and its cache are created on the first hash. A gateway that never verifies RandomX uses no cache memory |
| Cold build | A request that arrives during a cache build waits for the build, then hashes |
| Cache release | The cache stays resident until the process stops. A seed change drops the old cache before it builds the new one |
| Cache ownership | `randomx_core::RandomX` owns one cache and its VMs. `hash` needs `&mut self`, so one cache serves one thread |
| Work source | Both enforcement modes get RandomX jobs from P2Pool. `pow.p2pool` stays necessary with any hasher |
| `p2pool_accepted` | The gateway submits the browser result to P2Pool and issues a receipt on acceptance. It does not call `RandomXAlgorithm::verify` |
| Error mapping | `ApiError::proof` returns `503` only for `CapacityExceeded` and `PowBackendUnavailable`. Every other error becomes `401` |
| Browser | The adapter sends one `/api/verify` request per solution. An error shows the retry control, which reloads the page and discards the solution |
| Token binding | Tokens bind `pow.verification` (`local_verify` or `p2pool_accepted`). The hash location is not in any token |

The hash location and the enforcement mode are separate properties. Enforcement
decides what a proof means: a valid hash, or a share that P2Pool accepted. The
hash location only decides which machine computes the same function. A hash from a
remote verifier gives the same guarantee as a local hash. Thus this proposal
changes the hash location only and keeps enforcement as it is.

## 2. Goals and non-goals

Goals:

1. A gateway can check RandomX proofs without a resident RandomX cache.
2. One binary supports local hashing, remote hashing, or both in a configured order.
3. One process can serve its hasher to other gateways.
4. Hashing fails closed. No configuration accepts a proof without a hash that meets the target.
5. A cold or rebuilding cache never discards a solution that a browser already submitted.
6. The challenge protocol, tokens, cookies and `pow.verification` stay compatible.

Non-goals:

- `p2pool_accepted` stays on the gateway that owns the browser WebSocket. A remote
  verifier cannot replace pool acceptance.
- The remote verifier does not issue work. Gateways keep their P2Pool job source.
- A P2Pool broker, an edge runtime, tenant contexts and per-rule enforcement are
  separate proposals (section 14).

## 3. Decisions

| Decision | Reason |
| --- | --- |
| The remote verifier is a hash oracle | The gateway keeps all token checks, the target comparison and the receipt lifetime check. The verifier holds no signing key and knows nothing about challenges |
| An ordered `order` list selects hashers | One list states which hashers are enabled and which one runs first. A new hasher kind adds one list value |
| The default `order` is `[local]` | Current deployments keep the current behavior without a configuration change |
| A fallback occurs only when a hasher is unavailable | A returned hash is final. A retry on another hasher after a target miss would let a client probe verifiers |
| Requests wait for a cold build | The browser cannot resubmit a solution. A wait inside the deadline keeps the solution, and a `503` loses it |
| mTLS authenticates both sides | The gateway proves its identity with a client certificate and checks the identity of the verifier. TLS protects each response |
| The client is `reqwest` with rustls | The gateway already uses it for IP feeds and upgrades. It supports client identities, custom roots and a disabled redirect policy |
| The server is `hyper` with `tokio-rustls` | The verifier listener is a small API outside the Pingora proxy path. It does not change the gateway accept loop |
| Delivery has two phases | Phase 1 delivers remote hashing with one worker per process. Phase 2 adds several VMs on one cache and the combined gateway and verifier mode |
| An idle cache is released | A gateway or verifier without RandomX traffic returns to zero cache memory |

## 4. Configuration

```yaml
pow:
  algorithms:
    randomx:
      cache:
        idle_timeout: 30m
      verify:
        order: [remote, local]
        remote:
          endpoint: https://verifier.internal:9443
          server_name: verifier.internal
          timeout: 2s
          tls:
            ca: /etc/rilgate/verifier-ca.pem
            client_cert: /etc/rilgate/gateway.pem
            client_key: /etc/rilgate/gateway.key
      serve:
        listen: 10.0.0.5:9443
        workers: 4
        queue: 64
        max_connections: 256
        tls:
          cert: /etc/rilgate/verifier.pem
          key: /etc/rilgate/verifier.key
          client_ca: /etc/rilgate/gateway-ca.pem
```

| Setting | Default | Range and rules |
| --- | --- | --- |
| `cache.idle_timeout` | `30m` | A new `IdleTimeout` type that accepts `0s` (never release) up to `7d`. The existing `Time` type rejects zero |
| `verify.order` | `[local]` | One or two entries. Values are `local` and `remote`, each at most once. An empty list is an error |
| `verify.remote.endpoint` | none | Required when `order` contains `remote`. An `https` URL without a path, query or credentials |
| `verify.remote.server_name` | endpoint host | The name that the verifier certificate must contain |
| `verify.remote.timeout` | `2s` | `100ms` to `30s`. It covers DNS, connect, TLS, upload and the response read |
| `verify.remote.tls.*` | none | `ca` is required. `client_cert` and `client_key` must both be present |
| `serve` | absent | Present: the process serves a hasher on `serve.listen` |
| `serve.workers` | `1` in phase 1. In phase 2, the available CPU count | `1` to `64` |
| `serve.queue` | `64` | `1` to `4096` |
| `serve.max_connections` | `256` | `1` to `65536` |
| `serve.tls.*` | none | `cert` and `key` are required. `client_ca` is required in production |

- `serve.listen` must not equal any `server.listen` or `server.operations.listen`
  address.
- The existing `verifier_queue` keeps its meaning for the local hasher of a gateway.
- Every scalar and list setting gets a flag and a `RILGATE_*` variable, as all
  other settings do.

### Deployment patterns

| Pattern | Configuration | Phase |
| --- | --- | --- |
| Local only (current behavior) | `order: [local]`, which is the default | 1 |
| Remote only, no gateway cache | `order: [remote]` | 1 |
| Remote first, local fallback | `order: [remote, local]` | 1 |
| Local first, remote overflow | `order: [local, remote]` | 1 |
| Dedicated verifier | `rilgate verifier serve` (section 8) | 1 |
| Gateway that also serves other gateways | `order: [local]` with `serve` | 2 |

## 5. Order semantics

`RandomXAlgorithm::verify` keeps one outer timeout that ends at the challenge
expiry. The whole sequence of requests runs inside that timeout.

1. The gateway computes the time left before the challenge expiry.
2. The gateway sends the request to the next hasher in `order`. A remote request
   gets the smaller of `verify.remote.timeout` and the time left.
3. If the hasher returns a hash, the gateway compares it with the target. The result is final.
4. If the hasher is unavailable and time is left, the gateway goes back to step 1.
5. If no hasher returns a hash, the gateway rejects the proof with `503`.

| Outcome | Class | Next hasher | Response to the browser |
| --- | --- | --- | --- |
| Hash returned, target met | hashed | no | Receipt |
| Hash returned, target missed | hashed | no | `401` |
| Invalid seed or input | invalid | no | `401` |
| Local queue full | unavailable | yes | `503` if last |
| Local worker stopped, or cache initialization failed | unavailable | yes | `503` if last |
| Remote connection, DNS or TLS failure | unavailable | yes | `503` if last |
| Remote request timeout | unavailable | yes | `503` if last |
| Remote `503` | unavailable | yes | `503` if last |
| Remote `400` | invalid | no | `401` |
| Remote redirect, other status, malformed or oversized body, hash not 32 bytes | unavailable | yes | `503` if last |
| Challenge expired during the sequence | expired | no | `401` |

- An unavailable result is a `PowBackendUnavailable` error. An exhausted order maps
  to `503` and keeps the browser retry path. A verifier outage never looks like a
  wrong proof.
- The local cache exists only while `local` receives requests. With
  `order: [remote, local]`, the gateway builds a cache only during a remote outage
  and releases it after `cache.idle_timeout`.

### Duplicate work

A remote request that times out can continue on the verifier while the gateway
uses the next hasher. Thus one proof can cause one remote hash and one local hash.
The verifier limits this work:

- It drops a queued request when its deadline passes or its connection closes.
- A hash that started finishes, as in the local hasher. Its result is discarded.
- Per-gateway admission (section 8) limits how much work one gateway can queue.

## 6. Hasher contract

```rust
pub enum HashError {
  /// The hasher cannot answer now. The caller can try the next hasher.
  Unavailable(anyhow::Error),
  /// The request is invalid. The caller must not try another hasher.
  Invalid(anyhow::Error),
}

pub trait RandomXHasher: Send + Sync {
  fn kind(&self) -> HasherKind;
  fn hash(&self, seed: Vec<u8>, input: Vec<u8>, deadline: Instant)
    -> BoxFuture<'_, Result<[u8; 32], HashError>>;
}
```

- `LocalHasher` wraps the current `RandomXVerifier`.
- `RemoteHasher` is the HTTPS client.
- `OrderedHasher` applies the order semantics and is the only hasher that
  `RandomXAlgorithm` receives.
- `RandomXAlgorithm::verify` keeps the 32-byte seed check, the target comparison and
  the outer expiry timeout. The generic hasher API keeps the wider 1 to 256 byte
  limits.

## 7. Verifier protocol

The API path names the domain, so a backend change does not change the contract.

```text
POST /v1/randomx/hash
Content-Type: application/json

{"seed": "<64 hex>", "input": "<2..512 hex>", "deadline_unix_ms": 1792725000123}
```

| Response | Meaning |
| --- | --- |
| `200 {"hash": "<64 hex>"}` | Hash computed |
| `400` | Invalid seed, input or deadline |
| `503` with `Retry-After` | Queue full, admission limit reached, or the deadline passed before the hash started |

- `deadline_unix_ms` is an absolute Unix time in milliseconds. The gateway sends
  the earlier of the request timeout and the challenge expiry. A relative value
  would lose the transport delay.
- The verifier rejects a deadline in the past or more than 60 seconds in the
  future with `400`. Gateway and verifier clocks must stay synchronized, for
  example with NTP.
- Request and response bodies are at most 4 KiB. Both sides enforce the limit
  while they read the body.
- The client sends no redirect, and it treats a `3xx` response as unavailable.
- `GET /v1/ready` returns `200` when the verifier can accept work, also while its
  cache is cold. It returns `503` only when the verifier cannot hash, for example
  after a cache initialization failure or during shutdown.
- The verifier listener serves only these routes.

## 8. Verifier service

A dedicated verifier has no upstream application. It uses a separate command:

```bash
rilgate verifier serve --config verifier.yml
```

The command has its own resolution path:

- It reuses YAML parsing, environment expansion, flags and variables.
- It resolves only `environment`, `pow.algorithms.randomx.cache`,
  `pow.algorithms.randomx.serve`, `server.operations`, `server.metrics_enabled`
  and logging.
- It does not need `server.upstream` or `security`, and it does not start the
  reverse proxy.
- `rilgate verifier config check` and `rilgate verifier config show` validate and
  print this subset. The environment checks in section 11 apply.
- The operations listener serves `/api/live`, `/api/ready` and `/api/metrics` from
  a verifier state. The gateway handlers depend on the upstream and the pool, so
  they are not reused.

### Admission and fairness

- `serve.max_connections` limits accepted TCP connections.
- Each authenticated client certificate can hold at most half of `serve.queue`.
  One gateway cannot fill the whole queue.
- The verifier reads a request body only after admission, within the 4 KiB limit.
- When the client closes its connection, the verifier drops the request if the
  hash has not started.

### Cache lifecycle

Phase 1 has one worker and at most one resident cache per process, as the local
hasher has today.

Phase 2 adds these rules:

- One cache serves `serve.workers` VMs. Each VM adds its 2 MiB scratchpad and
  program state.
- The verifier has two cache slots. A slot that is in construction counts toward
  the limit, because construction allocates the cache memory first.
- A build for a seed runs once. Requests for the same seed wait for that build
  within their deadlines.
- After a seed change, the old cache stays resident while requests use it, and for
  `5m` after its last request. This covers jobs that alternate between two seeds.
- A request for a third seed while both slots are in use or in construction
  receives `503`.
- `cache.idle_timeout` releases a slot after the idle period.

### Phase 2: shared cache in `randomx-core`

`RandomX` owns its cache and VMs today. Phase 2 changes the API of `randomx-core`:

- The cache becomes a `RandomXCache` that workers share through an `Arc`.
- Each worker owns a VM that borrows the shared cache. The full hash keeps the
  current initialization and finalization.
- Compile-time assertions confirm that `RandomXCache` is `Send + Sync`.
- Concurrent reference-vector tests run several VMs on one cache.

Low-level VM execution already borrows an immutable cache. The change exposes this
at the public API.

### Combined gateway and verifier

In phase 2, a gateway with `serve` uses one hasher pool for its own proofs and for
remote requests. The pool owns the cache slots, the scheduler and the queue.
Local proofs use the `verifier_queue` limit, remote requests use `serve.queue`, and
the scheduler takes work from both queues in turn. A combined process never holds
a second set of caches.

## 9. Security

- In production, the client must have `tls.ca`, `client_cert` and `client_key`, and
  the endpoint must use `https`. The verifier must have `tls.client_ca`.
- The gateway checks that the verifier certificate contains
  `verify.remote.server_name`. A trusted CA alone is not enough.
- Use a private CA for gateway and verifier certificates. Do not trust public web
  CAs for this channel. Rotate certificates with an overlap: add the new CA to the
  trust store, deploy the new certificates, then remove the old CA.
- The verifier holds no signing key and receives no token, host, client IP or
  cookie. If an attacker controls a trusted verifier, the attacker can accept
  invalid proofs for every gateway that trusts it. The attacker cannot forge
  tokens or read requests. Gateways must trust only verifiers that they operate.
- The proof API rate limits of each gateway apply before a hash request leaves the
  gateway. Section 5 describes the one extra hash that a timeout can cause.
  Verifier admission (section 8) limits the total work, also when a gateway
  disables its rate limits.
- Fallback never accepts a proof. When every hasher is unavailable, the proof fails
  with `503`.

## 10. Operations

New metrics. Labels contain no seed, endpoint, address or certificate identity:

| Metric | Type | Labels |
| --- | --- | --- |
| `rilgate_randomx_hash_total` | counter | `hasher` (`local`, `remote`), `outcome` (`hashed`, `unavailable`, `invalid`) |
| `rilgate_randomx_hash_fallback_total` | counter | `from` (`local`, `remote`), `reason` (`timeout`, `connection`, `overloaded`, `protocol`) |
| `rilgate_randomx_remote_duration_seconds` | histogram | none |
| `rilgate_randomx_cache_slots` | gauge | `state` (`resident`, `building`) |
| `rilgate_verifier_service_requests_total` | counter | `outcome` (`hashed`, `invalid`, `overloaded`, `expired`, `cancelled`) |

The existing `rilgate_verifier_*` metrics keep their meaning for the local hasher.
`rilgate_randomx_cache_slots` counts the caches of the whole process.

Readiness:

- `/api/ready` of a gateway probes `GET /v1/ready` of the remote verifier only when
  these conditions are all true:
  - `pow.verification` is `local_verify`.
  - A configured challenge rule selects RandomX.
  - `order` has no `local` entry.
- The probe uses `server.readiness_timeout`, like the other dependency probes.
- A cold verifier is ready. Only a verifier that cannot hash is not ready.

## 11. Environment checks

| Identifier | Environment | Level | Condition |
| --- | --- | --- | --- |
| `verifier-remote-plain-http` | production | error | `verify.remote.endpoint` does not use `https` |
| `verifier-remote-credentials` | production | error | `verify.remote.tls` lacks `ca`, `client_cert` or `client_key` |
| `verifier-serve-unauthenticated` | production | error | `serve` has no `tls.client_ca` |
| `verifier-remote-missing` | both | error | `order` contains `remote` without `verify.remote.endpoint` |
| `verifier-serve-public` | production | warning | `serve.listen` is outside loopback and private networks |
| `verifier-local-fallback` | both | warning | `order` has `local` after `remote`. A remote outage makes every gateway build a 256 MiB cache at the same time |
| `verifier-remote-unused` | both | warning | `order` contains `remote`, but no challenge rule selects RandomX or `pow.verification` is `p2pool_accepted` |

## 12. Compatibility

- The default configuration behaves as today, except `cache.idle_timeout`. Set
  `idle_timeout: 0s` to keep the current behavior.
- Tokens, cookies, the browser protocol and `pow.verification` do not change.
  Gateways with different `order` values can share signing keys.
- A verifier outage returns `503` where it would otherwise return `401`. The
  browser shows its retry control in both cases.

## 13. Verification

| Area | Tests |
| --- | --- |
| Hash results | The local, remote and ordered hashers return the reference vector hash |
| Order | Each unavailable outcome in section 5 moves to the next hasher. A target miss and an invalid input do not. An exhausted order returns `503` |
| Deadlines | The deadline sent to the verifier is absolute and never after the challenge expiry. Transport delay and a late successful response are tested |
| Cold build | A proof that arrives during a cold build, an idle rebuild or a seed change succeeds within its deadline |
| Protocol | Malformed, oversized and redirect responses, wrong statuses and a 31-byte hash are unavailable |
| TLS | The verifier rejects an untrusted client certificate. The gateway rejects a verifier certificate with a wrong name |
| Admission | One client cannot hold more than half of the queue. A closed connection removes a queued request |
| Cache (phase 2) | At most two slots exist with three alternating seeds. Concurrent old-seed and new-seed requests succeed. A building slot counts toward the limit |
| Shared cache (phase 2) | Several VMs on one cache return the reference vectors concurrently |
| Readiness | Probes run only under the conditions in section 10 |

## 14. Delivery sequence and deferred work

Phase 1:

1. Cache idle release for the current local hasher, with the `IdleTimeout` type.
2. The `RandomXHasher` contract, `OrderedHasher`, the outcome table, the
   `PowBackendUnavailable` mapping and `verify.order` with `local` only.
3. `RemoteHasher` with mTLS, the absolute deadline, response limits, metrics and
   the environment checks.
4. `rilgate verifier serve` with one worker, admission limits, its own resolution
   path and its operations listener.
5. Documentation in `docs/pow.md`, `docs/configuration.md`, `docs/production.md`
   and `docs/architecture.md`.

Phase 2:

1. The shared `RandomXCache` API in `randomx-core`.
2. Several VMs per cache, the two-slot cache rules and single-flight builds.
3. The combined gateway and verifier mode with one hasher pool.

Deferred to separate proposals:

- A P2Pool broker that supplies jobs and pool acceptance outside the gateway.
- An edge runtime. It needs these parts:
  - the broker for RandomX jobs
  - a shared rate limiter
  - a scheduled IP feed refresh
  - a session owner for the proof WebSocket
- Per-rule enforcement. It changes token claims and needs an order of strength
  between `local_verify` and `p2pool_accepted`.
- Tenant contexts. Host binding already separates sites. A tenant concept enters
  the core only when host binding cannot meet a real requirement.
- Bearer token authentication for networks where mTLS is not possible.
