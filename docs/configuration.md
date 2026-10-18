# Configuration

The YAML contract has six groups: `environment`, `server`, `site`, `security`,
`pow`, and `policy`. Configuration has no schema version. Unknown fields,
unsupported values and conflicting settings are errors in every environment.
This unreleased schema replaces the previous configuration format; old keys
are rejected, with no compatibility loader or migration command.

The minimal configuration requires an upstream URL and a signing secret:

```yaml
server:
  upstream:
    url: http://127.0.0.1:3000
security:
  secret: "${RILGATE_SECRET}"
```

Generate at least 256 bits of randomness with `openssl rand -hex 32` and supply
`RILGATE_SECRET` through the process environment or deployment secret store.
Rilgate derives independent challenge and authentication keys using HKDF-SHA256
with separate context identifiers. Secret length is checked, but randomness
cannot be established by inspecting a string. Never commit a real secret.

The defaults are production mode, an IPv4 wildcard listener at `:8080`, embedded
site resources, local Fast verification, and the balanced policy. RandomX and
P2Pool are opt-in. The balanced preset challenges application traffic at Fast
difficulty 2 regardless of User-Agent, with public exceptions described in
[policies](policy.md). HTTPS is required for production authentication cookies.

Use [the commented distribution template](../config/rilgate.example.yml),
[custom policy example](../config/rilgate.custom.example.yml), or
[RandomX example](../config/rilgate.randomx.example.yml).

## Loading and validation

Rilgate reads `--config FILE`, then `RILGATE_CONFIG`, then `config/rilgate.yml`.
An explicitly selected missing file fails. If the default file is absent,
`RILGATE_UPSTREAM_URL` and `RILGATE_SECRET` can supply the essentials.
`.env` is loaded from the working directory without replacing existing process
variables. YAML is parsed before string interpolation, so environment values
cannot inject YAML structure. References are converted according to the expected
scalar type: strings remain strings, booleans require `true` or `false`, and
numbers must fit their numeric type. Literal YAML values retain strict typing. `${VARIABLE_NAME}` requires a present, nonempty
UTF-8 variable. Shell substitution and default-value syntax are unsupported.

Three explicit environment overrides are available: `RILGATE_SECRET`,
`RILGATE_UPSTREAM_URL`, and `RILGATE_LISTEN`. Other settings use YAML, optionally
with `${VARIABLE_NAME}` references. CLI `--env`, `--port`, and `--verification`
override the corresponding resolved values. Old automatic `POW_*`, `WEB_*`,
`SERVE_*`, `ENV`, `PORT`, and `CONFIG` aliases are unsupported.

```bash
rilgate --config /etc/rilgate/config.yml config check
rilgate --config /etc/rilgate/config.yml config show
rilgate --config /etc/rilgate/config.yml config show --defaults
```

`check` validates fields, cross-field constraints, policy expressions, and
algorithm selection without opening a listener or contacting dependencies.
`show` produces resolved YAML with every signing key and pool password redacted.
By default it omits unchanged defaults; `--defaults` includes the whole schema.
A redacted result is for inspection and cannot be used as a secret-bearing
configuration. Validate external templates with `rilgate web validate DIRECTORY`.

## Defaults

Durations use units such as `300ms`, `5s`, `1m`, `6h`. Timeouts retain subsecond
precision. Token TTLs require positive whole seconds because signed claims use
Unix seconds. Durations are positive and at most one year; readiness is at most
30s, and resource-watch debounce is at most 60s. Sizes require a positive integer
with `B`, `KiB`, `MiB`, or `GiB`, without rounding or overflow.

