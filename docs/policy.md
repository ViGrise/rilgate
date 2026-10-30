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
CEL expressions are outside the current policy schema.

## Custom evaluation

See [the complete example](../config/examples/custom.yml). Rules run in declaration order. A matching
`score` adds a signed integer with saturating i32 arithmetic and continues.

`allow`, `deny` and `challenge` are terminal. If no rule terminates,
thresholds run against the final score in declaration order. A threshold matches
`score >= min`.

The last threshold can omit `min` to match every score, including zero and
negative scores. Only one threshold can omit `min`, and it must be last. It uses
the rule name `score fallback` in logs and tokens; other thresholds use
`score >= N`. Explicit minimums stay unique and strictly descending, and
thresholds cannot use the `score` action.

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

`host` matches the host used for challenge binding: the `Host` header, or the
URI authority when the header is absent. The gateway supplies it in ASCII
lowercase, including the port when present. For example,
`host: {exact: example.com}` matches that host; `example.com:8443` needs a
matcher that includes its port. Host conditions are available in policy rules
and in `site.open_graph.when`.

The public `rilgate_policy::conditions::CompiledCondition` API compiles request
conditions against `IpSets` and `GeoDatabases` registries and matches them against a `Request`.
It shares the policy matcher and resource limits. Score matchers are rejected
outside policy rules, with the supplied configuration path. An unknown result
selects no request. `site.open_graph.when` uses this API with the live
`policy.ip_sets` and `policy.geoip` registries.

Multiple fields in a condition are combined with AND. `all`, `any`, and
`not` compose conditions recursively. `all` and `any` cannot be
empty.

| Field | Operators |
| --- | --- |
| `host`, `path`, `method`, `user_agent`, individual `headers` | `exact`, `prefix`, `contains`, `in`, `regex` |
| `client_ip` | `cidr`, `in_sets`, `reverse_dns` (a list of 1 to 16 domains) |
| `score` | `eq`, `gt`, `gte`, `lt`, `lte` |
| `load` | `window` (`1m`, `5m` default, `15m`), `gt`, `gte`, `lt`, `lte` |
| `country` | a list of 1 to 256 uppercase ISO 3166-1 alpha-2 codes |
| `asn` | a list of 1 to 256 autonomous system numbers |
| `dnsbl` | one DNSBL zone name |
| `rate` (named `counter`) | `eq`, `gt`, `gte`, `lt`, `lte` on non-negative integers |
| `honeypot` | `true` or `false` |

A string matcher requires exactly one operator. Matching is case-sensitive unless a
regex explicitly selects case-insensitive matching. Header names are case-insensitive.

Any repeated header value can satisfy its matcher. Missing headers do not match,
including an empty-string regex. `not` can test absence. A missing User-Agent is
an empty string. A missing client IP does not match CIDR. Numeric comparisons combine
with AND and impossible ranges are rejected.

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

`country` and `asn` match the client address against the MaxMind databases in
`policy.geoip`. A list matches when the database holds any listed value for the address.
An address that is missing from the database, or a missing client IP, does not match.
An IPv4-mapped IPv6 client matches as IPv4. Each condition needs its database in
`policy.geoip` and works in policy rules and in `site.open_graph.when`.

```yaml
- name: scraper-sources
  when:
    any:
      - country: [CN, RU]
      - asn: [16509, 14061]
  action: score
  score: 5
```

`dnsbl` is true when the zone lists the client address. Rilgate queries the A record of
the reversed address under the zone: the reversed octets for IPv4 and the 32 reversed nibbles
for IPv6. An answer in `127.0.0.0/8` means listed, and a missing record means not listed.
A policy uses at most 8 zones.

`client_ip.reverse_dns` is true when a PTR name of the client is a listed domain or a
subdomain of it, and the A or AAAA records of that name contain the client address.
The match works on label boundaries, ignores case and ignores a trailing dot. For example,
`googlebot.com` matches `crawl.googlebot.com` and does not match `evilgooglebot.com`.
Both conditions need a client IP, work only in policy rules, and treat an IPv4-mapped IPv6
client as IPv4.

```yaml
- name: verified-googlebot
  when:
    all:
      - user_agent: {contains: Googlebot}
      - client_ip: {reverse_dns: [googlebot.com, google.com]}
  action: allow

- name: listed-source
  when: {dnsbl: dnsbl.dronebl.org}
  action: score
  score: 10
```

