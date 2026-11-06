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
| `builtin:policies/crawler-protection.yml` | Deny rules for empty User-Agents, domain fronting, headless browsers and AI agents, verified crawlers, public paths and client scoring. Needs `builtin:signals/crawler-ip-sets.yml`. |
| `builtin:policies/crawler-protection-ai-search.yml` | `crawler-protection` that also allows verified AI search crawlers and user-triggered AI fetchers. Needs `builtin:signals/crawler-ip-sets.yml` and `builtin:signals/ai-crawler-ip-sets.yml`. |

The crawler-protection policy needs `signals.ip_sets: !include builtin:signals/crawler-ip-sets.yml`
for Google, Bing, Apple and DuckDuckGo search crawler feeds. Its other verified crawlers
use reverse DNS or fixed networks and need no feed. It includes score thresholds. Remote
feed loading requires outbound HTTPS for runtime readiness. Configuration checks validate
the policy without fetching the feeds.
The [custom example](../config/examples/custom.yml) demonstrates selecting it.

The AI search policy merges both signal files:

```yaml
signals:
  ip_sets:
    <<:
      - !include builtin:signals/crawler-ip-sets.yml
      - !include builtin:signals/ai-crawler-ip-sets.yml
policy: !include builtin:policies/crawler-protection-ai-search.yml
```

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
| `public-feeds.yml` | Allows `.rss` and `.atom` paths and paths that end in `/feed`. |
| `allow-git-clients.yml` | Allows Git and Git LFS clients on Git transport paths only. |
| `allow-container-clients.yml` | Allows container tools under `/v2/` only. |
| `allow-private-networks.yml` | Allows private, shared, loopback and link-local addresses. Behind a proxy, it needs `server.trusted_proxies`; otherwise every request matches. |
| `challenge-bots.yml` | Challenges empty or bot/tool-like User-Agents with Fast difficulty 1. |
| `tor-exit.yml` | Adds 20 for membership in the operator-defined `tor-exits` IP set. |
| `challenge-all.yml` | Terminal catchall challenge with Fast difficulty 2. Place it last. |
| `deny-ai-crawlers.yml` | Denies recognized AI-affiliated agents, AI scrapers, training and search crawlers, and user-triggered AI clients. |
| `deny-headless-browsers.yml` | Denies recognized headless User-Agents and Lightpanda client hints. |
| `deny-domain-fronting.yml` | Denies a nonempty `X-Tls-Sni-Name` that differs from the host. Requires an edge proxy in `server.trusted_proxies` that sets this header to the TLS server name. |
| `deny-empty-user-agent.yml` | Denies empty or missing User-Agent headers. |
| `client-score.yml` | Adds 10 for browser-like User-Agents, 15 for a present `CF-Worker` header, 5 for a present `X-Firefox-Ai` header, 20 for discontinued browsers and systems and 10 for `Custom-AsyncHttpClient`. Requires score thresholds to act on the result. |
| `browser-signals-score.yml` | Subtracts 10 for a complete browser header set, 5 for Chrome client hints and 2 for `Upgrade-Insecure-Requests`, and adds 5 for a missing `Accept` header. Automated clients can copy these headers. |
| `cloudflare-relay-score.yml` | Adds 15 for Cloudflare relay headers. Use it only on origins that are not behind Cloudflare. |
| `cloud-network-score.yml` | Adds 20 for Alibaba Cloud, Huawei Cloud and Tencent Cloud networks. Needs `signals.geoip.asn`. |

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

Each verified crawler rule checks the client network as well as the claimed
identity. Feed rules allow a request only when its User-Agent matches the crawler
and the client address is in the named IP set. Define the set with the publisher
feed before you include the rule:

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
| `verified-ai-search.yml` | OpenAI, Perplexity and Mistral documentation URLs | `openai-search-crawlers`, `perplexity-crawlers`, `chatgpt-user-fetchers`, `perplexity-user-fetchers`, `mistral-user-fetchers` | `builtin:signals/ai-crawler-ip-sets.yml` |
| `verified-ai-training.yml` | GPTBot documentation URL | `openai-training-crawlers` | `builtin:signals/ai-crawler-ip-sets.yml` |

