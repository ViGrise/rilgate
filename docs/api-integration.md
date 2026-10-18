# AJAX and API challenge responses

Rilgate can notify an application client about a required challenge without
replacing its API response with an HTML page. The application must handle this
notification, show the challenge UI and retry the intercepted request after
verification. A JSON response alone cannot make an unmodified application perform
those steps.

## Selecting a response format

```yaml
site:
  responses:
    format: auto
    json_paths: [/api, /graphql]
```

`format` accepts `auto` (default), `html` and `json`. Explicit HTML mode preserves
HTML challenges for GET/HEAD and a plain 401 response for other methods. JSON mode
uses JSON for policy challenges and denials. Neither setting changes upstream
responses or the internal proof API protocol.

In automatic mode, configured path prefixes force JSON. Prefix matching respects
path segments, so `/api` matches `/api` and `/api/users`, but not `/apiary`.
`json_paths` defaults to empty and cannot be used with HTML mode. It accepts at most
64 absolute paths, each at most 2048 bytes, without query strings, fragments,
percent escapes, backslashes or dot segments.

Otherwise Rilgate compares the supported `Accept` media ranges and quality values.
An explicit preference for `application/json` selects JSON; HTML wins ties. A
specific media range takes precedence over a wildcard. With absent or equally
weighted wildcard preferences, a request `Content-Type` of `application/json` or
`application/*+json`, or `X-Requested-With: XMLHttpRequest`, selects JSON.
Browser navigation otherwise receives HTML. `Content-Type` describes a request
body, so clients should prefer `Accept: application/json` to state their desired
response format. API prefixes cover clients that cannot set either header.

## Notification contract

A JSON challenge returns HTTP 403, `Content-Type: application/json`,
`Cache-Control: no-store`, and these headers:

```text
Rilgate-Challenge: required
Rilgate-Challenge-Url: /.well-known/rilgate/challenge?presentation_token=...
```

The bundled body includes `code: proof_required`, a localized `message`,
`challenge_url`, signed challenge presentation metadata and proof API endpoints.
`challenge.token` is a presentation token: clients implementing a native solver
first exchange it at the advertised challenge endpoint to obtain actual work.
RandomX with P2Pool acceptance uses the advertised WebSocket endpoint instead of
HTTP proof issue/verify. A policy denial returns 403 with `code: access_denied`
and no challenge notification headers. Solving PoW cannot override a deny policy.
HEAD has the same status and headers with no response body.

## Customizing the JSON body

Set `site.resources.directory` and override `templates/pages/challenge.json.jinja` or
`templates/pages/denied.json.jinja` there. These are MiniJinja templates and participate
in resource validation, seeding, watching and reloads like HTML templates.
For example, an application-specific challenge envelope can be:

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

Templates can rename, nest or omit fields to match the original application's
contract. Keep a challenge URL or presentation token wherever the client expects
it. `Rilgate-Challenge` and `Rilgate-Challenge-Url` remain available regardless of
body shape, so a shared request interceptor can work across different envelopes.
HTTP challenge status remains 403; changing the template does not change it.

Use `tojson` for dynamic values, including strings, instead of manually adding
quotes. Every challenge/denial JSON template must render a valid JSON object.
Validation covers each loaded locale and both algorithms; rendering also checks
the actual output. Invalid reloads retain the previous snapshot. At startup,
invalid external overrides fall back to the embedded bundle and are reported in
web diagnostics. Run `rilgate --config config.yml web validate` before deployment.
No separate JSON Schema is required. The template defines the response body;
application integration tests verify its expected envelope and fields.

## Same-origin iframe or popup

The notification URL always serves the HTML solver, even when API paths or
`format: json` select JSON elsewhere. Its presentation token is checked for
signature, expiration and the request host. The page supports same-origin framing
and sets `frame-ancestors 'self'`, `X-Frame-Options: SAMEORIGIN` and
`Referrer-Policy: no-referrer`. Keep the short-lived token URL out of application
analytics and redact query strings on this route in access logs.

After the proof is verified and the authentication cookie is set, this page sends:

```js
{ type: "rilgate:verified" }
```

It uses `postMessage` with its own origin as `targetOrigin`, targeting its parent
iframe and, when present, its popup opener. The embedded page displays success
without reloading or navigating the original application.

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

`response` is the intercepted challenge response and `retryOriginalRequest` is
application-owned logic. Add timeout, cancellation and expired-token handling to
the application's modal lifecycle, including removing its message listener.
Allow same-origin frames in the application's CSP. A popup uses the same protocol
but checks `event.source` against the window returned by `window.open`; open it
from a user gesture to satisfy popup blockers.

Store the request URL, method, headers and replayable body before sending it.
Retry only after completion and bound retries so an expired or insufficient proof
cannot cause a loop. Streaming bodies may need to be recreated. Rilgate rejects
the intercepted request before contacting the upstream; it does not retain or
automatically replay request bodies. The application still owns its normal
idempotency and retry rules. Use HTTPS in production so the Secure auth cookie
can be sent. This flow supports a gateway and application sharing an origin;
cross-origin embedding, CORS exposure and third-party cookie flows are not
configured by these settings.
