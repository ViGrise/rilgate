# Request policies

## Policy and request weight

The gateway evaluates `pow.bots` and `pow.thresholds` from the selected YAML
configuration, which defaults to `config/rilgate.yml` when present.

A `WEIGH` rule adds or subtracts points and policy evaluation continues.
`ALLOW`, `DENY`, and `CHALLENGE` are terminal bot-rule actions.

Example:

```yaml
pow:
  risk:
    default_weight: 0

  bots:
    - name: well-known
      path_regex: ^/\.well-known/.*$
      action: ALLOW

    - name: robots-txt
      path_regex: ^/robots\.txt$
      action: ALLOW

    - name: trusted-session
      headers_regex:
        Cookie: trusted_session=
      action: WEIGH
      weight:
        adjust: -10

    - name: suspicious-client
      user_agent_regex: (?i:bot|crawler)
      action: WEIGH
      weight:
        adjust: 20

    - name: generic-browser
      user_agent_regex: Mozilla|Opera
      action: WEIGH
      weight:
        adjust: 10
```

Supported request matchers:

```text
path_regex
user_agent_regex
headers_regex
```

When a rule contains more than one matcher, all of them must match.

After all matching `WEIGH` rules have been accumulated, the first matching
threshold decides what to do:

```yaml
pow:
  thresholds:
    - name: minimal-suspicion
      expression: weight <= 0
      action: ALLOW

    - name: mild-suspicion
      expression:
        all:
          - weight > 0
          - weight < 10
      action: CHALLENGE
      challenge:
        algorithm: fast
        difficulty: 1

    - name: moderate-suspicion
      expression:
        all:
          - weight >= 10
          - weight < 20
      action: CHALLENGE
      challenge:
        algorithm: fast
        difficulty: 2

    - name: high-suspicion
      expression:
        all:
          - weight >= 20
          - weight < 30
      action: CHALLENGE
      challenge:
        algorithm: fast
        difficulty: 4

    - name: extreme-suspicion
      expression: weight >= 30
      action: CHALLENGE
      challenge:
        algorithm: randomx
        difficulty: 8000
```

Supported threshold expressions are intentionally small and predictable:

```text
weight < N
weight <= N
weight == N
weight != N
weight >= N
weight > N
```

They can be grouped using:

```yaml
expression:
  all:
    - weight >= 10
    - weight < 20
```

or:

```yaml
expression:
  any:
    - weight < 0
    - weight >= 30
```

If no bot rule or threshold produces a terminal action, the gateway defaults to
`ALLOW`.

A direct `CHALLENGE` bot rule may specify its own algorithm and difficulty:

```yaml
- name: expensive-bot-challenge
  user_agent_regex: (?i:bot|crawler)
  action: CHALLENGE
  challenge:
    algorithm: randomx
    difficulty: 16000
```

If `challenge.difficulty` is omitted, the gateway derives it from weight.

For `randomx`:

```text
difficulty = base * 2^(weight / weight_step)
```

and clamps it at `pow.difficulty.max`.

The exponent uses integer division for positive weights and is capped at 63;
nonpositive weights use `pow.difficulty.base`. RandomX difficulty is at least
1000. The maximum applies to this derived fallback; an explicit
`challenge.difficulty` may exceed it.

For `fast`, the fallback follows the lightweight scale approximately used by
the default policy (`1`, `2`, `4`, `6`, ... leading zero hex digits).

The policy decision is calculated on the **original application request**, not
on the internal challenge API request. The gateway signs the original path,
matched rule, weight, difficulty, host, and enforcement mode into a short-lived
presentation token. The browser cannot change the path or lower its assigned
weight/difficulty before requesting PoW work.

For local testing only, `pow.risk.allow_debug_header` can enable the configured
debug header:

```yaml
pow:
  risk:
    allow_debug_header: true
    debug_header: "x-pow-weight"
```

Then:

```bash
curl -H 'x-pow-weight: 20' http://localhost:8080/
```

sets the initial weight to `20` before `bots` rules run. This is disabled by
default and is rejected in production mode.
