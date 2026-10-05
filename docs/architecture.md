# Workspace architecture

The workspace root is virtual; its default package is `rilgate`. The application
composes implementations and the gateway receives ready services. Public modules
are grouped by responsibility instead of collecting every subsystem in `src/`.

## Dependency direction

```text
rilgate -> config, gateway, challenge, pow implementations, pool, web, telemetry
config -> domain configuration types (challenge, policy, pool, pow, web)
gateway -> config, challenge, policy, pool, pow API, protocol, crypto, web, telemetry
challenge -> pow API, policy, protocol, crypto, core
pow-randomx -> pow API, pool provider contract, protocol, core, randomx-core
pow-hash -> pow API, protocol, core
pool -> stratum, protocol, crypto, core
stratum -> protocol, crypto
pow API -> protocol, core
policy -> protocol
web -> its own configuration and resource/runtime modules
```

`rilgate-test-utils` is a development dependency. It shares fixtures and explicit
service composition for gateway tests; production bootstrap remains in the app.
Domain crates do not depend on the configuration loader or application state.
The legacy deployment `pow` section is assembled into narrow domain settings,
so existing YAML and environment variables retain their behavior.

## Detailed source tree

```text
rilgate/
├── .cargo/
│   └── config.toml
├── .github/
│   └── workflows/
│       └── ci.yml
├── config/
│   └── rilgate.example.yml
├── crates/
│   ├── randomx-core/
│   │   ├── examples/
│   │   │   └── throughput.rs
│   │   ├── src/
│   │   │   ├── aes/
│   │   │   │   └── lookup.rs
│   │   │   ├── float/
│   │   │   │   ├── accelerated.rs
│   │   │   │   └── simd.rs
│   │   │   ├── aes.rs
│   │   │   ├── blake_generator.rs
│   │   │   ├── cache.rs
│   │   │   ├── constants.rs
│   │   │   ├── float.rs
│   │   │   ├── helpers.rs
│   │   │   ├── instruction.rs
│   │   │   ├── lib.rs
│   │   │   ├── program.rs
│   │   │   ├── reciprocal.rs
│   │   │   ├── superscalar.rs
│   │   │   ├── vm.rs
│   │   │   └── wasm_dataset.rs
│   │   ├── tests/
│   │   │   └── vectors.rs
│   │   └── Cargo.toml
│   ├── randomx-wasm/
│   │   ├── src/
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── rilgate/
│   │   ├── src/
│   │   │   ├── app/
│   │   │   │   ├── bootstrap.rs
│   │   │   │   └── mod.rs
│   │   │   ├── cli/
│   │   │   │   ├── args.rs
│   │   │   │   ├── config.rs
│   │   │   │   ├── mod.rs
│   │   │   │   ├── serve.rs
│   │   │   │   └── web.rs
│   │   │   ├── lib.rs
│   │   │   ├── main.rs
│   │   │   └── shutdown.rs
│   │   ├── tests/
│   │   │   └── startup.rs
│   │   └── Cargo.toml
│   ├── rilgate-challenge/
│   │   ├── src/
│   │   │   ├── authorization/
│   │   │   │   ├── mod.rs
│   │   │   │   └── service.rs
│   │   │   ├── challenge/
│   │   │   │   ├── issuance.rs
│   │   │   │   ├── mod.rs
│   │   │   │   ├── presentation.rs
│   │   │   │   └── work.rs
│   │   │   ├── engine/
│   │   │   │   ├── config.rs
│   │   │   │   ├── mod.rs
│   │   │   │   └── service.rs
│   │   │   ├── tokens/
│   │   │   │   ├── mod.rs
│   │   │   │   └── validation.rs
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── rilgate-config/
│   │   ├── src/
│   │   │   ├── assembly/
│   │   │   │   ├── defaults.rs
│   │   │   │   ├── mod.rs
│   │   │   │   ├── overrides.rs
│   │   │   │   ├── pow.rs
│   │   │   │   ├── upstream.rs
│   │   │   │   └── web.rs
│   │   │   ├── loader/
│   │   │   │   ├── file.rs
│   │   │   │   └── mod.rs
│   │   │   ├── model/
│   │   │   │   ├── gateway.rs
│   │   │   │   ├── mod.rs
│   │   │   │   ├── pow.rs
│   │   │   │   └── pow_validation.rs
│   │   │   ├── resolution/
│   │   │   │   ├── compatibility.rs
│   │   │   │   └── mod.rs
│   │   │   ├── runtime/
│   │   │   │   └── mod.rs
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── rilgate-core/
│   │   ├── src/
│   │   │   ├── error.rs
│   │   │   ├── lib.rs
│   │   │   └── time.rs
│   │   └── Cargo.toml
│   ├── rilgate-crypto/
│   │   ├── src/
│   │   │   ├── secret/
│   │   │   │   └── mod.rs
│   │   │   ├── token/
│   │   │   │   └── mod.rs
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── rilgate-gateway/
│   │   ├── src/
│   │   │   ├── auth/
│   │   │   │   ├── cookies.rs
│   │   │   │   └── mod.rs
│   │   │   ├── observability/
│   │   │   │   ├── health/
│   │   │   │   │   ├── dto.rs
│   │   │   │   │   ├── handlers.rs
│   │   │   │   │   ├── mod.rs
│   │   │   │   │   └── service.rs
│   │   │   │   └── mod.rs
│   │   │   ├── proof/
│   │   │   │   ├── websocket/
│   │   │   │   │   ├── messages.rs
│   │   │   │   │   ├── mod.rs
│   │   │   │   │   └── session.rs
│   │   │   │   ├── dto.rs
│   │   │   │   ├── handlers.rs
│   │   │   │   ├── mod.rs
│   │   │   │   └── routes.rs
│   │   │   ├── proxy/
│   │   │   │   ├── client.rs
│   │   │   │   ├── gate.rs
│   │   │   │   └── mod.rs
│   │   │   ├── runtime/
│   │   │   │   ├── app.rs
│   │   │   │   ├── mod.rs
│   │   │   │   └── state.rs
│   │   │   ├── transport/
│   │   │   │   ├── middleware/
│   │   │   │   │   ├── mod.rs
│   │   │   │   │   ├── rate_limit.rs
│   │   │   │   │   ├── request_id.rs
│   │   │   │   │   └── timeout.rs
│   │   │   │   ├── rate_limit/
│   │   │   │   │   └── mod.rs
│   │   │   │   ├── context.rs
│   │   │   │   ├── errors.rs
│   │   │   │   └── mod.rs
│   │   │   ├── web/
│   │   │   │   ├── assets.rs
│   │   │   │   ├── mod.rs
│   │   │   │   └── pages.rs
│   │   │   └── lib.rs
│   │   ├── tests/
│   │   │   ├── common/
│   │   │   │   └── mod.rs
│   │   │   ├── algorithms.rs
│   │   │   ├── gateway.rs
│   │   │   ├── proof_protocol.rs
│   │   │   ├── rate_limits.rs
│   │   │   ├── stratum.rs
│   │   │   ├── uploads.rs
│   │   │   └── web.rs
│   │   └── Cargo.toml
│   ├── rilgate-policy/
│   │   ├── src/
│   │   │   ├── policy/
│   │   │   │   ├── config.rs
│   │   │   │   ├── evaluator.rs
│   │   │   │   └── mod.rs
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── rilgate-pool/
│   │   ├── src/
│   │   │   ├── config/
│   │   │   │   └── mod.rs
│   │   │   ├── provider/
│   │   │   │   ├── contract.rs
│   │   │   │   ├── mod.rs
│   │   │   │   └── p2pool.rs
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── rilgate-pow/
│   │   ├── src/
│   │   │   ├── algorithm/
│   │   │   │   ├── contract.rs
│   │   │   │   ├── descriptor.rs
│   │   │   │   ├── mod.rs
│   │   │   │   └── registry.rs
│   │   │   ├── difficulty/
│   │   │   │   └── mod.rs
│   │   │   ├── target/
│   │   │   │   └── mod.rs
│   │   │   ├── work/
│   │   │   │   └── mod.rs
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── rilgate-pow-hash/
│   │   ├── src/
│   │   │   ├── sha256/
│   │   │   │   ├── mod.rs
│   │   │   │   └── verifier.rs
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── rilgate-pow-randomx/
│   │   ├── src/
│   │   │   ├── randomx/
│   │   │   │   ├── verification/
│   │   │   │   │   ├── mod.rs
│   │   │   │   │   ├── target.rs
│   │   │   │   │   └── workers.rs
│   │   │   │   ├── mod.rs
│   │   │   │   └── work.rs
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── rilgate-protocol/
│   │   ├── src/
│   │   │   ├── algorithm/
│   │   │   │   ├── enforcement.rs
│   │   │   │   ├── id.rs
│   │   │   │   └── mod.rs
│   │   │   ├── challenge/
│   │   │   │   └── mod.rs
│   │   │   ├── claims/
│   │   │   │   └── mod.rs
│   │   │   ├── difficulty/
│   │   │   │   └── mod.rs
│   │   │   ├── mining/
│   │   │   │   └── mod.rs
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── rilgate-stratum/
│   │   ├── src/
│   │   │   ├── client/
│   │   │   │   └── mod.rs
│   │   │   ├── codec/
│   │   │   │   └── mod.rs
│   │   │   ├── config/
│   │   │   │   └── mod.rs
│   │   │   ├── job/
│   │   │   │   └── mod.rs
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── rilgate-telemetry/
│   │   ├── src/
│   │   │   ├── tracing/
│   │   │   │   └── mod.rs
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── rilgate-test-utils/
│   │   ├── src/
│   │   │   ├── config/
│   │   │   │   └── mod.rs
│   │   │   ├── gateway/
│   │   │   │   └── mod.rs
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── rilgate-web/
│   │   ├── src/
│   │   │   ├── assets/
│   │   │   │   ├── metadata.rs
│   │   │   │   ├── mod.rs
│   │   │   │   └── response.rs
│   │   │   ├── config/
│   │   │   │   ├── mod.rs
│   │   │   │   └── validation.rs
│   │   │   ├── i18n/
│   │   │   │   ├── loader.rs
│   │   │   │   ├── locale.rs
│   │   │   │   ├── mod.rs
│   │   │   │   ├── store.rs
│   │   │   │   └── translator.rs
│   │   │   ├── management/
│   │   │   │   ├── diff.rs
│   │   │   │   ├── init.rs
│   │   │   │   ├── mod.rs
│   │   │   │   └── validate.rs
│   │   │   ├── resources/
│   │   │   │   ├── embedded.rs
│   │   │   │   ├── loader.rs
│   │   │   │   ├── mod.rs
│   │   │   │   └── seed.rs
│   │   │   ├── runtime/
│   │   │   │   ├── manager.rs
│   │   │   │   ├── mod.rs
│   │   │   │   ├── snapshot.rs
│   │   │   │   └── watcher.rs
│   │   │   ├── templates/
│   │   │   │   ├── context.rs
│   │   │   │   ├── mod.rs
│   │   │   │   └── renderer.rs
│   │   │   └── lib.rs
│   │   ├── tests/
│   │   │   └── runtime.rs
│   │   ├── Cargo.toml
│   │   └── README.md
│   └── xtask/
│       ├── src/
│       │   └── main.rs
│       └── Cargo.toml
├── deploy/
│   ├── docker/
│   │   ├── Dockerfile
│   │   └── compose.yml
│   ├── examples/
│   │   ├── caddy/
│   │   │   └── Caddyfile
│   │   └── nginx/
│   │       └── nginx.conf
│   ├── systemd/
│   │   └── rilgate.service
│   └── README.md
├── docs/
│   ├── proposals/
│   │   └── web.md
│   ├── algorithms.md
│   ├── architecture.md
│   ├── configuration.md
│   ├── development.md
│   ├── gateway.md
│   ├── policy.md
│   ├── pow.md
│   └── web.md
├── scripts/
│   ├── build.sh
│   ├── test.sh
│   └── validate-randomx-wasm.mjs
├── tests/
│   ├── support/
│   │   └── challenge-source.mjs
│   ├── browser-solver.test.mjs
│   ├── challenge-page.test.mjs
│   └── randomx-wasm.test.mjs
├── web/
│   ├── i18n/
│   │   └── en.json
│   ├── static/
│   │   ├── css/
│   │   │   └── rilgate.css
│   │   └── js/
│   │       ├── challenge/
│   │       │   ├── p2pool.js
│   │       │   ├── registry.js
│   │       │   └── runtime.js
│   │       ├── solvers/
│   │       │   ├── fast/
│   │       │   │   ├── adapter.js
│   │       │   │   ├── solver.js
│   │       │   │   └── worker.js
│   │       │   └── randomx/
│   │       │       ├── adapter.js
│   │       │       ├── randomx-worker.js
│   │       │       └── solver.js
│   │       └── challenge.js
│   └── templates/
│       ├── layouts/
│       │   └── base.html
│       ├── pages/
│       │   ├── challenge.html
│       │   ├── denied.html
│       │   └── error.html
│       └── partials/
│           ├── footer.html
│           └── pow-info.html
├── .dockerignore
├── .env.sample
├── .gitignore
├── .rustfmt.toml
├── Cargo.lock
├── Cargo.toml
├── LICENSE
├── README.md
└── rust-toolchain.toml
```

