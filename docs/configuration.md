# Configuration

The YAML contract has six groups: `environment`, `server`, `site`,
`security`, `pow`, and `policy`. Configuration has no schema version.
Unknown fields, unsupported values and conflicting settings are errors in every
environment. This unreleased schema replaces the previous configuration format. Old keys
are rejected, with no compatibility loader or migration command.

The minimal configuration requires an upstream URL. Production deployments also set a
signing secret:

```yaml
server:
  upstream:
    url: http://127.0.0.1:3000
security:
  secret: "${RILGATE_SECURITY_SECRET}"
```

1. Generate at least 256 bits of randomness with `openssl rand -hex 32`.
2. Supply `RILGATE_SECURITY_SECRET` through the process environment or deployment secret store.

Without `security.secret` and `security.keys`, the gateway generates a random 32-byte
secret at each start and reports the `generated-secret` warning. Access cookies and
proofs signed with it end at restart, and other instances cannot verify them. Use a
configured secret for production and for more than one instance. `config check --strict`
rejects a configuration without a secret.

Rilgate derives independent challenge and authentication keys using HKDF-SHA256 with
separate context identifiers. Secret length is checked. Inspection of a string cannot
establish randomness. Never commit a real secret.

The defaults are production mode, an IPv4 wildcard listener at `:8080`, embedded
site resources, local Fast verification, and the balanced policy. RandomX and pool access are
opt-in. The balanced preset challenges application traffic at Fast difficulty 2
regardless of User-Agent, with public exceptions described in [policies](policy.md).
HTTPS is required for production authentication cookies.

