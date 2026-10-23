# Configuration

The YAML contract has six groups: `environment`, `server`, `site`,
`security`, `pow`, and `policy`. Configuration has no schema version.
Unknown fields, unsupported values and conflicting settings are errors in every
environment. This unreleased schema replaces the previous configuration format. Old keys
are rejected, with no compatibility loader or migration command.

The minimal configuration requires an upstream URL and a signing secret:

```yaml
server:
  upstream:
    url: http://127.0.0.1:3000
security:
  secret: "${RILGATE_SECURITY_SECRET}"
```

1. Generate at least 256 bits of randomness with `openssl rand -hex 32`.
2. Supply `RILGATE_SECURITY_SECRET` through the process environment or deployment secret store.

Rilgate derives independent challenge and authentication keys using HKDF-SHA256 with
separate context identifiers. Secret length is checked. Inspection of a string cannot
establish randomness. Never commit a real secret.

The defaults are production mode, an IPv4 wildcard listener at `:8080`, embedded
site resources, local Fast verification, and the balanced policy. RandomX and P2Pool are
opt-in. The balanced preset challenges application traffic at Fast difficulty 2
regardless of User-Agent, with public exceptions described in [policies](policy.md).
HTTPS is required for production authentication cookies.

Use [the commented distribution template](../config/rilgate.example.yml), [custom policy
example](../config/rilgate.custom.example.yml), or [RandomX example](../config/rilgate.randomx.example.yml).

## Loading and validation

Rilgate reads `--config FILE`, then `RILGATE_CONFIG`. Without either, it reads the first of
`rilgate.yml` and `config/rilgate.yml` in the working directory that exists, and warns when both
exist. An explicitly selected missing file fails.

If no default file exists, `RILGATE_SERVER_UPSTREAM_URL` and `RILGATE_SECURITY_SECRET` can supply the essentials.
Rilgate loads `.env` from the working directory, or loads the file named by
`--env-file`. Existing process variables remain unchanged.

YAML is parsed before string interpolation, so environment values cannot inject YAML
structure. References are converted according to the expected scalar type. Strings
remain strings.

Booleans need `true` or `false`. Numbers must fit their numeric type. Literal
YAML values retain strict typing. `${VARIABLE_NAME}` requires a present, nonempty UTF-8
variable. Shell substitution and default-value syntax are unsupported.

Every scalar and list setting also has a flag and an environment variable. Precedence is
flag, then variable, then YAML, then the default. The flag is the YAML path with
`.` and `_` replaced by `-`. The variable is `RILGATE_`
followed by the flag in upper snake case. There are no exceptions.
[`.env.example`](../.env.example) lists every variable with its default.

| YAML path | Flag | Variable |
| --- | --- | --- |
| `environment` | `--environment`, `-E` | `RILGATE_ENVIRONMENT` |
| `server.upstream.url` | `--server-upstream-url`, `-u` | `RILGATE_SERVER_UPSTREAM_URL` |
| `server.listen` | `--server-listen`, `-l` | `RILGATE_SERVER_LISTEN` |
| `server.operations.listen` | `--server-operations-listen` | `RILGATE_SERVER_OPERATIONS_LISTEN` |
| `security.secret` | `--security-secret`, `-s` | `RILGATE_SECURITY_SECRET` |
| `server.max_connections` | `--server-max-connections` | `RILGATE_SERVER_MAX_CONNECTIONS` |
| `site.i18n.default` | `--site-i18n-default` | `RILGATE_SITE_I18N_DEFAULT` |
| `pow.p2pool.address` | `--pow-p2pool-address` | `RILGATE_POW_P2POOL_ADDRESS` |

`rilgate --help` lists every setting with its YAML path and default. Values are validated by
the schema exactly like `${VARIABLE_NAME}` references, so durations take `5s` and
sizes take `16MiB`. An invalid value names its flag or variable.

Boolean flags alone mean `true` and accept `--flag=false`. Lists are comma-separated
or repeated and replace the YAML list. An empty value clears it.

Any `pow.p2pool.*` value enables P2Pool with defaults for the rest. Policy rules, IP sets
and signing key rotation (`security.keys`) are YAML only. Pass the secret through
`RILGATE_SECURITY_SECRET`. Do not use `--security-secret`, because process arguments are visible to other
local users.

`server.listen` takes one address or a list of up to 16, such as `[":8080", "[::]:8443"]`. `--server-listen`
and `RILGATE_SERVER_LISTEN` take a comma-separated list. `[::]:8080` accepts IPv4 and IPv6 on
every operating system. When the list also holds `:8080` or `0.0.0.0:8080` with the
same port, the IPv4 wildcard serves IPv4 and `[::]:8080` serves IPv6 only. Other IPv6
addresses are IPv6 only. Duplicate addresses are rejected.

