# Configuration

Rilgate reads `config/rilgate.yml` by default. Select an operator-owned
file with `--config /path/to/config.yml` or the `CONFIG` environment variable.
CLI values take precedence over environment variables, then YAML and defaults.
The application loads `.env` when present; [.env.sample](../.env.sample) lists
common scalar overrides. Use [the full example](../config/rilgate.example.yml)
for available fields.

Production mode requires signing keys of at least 32 bytes and rejects placeholder
values. Store secrets outside committed configuration. Use `ENV=production` or
`--env production` for a deployment.

## Shared configuration when scaling

Every gateway instance should have the same:

```text
POW_SIGNING_KEY
POW_AUTH_SIGNING_KEY
web.base_path
difficulty policy
```

There is no shared runtime database requirement.

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
    supported_locales: [en]
    strict: false
    language_cookie_name: "rilgate_lang"

serve:
  port: 8080

upstream:
  url: "http://127.0.0.1:3000"
  preserve_host: true
  max_body_bytes: 16777216

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

  p2pool:
    host: "127.0.0.1"
    port: 3333
    password: "x"
    agent: "rilgate/0.1.0"
    connect_timeout_seconds: 5
    submit_timeout_seconds: 20

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
