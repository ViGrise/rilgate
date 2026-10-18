# Policy IP sets

## Scope

Named IP sets let policy conditions match client addresses against operator
lists and published crawler ranges. Sets are native policy data: the same
registry serves YAML conditions today and any future expression or script rule
type. This proposal covers static sets, remote feeds with background refresh,
three-valued evaluation for unavailable data, and diagnostics. Expression or
script rules (CEL or Lua) are a separate proposal that consumes this registry.

## Configuration

```yaml
policy:
  mode: custom
  ip_sets:
    office:
      cidrs: [203.0.113.0/24, 2001:db8::/32]
    google-common-crawlers:
      url: https://developers.google.com/static/crawling/ipranges/common-crawlers.json
      format: ip_prefixes_json
      refresh: 24h
      max_stale: 7d
  rules:
    - name: verified-google-crawler
      when:
        all:
          - client_ip: {in_sets: [google-common-crawlers]}
          - user_agent: {contains: Googlebot}
      action: allow
    - name: office
      when:
        client_ip: {in_sets: [office]}
      action: allow
```

- `ip_sets` requires `mode: custom`, like `rules`, `score` and `default_action`.
- A set name has 1 to 64 bytes from `a-z`, `0-9`, `-` and `_`, starting with a
  letter or digit. At most 64 sets are defined.
- A set defines exactly one source: `cidrs` (static) or `url` (remote feed).
- `format` is required with `url`; `ip_prefixes_json` is the only format.
  `refresh` defaults to `24h` and is bounded to `1h..=7d`. `max_stale` defaults
  to `7d`, is at least `refresh` and at most `30d`. Remote fields are rejected on
  a static set.
- In production, a remote `url` must use `https`. Development also accepts `http`.
- `client_ip` takes exactly one of `cidr` or `in_sets`. `in_sets` lists 1 to 16
  defined set names and matches when the address belongs to any of them. An
  unknown name is a configuration error. Combining `cidr` and `in_sets` uses an
  explicit `all` or `any`.
- A set holds at most 65 536 prefixes.

## Address semantics

The client address is canonicalized with `IpAddr::to_canonical`, so an
IPv4-mapped IPv6 peer is matched as IPv4. The trusted proxy resolver and the
proof API rate limiter use the same canonicalization. A configured or published
network inside `::ffff:0:0/96` is converted to the equivalent IPv4 network.
Other IPv6 networks, including `::/0`, keep IPv6 semantics and never match an
IPv4 client. Host bits in a network are truncated. The same index serves
`client_ip.cidr` and `in_sets`.

The index stores merged, sorted, inclusive ranges (`u32` for IPv4, `u128` for
IPv6) and answers membership with a binary search.

## Feed format

`ip_prefixes_json` is the object published by Google, Bing and OpenAI and
described by draft-illyes-aipref-jafar:

```json
{"creationTime": "...", "prefixes": [{"ipv4Prefix": "66.249.64.0/27"}, {"ipv6Prefix": "2001:4860:4801:10::/64"}]}
```

- `prefixes` is required. A prefix object with both or neither of `ipv4Prefix`
  and `ipv6Prefix` is ignored. A prefix value that is not a valid network rejects
  the document.
- `formatVersion` is optional. A major version other than `1` rejects the
  document. Unknown fields, `creationTime`, `synctoken` and `service` are ignored.
- A document with no usable prefix, or more than 65 536, is rejected.

## Refresh

Feeds are fetched in the background, never on the request path. The first
fetch starts at startup. A successful fetch publishes a new immutable snapshot
atomically; in-flight evaluations keep the snapshot they loaded.

- Requests carry `If-None-Match` and `If-Modified-Since` from the last success.
  `304 Not Modified` renews the current snapshot.
- The response body is limited to 4 MiB, the request to 30 seconds, and
  redirects to 3 hops that keep the scheme of the configured URL.
- After a success the next fetch waits `refresh`. After a failure, a set that
  has never loaded retries after 15 seconds, doubling per consecutive failure up
  to 5 minutes, because it holds the instance out of readiness. A loaded set
  waits 5 minutes, doubling per consecutive failure up to `refresh`. Every wait
  adds a uniform jitter of up to 10% so instances drift apart.
- A failure keeps the last valid snapshot and increments the failure counter.

## Unavailable data

A remote set is unknown before its first successful load and when its snapshot
is older than `max_stale`. Static sets are always known.

Conditions use three-valued logic. `all` is false when any child is false,
otherwise unknown when any child is unknown, otherwise true. `any` is true when
any child is true, otherwise unknown when any child is unknown, otherwise false.
`not` keeps unknown.

A rule whose condition is unknown never produces a more permissive outcome than
it would with complete data:

| Rule action | Unknown condition |
| --- | --- |
| `allow` | Skipped |
| `score` with a positive score | Applied |
| `score` with zero or a negative score | Skipped |
| `challenge` | Applied |
| `deny` | Challenge with the default algorithm and score-derived difficulty |

The deny downgrade bounds the impact of a feed outage on a denylist. A decision
is unresolved when any rule evaluated on the way to it, including the deciding
rule, had an unknown condition, so logs and metrics show the cause.
Configuration validation treats a deny rule that references a remote set as a
possible default challenge for algorithm and P2Pool checks.

## Diagnostics

- Readiness reports `ip_sets: false` and fails until every remote set has
  loaded once. A stale set after a successful load does not fail readiness, so a
  publisher outage cannot take all instances out of rotation together.
- Metrics, labelled by configured set name: `rilgate_ip_set_prefixes`,
  `rilgate_ip_set_age_seconds`, `rilgate_ip_set_stale`,
  `rilgate_ip_set_refresh_failures_total`. The gateway counts unresolved
  decisions in `rilgate_policy_unresolved_decisions_total`.

## Crates

```text
rilgate-policy   ip_sets/ index, set state, registry; schema and evaluation
rilgate-ip-feeds feed parsing, HTTP fetching and the refresh loop
rilgate          spawns feed refreshers for the challenge service's registry
rilgate-gateway  readiness, metrics and unresolved decision logging
```

`rilgate-policy` stays free of network and runtime dependencies.
`rilgate-ip-feeds` depends on `rilgate-policy`; nothing in the policy or gateway
crates depends on it.

## Verification limits

Matching a published range together with a declared User-Agent confirms that
the request comes from the publisher's network and claims the matching
identity. It is not cryptographic proof of identity. Publishers split crawler
groups (Google publishes common crawlers, special crawlers and user-triggered
fetchers separately), so each set names one group.

## Deferred

Each item is additive on top of this design:

- On-disk cache of the last valid feed body for offline restarts.
- Additional feed formats (`format` is an explicit enum).
- Forward-confirmed reverse DNS.
- A per-set override of the unknown-data rules.
- `rilgate policy explain` request simulation.
- Whole-policy hot reload.
