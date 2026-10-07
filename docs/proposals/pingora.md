# Native Pingora HTTP architecture

## Scope

Rilgate uses one public Pingora listener for application forwarding, policy and
authorization, challenge/denied/error pages, proof APIs, static assets, health,
and proof WebSockets. There is no Axum service, internal HTTP hop, Unix socket,
or alternate production HTTP engine.

The policy, challenge, crypto, P2Pool, RandomX, and web snapshot crates remain
shared application services. HTTP transport does not change signed token formats,
host scoping, proof algorithms, verifier capacity, or pool budgets.

```text
Client -> Pingora HTTP application
            +-> policy and token checks -> application upstream
            +-> challenge / denied / error response -> web snapshot
            +-> proof JSON API -> challenge service
            +-> proof WebSocket -> tungstenite -> Stratum session
            +-> static assets / health -> local response
```

## Transport boundaries

`transport/server.rs` adapts Pingora sessions to local handlers and upstream
forwarding. `proxy/gate.rs` evaluates application access and returns local
responses when necessary. `proxy/client.rs` validates upstream URLs, resolves
addresses, and constructs pooled HTTP peers. `transport/response.rs` provides
HTTP response bodies shared with static asset streaming.

The gateway uses Pingora's supported custom accept-loop API on the existing
Tokio runtime. This preserves the CLI and SIGHUP web reload handling without
introducing a second runtime or Pingora configuration file. Shutdown stops
acceptance, cancels local proof sessions, notifies Pingora, and drains active
connections with a deadline.

## Required contracts

- Policy and forwarding use the same validated path representation. Upstream
  base paths and query strings preserve their existing semantics.
- Original HTTP authority scopes tokens. Upstream TLS SNI uses the configured
  upstream hostname, independently of HTTP Host preservation.
- Rate limits use kernel socket peer addresses. Forwarded headers cannot select
  another client bucket.
- JSON APIs enforce their own bounded bodies. Application uploads are checked
  before forwarding when their length is known and incrementally while streaming.
- Requests are not automatically retried. Verification, exchange, and mining
  submission must not be replayed by the transport.
- Application WebSocket upgrades require the same policy/auth checks as HTTP.
  Proof WebSocket admission additionally validates presentation tokens and
  reserves a P2Pool slot before returning 101.
- Proof WebSocket framing uses tungstenite and a bounded in-memory duplex
  adapter. Frame/message limits, backpressure, session TTL, pending submissions,
  cancellation, and slot release remain enforced without a network bridge.
- Local response status, HEAD behavior, locale, cache headers, cookies, request
  IDs, static ETags, and web reload snapshots retain their public behavior.

## Verification

Gateway integration tests exercise the actual Pingora TCP listener, including
HTTP/2, upstream streaming, malformed/truncated uploads, body limits, normalized
paths, authorization scope, rate limits, proof and application WebSockets, and
shutdown. Test-only Axum origins and HTTP clients do not serve production traffic.
The complete Rust, Node, WASM, formatting, and Clippy checks remain required.

Standalone performance checks stay outside CI. Compare the integrated gateway
against the previous release with repeated runs, a direct-origin baseline,
realistic latency/payloads, and CPU/request, p99, memory, and error metrics. The
minimal local Pingora benchmark does not establish integrated production gains.
Linux validation is required before deployment because it is Pingora's primary
platform. Rollback uses the previous binary with the same configuration and keys.

## Separate extensions

Load balancing/failover, public TLS configuration, response caching, distributed
rate limiting, and metrics exporters are separate features. Challenge HTML keeps
its existing internal shell cache and per-request presentation token behavior.

## References

- [Pingora HTTP proxy and custom accept-loop API](https://github.com/cloudflare/pingora/blob/0.9.0/pingora-proxy/src/lib.rs)
- [ProxyHttp callbacks](https://github.com/cloudflare/pingora/blob/0.9.0/pingora-proxy/src/proxy_trait.rs)
- [TLS feature flags](https://github.com/cloudflare/pingora/blob/0.9.0/pingora/Cargo.toml)
- [Platform support](https://github.com/cloudflare/pingora/blob/0.9.0/README.md)