A lookup that fails or times out leaves the condition unknown. An `allow` rule on an unknown
condition never allows, and a `deny` rule challenges. Rilgate evaluates the policy once
without lookups, and looks up the client only when the evaluation reaches a DNS condition.
The first request from a new client can wait up to `policy.dns.timeout`.

`policy.dns` tunes the lookups and requires `mode: custom`. The settings apply only when a
rule uses `dnsbl` or `client_ip.reverse_dns`. Rilgate reads the DNS servers from the system
configuration. Results are cached per client, and a failed lookup is never cached.

| Setting | Default | Range |
| --- | --- | --- |
| `timeout` | `1s` | `1ms..5s` |
| `cache_ttl` | `1h` | at least `1s` |
| `negative_ttl` | `10m` | at least `1s` |
| `max_entries` | `65536` | `1..1048576` |

`cache_ttl` applies to a listed address or a verified name. `negative_ttl` applies to an
unlisted address or a client without a verified name.

## Counters and difficulty escalation

`policy.counters` defines up to 32 named request counters. Names contain 1 to
64 ASCII letters, digits, `-` or `_`. Counters require `mode: custom`.

Each counter has these settings:

| Setting | Meaning |
| --- | --- |
| `key` | Required list of 1 to 4 parts, with no duplicates |
| `window` | Required duration from `1s` to `1h` |
| `max_entries` | Maximum keys retained, from `1` to `1048576`; default `65536` |
| `count_when` | Optional request condition; absent selects every request that reaches the gate |

Key parts are `client_ip`, `host`, `method`, `path`, `user_agent`, `country`, `asn`,
or `{header: X-Name}`. Header names are case-insensitive; the first value is used.
Host is the lowercase host used for challenge binding, including any port.
`client_ip` defaults to IPv4 /32 and IPv6 /64. Use
`{client_ip: {ipv4_prefix: 24, ipv6_prefix: 48}}` to change the prefixes within
`1..32` and `1..128`. IPv4-mapped IPv6 addresses use IPv4 prefixes.
`country` and `asn` need their corresponding `policy.geoip` databases; omitting
the required database is a compile error. Configure an ASN database before using
the `cloud-asn` counter in the example below.

A missing key part (such as a missing client IP, header or GeoIP answer) gives no
key: the request is not counted and its rate is unknown. Text values of at most
256 bytes are stored as-is; longer values use a SHA-256 digest and still count.
This bounds key memory per entry.

`count_when` accepts request conditions except `score`, `rate`, `dnsbl` and
`client_ip.reverse_dns`, including inside `all`, `any` or `not`. Only a true result
counts; false and unknown do not count. Requests with access cookies count when
selected. Assets, proof API routes and internal routes do not reach the gate.
A counter that no rule reads is valid and does not count requests.

`rate: {counter: login, gte: 10}` reads a named counter in policy rules. It accepts
`eq`, `gt`, `gte`, `lt` and `lte` on non-negative integers, with AND logic and a
nonempty range. Unknown names are compile errors. `site.open_graph.when` rejects
`rate`. The value includes the current request when selected by `count_when`;
unselected requests read the current count without incrementing it. Missing keys
are unknown: an `allow` rule never allows, and positive risk scores still apply.
Counting runs once before evaluation; DNS reevaluation reuses the same values.

Each counter uses two fixed buckets. The count adds the current bucket to the
previous bucket weighted by the fraction of the window remaining, rounds down,
and saturates at `u32::MAX`. Keys idle for two windows expire. At `max_entries`,
the least recent key is evicted. Counts live in process memory. Reload preserves
a counter only when its name, key, window, capacity and `count_when` are equal.

```yaml
policy:
  mode: custom
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
  rules:
    - {name: login-burst, when: {rate: {counter: login, gte: 10}}, action: deny}
    - {name: page-rate, when: {rate: {counter: pages, gte: 60}}, action: score, score: 20}
  score:
    thresholds:
      - {min: 30, action: challenge, challenge: {algorithm: fast, difficulty: 5}}
      - {min: 20, action: challenge, challenge: {algorithm: fast, difficulty: 4}}
      - {action: challenge, challenge: {algorithm: fast, difficulty: 2}}
```