## Ownership and extension

The PoW API accepts `WorkRequest`, `PreparedWork` and `Proof`; it has no signing
keys or HTTP handlers. Signed lifecycle validation belongs to `rilgate-challenge`.
HTTP cookies, error mapping and reverse proxy behavior belong to `rilgate-gateway`.
Algorithm registration belongs to application bootstrap.

New algorithms get real implementation crates when their backend and protocol
are available. See [algorithm development](algorithms.md). Unimplemented algorithms,
replay storage, vardiff, Prometheus exporters, fuzz targets and benchmark harnesses
are not represented by empty crates or placeholder modules.

Canonical embedded resources are in root `web/`. The original
[web subsystem proposal](proposals/web.md) describes the resource model; the
tree above gives the implemented paths. Operator resources are seeded and loaded
from the configured runtime root, independently of repository source paths.

Rust integration tests belong to their owning crate. Node browser tests remain
in root `tests/`; native RandomX tests requiring substantial memory are explicitly
ignored by default.

## Runtime invariants

Challenge pages stay at the requested application URL. Their APIs and assets
resolve beneath the configured `web.base_path`. Policy rules choose algorithms
and difficulty independently of the global default.

Fast and RandomX local proofs can cross gateway instances sharing signing keys
and compatible configuration. P2Pool acceptance remains attached to one browser
WebSocket and its retained gateway Stratum session. RandomX work requires the
configured disclosure and explicit browser consent.

Native and browser RandomX share `randomx-core` and reference vectors. Light mode
uses a 256 MiB cache and derives dataset items on demand. Native verification
reuses the seed cache on a dedicated thread, keeping hashing off Tokio workers.
