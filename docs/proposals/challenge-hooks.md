# Challenge steps and challenge hooks

This proposal adds two features to the challenge flow:

1. **Challenge steps.** A policy rule can require an ordered list of steps, for
   example a Fast proof and then a CAPTCHA. The access cookie records each step
   that the client completed as a grant. The gate presents only the first step
   that has no grant.
2. **Challenge hooks.** An operator connects an external check (a CAPTCHA
   provider, a single sign-on service, an internal risk service) through
   configuration only. Rilgate has no provider-specific code. A hook is data
   that uses a small set of generic primitives: templates, a presentation mode
   and a declarative HTTP verification request.

The proof-of-work algorithms, Stratum pool acceptance and the refresh wait do not
change.

## 1. Current implementation

| Item | Current behavior |
| --- | --- |
| Policy challenge | `ChallengePolicy { algorithm, difficulty }` (`rilgate-policy/src/policy/config.rs`). A rule or a score threshold selects one algorithm |
| Decision | `PolicyDecision { action, weight, algorithm, difficulty, rule, unresolved, needs_lookups }`. `ChallengeService::evaluate_context` fills a missing algorithm from `pow.default_algorithm` and a missing difficulty from the weight |
| Presentation token | `PresentationClaims` v1 has one `algorithm`, `difficulty` and `verification` |
| Access cookie | `AuthClaims` v1 has one `algorithm`, `difficulty` and `verification` |
| Token versions | `validate_lifetime` accepts only `v == 1` for every token type |
| Gate check | `proxy/gate.rs` accepts the cookie when its algorithm equals the required one and its difficulty is high enough. Any proof of work satisfies a refresh wait. A RandomX fallback cookie is accepted while the fallback is active |
| Receipt exchange | `exchange_receipt_with_claims` signs a new cookie from one receipt. With `max_networks > 1`, it merges the network list of a previous cookie with the same algorithm and binding |
| Browser flow | `challenge.js`: client-config, `PowClient.algorithm(id).run(config)`, `/api/session`, then a reload or `postMessage` in embedded mode |
| CSP | `rilgate-web/src/csp.rs` starts from `default-src 'none'`. It has no `frame-src` directive, so every frame is blocked |
| Built-in catalogs | `builtin:policies/`, `builtin:rules/` and `builtin:signals/` are embedded from `config/` by `rilgate-config/src/loader/builtin.rs` |

## 2. Challenge steps

### 2.1 Policy

`ChallengePolicy` accepts exactly one of three forms:

```yaml
challenge: {algorithm: fast, difficulty: 2}   # one proof-of-work step
challenge: {hook: captcha}                     # one hook step
challenge:
  steps:                                       # ordered steps
    - {algorithm: fast, difficulty: 2}
    - {hook: captcha}
```

- A step is `{algorithm, difficulty}` or `{hook}`. A step without `difficulty`
  gets the weight-based difficulty, as a single-algorithm rule does today.
- `steps` has 1 to 4 entries. Each method appears at most once: `pow:<algorithm>`
  or `hook:<name>`.
- `refresh` is valid only as a one-step challenge, because it is a wait page
  and has no grant that other steps can build on.
- Every `hook` name must be defined in `challenge_hooks`. A policy reload
  validates hook names against the running hook set.
- Score thresholds accept the same three forms.

The order of steps controls cost: a cheap proof runs before a check that costs
money or quota. The order is not a security control. Every step is required, and
the gate is the only place that decides.

### 2.2 Decision

`PolicyDecision` replaces `algorithm` and `difficulty` with
`steps: Arc<[ChallengeStep]>`. `evaluate_context` fills the algorithm and
difficulty of each proof-of-work step as it does today for the single algorithm.
The decision log keeps its `algorithm` and `difficulty` fields for the presented
step and adds `step` (1-based) and `steps` (for example `fast:2,hook:captcha`).

### 2.3 Grants

An access cookie holds a list of grants. Each grant has its own expiry.

```text
AuthClaims v2 {
  v: 2, host, issued_at, expires_at, binding?, networks?,
  grants: [
    {"pow":  {"algorithm": "fast", "difficulty": 2, "enforce": "local_verify",
              "fallback_for": null, "expires_at": 1767225600}},
    {"hook": {"name": "captcha", "expires_at": 1767189600}}
  ]
}
```