The [default configuration](../config/rilgate.yml) includes the setting reference
in comments. [Feature examples](../config/examples/) live under
`config/examples/`: [custom policy](../config/examples/custom.yml),
[HashWX](../config/examples/hashwx.yml), [RandomX](../config/examples/randomx.yml)
and [multiple files](../config/examples/includes.yml). The source catalogs in
`config/rules/`, `config/policies/` and `config/signals/` are embedded into the binary; see
[built-in policies and rules](policy.md#built-in-policies-and-rules).

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
followed by the flag in upper snake case. Indexed pool and proxy entries use variables without flags.
[`.env.example`](../.env.example) lists every variable with its default.

| YAML path | Flag | Variable |
| --- | --- | --- |
| `environment` | `--environment`, `-E` | `RILGATE_ENVIRONMENT` |
| `server.upstream.url` | `--server-upstream-url`, `-u` | `RILGATE_SERVER_UPSTREAM_URL` |
| `server.listen` | `--server-listen`, `-l` | `RILGATE_SERVER_LISTEN` |
| `server.operations.listen` | `--server-operations-listen` | `RILGATE_SERVER_OPERATIONS_LISTEN` |
| `server.operations.reload` | `--server-operations-reload` | `RILGATE_SERVER_OPERATIONS_RELOAD` |
| `server.watch_config` | `--server-watch-config` | `RILGATE_SERVER_WATCH_CONFIG` |
| `security.secret` | `--security-secret`, `-s` | `RILGATE_SECURITY_SECRET` |
| `server.max_connections` | `--server-max-connections` | `RILGATE_SERVER_MAX_CONNECTIONS` |
| `site.i18n.default` | `--site-i18n-default` | `RILGATE_SITE_I18N_DEFAULT` |
| `pow.stratum_pool.pools[0].address` | none | `RILGATE_POW_STRATUM_POOL_POOLS_0_ADDRESS` |

`rilgate --help` lists shared scalar settings with their YAML paths and defaults. Values are validated by
the schema exactly like `${VARIABLE_NAME}` references, so durations take `5s` and
sizes take `16MiB`. An invalid value names its flag or variable.

Boolean flags alone mean `true` and accept `--flag=false`. Lists are comma-separated
or repeated and replace the YAML list. An empty value clears it.

Any `pow.stratum_pool.*` value enables pool access with defaults for the rest. Policy rules, IP sets
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
Enable `server.operations.reload` to also serve `POST /reload`.
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
given by `--version`, after it checks the release `SHA256SUMS`. A static Linux binary
upgrades to the static build of the release. It keeps the current
binary when the release is not newer unless `--force` is set.

Run these from the directory that holds `rilgate.yml`, or add `--config FILE` for a file
elsewhere:

```bash
rilgate config check
rilgate config show
rilgate config show --defaults
rilgate reload
```

`check` validates fields, cross-field constraints, policy expressions, and
algorithm selection. It does not open a listener or contact dependencies. `show`
produces resolved YAML with every signing key and pool password redacted.

By default it omits unchanged defaults. `--defaults` includes the whole schema. A
redacted result is for inspection and cannot be used as a secret-bearing configuration.
Validate external templates with `rilgate web validate DIRECTORY`.

## Splitting configuration files

Use `!include` at any value position to load a local YAML file. See the
[multi-file example](../config/examples/includes.yml) for sections,
nested includes, CIDR lists and policy rules.

```yaml
# rilgate.yml
server: !include ./config/server.yml
security: !include ./config/security.yml
policy:
  default_action: challenge
  rules:
    - name: health
      when: {path: {exact: /health}}
      action: allow
    - !include ./config/bot-rules.yml
```

```yaml
# config/server.yml
upstream:
  url: http://127.0.0.1:3000
```

```yaml
# config/security.yml
secret: "${RILGATE_SECURITY_SECRET}"
```

```yaml
# config/bot-rules.yml
- name: badbot
  when: {user_agent: {contains: BadBot}}
  action: deny
- name: scraper
  when: {user_agent: {contains: ExampleScraper}}
  action: deny
```

Use `builtin:` to load resources shipped inside the binary:

```yaml
policy: !include builtin:policies/balanced.yml
```

Or insert a built-in rule in a custom policy:

```yaml
policy:
  rules:
    - !include builtin:rules/public-endpoints.yml
    - !include ./local-rules.yml
    - !include builtin:rules/challenge-all.yml
```

The built-in catalogs are `rules/`, `policies/` and `signals/`. Example configurations are
local files. A built-in reference always selects embedded content, with no
filesystem fallback or shadowing. Relative includes inside a built-in document
stay inside the catalogs; `../rules/...` resolves from a built-in policy to a
built-in rule. Built-in paths use forward slashes and cannot escape the catalog
root. Built-in references also work in the string-based configuration parser;
local includes require a source file context.

A file supplies the value at the include position, without repeating the parent
key. Mapping values can mix inline content and includes. Includes can also
supply scalars, an entire list, or the root document.

When a direct list item is `!include`, a list from that file inserts its items
into the containing list in declaration order. A map or scalar inserts one
item. Ordinary nested lists preserve their structure. Use `<<` to merge maps
into their parent. The resolved values must satisfy the existing schema.

Relative include paths resolve from the directory of the file containing the
tag, independent of the process working directory. Absolute paths are also
accepted. Include paths are literal strings; `${ENV}` references are supported
in configuration values inside the files. Other path settings retain their
existing path rules. YAML anchors and aliases stay within their own document.

Includes resolve before environment interpolation and environment/CLI overrides.
Precedence remains flags, variables, resolved YAML, then defaults. `config check`
and verifier commands use the same loader. `config show` emits resolved YAML,
keeps secret redaction and omits unchanged defaults unless `--defaults` is set.
Verifier output retains its existing subset of settings.

Each included file must contain one nonempty YAML document. Explicit `null` and
empty lists are values subject to schema validation. Missing files, invalid
YAML, unsupported tags, cycles and exceeded limits stop loading. Errors report
file and include context, with parser line/column when available. Schema errors
after parsing identify logical configuration paths; semantic errors involving
several fields can report several sources.

Loading is bounded to 1 MiB of input across all sources, 256 source reads, 16 active
sources in an include chain including the main document, 64 levels of traversal and
65,536 visited value nodes including include nodes. Repeated includes count
again toward the budgets. Embedded content counts toward these same budgets.
Parser recursion and alias-repetition limits also
apply. Files must contain UTF-8 YAML. URL loading and glob patterns are unsupported.

### Reloading the policy

Use one of these triggers to reload the signals and policy sections:

- On Unix, send `SIGHUP` to the gateway process.
- Enable `server.operations.reload` and run `rilgate reload`, or send a request to the reload route.
- Enable `server.watch_config` to watch the main file and all file-based `!include` sources.

Both settings default to `false`. Use `--server-operations-reload` or
`RILGATE_SERVER_OPERATIONS_RELOAD` for the route, and `--server-watch-config` or
`RILGATE_SERVER_WATCH_CONFIG` for the watcher. Changes to either setting require a restart.
The watcher ignores access events and reloads after 500 ms without a file change.
It tracks added and removed includes after a successful load. `builtin:` sources are embedded and have no file to watch.
With configuration from flags and environment only, Rilgate logs a warning and starts no configuration watcher.
An invalid file keeps the running policy. The next valid save can apply.

```yaml
server:
  operations:
    listen: 127.0.0.1:9090
    reload: true
  watch_config: true
```

```sh
rilgate --config rilgate.yml reload
docker-compose exec rilgate rilgate reload
```

The command uses the same file lookup, environment and flags as `serve`.
It uses the first operations address, replaces wildcard IP addresses with loopback,
and sends plain HTTP/1.1 with a 10 s timeout. It sends no credentials and follows no redirects.
The reload route needs an operations listener. Keep that listener private.
A connection failure or 404 reports that the gateway may be stopped or `server.operations.reload` may be off.

The route is `POST {site.base_path}/reload` on the operations listener only.
It is absent when `server.operations.reload` is `false` (404).
Any request with an `Origin` header receives 403. Other methods receive 405 with `Allow: POST`.
The request body is ignored. The server reads its own configuration with its own environment,
working directory and saved overrides. Reload requests run one at a time.

| HTTP status | JSON response |
| --- | --- |
| 200 | `{"status":"reloaded"}` or `{"status":"unchanged"}` |
| 409 | `{"status":"restart_required","sections":["server","site"]}` (changed section names) |
| 500 | `{"status":"failed","error":"<error chain>"}` |

Responses use `application/json`. The command prints `Policy reloaded` or `Policy is unchanged`
and exits 0. A restart reports `Configuration change needs a restart: server, site` and exits 1.
Other errors print `Reload failed: <error>` and exit 1. Rejected changes keep the running configuration.

Docker Desktop file events from a macOS or Windows host often do not reach the container.
Use `docker-compose exec rilgate rilgate reload` in that case.

The reload applies the `policy` section: rules, IP sets, GeoIP databases, DNS settings, the
honeypot and the `site.open_graph.when` condition that uses the new IP sets and
databases. Requests that are in progress finish with the policy they started with.

The reload applies nothing in these cases, and the running configuration stays:

- Another section differs from the running one. Rilgate logs the names of those
  sections with "Configuration change needs a restart".
- The new configuration fails to load or validate, or the new policy fails to
  compile. A policy that removes an IP set or database that the Open Graph
  condition uses fails in the same way. Rilgate logs the error.

A remote IP set or GeoIP database whose source is unchanged keeps its loaded data
across a reload, so verified crawlers stay known and MaxMind downloads do not repeat.
Its next fetch follows its last success and its `refresh` value. A new set or database,
or one with a changed source, loads from the start.

To split an existing configuration, move a section's value into a separate
file, replace the value with `!include ./path.yml`, and run:

```sh
rilgate --config rilgate.yml config check
rilgate --config rilgate.yml config show
```

### Merging and overriding

Use `<<` in any mapping, including the document root, to merge a map or a list
of maps. Bases apply in list order, with later bases taking precedence. Inline
sibling keys take precedence over every base, regardless of where `<<` appears.
Maps merge recursively; lists and scalars replace the previous value whole.
Explicit `null` also replaces the previous value, subject to schema validation.

```yaml
signals:
  ip_sets:
    <<: !include builtin:signals/crawler-ip-sets.yml
    internal: {cidrs: [10.0.0.0/8]}
policy:
  <<: !include builtin:policies/crawler-protection.yml
  default_action: challenge
```

For several bases, use `<<: [!include ./base-policy.yml, !include ./site-policy.yml]`.
Inline maps and YAML aliases within the same document also work as bases. A direct
include in the base list inserts its items if it supplies a list of maps. Nested
includes and merges resolve before their base is merged. Other base types fail
with source context. Keys keep their first position across bases, followed by new
inline keys. Errors retain the source and file-local path of the winning value.
`config show` contains the resolved values, with no `<<` or `!include`.

## Environment checks

Validation also checks settings against the selected `environment`. An error stops
`config check` and startup. Commands print a warning on standard error, and `serve`
logs it.

Startup continues because the setting is valid in some deployments. Each finding ends
with a stable identifier. `rilgate config check --strict` fails when any warning is present. To enforce a
warning-free configuration, run it in CI or before deployment.

| Identifier | Environment | Level | Condition |
| --- | --- | --- | --- |
| `verifier-remote-plain-http` | production | error | A selected remote endpoint uses HTTP |
| `verifier-remote-credentials` | production | error | Remote hashing lacks CA, client certificate or client key |
| `verifier-remote-missing` | both | error | Order selects remote without an endpoint |
| `verifier-local-fallback` | both | warning | Local follows remote; outages can build a 256 MiB cache on every gateway |
| `verifier-remote-unused` | both | warning | Order selects remote while no challenge selects RandomX or verification is stratum_pool_accepted |
| `stratum-pool-plain-tcp` | both | production error, development warning | A public pool has TLS disabled |
| `stratum-proxy-plain-credentials` | both | production error, development warning | A public proxy has credentials |
| `stratum-pool-per-challenge-public` | both | warning | A public pool uses per-challenge job sessions |
| `trusted-proxy-broad` | production | error | A public `server.trusted_proxies` network is broader than `/8` (IPv4) or `/16` (IPv6) |
| `debug-score-header` | production | error | `policy.score.debug_header.enabled` is `true` |
| `ip-set-plain-http` | production | error | A remote IP set `url` does not use `https` |
| `asn-set-plain-http` | production | error | A remote ASN set `url` does not use `https` |
| `trusted-proxy-public` | production | warning | A `server.trusted_proxies` network contains public addresses, such as CDN ranges |
| `operations-listen-public` | production | warning | A `server.operations.listen` address is outside loopback and private networks, including `0.0.0.0` |
| `proof-rate-limit-disabled` | production | warning | `pow.rate_limit.enabled` is `false` |
| `resource-watch-enabled` | production | warning | `site.resources.watch.enabled` is `true` |
| `auth-ttl-long` | production | warning | `security.auth.ttl` exceeds `7d` |
| `development-public-listener` | development | warning | A `server.listen` address is beyond loopback, including the default `:8080` |
| `metrics-unreachable` | both | warning | `server.metrics_enabled` is `true` with no `server.operations.listen` address |
| `generated-secret` | both | warning | Neither `security.secret` nor `security.keys` is set, so the gateway signs with a random secret for each start |

Loopback, private (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `fc00::/7`), shared
(`100.64.0.0/10`) and link-local networks count as non-public. Signing key strength is
checked in every environment.

For pool and proxy checks, single-label DNS names, such as `p2pool`, count as
non-public. Names under `.local`, `.internal`, `.lan`, `.home.arpa`, `.svc` and
`.cluster.local` also count as non-public. These checks ignore letter case and a
final dot in a DNS name.

## Challenge browser support

The embedded fast challenge targets IE11, Safari and iOS 9+, and Chrome 49 (including
Windows XP). It also targets Firefox 52 ESR, Android 4.4 WebView, Samsung Internet 4,
and KaiOS 2.5. Its classic ES5 startup bundle includes a small Promise fallback.

Requests use XMLHttpRequest when fetch or AbortController is unavailable, and SHA-256
work yields in short chunks on the main thread when workers are absent. JavaScript must
be enabled. This compatibility target is established by syntax checks and API fallback
tests. The Playwright suite exercises current Chromium, Firefox and WebKit, not those
historical browser releases.

RandomX requires a modern browser with ES modules, workers, BigInt and WebAssembly,
plus about 256 MiB per worker for its cache. See [RandomX throughput](pow.md#randomx-throughput)
for worker selection and the scalar module. An incapable browser displays the localized
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

## Policy decision logs

`server.decision_log` controls policy decision logs at info level. Set it to `off` to
suppress them, `deny` to log denied requests, `challenge` (the default) to log denied
and challenged requests, or `all` to include allowed requests. Requests allowed by a
valid authentication cookie are logged as `allow`. The equivalent overrides are
`RILGATE_SERVER_DECISION_LOG` and `--server-decision-log`.

Each decision includes `request_id`, `client_ip`, `method`, `host`, `path`, `action`,
`rule`, `weight`, `algorithm`, `difficulty`, and `unresolved`. These logs contain client
IP addresses and request paths. Algorithm and difficulty are optional policy values.

## Client IP forwarding

`server.trusted_proxies` lists the immediate proxy peers allowed to supply the client IP.
Set `server.client_ip_header` to an HTTP header name, such as `cf-connecting-ip`, to read
one IP address from that header. The edge must overwrite the header. Duplicate values,
address lists, and invalid IP addresses are rejected. IPv4-mapped IPv6 addresses resolve
to IPv4. Headers from other peers are ignored.

When the configured header is absent, Rilgate reads `X-Forwarded-For` from right to left
and stops at the nearest untrusted hop. The same behavior applies when
`server.client_ip_header` is unset. Use `RILGATE_SERVER_CLIENT_IP_HEADER` or
`--server-client-ip-header` to set the header through the environment or CLI.

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
| `server.operations.reload` | `false` |
| `server.watch_config` | `false` |
| `server.upstream.preserve_host` | `true` |
| `server.upstream.max_body_size` | `16MiB` |
| `server.upstream.client_ip_headers` | `[x-forwarded-for, x-real-ip, forwarded]`, replaced with the resolved client address; `[]` forwards them unchanged |
| `server.upstream.timeout.connect`, `.read` | `5s`, `30s` |
| `server.max_connections`, `.max_requests`, `.h2_max_streams` | `1024`, `512`, `64` |
| `server.trusted_proxies` | `[]`, trust no peers |
| `server.client_ip_header` | none, use `X-Forwarded-For` from trusted peers |
| `server.decision_log` | `challenge`, log denied and challenged requests at info level |
| `server.metrics_enabled`, `.readiness_timeout`, `.request_timeout` | `false`, `2s`, `30s` |
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
| `pow.algorithms.randomx.cache.idle_timeout` | `30m` (0s disables release, at most 7d) |
| `pow.algorithms.randomx.verify.order` | `[local]` (1-2 distinct entries: local, remote) |
| `pow.algorithms.randomx.verify.remote.endpoint` | none (required for remote; HTTPS origin, HTTP only in development) |
| `pow.algorithms.randomx.verify.remote.server_name` | endpoint host |
| `pow.algorithms.randomx.verify.remote.on_deadline_rejected` | `fallback` (`fallback` tries the next hasher; `reject` returns 401) |
| `pow.algorithms.randomx.verify.remote.timeout` | `2s` (100ms to 30s, also bounded by challenge expiry) |
| `pow.algorithms.randomx.verify.remote.tls.ca`, `.client_cert`, `.client_key` | none (all required in production; client cert and key must be paired; absent CA uses webpki roots in development) |
| `pow.algorithms.randomx.difficulty.base`, `.score_step`, `.max` | `1000`, `10`, `32000` |
| `pow.algorithms.randomx.browser.module` | `/static/js/solvers/randomx/solver.js` |
| `signals.ip_sets.<name>.refresh`, `.max_stale` | `24h` (1h to 7d), `7d` (refresh to 30d) |
| `signals.asn_sets.<name>.refresh`, `.max_stale` | `24h` (1h to 7d), `7d` (refresh to 30d) |

When `pow.stratum_pool` is enabled, its defaults are:

- One pool named `default`, at `127.0.0.1:3333`, with login `x+{difficulty}` and password `x`.
- No proxies, TLS disabled, failover timeout `15s`, failover cooldown `30s`, and agent `rilgate/<version>`.
- Connect timeout `5s` and submit timeout `20s`.
- Session capacity `64` and session TTL `15m`.
- Job-session mode `pooled`, maximum `16`, and idle `1m`.

Set mode `per_challenge` to create one connection per job. `max` and `idle`
configure the pooled mode. Zero never selects a job acquisition strategy.

An explicit policy defaults to `default_action: allow`, no rules, score initial
`0`, debug header disabled with name `x-pow-weight`, and no thresholds. Custom
settings can be supplied directly or merged with preset files. Signals default to no IP sets. An IP set feed `url` must use `https`
in production. Development also accepts `http`. Debug headers are forbidden in
production.

## Stratum pools and proxies

The old `pow.p2pool` section, `p2pool_accepted` configuration value and
`RILGATE_POW_P2POOL_*` variables fail at startup with the new names. Old flags
are unknown arguments. Existing tokens accept `p2pool_accepted` for one release.
New tokens contain only `stratum_pool_accepted`.

Set `pow.stratum_pool.pools` to a list of 1 to 8 pools. The gateway tries them
in order. Each entry requires a unique `name` and an `address`. Names use 1 to
64 lowercase ASCII letters, digits, hyphens or underscores. An address is
`HOST:PORT` or `[IPv6]:PORT`. It has no scheme, credentials or path.

`login.user` defaults to `x+{difficulty}`. It must contain `{difficulty}` exactly
once, with no other placeholders. The substituted value must fit in 256 bytes.
Environment references resolve first. `login.password` defaults to `x`.
Both login fields are secrets in displayed configuration.

| Pool field | Default | Operation |
| --- | --- | --- |
| `tls.enabled` | `false` | Verify TLS from the gateway to the pool |
| `tls.server_name` | address host | Verify this DNS name or IP address |
| `tls.ca` | none | Trust a PEM CA file; otherwise use webpki roots |
| `proxies` | shared list | Replace `pow.stratum_pool.proxies`; `[]` connects directly |

Each proxy list has 0 to 8 entries. Each proxy requires a unique `name` and a
`url`. Proxy names follow the pool name rules. URLs use `socks5://HOST:PORT`,
`socks5h://HOST:PORT` or `http://HOST:PORT`, with no credentials, path, query or
fragment. Set `username` and `password` together, or omit both. Displayed
configuration masks URLs and credentials.

`socks5` resolves the pool host on the gateway. `socks5h` resolves it on the
proxy. `http` uses CONNECT. TLS verifies the pool certificate through the proxy.
Certificate verification is always on. HTTP response headers are limited to 8 KiB.

`failover.cooldown` defaults to `30s` and accepts `1s` to `10m`. A TCP connection,
proxy handshake or TLS handshake failure puts the proxy in cooldown. The gateway
then tries the next proxy. A login failure puts the pool in cooldown and selects
the next pool. If all entries of a list are in cooldown, the gateway tries them
in order. `timeout.connect` limits the transport phase and the login round trip
separately. The transport phase includes TCP, proxy and TLS handshakes.

`failover.timeout` limits the total connection time across all pools and proxies,
including login. It defaults to `15s` and accepts `1s` to `5m`. It must be at least
`timeout.connect`. Each phase uses the smaller of its timeout and the time left.
When the budget expires, the gateway stops and reports that all Stratum pools
are unavailable. A try that this budget stops does not start a pool or
proxy cooldown. A failure before the budget expires still starts the applicable
cooldown. Readiness probes keep the separate `server.readiness_timeout` bound.
Use `--pow-stratum-pool-failover-timeout` or
`RILGATE_POW_STRATUM_POOL_FAILOVER_TIMEOUT` to set the budget.

The provider loads the TLS CA file once at startup. A missing or invalid CA file
stops startup with an error that names the pool.

### Indexed environment variables

Pool and proxy entries have no CLI flags. Use indexed variables or YAML:

```bash
RILGATE_POW_STRATUM_POOL_POOLS_0_NAME=local
RILGATE_POW_STRATUM_POOL_POOLS_0_ADDRESS=127.0.0.1:3333
RILGATE_POW_STRATUM_POOL_POOLS_0_LOGIN_USER=x+{difficulty}
RILGATE_POW_STRATUM_POOL_POOLS_0_PROXIES=
RILGATE_POW_STRATUM_POOL_POOLS_1_NAME=backup
RILGATE_POW_STRATUM_POOL_POOLS_1_ADDRESS=pool.example:443
RILGATE_POW_STRATUM_POOL_POOLS_1_TLS_ENABLED=true
RILGATE_POW_STRATUM_POOL_PROXIES_0_NAME=tunnel
RILGATE_POW_STRATUM_POOL_PROXIES_0_URL=socks5h://127.0.0.1:1080
```

| Variable prefix | Fields |
| --- | --- |
| `RILGATE_POW_STRATUM_POOL_POOLS_<N>_` | `NAME`, `ADDRESS`, `LOGIN_USER`, `LOGIN_PASSWORD`, `TLS_ENABLED`, `TLS_SERVER_NAME`, `TLS_CA`, `PROXIES` |
| `RILGATE_POW_STRATUM_POOL_PROXIES_<N>_` | `NAME`, `URL`, `USERNAME`, `PASSWORD` |
| `RILGATE_POW_STRATUM_POOL_POOLS_<N>_PROXIES_<M>_` | `NAME`, `URL`, `USERNAME`, `PASSWORD` |

Indexes start at 0 and have no gaps. A variable changes one field of the YAML
item with that index. The next index adds one item at the end. An unknown field
fails with the valid field list. Invalid values name their source variable.
An empty `POOLS_<N>_PROXIES` value sets an empty proxy list for that pool.
Shared settings keep their flags, including `--pow-stratum-pool-failover-cooldown`.

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
- `client_ip.max_networks` accepts integers from 1 to 8, with default 1 preserving the
  legacy cookie format. Set it to 4 to remember up to four proven networks. Each new
  network still needs a solved challenge. Entries expire independently, and a merged
  cookie uses the lower difficulty. A different bound User-Agent family or algorithm
  starts a new cookie. With several gateway instances, deploy the same version
  everywhere before raising this value: older instances reject the network form and
  challenge again. `rilgate_auth_network_merges_total` counts successful merges.
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

`site.responses.challenge_status` sets the HTML challenge status (default 200).
`site.responses.deny_status` sets the HTML and JSON denial status (default 403).
JSON challenges always return 403 with `Rilgate-Challenge: required`.
See [response status codes](web.md#response-status-codes) for allowed values.

## Dedicated RandomX verifier

Run the hash service with a separate command:

```sh
rilgate verifier config check --config /etc/rilgate/verifier.yml --strict
rilgate verifier config show --config /etc/rilgate/verifier.yml --defaults
rilgate verifier serve --config /etc/rilgate/verifier.yml
```

This command resolves only `environment`, `pow.algorithms.randomx.cache`,
`pow.algorithms.randomx.serve`, `server.operations` and `server.metrics_enabled`.
It uses the same YAML loader, environment variables, flags and logging options
as the gateway. It needs no upstream URL, signing secret, policy or pool.
Configuration checks load TLS files but do not open a listener.

```yaml
server:
  operations:
    listen: 127.0.0.1:9091
  metrics_enabled: true
pow:
  algorithms:
    randomx:
      cache:
        idle_timeout: 30m
      serve:
        listen: 10.0.0.5:9443
        workers: 1
        queue: 64
        max_connections: 256
        tls:
          cert: /etc/rilgate/verifier.pem
          key: /etc/rilgate/verifier.key
          client_ca: /etc/rilgate/gateway-ca.pem
```

| Setting under `pow.algorithms.randomx.serve` | Default | Requirement |
| --- | --- | --- |
| `listen` | none | One socket address. Must differ from gateway and operations addresses. |
| `workers` | `1` | Must be `1`. Several workers are reserved for phase 2. |
| `queue` | `64` | `1` to `4096` admitted hash requests. |
| `max_connections` | `256` | `1` to `65536` hash connections. |
| `tls.cert`, `tls.key` | none | Both files are required in every environment. |
| `tls.client_ca` | none | Required in production. Authenticates gateway certificates. |

Each client certificate can hold half the admission limit, rounded down, with a
minimum of one request. Gateways that share one certificate share this half, so
give each gateway its own certificate. Requests without a client certificate use
only the total limit. Admission applies before the service reads a body.
Excess hash requests receive `503` with `Retry-After: 1`. Excess connections are
closed. The service accepts JSON bodies of at most 4 KiB.

The verifier listener serves only `POST /v1/randomx/hash` and `GET /v1/ready`.
The operations listener serves `GET /api/live`, `GET /api/ready` and, when metrics
are enabled, `GET /api/metrics`. These routes do not check an upstream or pool.
A cold worker is ready. A worker failure makes it not ready for one second.
The next hash can retry initialization. During shutdown, it stays not ready.
The verifier stops new hash connections and allows up to 30 seconds for active
requests to finish. Each operations listener has a separate limit of 16 connections.

Production rejects a missing client CA with
`verifier-serve-unauthenticated`. A public verifier address produces the
`verifier-serve-public` warning. Public operations addresses also produce a
warning. A gateway configuration with `serve` is rejected. Run a dedicated
verifier process; combined mode is reserved for phase 2.

## HashWX configuration

Use a custom policy to select HashWX. Presets select Fast explicitly. HashWX
is registered and its cost settings are validated only when a challenge rule
selects it with local verification.

```yaml
pow:
  verification: local_verify
  default_algorithm: hashwx
  algorithms:
    hashwx:
      nonces_per_seed: 65536
      difficulty:
        base: 50000
        score_step: 10
        max: 5000000
policy:
  default_action: challenge
```

| YAML path under `pow.algorithms.hashwx` | Default | Valid values |
| --- | ---: | --- |
| `nonces_per_seed` | 65536 | 463 to 1048576 |
| `difficulty.base` | 50000 | 1 to 2^40 |
| `difficulty.score_step` | 10 | Positive i32 |
| `difficulty.max` | 5000000 | `base` to 2^40 |

The base and max defaults are placeholders until a mid-range phone is measured.
The iPhone 15 Pro Max result does not supply that calibration. An explicit
rule difficulty must be between 1 and 2^40 and overrides the score mapping,
including its `max` cap. `pow.challenge.ttl` controls the work lifetime.

Each setting has the corresponding CLI flag and environment variable:

| Flag | Environment variable |
| --- | --- |
| `--pow-algorithms-hashwx-nonces-per-seed` | `RILGATE_POW_ALGORITHMS_HASHWX_NONCES_PER_SEED` |
| `--pow-algorithms-hashwx-difficulty-base` | `RILGATE_POW_ALGORITHMS_HASHWX_DIFFICULTY_BASE` |
| `--pow-algorithms-hashwx-difficulty-score-step` | `RILGATE_POW_ALGORITHMS_HASHWX_DIFFICULTY_SCORE_STEP` |
| `--pow-algorithms-hashwx-difficulty-max` | `RILGATE_POW_ALGORITHMS_HASHWX_DIFFICULTY_MAX` |

`--pow-default-algorithm hashwx` and `RILGATE_POW_DEFAULT_ALGORITHM=hashwx`
select it for challenge actions that omit an algorithm. HashWX work is local.
It needs no `pow.stratum_pool` entry or dedicated verifier queue.

## Open Graph passthrough

Enable `site.open_graph` to expose anonymous upstream preview metadata in HTML
challenges for protected GET requests:

```yaml
site:
  open_graph:
    enabled: false
    ttl: 24h
    negative_ttl: 5m
    timeout: 2s
    max_entries: 10000
    include_query: false
    request_headers:
      user-agent: Rilgate-OpenGraph/1.0
      x-forwarded-host: "{host}"
      x-forwarded-proto: https
    tags:
      prefixes: ["og:", "twitter:", "fediverse:"]
      names: [description, keywords, author]
    override: {}
```

`ttl` accepts `1m` through `7d` and also applies to successful pages with no tags.
`negative_ttl` accepts `10s` through `ttl` for failed lookups. `timeout` accepts
`100ms` through `10s` and limits how long the challenge response waits; the fetch
continues and fills the cache after that deadline. `max_entries` accepts 1 through
1,000,000. The cache is in memory per process. Its key contains the request host
and upstream path. `include_query: true` also includes the query in the key and
prefetch URL; the default shares one lookup across query variants.

`request_headers` replaces the default map, including when it is empty. Client
headers are never copied. Names are case-insensitive and stored lowercase; at
most 16 headers are accepted. Values support only `{host}`, expanded to the same
request host used for challenge binding. Unknown placeholders and invalid HTTP
header values are errors. Values have a 1024-byte limit; an expansion that exceeds
it or produces an invalid value skips that header. The names `host`,
`content-length`, `transfer-encoding`, `connection`, `upgrade`, `te`, `trailer`,
`keep-alive` and `proxy-connection` are forbidden. Host follows
`server.upstream.preserve_host`. Prefetches request HTML through the Accept header.

A nonempty `override` map renders configured tags without fetching. It accepts
at most 32 entries with names of at most 64 bytes and content of at most 1024
bytes. Each name must match `tags`, compared in ASCII lowercase.
`enabled: false` disables both fetching and preview rendering.

`tags.prefixes` matches the beginning of a tag name; `tags.names` matches a
whole name. Each configured list replaces its default independently; omitting a
list keeps its default. Entries are stored lowercase and must contain 1 to 64
bytes of `a-z`, `0-9`, `:`, `_`, `.` or `-`. Duplicate entries are errors. The
limits are 16 prefixes and 32 names; both lists cannot be empty.

`when` optionally selects requests with the shared [policy condition grammar](policy.md#conditions),
including `host` and `signals.ip_sets`. Without `when`, all eligible requests
look up tags. The selection also applies to overrides. A condition that evaluates
to unknown, such as an unloaded IP set, skips the lookup. Unselected requests do
not increment lookup metrics. `score` is rejected anywhere in `when` with its
configuration path because score is only available in policy rules.

For example, select two hosts and exclude an administrative path:

```yaml
site:
  open_graph:
    enabled: true
    when:
      all:
        - host: {in: [example.com, www.example.com]}
        - not: {path: {prefix: /admin/}}
```

See [gateway behavior](gateway.md#open-graph-passthrough) for fetch limits and
[web resources](web.md) for custom layout integration.