A client with a higher-difficulty cookie keeps it when its rate falls. A client
is challenged once when its rate enters a higher difficulty, then the new cookie
passes requests in that tier.

## Honeypot

`policy.honeypot` turns on a hidden link in the challenge and refresh pages. A person does not
see the link. A crawler that follows every link requests it. The link points to
`{site.base_path}/trap/<random>`, where `<random>` is 16 random bytes in hex that change on
every page. The request flags the client, and the trap URL answers like an unknown internal
route (the 404 page).

The condition `honeypot: true` matches flagged clients, and `honeypot: false` matches the
others. The condition needs `policy.honeypot` and works only in policy rules.

```yaml
policy:
  mode: custom
  honeypot:
    ttl: 24h
    max_entries: 65536
  rules:
    - name: link-followers
      when: {honeypot: true}
      action: deny
```

| Setting | Default | Range |
| --- | --- | --- |
| `ttl` | `24h` | `1s..30d` |
| `max_entries` | `65536` | `1..1048576` |

Rilgate keeps flags in memory, so a restart clears them. A flag covers one IPv4 address or one
IPv6 /64. A request without a resolved client IP flags nobody, so one hit cannot flag every
client behind a proxy that Rilgate does not trust. When the table is full, Rilgate drops
expired flags first and then the oldest flag. The metric `rilgate_honeypot_hits_total` counts
the requests to the trap URL. A page template can place the link with the variable
`honeypot_url`, which is an empty string when the honeypot is off.

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
possible default challenge for algorithm and Stratum pool checks. A deny rule that uses
`country` or `asn` gets the same treatment.

A match of both a published range and a declared User-Agent confirms that the request
comes from the publisher's network. It also confirms that the request claims the
matching identity. It is not cryptographic proof of identity. Publishers split crawler
groups, so each set names one group.

## GeoIP databases

`policy.geoip` configures one database for `country` and one for `asn`. It requires
`mode: custom`. A database defines exactly one source: `path`, a local MaxMind `.mmdb`
file, or `maxmind`, a download. A `country` database accepts a country or city edition
and an `asn` database accepts an ASN edition. The software refuses a database of another type.

```yaml
policy:
  mode: custom
  geoip:
    country:
      path: /var/lib/rilgate/GeoLite2-Country.mmdb
    asn:
      maxmind:
        edition: GeoLite2-ASN
        account_id: "123456"
        license_key: "${MAXMIND_LICENSE_KEY}"
      refresh: 24h
```

A `maxmind` source downloads `https://download.maxmind.com/geoip/databases/{edition}/download?suffix=tar.gz`
with HTTP basic authentication from `account_id` and `license_key`, and reads the first
`.mmdb` file in the archive. `license_key` is a secret, so `rilgate config show` redacts it.
`edition` has 1 to 64 characters from `A-Z`, `a-z`, `0-9` and `-`.

`refresh` defaults to `24h`. Its range is `1h..=7d` for a `maxmind` source and
`1m..=7d` for a `path` source. A `path` source reloads when the modification time of the file
changes. `max_stale` defaults to `30d`, is at least `refresh` and at most `90d`. A download or
file is limited to 128 MiB. A failed refresh keeps the previous database and increments a
failure counter. A database that fails to parse never replaces a working one.

Databases load in the background. The conditions are unknown until the database loads
for the first time, and again after `max_stale` without a successful refresh. Unknown
conditions follow the rules in [IP sets](#ip-sets). Readiness fails until each
configured database has loaded once. `/metrics` reports
`rilgate_geoip_database_loaded`, `rilgate_geoip_database_stale` and
`rilgate_geoip_database_refresh_failures_total` per database.

A `maxmind` source that fails retries with the delays of a loaded IP set, starting at
5 minutes, because MaxMind limits the downloads per day for each account.

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

## Refresh selection

Select the script-free refresh challenge in a custom rule:

```yaml
policy:
  mode: custom
  default_action: allow
  rules:
    - name: refresh-protected
      when:
        path: {prefix: /protected/}
      action: challenge
      challenge: {algorithm: refresh, difficulty: 3}
```

Difficulty is a delay of 1 to 10 seconds. See [the refresh algorithm](pow.md#refresh---delay-without-javascript).

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