`server.operations.listen` uses the same address syntax and accepts one address or a list of up to 16.
It defaults to `[]` (disabled). Its flag and environment variable take a
comma-separated list. Duplicate addresses and addresses equal to any `server.listen`
address are rejected. Dual-stack binding considers both lists together.

Only the operations listeners serve `/api/live`, `/api/health`, `/api/ready` and
`/api/metrics`, relative to `site.base_path`. Public listeners return 404 for those paths.
Operations listeners return 404 for all other paths and never evaluate policy or proxy
upstream.

These endpoints have no authentication. Bind them to loopback or a private monitoring
network. `server.metrics_enabled` still controls metrics availability. With the default empty list,
these routes are served nowhere.

`--port` (`RILGATE_PORT`) replaces the port of every `server.listen` address. It
preserves operations addresses. `--log-level` (`RUST_LOG`) sets the log filter and
`--log-format` (`RILGATE_LOG_FORMAT`) selects `text` or `json` output. Old automatic
`POW_*`, `WEB_*`, `SERVE_*`, `ENV`, `PORT`, and
`CONFIG` aliases are unsupported.

`rilgate upgrade` replaces the running binary with the latest GitHub release, or the tag
given by `--version`, after it checks the release `SHA256SUMS`. It keeps the current
binary when the release is not newer unless `--force` is set.

Run these from the directory that holds `rilgate.yml`, or add `--config FILE` for a file
elsewhere:

```bash
rilgate config check
rilgate config show
rilgate config show --defaults
```

`check` validates fields, cross-field constraints, policy expressions, and
algorithm selection. It does not open a listener or contact dependencies. `show`
produces resolved YAML with every signing key and pool password redacted.

By default it omits unchanged defaults. `--defaults` includes the whole schema. A
redacted result is for inspection and cannot be used as a secret-bearing configuration.
Validate external templates with `rilgate web validate DIRECTORY`.

## Environment checks

Validation also checks settings against the selected `environment`. An error stops
`config check` and startup. Commands print a warning on standard error, and `serve`
logs it.

Startup continues because the setting is valid in some deployments. Each finding ends
with a stable identifier. `rilgate config check --strict` fails when any warning is present. To enforce a
warning-free configuration, run it in CI or before deployment.

| Identifier | Environment | Level | Condition |
| --- | --- | --- | --- |
| `trusted-proxy-broad` | production | error | A public `server.trusted_proxies` network is broader than `/8` (IPv4) or `/16` (IPv6) |
| `debug-score-header` | production | error | `policy.score.debug_header.enabled` is `true` |
| `ip-set-plain-http` | production | error | A remote IP set `url` does not use `https` |
| `trusted-proxy-public` | production | warning | A `server.trusted_proxies` network contains public addresses, such as CDN ranges |
| `operations-listen-public` | production | warning | A `server.operations.listen` address is outside loopback and private networks, including `0.0.0.0` |
| `proof-rate-limit-disabled` | production | warning | `pow.rate_limit.enabled` is `false` |
| `resource-watch-enabled` | production | warning | `site.resources.watch.enabled` is `true` |
| `auth-ttl-long` | production | warning | `security.auth.ttl` exceeds `7d` |
| `development-public-listener` | development | warning | A `server.listen` address is beyond loopback, including the default `:8080` |
| `metrics-unreachable` | both | warning | `server.metrics_enabled` is `true` with no `server.operations.listen` address |

Loopback, private (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `fc00::/7`), shared
(`100.64.0.0/10`) and link-local networks count as non-public. Signing key strength is
checked in every environment.

## Challenge browser support

The embedded fast challenge targets IE11, Safari and iOS 9+, and Chrome 49 (including
Windows XP). It also targets Firefox 52 ESR, Android 4.4 WebView, Samsung Internet 4,
and KaiOS 2.5. Its classic ES5 startup bundle includes a small Promise fallback.

Requests use XMLHttpRequest when fetch or AbortController is unavailable, and SHA-256
work yields in short chunks on the main thread when workers are absent. JavaScript must
be enabled. This compatibility target is established by syntax checks and API fallback
tests. The Playwright suite exercises current Chromium, Firefox and WebKit, not those
historical browser releases.

RandomX requires a modern browser with ES modules, workers, BigInt and WebAssembly SIMD,
plus about 256 MiB of memory for its cache. An incapable browser displays the localized
failure and retry controls after consent. It never switches a RandomX challenge to the
fast algorithm on the client.

`rilgate web init` and automatic seeding retain readable source files. The server assembles
`static/js/challenge.bundle.js` from generated ES5 outputs when each source matches the embedded source
digest, including seeded, unedited files. An operator-edited startup member or fast
worker is served as authored, without the legacy browser guarantee.

Individual script routes remain available. A change to a source member updates the
assembled bundle on resource reload. A custom template must load the classic bundle to
use it. No JavaScript compiler runs in the server.

