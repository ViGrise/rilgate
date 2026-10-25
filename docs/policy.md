# Request policies

`policy` owns request classification, scoring and challenge decisions. The internal
namespace is handled by the dedicated router before application policies, including when
a rule allows all of `/.well-known/`. An unknown internal route remains an internal 404 and
is never forwarded by an allow rule.

## Presets

| Mode | Behavior |
| --- | --- |
| `off` | Allow application traffic without a PoW requirement |
| `light` | Challenge missing or bot-like User-Agents at Fast difficulty 1 |
| `balanced` | Challenge application traffic at Fast difficulty 2 |
| `strict` | Challenge application traffic at Fast difficulty 4 |
| `custom` | Use only the operator's rules, thresholds and default action |

Light matches User-Agents containing bot, crawler, spider, curl, wget, python, or
scrapy, case-insensitively, or an empty User-Agent. Light, balanced and strict allow
exact `/robots.txt`, `/favicon.ico`, and the `/.well-known/acme-challenge/` prefix. Presets never need
RandomX or Stratum pool. Any custom `rules`, `score`, or `default_action` in a preset is
rejected. Such settings are not silently ignored.

## Built-in policies and rules

`config/policies/` and `config/rules/` contain the source catalogs embedded into
the binary at build time. `config/examples/` contains example operator
configurations. Installed binaries load built-in resources without a checkout or
copied catalog directories.

Select a complete policy:

```yaml
policy: !include builtin:policies/balanced.yml
```

| Built-in policy | Behavior |
| --- | --- |
| `builtin:policies/balanced.yml` | Balanced preset, the default policy. The production configuration selects it. |
| `builtin:policies/light.yml` | Light preset. |
| `builtin:policies/strict.yml` | Strict preset. |
| `builtin:policies/off.yml` | Allows application traffic without a proof requirement. |
| `builtin:policies/crawler-protection.yml` | Optional custom policy with AI/headless deny groups, verified Google, Bing, Apple and DuckDuckGo search crawlers, public paths and client scoring. |

The crawler-protection policy includes the Google, Bing, Apple and DuckDuckGo
search crawler feeds and the score thresholds it needs. Remote feed loading
requires outbound HTTPS for runtime readiness. Configuration checks validate the
policy without fetching the feeds.
The [custom example](../config/examples/custom.yml) demonstrates selecting it.

Customize a built-in custom policy with a map merge:

```yaml
policy:
  <<: !include builtin:policies/crawler-protection.yml
  default_action: challenge
  ip_sets:
    internal: {cidrs: [10.0.0.0/8]}
```

The added IP set keeps the built-in crawler set and rules. Maps merge recursively;
lists and scalars replace previous values. The `balanced`, `light`, `strict` and
`off` presets still reject custom `rules`, `score` and `default_action`.

For an operator-defined policy, mix built-in rules, inline rules and local files
in evaluation order:

```yaml
policy:
  mode: custom
  default_action: allow
  rules:
    - !include builtin:rules/public-endpoints.yml
    - name: health
      when: {path: {exact: /health}}
      action: allow
    - !include builtin:rules/challenge-all.yml
```

| Built-in rule | Behavior |
| --- | --- |
| `public-endpoints.yml` | Allows `/.well-known/`, favicon image variants, `/robots.txt` and `/sitemap.xml`. Broader than preset exceptions. |
| `challenge-bots.yml` | Challenges empty or bot/tool-like User-Agents with Fast difficulty 1. |
| `challenge-all.yml` | Terminal catchall challenge with Fast difficulty 2. Place it last. |
| `deny-ai-crawlers.yml` | Denies recognized AI-affiliated agents, training and search crawlers, and user-triggered AI clients. |
| `deny-headless-browsers.yml` | Denies recognized headless User-Agents and Lightpanda client hints. |
| `client-score.yml` | Adds 10 for browser-like User-Agents, 15 for a present `CF-Worker` header and 5 for a present `X-Firefox-Ai` header. Requires score thresholds to act on the result. |

Rule names in this table use the `builtin:rules/` prefix. The optional deny
groups can also match authorized automation. The default balanced policy keeps
challenging application traffic regardless of User-Agent. Built-in references
always select embedded content. Use an explicit local include path for an
operator-maintained copy.

Each verified search crawler rule allows a request only when its User-Agent
matches the crawler and the client address is in the named IP set. Define the
set with the publisher feed before you include the rule:

```yaml
policy:
  mode: custom
  ip_sets:
    bing-crawlers:
      url: https://www.bing.com/toolbox/bingbot.json
      format: ip_prefixes_json
  rules:
    - !include builtin:rules/bingbot.yml
```

