# Adding a proof-of-work algorithm

## Backend contract

Implement `rilgate_pow::Algorithm` in a dedicated crate such as `crates/rilgate-pow-<name>`, with modules grouped
by responsibility. The contract provides a stable descriptor, weight-to-difficulty
mapping, difficulty and enforcement validation, TTL, work issuance and proof
verification.

`issue(WorkRequest)` receives the difficulty and enforcement mode and returns `PreparedWork`.
`verify(&PreparedWork, Proof)` validates unsigned work and a nonce. The adapter does not receive signed
tokens or signing keys. `rilgate-challenge` checks signatures, versions, lifetime, host and
enforcement before dispatching, then issues a receipt. It checks lifetime again after
verification.

Adapters validate work dimensions and nonce encoding before expensive work, and bound
CPU, memory, queue capacity and execution time. RandomX initializes its native verifier
only on the first valid hashing request.

The current protocol uses explicit `PowAlgorithm`, `ChallengeClaims`, and `ChallengeResponse` variants
for Fast, HashWX and RandomX. A new wire format requires an ID and payload in `rilgate-protocol`,
unsigned work in `rilgate-pow`, and mappings in `rilgate-challenge/src/challenge/work.rs`. Before you expose the new
payload, define version compatibility and validation. Descriptor versions are metadata.
Current tokens use version 1. HashWX uses that version with the explicit `hashwx`
tag. Older builds reject the tag. Enable a new algorithm only after every
gateway instance supports its token variants.

Register the backend in `crates/rilgate/src/app/bootstrap.rs`. Duplicate IDs are rejected. Startup checks every
configured algorithm and validates policy difficulties through the registered
implementations.

Policy evaluation does not contain hashing rules. Configuration loading remains in
`rilgate-config`. Backend constructors receive only their own narrow settings. They do not
receive the full application config.

GhostRider and BLAKE3 are extension candidates. Their backends are not implemented. When
their real implementation, wire contract, browser support and test vectors are
available, add their crates.

## Work providers

When work comes from an external source, implement `rilgate_pool::WorkProvider`. Its associated
`Work` type describes that provider's output. Inject the provider into the
algorithm adapter.

Keep network protocols outside the hash engine. RandomX consumes Stratum jobs from the
Stratum pool provider. Fast creates local random challenges without an external provider.
Stratum owns framing and sessions. Stratum pool owns provider capacity and deployment policy.

## Browser contract

Keep browser assets together under `web/static/js/solvers/<name>/`:

```text
adapter.js   registers the algorithm's run(config) operation
solver.js    owns calculation and cancellation
worker.js    performs expensive calculation off the page thread
```

1. Register the adapter through `window.PowClient.register(id, adapter)`.
2. Add the adapter, and any classic solver or worker it needs, to the `STARTUP` list
   in `crates/xtask/src/web.rs`. Place them after the shared runtime and registry and before
   `challenge.js`.
3. List the algorithm's served files in the required resource check in `crates/rilgate-web/src/runtime/snapshot.rs`.

`cargo xtask build-web` lowers each member to ES5 and assembles `challenge.bundle.js`. Modern-only work such
as WebAssembly loads as a separate module after the adapter checks browser capabilities,
as RandomX does. The page entrypoint selects an adapter through the registry and checks
the algorithm list returned by client-config. Shared fetch, translation, exchange, and
status helpers belong to the challenge runtime. Mining transport belongs to the Stratum pool
browser module. Keep resource paths in the server descriptor and configuration aligned
with packaged assets.

Declare consent and memory requirements in the algorithm descriptor. Fast and
HashWX start automatically. RandomX requires consent before calculation or
mining work. Release workers on page exit,
completion, and error. Job rotation must discard superseded results. The server must
enforce expiry even if the browser stops its work.

## Validation

Run the registered implementation through challenge issuance, verification, receipt
exchange, and access-token verification. Check host binding, unsupported versions,
expiry, invalid work, incorrect proofs, disabled algorithms, and resource capacity.
`crates/rilgate-gateway/tests/algorithms.rs` shows an injected backend with its own policy validation and shared
lifecycle checks.

```sh
scripts/test.sh
```

The pool provider selects named pools and proxies in order. Stratum transport
owns SOCKS5, HTTP CONNECT and verified TLS. A live job session retains one pool
connection. Pool difficulty diagnostics keep the job target unchanged.