| Setting | Default |
| --- | --- |
| `environment` | `production` |
| `server.listen` | `:8080` |
| `server.upstream.preserve_host` | `true` |
| `server.upstream.max_body_size` | `16MiB` |
| `server.upstream.timeout.connect`, `.read` | `5s`, `30s` |
| `server.max_connections`, `.max_requests`, `.h2_max_streams` | `1024`, `512`, `64` |
| `server.trusted_proxies` | `[]`, trust no peers |
| `server.metrics_enabled`, `.readiness_timeout` | `false`, `2s` |
| `site.name`, `.url`, `.base_path`, `.stylesheet` | `Rilgate`, `https://rilgate.com`, `/.well-known/rilgate`, `css/rilgate.css` |
| `site.resources.directory`, `.seed` | none, `if_empty` |
| `site.resources.watch.enabled`, `.debounce` | `false`, `300ms` |
| `site.templates.cache`, `.strict`, `.recursion_limit`, `.fuel` | `true`, `true`, `64`, `50000` |
| `site.i18n.default`, `.supported`, `.strict`, `.cookie` | `en`, `[]` (discover), `false`, `rilgate_lang` |
| `security.auth.cookie`, `.ttl` | `rilgate_auth`, `6h` |
| `pow.verification`, `.default_algorithm` | `local_verify`, `fast` |
| `pow.challenge.ttl`, `.receipt_ttl` | `1m`, `30s` |
| `pow.rate_limit.enabled`, `.max_clients` | `true`, `4096` |
| `pow.rate_limit.client.burst`, `.refill_per_second` | `10`, `0.1` |
| `pow.rate_limit.global.burst`, `.refill_per_second` | `64`, `16` |
| `pow.algorithms.randomx.challenge_ttl`, `.nonce_offset`, `.verifier_queue` | `1h`, `39`, `8` |
| `pow.algorithms.randomx.difficulty.base`, `.score_step`, `.max` | `1000`, `10`, `32000` |
| `pow.algorithms.randomx.browser.module` | `/static/js/solvers/randomx/solver.js` |
| `policy.mode` | `balanced` |

When `pow.p2pool` is enabled, its defaults are address `127.0.0.1:3333`, password
`x`, agent `rilgate/0.1.0`, connect/submit timeouts `5s`/`20s`, session capacity
`64`, session TTL `15m`, and job-session mode `pooled`, maximum `16`, idle `1m`.
Set mode `per_challenge` to create one connection per job; `max` and `idle`
configure the pooled mode. Zero never selects a job acquisition strategy.

Custom policies default to `default_action: allow`, no rules, score initial `0`,
debug header disabled with name `x-pow-weight`, and no thresholds. Custom settings
are rejected in preset modes. Debug headers are forbidden in production.

## Separate keys and rotation

Use explicit keys instead of a master secret when independent key management or
rolling rotation is required. The two modes are mutually exclusive.

```yaml
security:
  keys:
    challenge: "${RILGATE_POW_SIGNING_KEY}"
    auth: "${RILGATE_AUTH_SIGNING_KEY}"
    challenge_rotation:
      active_id: proof-next
      previous:
        proof-current: "${RILGATE_PREVIOUS_POW_KEY}"
    auth_rotation:
      active_id: auth-next
      previous:
        auth-current: "${RILGATE_PREVIOUS_AUTH_KEY}"
```

Each key must have at least 32 bytes. Key IDs contain 1 to 32 ASCII letters,
digits, underscores or hyphens. Each ring permits at most eight verification
keys. The active ID cannot be in `previous`. Omit the active ID for unversioned
signing; a verification entry named `legacy` explicitly accepts unversioned
signatures during a named-key transition. Changing a master secret invalidates
its existing tokens; use explicit keys when an overlap window is required.
Secrets never appear in configuration
output or debug representations. See [production rotation](production.md).

## API response presentation

`site.responses.format` selects `auto`, `html` or `json`. Automatic mode negotiates
request headers; `site.responses.json_paths` can force JSON for API path prefixes.
Operator JSON templates under the external web resource directory can match an
application-specific envelope. See [AJAX and API integration](api-integration.md)
for notification headers, JSON body templates and iframe completion.
