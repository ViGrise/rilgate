# Request policies

`policy` owns request classification, scoring and challenge decisions. The internal
namespace is handled by the dedicated router before application policies, including when
a rule allows all of `/.well-known/`. An unknown internal route remains an internal 404 and
is never forwarded by an allow rule.

## Presets

Select a preset with `policy: !include builtin:policies/<name>.yml`.
Without a policy section, Rilgate uses `builtin:policies/balanced.yml`.
An explicit `policy: {}` allows every request. Custom rules need no mode switch.
Preset files can be merged with local settings.

## Built-in policies and rules

`config/policies/`, `config/signals/` and `config/rules/` contain the source catalogs embedded into
the binary at build time. `config/examples/` contains example operator
configurations. Installed binaries load built-in resources without a checkout or
copied catalog directories.

Select a complete policy:

```yaml
policy: !include builtin:policies/balanced.yml
```

| Built-in policy | Behavior |
| --- | --- |
| `builtin:policies/balanced.yml` | `public-endpoints` allows public paths. `protected-traffic` challenges with Fast difficulty 2. Default when policy is absent. |
| `builtin:policies/light.yml` | `public-endpoints` allows public paths. `suspicious-client` challenges empty or bot-like agents with Fast difficulty 1. |
| `builtin:policies/strict.yml` | `public-endpoints` allows public paths. `protected-traffic` challenges with Fast difficulty 4. |
| `builtin:policies/off.yml` | No rules. Default action `allow`. |
| `builtin:policies/crawler-protection.yml` | AI/headless deny rules, verified search crawlers, public paths and client scoring. Needs `builtin:signals/crawler-ip-sets.yml`. |

The crawler-protection policy needs `signals.ip_sets: !include builtin:signals/crawler-ip-sets.yml`
for Google, Bing, Apple and DuckDuckGo search crawler feeds. It includes score thresholds. Remote feed loading
requires outbound HTTPS for runtime readiness. Configuration checks validate the
policy without fetching the feeds.
The [custom example](../config/examples/custom.yml) demonstrates selecting it.

Customize a built-in custom policy with a map merge:

```yaml
signals:
  ip_sets:
    <<: !include builtin:signals/crawler-ip-sets.yml
    internal: {cidrs: [10.0.0.0/8]}
policy:
  <<: !include builtin:policies/crawler-protection.yml
  default_action: challenge
```

Maps merge recursively. Lists and scalars replace previous values.
Signals are shared by conditions independently of the selected policy.

For an operator-defined policy, mix built-in rules, inline rules and local files
in evaluation order:

```yaml
policy:
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
| `tor-exit.yml` | Adds 20 for membership in the operator-defined `tor-exits` IP set. |
| `challenge-all.yml` | Terminal catchall challenge with Fast difficulty 2. Place it last. |
| `deny-ai-crawlers.yml` | Denies recognized AI-affiliated agents, training and search crawlers, and user-triggered AI clients. |
| `deny-headless-browsers.yml` | Denies recognized headless User-Agents and Lightpanda client hints. |
| `deny-domain-fronting.yml` | Denies a nonempty `X-Tls-Sni-Name` that differs from the host. Requires an edge proxy in `server.trusted_proxies` that sets this header to the TLS server name. |
| `deny-empty-user-agent.yml` | Denies empty or missing User-Agent headers. |
| `client-score.yml` | Adds 10 for browser-like User-Agents, 15 for a present `CF-Worker` header and 5 for a present `X-Firefox-Ai` header. Requires score thresholds to act on the result. |

Rule names in this table use the `builtin:rules/` prefix. The optional deny
groups can also match authorized automation. The default balanced policy keeps
challenging application traffic regardless of User-Agent. Built-in references
always select embedded content. Use an explicit local include path for an
operator-maintained copy.

The Tor exit rule requires this set:

```yaml
signals:
  ip_sets:
    tor-exits:
      url: https://check.torproject.org/torbulkexitlist
      format: list_text
      refresh: 1h
      max_stale: 1d
policy:
  rules:
    - !include builtin:rules/tor-exit.yml
```

Its score of 20 combines with 10 from `client-score.yml` to reach the 30 tier.

Each verified search crawler rule allows a request only when its User-Agent
matches the crawler and the client address is in the named IP set. Define the
set with the publisher feed before you include the rule:

```yaml
signals:
  ip_sets:
    bing-crawlers:
      url: https://www.bing.com/toolbox/bingbot.json
      format: ip_prefixes_json
policy:
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
CEL expressions are outside the current policy schema.

## Custom evaluation