| Verified search crawler rule | User-Agent | IP set | Feed URL |
| --- | --- | --- | --- |
| `googlebot.yml` | Googlebot documentation URL | `google-common-crawlers` | `https://developers.google.com/static/crawling/ipranges/common-crawlers.json` |
| `bingbot.yml` | Bingbot documentation URL | `bing-crawlers` | `https://www.bing.com/toolbox/bingbot.json` |
| `applebot.yml` | Applebot documentation URL | `apple-crawlers` | `https://search.developer.apple.com/applebot.json` |
| `duckduckbot.yml` | DuckDuckBot documentation URL | `duckduckgo-crawlers` | `https://duckduckgo.com/duckduckbot.json` |

The crawler-protection policy evaluates deny rules before crawler and public
allows, then applies client scores. It uses Fast difficulties 1, 2, 4 and 6 at
scores 1, 10, 20 and 30. Its fallback allows requests without a positive score.
GeoIP, ASN, CEL and honeypot conditions are outside the current policy schema.

## Custom evaluation

See [the complete example](../config/examples/custom.yml). Rules run in declaration order. A matching
`score` adds a signed integer with saturating i32 arithmetic and continues.

`allow`, `deny` and `challenge` are terminal. If no rule terminates,
thresholds run against the final score in declaration order. A threshold matches
`score >= min`.

Minimums must be unique and strictly descending. The first match wins. Otherwise
`default_action` applies. Its default is `allow`. It can be `allow`, `deny`,
or `challenge`, but never `score`.

```yaml
policy:
  mode: custom
  default_action: deny
  rules:
    - name: browser-score
      when:
        user_agent:
          contains: Mozilla
      action: score
      score: 10
    - name: allow-public-api
      when:
        all:
          - path: {prefix: /api/}
          - any:
              - method: {exact: POST}
              - headers: {X-Allowed: {exact: 'yes'}}
          - not:
              path: {prefix: /api/private/}
      action: allow
  score:
    initial: 0
    thresholds:
      - min: 10
        action: challenge
        challenge:
          algorithm: fast
          difficulty: 2
```

Every rule needs a unique nonempty name and a nonempty `when`. `score` is
required only on a score action. `challenge` settings are accepted only on a challenge
action, in rules and thresholds. Custom policies never inherit preset exceptions.

## Conditions

Multiple fields in a condition are combined with AND. `all`, `any`, and
`not` compose conditions recursively. `all` and `any` cannot be
empty.

| Field | Operators |
| --- | --- |
| `path`, `method`, `user_agent`, individual `headers` | `exact`, `prefix`, `contains`, `in`, `regex` |
| `client_ip` | `cidr`, `in_sets` |
| `score` | `eq`, `gt`, `gte`, `lt`, `lte` |

A string matcher requires exactly one operator. Matching is case-sensitive unless a
regex explicitly selects case-insensitive matching. Header names are case-insensitive.

Any repeated header value can satisfy its matcher. Missing headers do not match,
including an empty-string regex. `not` can test absence. A missing User-Agent is
an empty string. A missing client IP does not match CIDR. Numeric comparisons combine
with AND and impossible ranges are rejected.

Client IP comes from the TCP peer unless `server.trusted_proxies` authorizes a forwarded chain.
Forwarded headers from untrusted peers cannot influence IP rules. The same resolved
address controls proof API quotas.

Policies permit at most 128 rules plus thresholds and 512 compiled condition nodes. They
permit at most 16 nested condition levels and 64 entries per membership list. Matcher
strings have limits of 2048 bytes each and 64 KiB in total.

A custom policy defines at most 64 IP sets. An `in_sets` list names at most 16 sets.
A set holds at most 65 536 prefixes. Regex compilation has bounded program and DFA
sizes. The request path and method come from the actual HTTP request, and policy and
forwarding use the same validated path.

## IP sets

A named IP set groups addresses that conditions match with `client_ip.in_sets`. `ip_sets`
requires `mode: custom`. A set name has 1 to 64 bytes from `a-z`, `0-9`,
`-` and `_`.

It starts with a letter or digit. At most 64 sets are defined. A set defines exactly one
source: `cidrs` (static) or `url` (remote feed).