| Required step | Grant that satisfies it |
| --- | --- |
| `pow:<alg>`, difficulty `d` | A proof-of-work grant with the same algorithm, the configured verification mode and difficulty `>= d` |
| `refresh` | Any proof-of-work grant, or a refresh grant with enough difficulty |
| `hook:<name>` | A hook grant with the same name |
| `hook:<name>` while its fallback is presented | A proof-of-work grant that satisfies the fallback step |
| `hook:<name>` with `unavailable.grant: until_expiry` | Also a proof-of-work grant with `fallback_for: <name>` |
| RandomX step while `pow.algorithms.randomx.fallback` is active | A grant that satisfies the RandomX fallback step, as today |

When every step has a grant, the gate allows the request. Otherwise it presents
the first step that has no grant.

- A grant is valid while `now < expires_at`. A proof-of-work grant expires after
  `security.auth.ttl`. A hook grant expires after its `grant_ttl`.
- The token `expires_at` is the latest grant expiry. The token is invalid when
  no valid grant remains.
- A grant for a hook that is no longer configured, or for an algorithm that is no
  longer registered, is ignored.

### 2.4 Writing the cookie

Receipt exchange, `/api/verify` with `set_cookie: true` and hook verification
write the cookie as a merge:

1. Read the current access cookie. Keep its grants only when its signature,
   lifetime, host and binding are valid for this request.
2. Drop expired grants. Replace a grant with the same method, or add the new one.
3. Sign the result.

A cookie whose only grant is one proof-of-work grant without `fallback_for` is
signed as v1, with the current fields and the current network merge. Any other
grant set is signed as v2. A deployment that does not use steps or hooks keeps
v1 cookies.

### 2.5 Presentation and the next step

`PresentationClaims` v2 carries the whole step list and the presented step:

```text
PresentationClaims v2 {
  v: 2, host, path, rule, weight, challenge_id,
  steps: [...], step: 0, issued_at, expires_at
}
```

When the gate presents the fallback of a hook (3.5), the presented step is the
fallback proof-of-work step with `fallback_for: <hook name>`. A grant from that
step records `fallback_for`.

A one-step proof-of-work challenge without `fallback_for` is signed as v1.
`/api/challenge` accepts both versions and issues work for the presented
proof-of-work step.

`/api/verify`, `/api/exchange` and the hook verification endpoint accept an
optional `presentation_token`. When it is valid for the host, the response
contains `next` if a step still has no grant after the merge:

```json
{
  "receipt": "...",
  "cookie_set": true,
  "next": {
    "presentation_token": "...",
    "step": 2,
    "steps": 2,
    "method": "hook",
    "hook": { "name": "captcha", "mode": "embed", "...": "..." }
  }
}
```

The server does not run the policy again for `next`. The step list is in the
signed presentation token. `next` is a convenience: a client that ignores it gets
the missing step from the gate on the next request.

## 3. Challenge hooks

### 3.1 Configuration

`challenge_hooks` is a top-level group after `pow`. It maps a hook name to its
definition. A change needs a restart, as `pow` does.

```yaml
challenge_hooks:
  captcha:
    <<: !include builtin:hooks/turnstile.yml
    params: {site_key: 0x4AAAAAAA...}
    secrets: {secret: ${RILGATE_TURNSTILE_SECRET}}
    grant_ttl: 1h
    unavailable:
      action: fallback
      fallback: {algorithm: fast, difficulty: 4}
      grant: while_unavailable

  sso-check:
    presentation:
      mode: redirect
      url: "https://sso.example.com/rilgate/start?state={state}&return={callback}"
      result_param: code
    verify:
      request:
        url: https://sso.internal/rilgate/introspect
        encoding: json
        headers: {authorization: "Bearer {secrets.token}"}
        fields: {code: "{result}", ip: "{client_ip}"}
      accept:
        - {field: /active, equals: true}
    secrets: {token: ${RILGATE_SSO_TOKEN}}
```

| Setting | Default | Range and rules |
| --- | --- | --- |
| hook name | | `[a-z0-9][a-z0-9-]{0,31}`, at most 16 hooks |
| `params` | `{}` | Map of string, number or boolean. Public: browser templates can use it |
| `secrets` | `{}` | Map of secret strings. Redacted by `config show`. Only `verify.request` can use it |
| `grant_ttl` | `security.auth.ttl` | `1m` to `security.auth.ttl` |
| `presentation` | | Required. See 3.3 |
| `verify` | | Required. See 3.4 |
| `unavailable` | `action: deny` | See 3.5 |

`builtin:hooks/` is a fourth built-in catalog in `config/hooks/`. Each preset uses
only the generic settings in this section. Every preset needs `params.site_key`
and `secrets.secret_key`.