See [the complete example](../config/examples/custom.yml). Rules run in declaration order. A matching
`score` adds a signed integer with saturating i32 arithmetic and continues.

`allow`, `deny` and `challenge` are terminal. If no rule terminates,
thresholds run against the final score in declaration order. A threshold matches
`score >= min`.

The last threshold can omit `min` to match every score, including zero and
negative scores. Only one threshold can omit `min`, and it must be last. It uses
the rule name `score fallback` in logs and tokens. Other thresholds use
`score >= N`. Explicit minimums stay unique and strictly descending, and
thresholds cannot use the `score` action.

Minimums must be unique and strictly descending. The first match wins. Otherwise
`default_action` applies. Its default is `allow`. It can be `allow`, `deny`,
or `challenge`, but never `score`.

```yaml
policy:
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

Policies permit at most 128 rules plus thresholds and 512 compiled condition nodes. They
permit at most 16 nested condition levels and 64 entries per membership list. Matcher
strings have limits of 2048 bytes each and 64 KiB in total.

Regex compilation has bounded program and DFA sizes. The request path and method come
from the actual HTTP request. Policy and forwarding use the same validated path.

## Conditions

`host` matches the host used for challenge binding: the `Host` header, or the
URI authority when the header is absent. The gateway supplies it in ASCII
lowercase, including the port when present. For example,
`host: {exact: example.com}` matches that host. `example.com:8443` needs a
matcher that includes its port. Host conditions are available in policy rules
and in `site.open_graph.when`.

The public `rilgate_policy::conditions::CompiledCondition` API compiles request
conditions against `PolicySets` and `GeoDatabases` registries and matches them against a `Request`.
It shares the policy matcher and resource limits. Score matchers are rejected
outside policy rules, with the supplied configuration path. An unknown result
selects no request. `site.open_graph.when` uses this API with the live
`signals.ip_sets`, `signals.asn_sets` and `signals.geoip` registries.

Multiple fields in a condition are combined with AND. `all`, `any`, and
`not` compose conditions recursively. `all` and `any` cannot be
empty.

| Field | Operators |
| --- | --- |
| `host`, `path`, `method`, `user_agent`, individual `headers` and `edge_headers` | `exact`, `prefix`, `contains`, `in`, `regex`, `same_as` |
| `client_ip` | `cidr`, `in_sets`, `reverse_dns` (a list of 1 to 16 domains) |
| `score` | `eq`, `gt`, `gte`, `lt`, `lte` |
| `load` | `window` (`1m`, `5m` default, `15m`), `gt`, `gte`, `lt`, `lte` |
| `country` | a list of 1 to 256 uppercase ISO 3166-1 alpha-2 codes |
| `asn` | a list of 1 to 256 autonomous system numbers or `{in_sets: [name]}` |
| `dnsbl` | one DNSBL zone name |
| `rate` (named `counter`) | `eq`, `gt`, `gte`, `lt`, `lte` on non-negative integers |
| `honeypot` | `true` or `false` |

`client_ip.in_sets`, `country`, `asn`, `dnsbl`, `client_ip.reverse_dns`, `honeypot`
and `rate` read data from the `signals` section. See [signals](signals.md) for each block,
its settings and its unknown results.

A string matcher requires exactly one operator. Matching is case-sensitive unless a
regex explicitly selects case-insensitive matching or `same_as: host` compares with the host name.
`same_as: host` ignores ASCII case and removes the request host's port. A bracketed
host name keeps its brackets. Otherwise, a final colon followed only by ASCII digits marks the port.
Header names are case-insensitive.

`edge_headers` matches headers that the edge sets, only from a trusted peer. Like
`headers`, it works in rules, `count_when` and `site.open_graph.when`. The trusted peers
and the client IP resolution are described in
[client address and forwarding](configuration.md#client-address-and-forwarding). The same
resolved address controls IP rules and proof API quotas.

Any repeated header value can satisfy its matcher. Missing headers do not match,
including an empty-string regex. `not` can test absence. A missing User-Agent is
an empty string. A missing client IP does not match CIDR. Numeric comparisons combine
with AND and impossible ranges are rejected.

`path` conditions and counter `path` keys use the canonical request path: percent-encoded
unreserved characters and `%2F` are decoded, and runs of `/` collapse to one. Matching stays
case-sensitive, so use `(?i)` in a regex for an upstream that ignores case. Rilgate rejects
dot segments, including `..;`, with `400`.

`load` compares the host load average over `window` with decimal bounds of at
least 0. Rilgate samples the average every 5 seconds, and only when a condition
uses `load`. The condition is unknown before the first sample and on platforms
without a load average, so an `allow` rule on it never allows.

```yaml
- name: shed-load
  when:
    all:
      - load: {window: 5m, gte: 4.0, lt: 8.0}
      - path: {prefix: /search}
  action: score
  score: 5