`client_ip` takes exactly one of `cidr` or `in_sets`. `in_sets` lists 1 to
16 defined names and matches when the address belongs to any of them. An unknown name is
a configuration error. To combine `cidr` and `in_sets`, use an explicit
`all` or `any`.

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
```

`format` is required with `url`. `ip_prefixes_json` is the only format.
`refresh` defaults to `24h` and is bounded to `1h..=7d`.

`max_stale` defaults to `7d`, is at least `refresh` and at most
`30d`. Remote fields are rejected on a static set. In production a remote
`url` must use `https`. Development also accepts `http`.

The client address is canonicalized with `IpAddr::to_canonical`, so an IPv4-mapped IPv6 peer
matches as IPv4. The trusted proxy resolver and the proof API rate limiter use the same
canonicalization. A configured or published network inside `::ffff:0:0/96` is converted to
the equivalent IPv4 network. Other IPv6 networks, including `::/0`, keep IPv6
semantics and never match an IPv4 client. Host bits in a network are truncated. The same
index serves `client_ip.cidr` and `in_sets`.

Feeds are fetched in the background, never on the request path. The first fetch starts
at startup. `ip_prefixes_json` is the object published by Google, Bing, Apple,
DuckDuckGo and OpenAI:

```json
{"creationTime": "...", "prefixes": [{"ipv4Prefix": "66.249.64.0/27"}, {"ipv6Prefix": "2001:4860:4801:10::/64"}]}
```

`prefixes` is required. A prefix object with both or neither of `ipv4Prefix` and
`ipv6Prefix` is ignored. A value that is not a valid network rejects the document.

`formatVersion` is optional and a major version other than `1` rejects the
document, while other unknown fields are ignored. A document with no usable prefix, or
more than 65 536, is rejected. A successful fetch publishes a new immutable snapshot
atomically, so in-flight evaluations keep the snapshot they loaded.

Requests carry `If-None-Match` and `If-Modified-Since` from the last success and `304 Not Modified`
renews the current snapshot. The response body is limited to 4 MiB and the request to 30
seconds. Redirects are limited to 3 hops that keep the scheme of the configured URL.

After a success the next fetch waits `refresh`. After a failure, a set that never
loaded retries after 15 seconds, with double the wait per consecutive failure up to 5
minutes. A loaded set waits 5 minutes, with double the wait per consecutive failure up
to `refresh`. Every wait adds a uniform jitter of up to 10%. A failure keeps the last
valid snapshot and increments the failure counter.

A remote set is unknown before its first successful load and when its snapshot is older
than `max_stale`. Static sets are always known. Conditions use three-valued logic:
`all` is false when any child is false, otherwise unknown when any child is
unknown, otherwise true.

`any` is true when any child is true, otherwise unknown when any child is
unknown, otherwise false. `not` keeps unknown. A rule whose condition is unknown
never produces a more permissive outcome than it would with complete data:

| Rule action | Unknown condition |
| --- | --- |
| `allow` | Skipped |
| `score` with a positive score | Applied |
| `score` with zero or a negative score | Skipped |
| `challenge` | Applied |
| `deny` | Challenge with the default algorithm and score-derived difficulty |

The deny downgrade bounds the impact of a feed outage on a denylist. A decision is
unresolved when any rule evaluated on the way to it, including the deciding rule, had an
unknown condition. Logs and metrics report unresolved decisions so the cause stays
visible. Configuration validation treats a deny rule that references a remote set as a
possible default challenge for algorithm and Stratum pool checks.

A match of both a published range and a declared User-Agent confirms that the request
comes from the publisher's network. It also confirms that the request claims the
matching identity. It is not cryptographic proof of identity. Publishers split crawler
groups, so each set names one group.

## Challenge selection and score debugging

A challenge without an algorithm uses `pow.default_algorithm`. Explicit Fast difficulty must be
1..64 leading zero hexadecimal digits. Explicit RandomX difficulty must be at least
1000. It is used as supplied. Without an explicit RandomX difficulty:

```text
difficulty = base * 2^(score / score_step)
```

The exponent uses integer division and is clamped to 0..63. Multiplication saturates and
the result is bounded by `pow.algorithms.randomx.difficulty.max`. Fast fallback difficulty is 1 below score 10,
otherwise `2 * (score / 10)`, bounded to 1..64. RandomX rules need explicit `pow.stratum_pool`
infrastructure. `stratum_pool_accepted` permits only RandomX challenges and grants access only
after an actual accepted Stratum response on the owning session.

In development only, set `policy.score.debug_header.enabled: true` to let the configured header replace the initial
score. Its default name is `x-pow-weight`. The gateway signs the selected rule, final
score, difficulty, algorithm, host and verification backend into the presentation. A
browser cannot alter these claims.

## HashWX selection

Select HashWX in a custom rule or score threshold:

```yaml
policy:
  mode: custom
  default_action: allow
  rules:
    - name: hashwx-protected
      when:
        path: {prefix: /protected/}
      action: challenge
      challenge: {algorithm: hashwx, difficulty: 50000}
```

Use `pow.verification: local_verify`. HashWX difficulty is an expected hash
count, between 1 and 2^40. Fast difficulty counts leading zero hex digits.
When difficulty is omitted, `pow.algorithms.hashwx.difficulty` maps the score
to work. Explicit rule difficulty takes precedence over that mapping.

The shipped HashWX base and max values are placeholders. Measure a mid-range
phone before choosing a production cost. A browser with no WASM support gets
the failure and retry controls. The client keeps the selected algorithm;
capability fallback is not implemented.

A challenge action accepts an existing cookie only when algorithm and
enforcement match and cookie difficulty meets the current requirement.
A Fast cookie therefore cannot satisfy a HashWX rule. Host and optional client
bindings still apply. A DENY rule still rejects a valid HashWX cookie.
