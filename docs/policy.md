# Request policies

`policy` owns request classification, scoring and challenge decisions. The
internal namespace is handled by the dedicated router before application
policies, including when a rule allows all of `/.well-known/`. An unknown
internal route remains an internal 404 and is never forwarded by an allow rule.

## Presets

| Mode | Behavior |
| --- | --- |
| `off` | Allow application traffic without requiring PoW |
| `light` | Challenge missing or bot-like User-Agents at Fast difficulty 1 |
| `balanced` | Challenge application traffic at Fast difficulty 2 |
| `strict` | Challenge application traffic at Fast difficulty 4 |
| `custom` | Use only the operator's rules, thresholds and default action |

Light matches User-Agents containing bot, crawler, spider, curl, wget, python,
or scrapy, case-insensitively, or an empty User-Agent. Light, balanced and strict
allow exact `/robots.txt`, `/favicon.ico`, and the `/.well-known/acme-challenge/`
prefix. Presets never require RandomX or P2Pool. Any custom `rules`, `score`, or
`default_action` in a preset is rejected rather than silently ignored.

## Custom evaluation

See [the complete example](../config/rilgate.custom.example.yml). Rules run in
declaration order. A matching `score` adds a signed integer with saturating i32
arithmetic and continues. `allow`, `deny` and `challenge` are terminal. If no rule
terminates, thresholds run against the final score in declaration order. A
threshold matches `score >= min`; minimums must be unique and strictly descending.
The first match wins. Otherwise `default_action` applies, defaulting to `allow`.
It can be `allow`, `deny`, or `challenge`, but never `score`.

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

Every rule needs a unique nonempty name and a nonempty `when`. `score` is required
only on a score action. `challenge` settings are accepted only on a challenge
action, in rules and thresholds. Custom policies never inherit preset exceptions.

## Conditions

Multiple fields in a condition are combined with AND. `all`, `any`, and `not`
compose conditions recursively; `all` and `any` cannot be empty.

| Field | Operators |
| --- | --- |
| `path`, `method`, `user_agent`, individual `headers` | `exact`, `prefix`, `contains`, `in`, `regex` |
| `client_ip` | `cidr` |
| `score` | `eq`, `gt`, `gte`, `lt`, `lte` |

A string matcher requires exactly one operator. Matching is case-sensitive
unless a regex explicitly selects case-insensitive matching. Header names are
case-insensitive; any repeated header value may satisfy its matcher. Missing
headers do not match, including an empty-string regex; `not` can test absence.
A missing User-Agent is an empty string. A missing client IP does not match CIDR.
Numeric comparisons combine with AND and impossible ranges are rejected.

Client IP comes from the TCP peer unless `server.trusted_proxies` authorizes a
forwarded chain. Forwarded headers from untrusted peers cannot influence IP rules.
The same resolved address controls proof API quotas.

Policies permit at most 128 rules plus thresholds, 512 compiled condition nodes,
16 nested condition levels, 64 entries per membership list, 2048 bytes per
matcher string and 64 KiB of matcher strings in total. Regex compilation has
bounded program and DFA sizes. The request path and method come from the actual
HTTP request, and policy and forwarding use the same validated path.

## Challenge selection and score debugging

A challenge without an algorithm uses `pow.default_algorithm`. Explicit Fast
difficulty must be 1..64 leading zero hexadecimal digits. Explicit RandomX
difficulty must be at least 1000; it is used as supplied. Without an explicit
RandomX difficulty:

```text
difficulty = base * 2^(score / score_step)
```

The exponent uses integer division and is clamped to 0..63. Multiplication
saturates and the result is bounded by `pow.algorithms.randomx.difficulty.max`.
Fast fallback difficulty is 1 below score 10, otherwise `2 * (score / 10)`,
bounded to 1..64. RandomX rules require explicit `pow.p2pool` infrastructure.
`p2pool_accepted` permits only RandomX challenges and grants access only after
an actual accepted Stratum response on the owning session.

In development only, set `policy.score.debug_header.enabled: true` to let the
configured header replace the initial score. Its default name is `x-pow-weight`.
The gateway signs the selected rule, final score, difficulty, algorithm, host and
verification backend into the presentation. A browser cannot alter these claims.