```

## Difficulty escalation

Rules read a [request counter](signals.md#counters) with the `rate` condition.

`rate: {counter: login, gte: 10}` reads a named counter in policy rules. It accepts
`eq`, `gt`, `gte`, `lt` and `lte` on non-negative integers, with AND logic and a
nonempty range. Unknown names are compile errors. `site.open_graph.when` rejects
`rate`. The value includes the current request when `count_when` selects it.
Unselected requests read the current count without incrementing it. Missing keys
are unknown: an `allow` rule never allows, and positive risk scores still apply.
Counting runs once before evaluation. DNS reevaluation reuses the same values.

```yaml
signals:
  counters:
    pages:
      key: [client_ip, host]
      window: 30s
      count_when:
        not: {host: {exact: cdn.example.com}}
    login:
      key: [{client_ip: {ipv4_prefix: 24, ipv6_prefix: 48}}]
      window: 1m
      max_entries: 16384
      count_when: {path: {prefix: /login}}
    cloud-asn:
      key: [asn, host]
      window: 1m
      count_when: {asn: [13335, 136907, 45102]}
policy:
  rules:
    - {name: login-burst, when: {rate: {counter: login, gte: 10}}, action: deny}
    - {name: page-rate, when: {rate: {counter: pages, gte: 60}}, action: score, score: 20}
  score:
    thresholds:
      - {min: 30, action: challenge, challenge: {algorithm: fast, difficulty: 5}}
      - {min: 20, action: challenge, challenge: {algorithm: fast, difficulty: 4}}
      - {action: challenge, challenge: {algorithm: fast, difficulty: 2}}
```

A client with a higher-difficulty cookie keeps it when its rate falls. Rilgate challenges
a client once when its rate enters a higher difficulty. The cookie then passes requests
in that tier.

## Challenge selection

A challenge without an algorithm uses `pow.default_algorithm`. An explicit difficulty in a
rule takes precedence over the score mapping of the algorithm. Each algorithm below
states its difficulty unit and range.

A challenge action accepts an existing cookie only when algorithm and verification mode match
and cookie difficulty meets the current requirement. A Fast cookie therefore cannot
satisfy a HashWX rule. Host and optional client bindings still apply. A DENY rule still
rejects a valid cookie.

### Fast

Explicit Fast difficulty counts leading zero hexadecimal digits, from 1 to 64. Without an
explicit difficulty, the score selects it: 1 below score 10, otherwise `2 * (score / 10)`,
bounded to 1..64.

### HashWX

Select HashWX in a custom rule or score threshold:

```yaml
policy:
  default_action: allow
  rules:
    - name: hashwx-protected
      when:
        path: {prefix: /protected/}
      action: challenge
      challenge: {algorithm: hashwx, difficulty: 50000}
```

HashWX difficulty is an expected hash count, from 1 to 2^40. HashWX always uses
gateway verification. When difficulty is omitted,
`pow.algorithms.hashwx.difficulty` maps the score to work. An explicit rule difficulty
overrides that mapping, including its `max` cap. See
[HashWX configuration](configuration.md#hashwx) for the cost settings and
[browser support](web.md#hashwx) for the client requirements. A browser with no WASM
support gets the failure and retry controls. The client keeps the selected algorithm.

### Refresh

Select the script-free refresh challenge in a custom rule:

```yaml
policy:
  default_action: allow
  rules:
    - name: refresh-protected
      when:
        path: {prefix: /protected/}
      action: challenge
      challenge: {algorithm: refresh, difficulty: 3}
```

Difficulty is a delay of 1 to 10 seconds. See [the refresh algorithm](pow.md#refresh---delay-without-javascript).

### RandomX

Explicit RandomX difficulty must be at least 1000. It is used as supplied. Without an
explicit RandomX difficulty:

```text
difficulty = base * 2^(score / score_step)
```

The exponent uses integer division and is clamped to 0..63. Multiplication saturates and
the result is bounded by `pow.algorithms.randomx.difficulty.max`. See
[verification modes](pow.md#verification-modes) for RandomX proof acceptance and
policies that also select Fast, HashWX and refresh.

### Score debugging

In development only, set `policy.score.debug_header.enabled: true` to let the configured header replace the initial
score. Its default name is `x-pow-weight`. The gateway signs the selected rule, final
score, difficulty, algorithm, host and verification backend into the presentation. A
browser cannot alter these claims.
