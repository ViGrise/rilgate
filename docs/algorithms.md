# Adding a proof-of-work algorithm

## Backend contract

Implement `rilgate_pow::Algorithm` in a dedicated crate such as
`crates/rilgate-pow-<name>`, with modules grouped by responsibility. The contract
provides a stable descriptor, weight-to-difficulty mapping, difficulty and
enforcement validation, TTL, work issuance and proof verification.

`issue(WorkRequest)` receives the difficulty and enforcement mode and returns
`PreparedWork`. `verify(&PreparedWork, Proof)` validates unsigned work and a nonce.
The adapter does not receive signed tokens or signing keys. `rilgate-challenge`
checks signatures, versions, lifetime, host and enforcement before dispatching,
then issues a receipt. It checks lifetime again after verification.

Adapters validate work dimensions and nonce encoding before expensive work, and
bound CPU, memory, queue capacity and execution time. RandomX initializes its
native verifier only on the first valid hashing request.

The current protocol uses explicit `PowAlgorithm`, `ChallengeClaims`, and
`ChallengeResponse` variants for Fast and RandomX. A new wire format requires an
ID and payload in `rilgate-protocol`, unsigned work in `rilgate-pow`, and mappings
in `rilgate-challenge/src/challenge/work.rs`. Define version compatibility and
validation before exposing the new payload. Descriptor versions are metadata;
current tokens use version 1.

Register the backend in `crates/rilgate/src/app/bootstrap.rs`. Duplicate IDs are
rejected. Startup checks every configured algorithm and validates policy
difficulties through the registered implementations. Policy evaluation does not
contain hashing rules. Configuration loading remains in `rilgate-config`; backend
constructors receive their own narrow settings, not the full application config.

GhostRider and BLAKE3 are extension candidates, not implemented backends. Add their
crates when their real implementation, wire contract, browser support and test
vectors are available.

## Work providers

Implement `rilgate_pool::WorkProvider` when work comes from an external source.
Its associated `Work` type describes that provider's output. Inject the provider
into the algorithm adapter and keep network protocols out of the hash engine.
RandomX consumes Stratum jobs from the P2Pool provider; Fast creates local random
challenges without an external provider. Stratum owns framing and sessions;
P2Pool owns provider capacity and deployment policy.

## Browser contract

Keep browser assets together under `web/static/js/solvers/<name>/`:

```text
adapter.js   registers the algorithm's run(config) operation
solver.js    owns calculation and cancellation
worker.js    performs expensive calculation off the page thread
```

Register the adapter through `window.PowClient.register(id, adapter)` and import
its module in `web/static/js/challenge.js` after the shared runtime and registry.
The page entrypoint selects an adapter
through the registry and checks the algorithm list returned by client-config.
Shared fetch, translation, exchange, and status helpers belong to the challenge
runtime. Mining transport belongs to the P2Pool browser module. Keep resource
paths in the server descriptor and configuration aligned with packaged assets.

Require explicit consent for expensive or mining work. Release workers on page
exit, completion, and error. Job rotation must discard superseded results, and
expiry must remain enforced by the server even if the browser stops its work.

## Validation

Run the registered implementation through challenge issuance, verification,
receipt exchange, and access-token verification. Check host binding, unsupported
versions, expiry, invalid work, incorrect proofs, disabled algorithms, and resource
capacity. `crates/rilgate-gateway/tests/algorithms.rs` demonstrates an injected backend
with its own policy validation and shared lifecycle checks.

```sh
scripts/test.sh
```
