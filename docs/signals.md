# Signals

`signals` defines `ip_sets`, `asn_sets`, `geoip`, `dns`, `honeypot` and `counters`. Policy
rules, counters and `site.open_graph.when` read these signal blocks. The `signals` section
reloads with `policy`.

The rule grammar is in [conditions](policy.md#conditions).

## IP sets

A named IP set groups addresses that conditions match with `client_ip.in_sets`. Define
these sets in `signals.ip_sets`. A set name has 1 to 64 bytes from `a-z`, `0-9`, `-` and
`_`, and starts with a letter or digit. The signals section defines at most 64 IP sets. A set
holds at most 65 536 prefixes. A set defines exactly one source: `cidrs` (static) or `url`
(remote feed).

`client_ip` takes exactly one of `cidr` or `in_sets`. `in_sets` lists 1 to 16 defined
names and matches when the address belongs to any of them. An unknown name is a
configuration error. To combine `cidr` and `in_sets`, use an explicit `all` or `any`.

```yaml
signals:
  ip_sets:
    office:
      cidrs: [203.0.113.0/24, 2001:db8::/32]
    google-common-crawlers:
      url: https://developers.google.com/static/crawling/ipranges/common-crawlers.json
      format: ip_prefixes_json
      refresh: 24h
      max_stale: 7d
policy:
  rules:
    - name: verified-google-crawler
      when:
        all:
          - client_ip: {in_sets: [google-common-crawlers]}
          - user_agent: {contains: Googlebot}
      action: allow
```

Specify `format` with `url`.

| Name | Form | Behavior |
| --- | --- | --- |
| IP prefix JSON | `ip_prefixes_json` | Reads `prefixes` objects with `ipv4Prefix` or `ipv6Prefix`. IP sets only. |
| Text list | `list_text` | Reads one address, network or ASN per line. |
| Regex | `{regex: '(?P<value>...)'}` | Reads the named `value` capture from each match. |
| JSONPath | `{json_path: ['$.prefixes[*].ip_prefix']}` | Reads strings or non-negative integers from RFC 9535 queries. |

Text lists require UTF-8. Rilgate removes one trailing carriage return and cuts each line
at the first `#` or `;`. It trims ASCII whitespace and skips empty lines. Each remaining
line must contain one token.

Regex feeds also require UTF-8. Matches without a participating `value` capture contribute
no value. Patterns permit at most 1024 bytes and a compiled size of 1 MiB. Specify a named
`value` capture.

JSONPath accepts 1 to 16 valid paths. Rilgate queries paths in order and reads each result
in query order. Other JSON node types reject the feed. Every extractor requires 1 to 65536
values.

An invalid value rejects the feed. Error messages identify the line, match or JSONPath
result. Quoted values in errors contain at most 64 bytes. They end at a character
boundary. IP values accept IPv4 or IPv6 addresses and networks. Single addresses become
`/32` or `/128` host networks.

```yaml
signals:
  ip_sets:
    tor-exits:
      url: https://check.torproject.org/torbulkexitlist
      format: list_text
      refresh: 1h
      max_stale: 1d
    proxy-addresses:
      url: https://example.com/proxies.txt
      format: {regex: '(?m)^(?P<value>[0-9.]+):\d+$'}
    aws-ec2:
      url: https://ip-ranges.amazonaws.com/ip-ranges.json
      format: {json_path: ["$.prefixes[?@.service == 'EC2'].ip_prefix"]}
```

`refresh` defaults to `24h`. Its range is `1h..=7d`.

`max_stale` defaults to `7d`, is at least `refresh` and at most `30d`. Static sets reject
remote fields. In production a remote `url` must use `https`. Development also accepts
`http`.

Rilgate canonicalizes the client address with `IpAddr::to_canonical`, so an IPv4-mapped
IPv6 peer matches as IPv4. The trusted proxy resolver and the proof API rate limiter use
the same canonicalization. Rilgate converts a configured or published network inside
`::ffff:0:0/96` to the equivalent IPv4 network. Other IPv6 networks, including `::/0`,
keep IPv6 semantics and never match an IPv4 client. Rilgate truncates host bits in a
network. The same index serves `client_ip.cidr` and `in_sets`.

Rilgate fetches feeds in the background, never on the request path. The first fetch starts
at startup. Google, Bing, Apple, DuckDuckGo and OpenAI publish the `ip_prefixes_json`
object:

```json
{"creationTime": "...", "prefixes": [{"ipv4Prefix": "66.249.64.0/27"}, {"ipv6Prefix": "2001:4860:4801:10::/64"}]}
```

Specify `prefixes`. Rilgate ignores a prefix object with both or neither of `ipv4Prefix`
and `ipv6Prefix`. A value that is not a valid network rejects the document.

`formatVersion` is optional. Rilgate rejects a major version other than `1` and ignores
other unknown fields. Rilgate rejects a document with no usable prefix or more than 65 536
prefixes. A successful fetch publishes an immutable snapshot atomically, so in-flight
evaluations keep the snapshot they loaded.

Requests carry `If-None-Match` and `If-Modified-Since` from the last success.
`304 Not Modified` renews the current snapshot. Rilgate limits the response body to 4 MiB
and the request to 30 seconds. Rilgate limits redirects to 3 hops that keep the scheme of
the configured URL.

After a success the next fetch waits `refresh`. After a failure, a set that never loaded
retries after 15 seconds, with double the wait per consecutive failure up to 5 minutes. A
loaded set waits 5 minutes, with double the wait per consecutive failure up to `refresh`.
Every wait adds a uniform jitter of up to 10%. A failure keeps the last valid snapshot and
increments the failure counter.

A remote set is unknown before its first successful load and when its snapshot is older
than `max_stale`. Static sets are always known. Conditions use three-valued logic: `all`
is false when any child is false, otherwise unknown when any child is unknown, otherwise
true.

`any` is true when any child is true, otherwise unknown when any child is unknown,
otherwise false. `not` keeps unknown. A rule whose condition is unknown never produces a
more permissive outcome than it would with complete data:

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
comes from the publisher's network. It also confirms that the request claims the matching
identity. It is not cryptographic proof of identity. Publishers split crawler groups, so
each set names one group.

## ASN sets

`signals.asn_sets` defines named ASN sets. Each registry permits at most 64 sets, each
with 1 to 65536 entries. Set names follow the IP set rules. Choose exactly one source:
static `asns` or a remote `url` with `format`. ASN values range from 1 to 4294967295.
Remote values accept decimal numbers with an optional case-insensitive `AS` prefix. ASN
feeds accept `list_text`, `regex` and `json_path`. They reject `ip_prefixes_json`.
`refresh` defaults to `24h`, with a range of `1h` to `7d`. `max_stale` defaults to `7d`,
with a range from `refresh` to `30d`. Static sets reject `format`, `refresh` and
`max_stale`. Production feed URLs require HTTPS. Development also accepts HTTP. Feed
loading, body limits, retries and snapshot retention follow the IP set rules.

`asn: {in_sets: [datacenter]}` resolves the client ASN through `signals.geoip.asn` and
matches any listed set. The list accepts 1 to 16 defined names. An unknown name causes a
configuration error. This condition works in rules, `count_when` and
`site.open_graph.when`. A missing client IP or an address absent from the database does
not match. An unavailable or stale database makes the condition unknown. A matching set
makes the condition true, even when another set is unknown. Otherwise, an unloaded or
stale remote set makes it unknown. Readiness requires every remote ASN set to load once.
Metrics use the `rilgate_asn_set_*` families.

```yaml
signals:
  asn_sets:
    datacenter:
      url: https://raw.githubusercontent.com/X4BNet/lists_vpn/main/input/datacenter/ASN.txt
      format: list_text
  geoip:
    asn:
      path: /var/lib/rilgate/GeoLite2-ASN.mmdb
policy:
  rules:
    - name: datacenter-score
      when: {asn: {in_sets: [datacenter]}}
      action: score
      score: 5
```

## GeoIP databases

`signals.geoip` configures one database for `country` and one for `asn`. A database
defines exactly one source: `path`, a local MaxMind `.mmdb` file, or `maxmind`, a
download. A `country` database accepts a country or city edition and an `asn` database
accepts ASN or ISP editions. The software refuses a database of another type.

```yaml
signals:
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

A `maxmind` source downloads
`https://download.maxmind.com/geoip/databases/{edition}/download?suffix=tar.gz` with HTTP
basic authentication from `account_id` and `license_key`, and reads the first `.mmdb` file
in the archive. `license_key` is a secret, so `rilgate config show` redacts it. `edition`
has 1 to 64 characters from `A-Z`, `a-z`, `0-9` and `-`.

`refresh` defaults to `24h`. Its range is `1h..=7d` for a `maxmind` source and `1m..=7d`
for a `path` source. A `path` source reloads when the modification time of the file
changes. `max_stale` defaults to `30d`, is at least `refresh` and at most `90d`. Rilgate
limits a download or file to 128 MiB. A failed refresh keeps the previous database and
increments a failure counter. A database that fails to parse never replaces a working one.

`country` and `asn` match the client address against the MaxMind databases in
`signals.geoip`. A list matches when the database holds any listed value for the address.
An address that is missing from the database, or a missing client IP, does not match. An
IPv4-mapped IPv6 client matches as IPv4. Each condition needs its database in
`signals.geoip` and works in policy rules and in `site.open_graph.when`. `asn` also
accepts `{in_sets: [datacenter]}`, as described in [ASN sets](#asn-sets).

```yaml
- name: scraper-sources
  when:
    any:
      - country: [CN, RU]
      - asn: [16509, 14061]
  action: score
  score: 5
```

Databases load in the background. The conditions are unknown until the database loads for
the first time, and again after `max_stale` without a successful refresh. Unknown
conditions follow the rules in [IP sets](#ip-sets). Readiness fails until each configured
database has loaded once. `/metrics` reports `rilgate_geoip_database_loaded`,
`rilgate_geoip_database_stale` and `rilgate_geoip_database_refresh_failures_total` per
database.

A `maxmind` source that fails retries with the delays of a loaded IP set, starting at 5
minutes. MaxMind limits the downloads per day for each account.

## DNS lookups

`signals.dns` tunes the lookups. The settings apply only when a rule uses `dnsbl` or
`client_ip.reverse_dns`. Rilgate reads the DNS servers from the system configuration.
Rilgate caches results per client. It never caches a failed lookup.

| Setting | Default | Range |
| --- | --- | --- |
| `timeout` | `1s` | `1ms..5s` |
| `cache_ttl` | `1h` | at least `1s` |
| `negative_ttl` | `10m` | at least `1s` |
| `max_entries` | `65536` | `1..1048576` |

`dnsbl` is true when the zone lists the client address. Rilgate queries the A record of
the reversed address under the zone: the reversed octets for IPv4 and the 32 reversed
nibbles for IPv6. An answer in `127.0.0.0/8` means listed, and a missing record means not
listed. A policy uses at most 8 zones.

`client_ip.reverse_dns` requires a PTR name of the client to be a listed domain or a
subdomain of it. The A or AAAA records of that name must also contain the client address
for the condition to be true. The match works on label boundaries, ignores case and
ignores a trailing dot. For example, `googlebot.com` matches `crawl.googlebot.com` and
does not match `evilgooglebot.com`. Both conditions need a client IP, work only in policy
rules, and treat an IPv4-mapped IPv6 client as IPv4.

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

A lookup that fails or times out leaves the condition unknown. An `allow` rule on an
unknown condition never allows, and a `deny` rule challenges. Rilgate evaluates the policy
once without lookups, and looks up the client only when the evaluation reaches a DNS
condition. The first request from a client can wait up to `signals.dns.timeout`.

`cache_ttl` applies to a listed address or a verified name. `negative_ttl` applies to an
unlisted address or a client without a verified name.

## Honeypot

`signals.honeypot` turns on a hidden link in the challenge and refresh pages. A person
does not see the link. A crawler that follows every link requests it. The link points to
`{site.base_path}/trap/<random>`, where `<random>` is 16 random bytes in hex that change
on every page. The request flags the client, and the trap URL answers like an unknown
internal route (the 404 page).

The condition `honeypot: true` matches flagged clients, and `honeypot: false` matches the
others. The condition needs `signals.honeypot` and works only in policy rules.

```yaml
signals:
  honeypot:
    ttl: 24h
    max_entries: 65536
policy:
  rules:
    - name: link-followers
      when: {honeypot: true}
      action: deny
```

| Setting | Default | Range |
| --- | --- | --- |
| `ttl` | `24h` | `1s..30d` |
| `max_entries` | `65536` | `1..1048576` |

Rilgate keeps flags in memory, so a restart clears them. A flag covers one IPv4 address or
one IPv6 /64. A request without a resolved client IP flags nobody, so one hit cannot flag
every client behind a proxy that Rilgate does not trust. When the table is full, Rilgate
drops expired flags first and then the oldest flag. The metric
`rilgate_honeypot_hits_total` counts the requests to the trap URL. A page template can
place the link with the variable `honeypot_url`, which is an empty string when the
honeypot is off.

## Counters

`signals.counters` defines up to 32 named request counters. Names contain 1 to 64 ASCII
letters, digits, `-` or `_`.

Each counter has these settings:

| Setting | Meaning |
| --- | --- |
| `key` | Required list of 1 to 4 parts, with no duplicates |
| `window` | Required duration from `1s` to `1h` |
| `max_entries` | Maximum keys retained, from `1` to `1048576`. Default `65536` |
| `count_when` | Optional request condition. Absent selects every request that reaches the gate |

Key parts are `client_ip`, `host`, `method`, `path`, `user_agent`, `country`, `asn`, or
`{header: X-Name}`. Header names are case-insensitive. Rilgate uses the first value. Host
is the lowercase host used for challenge binding, including any port. `client_ip` defaults
to IPv4 /32 and IPv6 /64. Use `{client_ip: {ipv4_prefix: 24, ipv6_prefix: 48}}` to change
the prefixes within `1..32` and `1..128`. IPv4-mapped IPv6 addresses use IPv4 prefixes.
`country` and `asn` need their corresponding `signals.geoip` databases. Omission of the
required database is a compile error. Configure an ASN database before you use the
`cloud-asn` counter in the
[difficulty escalation example](policy.md#difficulty-escalation).

A missing key part (such as a missing client IP, header or GeoIP answer) gives no key.
Rilgate does not count the request, and its rate is unknown. Rilgate stores text values of
at most 256 bytes as-is. Longer values use a SHA-256 digest and still count. This bounds
key memory per entry.

`count_when` accepts request conditions except `score`, `rate`, `dnsbl` and
`client_ip.reverse_dns`, including inside `all`, `any` or `not`. Only a true result
counts. False and unknown do not count. Requests with access cookies count when selected.
Assets, proof API routes and internal routes do not reach the gate. A counter that no rule
reads is valid and does not count requests.

Each counter uses two fixed buckets. The count adds the current bucket to the previous
bucket weighted by the fraction of the window remaining, rounds down, and saturates at
`u32::MAX`. Keys idle for two windows expire. At `max_entries`, Rilgate evicts the least
recent key. Counts live in process memory. Reload preserves a counter only when its name,
key, window, capacity and `count_when` are equal.

Rules read a counter with the `rate` condition, as described in
[difficulty escalation](policy.md#difficulty-escalation).
