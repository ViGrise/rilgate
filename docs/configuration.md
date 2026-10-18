# Configuration

Rilgate reads `config/rilgate.yml` by default. Select an operator-owned
file with `--config /path/to/config.yml` or the `CONFIG` environment variable.
CLI values take precedence over environment variables, then YAML and defaults.
The application loads `.env` when present; [.env.sample](../.env.sample) lists
common scalar overrides. Use [the full example](../config/rilgate.example.yml)
for available fields.

The repository includes `config/rilgate.example.yml` as a reference, but no
`config/rilgate.yml`. Copy the example to `config/rilgate.yml` before running
the quick start. When
the default file is absent in development, environment variables and built-in
defaults are used. Those defaults contain no bot rules and allow requests at
weight zero. Production requires a configuration file with explicit policy.
An explicitly selected configuration file must exist and be readable.

Scalar environment names replace dots with underscores and use uppercase,
such as `SERVE_PORT`, `UPSTREAM_MAX_BODY_BYTES`, and `POW_P2POOL_MAX_SESSIONS`.
`PORT` is an alias for `SERVE_PORT`; `APP_ENV` and `ENV` select the environment.
`pow.bots` and `pow.thresholds` are YAML lists rather than scalar environment
overrides. Unknown fields in policy rules and the `web` section are rejected;
unrecognized scalar and sequence keys elsewhere produce a warning in development
and are rejected in production.

See [production operations](production.md) for the production template, limits,
trusted proxies, key rotation and monitoring. Production requires explicit,
nonempty `pow.thresholds` in a configuration file and rejects unknown keys.

Production mode requires signing keys of at least 32 bytes and rejects placeholder
values. Store secrets outside committed configuration. Use `ENV=production` or
`--env production` for a deployment.

## Shared configuration when scaling

Every gateway instance should have the same:

```text
compatible proof and authentication verification keyrings
web.base_path
pow.enforce
pow.auth_cookie_name
compatible token lifetimes
difficulty policy
```

There is no shared runtime database requirement.
Local proofs and receipts can cross instances with compatible settings. An
active P2Pool-accepted challenge retains its Stratum connection on the instance
handling its WebSocket. Rate limits and pool/verifier capacity remain local to
each instance.

## Configuration example

```yaml
env: development

web:
  name: "Rilgate"
  website_url: "https://rilgate.com"
  base_path: "/.well-known/rilgate"
  stylesheet_path: "css/rilgate.css"
  # root: /etc/rilgate/web
  seed:
    empty_directory: true
  watch:
    enabled: false
    debounce: 300ms
  templates:
    cache: true
    strict: true
    recursion_limit: 64
    fuel: 50000
  i18n:
    default_locale: en
    strict: false
    language_cookie_name: "rilgate_lang"

serve:
  port: 8080

upstream:
  url: "http://127.0.0.1:3000"
  preserve_host: true
  max_body_bytes: 16777216
  connect_timeout_seconds: 5
  read_timeout_seconds: 30

pow:
  enforce: local_verify
  default_algorithm: randomx

  signing_key: "replace-me"
  auth_signing_key: "replace-me-too"

  challenge_ttl_seconds: 60
  randomx_challenge_ttl_seconds: 3600
  receipt_ttl_seconds: 30
  auth_ttl_seconds: 21600
  auth_cookie_name: "rilgate_auth"
  nonce_offset: 39
  verifier_queue_capacity: 8

  rate_limit:
    enabled: true
    client_burst: 10
    client_refill_per_second: 0.1
    global_burst: 64
    global_refill_per_second: 16
    max_clients: 4096

  p2pool:
    host: "127.0.0.1"
    port: 3333
    password: "x"
    agent: "rilgate/0.1.0"
    connect_timeout_seconds: 5
    submit_timeout_seconds: 20
    max_sessions: 64
    session_ttl_seconds: 900
    max_job_sessions: 16
    job_session_idle_seconds: 60

  difficulty:
    base: 1000
    weight_step: 10
    max: 32000

  risk:
    default_weight: 0
    allow_debug_header: false
    debug_header: "x-pow-weight"

  bots:
    - name: generic-browser
      user_agent_regex: Mozilla|Opera
      action: WEIGH
      weight:
        adjust: 10

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

  browser:
    solver_module_path: "/static/js/solvers/randomx/solver.js"
```