Place the `verified-ai-*` rules before `deny-ai-crawlers.yml`, so the deny rules
still reject the same User-Agents from other networks.

Reverse DNS rules need no feed. They allow a request when its User-Agent matches
and the forward-confirmed PTR name is in the listed domain. Rilgate performs the
lookup only for requests with a matching User-Agent.

| Reverse DNS rule | User-Agent | Domains |
| --- | --- | --- |
| `yandexbot.yml` | Yandex documentation URL | `yandex.ru`, `yandex.net`, `yandex.com` |
| `qwantbot.yml` | Qwant documentation URL | `qwant.com` |
| `mojeekbot.yml` | Mojeek documentation URL | `mojeek.com` |
| `ccbot.yml` | `CCBot` | `crawl.commoncrawl.org` |
| `telegram-preview.yml` | `TelegramBot` | `ptr.telegram.org` |
| `vk-preview.yml` | VK share URL | `go.mail.ru` |

Fixed network rules hold their address ranges in the rule file. `kagibot.yml`,
`marginalia-crawler.yml` and `wikimedia-citation-fetchers.yml` also require a
matching User-Agent. `internet-archive.yml` and `arquivo-pt.yml` match by network
alone, because archive capture services also send browser User-Agents.

The crawler-protection policy evaluates deny rules before crawler and public
allows, then applies client scores. It uses the refresh challenge at score 1 and
Fast difficulties 2, 4 and 6 at scores 10, 20 and 30. Its fallback allows requests
without a positive score. `telegram-preview.yml`, `vk-preview.yml` and the tool
client rules are not part of the preset; add them when the site needs them.
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
paths that need URL normalization, as described in [reverse proxy](gateway.md#reverse-proxy).

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

The access cookie keeps one PoW grant per algorithm. A higher-difficulty grant passes requests
when the rate falls; a higher requirement asks the client for another proof of that algorithm.
A client that already has a sufficient Fast grant is asked only for the step it lacks.

## Challenge selection

A proof-of-work challenge without an algorithm uses `pow.default_algorithm`. An explicit
difficulty takes precedence over the score mapping. Each algorithm below states its unit
and range. A rule or score threshold can select `challenge: {algorithm: fast, difficulty: 2}`,
`challenge: {hook: captcha}`, or an ordered `steps` list. Omitted PoW fields use their defaults.
[Challenge hooks](configuration.md#challenge-hooks) defines external checks and their configuration.

A challenge action requires a grant for every step and presents the first step with no grant.
A PoW grant must match the algorithm and configured verification mode and meet the difficulty.
Any PoW grant satisfies a refresh wait; a refresh grant must meet the wait difficulty. A hook
grant satisfies the hook with the same name. While a fallback is active, a grant for the
required step or its effective fallback satisfies that step. A hook with `grant: until_expiry`
also accepts a PoW grant tagged with `fallback_for` matching its name until that grant expires.
Host and optional client bindings apply, and a DENY rule rejects a valid cookie.

A `steps` list has 1 to 4 entries, each algorithm and hook appears at most once, and `refresh`
is valid alone. Each entry uses either `{algorithm, difficulty}` or `{hook}`. Top-level `steps`,
`hook`, and `algorithm` with `difficulty` are mutually exclusive. Compilation validates PoW
steps against registered algorithms and difficulty ranges and hook names against the running
hook set, including on policy reload. The cookie retains one grant per method with its own
expiry, drops expired or unconfigured grants, and expires when its latest grant expires.
PoW grants use `security.auth.ttl`; hook grants use their configured `grant_ttl`.

```yaml
policy:
  rules:
    - name: protected-endpoint
      when: {path: {prefix: /protected/}}
      action: challenge
      challenge:
        steps:
          - {algorithm: fast, difficulty: 2}
          - {hook: captcha}
```

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
      challenge: {algorithm: hashwx, difficulty: 610000}
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

Difficulty is a delay from 1 second to `pow.algorithms.refresh.max`, 10 seconds by default. See [the refresh algorithm](pow.md#refresh---delay-without-javascript).

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