| Preset | Widget | Verification checks |
| --- | --- | --- |
| `turnstile.yml` | `turnstile.render`, `cData` set to `{challenge_id}` | `success`, `hostname` equals `{host}`, `cdata` equals `{challenge_id}`. Sends `idempotency_key: {verification_id}` and retries attempt timeouts |
| `hcaptcha.yml` | `hcaptcha.render` with `recaptchacompat=off` | `success`. Sends `sitekey`. hCaptcha documents that `hostname` is not for authentication |
| `recaptcha-v2.yml` | `grecaptcha.render` | `success`, `hostname` equals `{host}` |
| `recaptcha-v3.yml` | `grecaptcha.execute`, `returns: promise` | `success`, `hostname` equals `{host}`, `action` equals `{params.action}`, `score` at least `{params.min_score}`. Defaults `action: rilgate`, `min_score: 0.5` |

The provider test keys return fixed `hostname` and `cdata` values, so the
`hostname` and `cdata` checks reject them on other hosts.

### 3.2 Templates

A template is a string with placeholders in single braces, the same syntax as
`site.open_graph.request_headers`. `{{` writes `{` and `}}` writes `}`.

| Placeholder | Browser templates | `verify.request` |
| --- | --- | --- |
| `{challenge_id}`, `{host}`, `{locale}` | yes | yes |
| `{params.<key>}` | yes | yes |
| `{state}`, `{callback}` | redirect `url` only | no |
| `{client_ip}`, `{user_agent}` | no | yes |
| `{verification_id}` | no | yes |
| `{result}` | no | yes |
| `{secrets.<key>}` | no | yes |

- A placeholder that is not allowed in its context, or an unknown `params` or
  `secrets` key, is a configuration error.
- A template that is exactly one placeholder keeps the type of its value, so
  `"{params.min_score}"` stays a number. Other templates produce strings.
- The value is escaped for its context: percent-encoding in URLs, form encoding
  in `form` bodies, JSON strings in `json` bodies and widget arguments. A header
  value that is not a valid HTTP header value makes the verification fail.
- `{client_ip}` is an empty string when the client address is unknown.
- `{verification_id}` is a UUID that one verification keeps for all its attempts.
  Use it as an idempotency key.
- A `form` or `json` field whose value is an empty string is not sent.
- `{callback}` is `https://<host><base_path>/api/hook/callback`, with `http` in
  the development environment.
- `{state}` is the presentation token of the hook step.

### 3.3 Presentation

`presentation.mode` selects how the browser produces a `result`.

**`redirect`**

| Setting | Rules |
| --- | --- |
| `url` | Absolute `https` URL template (`http` in development) |
| `result_param` | Query parameter that carries the result back. Default `result` |

The external service sends the browser to `{callback}` with `state` and
`result_param` in the query. A first redirect step answers GET and HEAD HTML
requests with `302` and `Cache-Control: no-store`. Other requests get the current
challenge response, with the hook URL in the challenge URL header. A redirect
step after another step uses `next.hook.url`, and the page navigates to it.

**`embed`**

The challenge page loads a third-party script and gets the result in the page.
Exactly one of `widget` or `script` is set.

`widget` describes any widget that has a render or execute function:

```yaml
presentation:
  mode: embed
  widget:
    script: https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit
    integrity: null
    call: turnstile.render
    args:
      - $container
      - sitekey: "{params.site_key}"
        cData: "{challenge_id}"
        callback: $result
        error-callback: $error
        expired-callback: $expired
    returns: callback
  csp:
    script-src: [https://challenges.cloudflare.com]
    frame-src: [https://challenges.cloudflare.com]
```

- `onload` is the name of the script query parameter that takes a load callback.
  When it is set, the page adds a generated global function name to the script
  URL and calls `call` only after that function runs. When it is not set, the
  page waits until `call` resolves to a function.
- `call` is a dotted path of 1 to 4 JavaScript identifiers. The page resolves
  it through own properties of `window` and never evaluates code.
- `args` is a JSON list. The exact strings `$container`, `$result`, `$error` and
  `$expired` become the widget container and the runtime callbacks. Other
  strings are templates.
- `returns: callback` requires `$result` in `args`. `returns: promise` forbids
  it, and the call must return a promise of a nonempty string. reCAPTCHA v3
  uses `call: grecaptcha.execute` with `returns: promise`.
- `integrity` sets the Subresource Integrity attribute when it is not null.

`script` loads a script that registers its own adapter:

```js
window.PowClient.registerHook("sso-check", {
  mount(ctx) {
    // ctx: container, params, challengeId, locale, signal
    return Promise.resolve(result);
  },
});
```