Before you build the server, build the generated outputs with `cargo xtask build-web`. This stages
the outputs as `cargo xtask build-wasm` stages the RandomX WASM. The [development guide](development.md)
describes the regeneration workflow.

## Defaults

Durations use units such as `300ms`, `5s`, `1m`, `6h`.
Timeouts retain subsecond precision. Token TTLs need positive whole seconds because
signed claims use Unix seconds. Durations are positive and at most one year. Readiness
is at most 30s, and resource-watch debounce is at most 60s. Sizes need a positive
integer with `B`, `KiB`, `MiB`, or `GiB`, without rounding
or overflow.

| Setting | Default |
| --- | --- |
| `environment` | `production` |
| `server.listen` | `:8080` (one address or a list) |
| `server.operations.listen` | `[]` (disabled) |
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
| `policy.ip_sets.<name>.refresh`, `.max_stale` | `24h` (1h to 7d), `7d` (refresh to 30d) |

When `pow.p2pool` is enabled, its defaults are:

- Address `127.0.0.1:3333`, password `x`, and agent `rilgate/<version>`.
- Connect timeout `5s` and submit timeout `20s`.
- Session capacity `64` and session TTL `15m`.
- Job-session mode `pooled`, maximum `16`, and idle `1m`.

Set mode `per_challenge` to create one connection per job. `max` and `idle`
configure the pooled mode. Zero never selects a job acquisition strategy.

Custom policies default to `default_action: allow`, no rules, no IP sets, score initial
`0`, debug header disabled with name `x-pow-weight`, and no thresholds. Custom
settings are rejected in preset modes. An IP set feed `url` must use `https`
in production. Development also accepts `http`. Debug headers are forbidden in
production.

## Separate keys and rotation

If independent key management or rolling rotation is required, use explicit keys. In
this mode, do not use a master secret. The two modes are mutually exclusive.

```yaml
security:
  keys:
    challenge: "${RILGATE_SECURITY_KEYS_CHALLENGE}"
    auth: "${RILGATE_SECURITY_KEYS_AUTH}"
    challenge_rotation:
      active_id: proof-next
      previous:
        proof-current: "${RILGATE_SECURITY_KEYS_CHALLENGE_PREVIOUS}"
    auth_rotation:
      active_id: auth-next
      previous:
        auth-current: "${RILGATE_SECURITY_KEYS_AUTH_PREVIOUS}"
```

Each key must have at least 32 bytes. Key IDs contain 1 to 32 ASCII letters, digits,
underscores or hyphens. Each ring permits at most eight verification keys.

The active ID cannot be in `previous`. Omit the active ID for unversioned signing. A
verification entry named `legacy` explicitly accepts unversioned signatures during a
named-key transition.

A change to a master secret invalidates its existing tokens. If an overlap window is
required, use explicit keys. Secrets never appear in configuration output or debug
representations. See [production rotation](production.md).

## Authentication cookie binding

By default the authentication cookie is a portable bearer credential: a cookie earned on
one machine works from any other until it expires. Set `security.auth.bind` to tie it to the
client that solved the challenge.

```yaml
security:
  auth:
    bind:
      client_ip: {ipv4_prefix: 24, ipv6_prefix: 64}
      user_agent: family
      headers: [accept-language]
      proxy_headers: [x-ja4]
```

- `client_ip` binds the network of the resolved client IP. If the section is present,
  even as `{}`, it enables the binding with the defaults shown. Prefixes are 0
  to 32 and 0 to 128. The resolved IP honors `server.trusted_proxies`, so that list must be correct.
- `user_agent` is `full` for the exact header or `family` for the header
  without version numbers. A browser update keeps a `family` cookie valid. A
  different browser does not.
- `headers` lists client request headers, read on every request.
- `proxy_headers` lists headers that an edge proxy sets, such as a JA4 TLS fingerprint.
  They are read only when the immediate TCP peer is inside `server.trusted_proxies`. From any other
  peer they count as absent, so a client cannot choose their value.

Each list holds at most 16 distinct, valid header names. A missing header differs from
an empty one. Settings are also available as flags and variables, such as `--security-auth-bind-client-ip-ipv4-prefix`.
If you set either prefix, IP binding is enabled.

A cookie that does not match its binding is treated as absent. The client receives the
challenge and never a 403. `rilgate_auth_binding_mismatches_total` counts the event.

Clients whose network or browser changes, such as mobile and VPN users, solve again. The
binding is a keyed digest derived from the auth signing key. A change to that key makes
every bound client solve again. Without any binding configured, behavior is unchanged.

## API response presentation

`site.responses.format` selects `auto`, `html` or `json`. Automatic mode
negotiates request headers. `site.responses.json_paths` can force JSON for API path prefixes. Operator
JSON templates under the external web resource directory can match an
application-specific envelope. See [AJAX and API integration](api-integration.md) for
notification headers, JSON body templates and iframe completion.
