# AJAX and API challenge responses

Rilgate can notify an application client about a required challenge. The API response
stays an API response, and Rilgate does not replace it with an HTML page. The application
must handle this notification and show the challenge UI. After verification, it must retry the intercepted request. A JSON
response alone cannot make an unmodified application do those steps.

## Selecting a response format

```yaml
site:
  responses:
    format: auto
    json_paths: [/api, /graphql]
```

`format` accepts `auto` (default), `html` and `json`. Explicit
HTML mode preserves HTML challenges for GET and HEAD and a plain 401 response for other
methods. JSON mode uses JSON for policy challenges and denials. Neither setting changes
upstream responses or the internal proof API protocol.

In automatic mode, configured path prefixes force JSON. Prefix matching respects path
segments, so `/api` matches `/api` and `/api/users`, but not `/apiary`.
`json_paths` defaults to empty and cannot be used with HTML mode. It accepts at most 64
absolute paths, each at most 2048 bytes, without query strings, fragments, percent
escapes, backslashes or dot segments.

Otherwise Rilgate compares the supported `Accept` media ranges and quality values.
An explicit preference for `application/json` selects JSON. HTML wins ties.

A specific media range takes precedence over a wildcard. With absent or equally weighted
wildcard preferences, a request `Content-Type` of `application/json` or `application/*+json`, or
`X-Requested-With: XMLHttpRequest`, selects JSON. Browser navigation otherwise receives HTML. `Content-Type`
describes a request body, so clients should prefer `Accept: application/json` to state their desired
response format. API prefixes cover clients that cannot set either header.

## Notification contract

A JSON challenge returns HTTP 403, `Content-Type: application/json`, `Cache-Control: no-store`, and these headers:

```text
Rilgate-Challenge: required
Rilgate-Challenge-Url: /.well-known/rilgate/challenge?presentation_token=...
```

The bundled body includes `code: proof_required`, a localized `message`, `challenge_url`, signed
challenge presentation metadata and proof API endpoints. `challenge.token` is a presentation
token. Clients that implement a native solver first exchange it at the advertised
challenge endpoint to get actual work.

Native solvers and JSON clients can POST `challenge_token`, `nonce` and the optional
boolean `set_cookie` to the advertised verify endpoint. `set_cookie` defaults to
`false`; the response contains `receipt` and sets no cookie. With `set_cookie: true`,
a successful response still contains `receipt`, adds `cookie_set: true` and sets the
access cookie with the same host and client binding rules as receipt exchange.
Clients can then retry the protected request with that cookie. If `cookie_set: true`
is absent, POST the receipt to the advertised exchange endpoint first. Proof and
exchange failures retain their existing errors and set no cookie.

RandomX with Stratum pool acceptance uses the advertised WebSocket endpoint. It does not use
HTTP proof issuance and verification. A policy denial returns 403 with `code: access_denied` and
no challenge notification headers. A PoW solution cannot override a deny policy. HEAD
has the same status and headers with no response body.

## Customizing the JSON body

1. Set `site.resources.directory`.
2. Override `templates/pages/challenge.json.jinja` or `templates/pages/denied.json.jinja` there.

These are MiniJinja templates and participate in resource validation, seeding, watching
and reloads like HTML templates. For example, an application-specific challenge envelope
can be:

```jinja
{
  "success": false,
  "error": {
    "type": "verification_required",
    "message": {{ t("challenge.description") | tojson }},
    "verification_url": {{ challenge.url | tojson }}
  }
}
```

Templates can rename, nest or omit fields to match the original application's contract.
Keep a challenge URL or presentation token wherever the client expects it. `Rilgate-Challenge`
and `Rilgate-Challenge-Url` remain available regardless of body shape, so a shared request
interceptor can work across different envelopes. HTTP challenge status remains 403.
Changes to the template do not change it.

Use `tojson` for dynamic values, including strings. Do not add quotes manually.
Every challenge or denial JSON template must render a valid JSON object.

Validation covers each loaded locale and both algorithms. Rendering also checks the
actual output. Invalid reloads retain the previous snapshot.

At startup, invalid external overrides cause use of the embedded bundle. Web diagnostics
report them. Run `rilgate web validate DIRECTORY` before deployment. No separate JSON Schema is required.
The template defines the response body. Application integration tests check its expected
envelope and fields.

## Same-origin iframe or popup

The notification URL always serves the HTML solver, even when API paths or `format: json`
select JSON elsewhere. Its presentation token is checked for signature, expiration and
the request host. The page supports same-origin framing and sets `frame-ancestors 'self'`,
`X-Frame-Options: SAMEORIGIN` and `Referrer-Policy: no-referrer`. Exclude the short-lived token URL from application analytics.
Redact query strings on this route in access logs.

After the proof is verified and the authentication cookie is set, this page sends:

```js
{ type: "rilgate:verified" }
```

It uses `postMessage` with its own origin as `targetOrigin`. It targets its parent iframe
and, when present, its popup opener. The embedded page displays success without a reload
or navigation of the original application.

The application's handler must check both the sender origin and window identity:

```js
const frame = document.createElement("iframe");
const challengeUrl = new URL(response.headers.get("Rilgate-Challenge-Url"), location.href);
if (challengeUrl.origin !== location.origin) throw new Error("Unexpected challenge origin");

function onVerified(event) {
  if (event.origin !== location.origin || event.source !== frame.contentWindow) return;
  if (event.data?.type !== "rilgate:verified") return;
  window.removeEventListener("message", onVerified);
  frame.remove();
  retryOriginalRequest();
}

window.addEventListener("message", onVerified);
frame.src = challengeUrl.href;
document.body.append(frame);
```

`response` is the intercepted challenge response and `retryOriginalRequest` is application-owned
logic. Add timeout, cancellation and expired-token handling to the application's modal
lifecycle. Include removal of its message listener.

Allow same-origin frames in the application's CSP. A popup uses the same protocol. It
checks `event.source` against the window returned by `window.open`. To satisfy popup
blockers, open it from a user gesture.

1. Before you send the request, store its URL, method, headers and replayable body.
2. Only after completion, retry the request.
3. Limit the number of retries, so an expired or insufficient proof cannot cause a loop.

Streaming bodies can need recreation. Rilgate rejects the intercepted request before it
contacts the upstream. It does not retain or automatically replay request bodies.

The application still owns its normal idempotency and retry rules. Use HTTPS in
production so the Secure auth cookie can be sent. This flow supports a gateway and
application that share an origin. Cross-origin embedding, CORS exposure and third-party
cookie flows are not configured by these settings.

Pool failover does not change the client API. A WebSocket job and its share
submission use one pool session. A closed session requires a new WebSocket.
Clients must not move a job ID to another connection. Operational readiness
lists configured pool names and results, without addresses or credentials.