`presentation.csp` adds sources to the CSP of a challenge page whose step list
contains that hook. It accepts `script-src`, `style-src`, `img-src`, `font-src`,
`connect-src` and `frame-src`, with at most 16 sources each. Each source is an `https` URL (`http` in
development) with a host, an optional `*.` prefix on the host, an optional port
and an optional path that ends with `/`. It has no query or fragment. The URL of
`widget.script` or `script.src` must match a `script-src` source by the CSP
matching rules.

### 3.4 Verification

Rilgate sends one declarative HTTP request to check the result.

```yaml
verify:
  request:
    method: POST
    url: https://challenges.cloudflare.com/turnstile/v0/siteverify
    encoding: form
    headers: {}
    fields:
      secret: "{secrets.secret}"
      response: "{result}"
      remoteip: "{client_ip}"
  accept:
    - {field: /success, equals: true}
    - {field: /hostname, equals: "{host}"}
    - {field: /cdata, equals: "{challenge_id}"}
  timeout: 5s
  attempt_timeout: 2s
  retry: {attempts: 3, backoff: 200ms}
```

| Setting | Default | Range and rules |
| --- | --- | --- |
| `request.method` | `POST` | `POST` or `GET`. `GET` sends `fields` in the query |
| `request.url` | | Absolute `https` URL template (`http` in development) |
| `request.encoding` | `form` | `form` or `json` |
| `request.headers` | `{}` | At most 16 headers, templates allowed |
| `request.fields` | `{}` | At most 32 fields, templates allowed |
| `accept` | | 1 to 16 conditions. Every condition must hold |
| `timeout` | `5s` | `100ms` to `30s`. Deadline for all attempts |
| `attempt_timeout` | `2s` | `100ms` to `timeout` |
| `retry.attempts` | `3` | `1` to `5` attempts in total |
| `retry.backoff` | `200ms` | `10ms` to `5s`. Doubles after each attempt, with full jitter |
| `retry.on_timeout` | `false` | `true` also retries an attempt timeout. Set it only when the request sends an idempotency key, such as `{verification_id}` |

An `accept` condition has a `field` (JSON Pointer) and one operator: `equals`
(scalar or template), `in` (list of scalars), `min` or `max` (number or a
one-placeholder template that resolves to a number). A missing field fails the
condition.

The HTTP client uses rustls, sends no cookies, follows no redirects and reads at
most 16 KiB of JSON with a nesting depth of at most 32. The browser `result` has
at most 8192 bytes.

| Outcome | Condition |
| --- | --- |
| `pass` | 2xx response, valid JSON and every `accept` condition holds |
| `fail` | 2xx response with valid JSON where a condition fails, or a 4xx status other than 429 |
| `unavailable` | All attempts end in a retryable error, an attempt timeout, the deadline, invalid JSON or a body over the limit |

Retryable errors are a connection or TLS failure before the request is sent and
the statuses 429, 502, 503 and 504. A `Retry-After` value is used when it fits in
the deadline. Without `retry.on_timeout`, an attempt timeout is not retried,
because the provider may already have consumed a single-use result, and the
outcome is `unavailable`.

### 3.5 Unavailable hooks

```yaml
unavailable:
  action: deny          # deny | fallback
  fallback: {algorithm: fast, difficulty: 4}
  grant: while_unavailable   # while_unavailable | until_expiry
  after: 3
  recheck: 30s
```

| Setting | Default | Rules |
| --- | --- | --- |
| `action` | `deny` | `fallback` requires `fallback` |
| `fallback` | | A proof-of-work step. The algorithm must be registered and the difficulty valid. `refresh` is not allowed |
| `grant` | `while_unavailable` | See below |
| `after` | `3` | `1` to `100` consecutive `unavailable` outcomes mark the hook unavailable on this instance |
| `recheck` | `30s` | `1s` to `1h`. Time before the gate presents the hook again |

- With `action: deny`, a verification with the outcome `unavailable` returns
  `503` with `{"code": "hook_unavailable"}` and `Retry-After: 1`. The page shows
  the `unavailable` state and its retry control.
- With `action: fallback`, the same response contains `next` with the fallback
  step, so the page starts the fallback proof without a reload. While the hook is
  marked unavailable, the gate presents the fallback step directly. After
  `recheck`, the gate presents the hook again. A `pass` or `fail` outcome marks
  the hook available. An `unavailable` outcome restarts `recheck`.
- `grant: while_unavailable` accepts the fallback grant for the hook step only
  while the hook is marked unavailable. When the hook works again, the client
  gets the hook step on its next request.
- `grant: until_expiry` accepts the fallback grant until it expires.
- The availability state belongs to each instance and is not shared.

### 3.6 Endpoints

| Endpoint | Method | Purpose |
| --- | --- | --- |
| `{base_path}/api/hook/verify` | POST | Body `{presentation_token, result}`. A `pass` sets the access cookie and returns `{cookie_set: true}`, with `next` when a step remains |
| `{base_path}/api/hook/callback` | GET | Query `state` and `result_param`. Verifies, sets the access cookie on `pass` and redirects to the original path, where the gate presents any remaining step |

Both are proof endpoints: they need a resolved client address and use the proof
rate limit. The hook name comes from the signed presentation token. A token whose
presented step is not a hook step is rejected with `401`.

A hook `pass` adds a hook grant with the cookie merge in 2.4. A `fail` returns
`401` with `{"code": "hook_rejected"}`.

## 4. Web runtime

- The bootstrap gets `method` (`pow` or `hook`), `step`, `steps` and, for a hook
  step, `hook`: the name, the mode and the rendered `widget` or `script`
  settings. Browser templates are rendered per request.
- `web/static/js/challenge/hook.js` runs a hook step. It loads the script with up
  to 3 attempts, a 10-second timeout each and the backoff of the bundle loader.
  It waits up to 10 seconds for `call` to resolve to a function, calls it and
  sends `POST /api/hook/verify` with the result.
- `$error` and `$expired` render the widget again in a new child of the
  container. A `fail` outcome shows the `rejected` state and renders the widget
  again. A `503` shows `unavailable`, or starts `next` when the response has it.
  A script that does not load shows `blocked`.
- `challenge.js` runs steps in a loop: it runs the presented step, applies
  `next` when the verify or exchange response has it, and ends with
  `/api/session` and the current reload or `postMessage`.
- `PowClient.verify` and `PowClient.exchange` send `POW_PRESENTATION_TOKEN` and
  keep `next` for `challenge.js`.
- `challenge.html.jinja` gets `<div id="hook-container" hidden>`. The page shows
  "Step N of M" when `steps > 1`. The `pow-hook` class on `<html>` hides the
  proof progress and statistics. When an operator template has no container, the
  runtime creates one in `<main>`.
- The CSP of a challenge page adds the sources of every embed hook in its step
  list, because the page runs later steps without a reload. A page without a
  hook step adds no source.
- The JSON challenge body gets `challenge.method`, `challenge.step`,
  `challenge.steps` and `challenge.hook`.

A redirect step finishes with a top-level redirect to the protected path. It does
not send the embedded `postMessage`.

### Trust

The challenge page is served on the application host. An embedded hook script
runs with the origin of the application. The access cookie is `HttpOnly`, so the
script cannot read it, but it can call the application on that page. Use only
providers that you trust with that access.

## 5. Observability

| Metric | Labels |
| --- | --- |
| `rilgate_challenge_hook_verifications_total` | `hook`, `outcome` = `pass`, `fail`, `unavailable` |
| `rilgate_challenge_hook_retries_total` | `hook` |
| `rilgate_challenge_hook_available` (gauge) | `hook` |
| `rilgate_challenge_hook_fallbacks_total` | `hook` |
| `rilgate_challenge_steps_completed_total` | `step` = `1` to `4`, `method` = `pow`, `hook` |

Hook names are bounded by configuration. Readiness does not depend on hooks: a
provider outage must not take the gateway out of a load balancer.

## 6. Compatibility and rollout

- Gateways that do not use steps or hooks issue v1 presentation tokens and v1
  cookies. Their tokens and cookies do not change.
- An older gateway rejects v2 tokens and cookies and challenges the client again.
  Configure steps or hooks only after every instance that shares the signing keys
  runs a version that reads v2.
- `VerifyRequest` and `ExchangeRequest` accept the optional `presentation_token`
  field. Older clients do not send it and get no `next`.

## 7. Delivery

1. **Challenge steps.** Policy `steps`, `PolicyDecision.steps`, presentation and
   cookie v2, grant merge, the first step without a grant in the gate, `next` in
   the proof API and the step loop in the browser. Proof-of-work steps only.
2. **Challenge hooks.** The `challenge_hooks` group, the
   `rilgate-challenge-hooks` crate (templates, request, retry, accept,
   availability), hook steps and grants, the hook endpoints, redirect
   presentation, the embed runtime, CSP sources, the built-in hook presets and
   metrics.

Each part is one pull request with its tests and documentation.
