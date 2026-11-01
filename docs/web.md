# Web resources

For configurable JSON challenge responses and same-origin iframe completion, see [AJAX
and API integration](api-integration.md).

## Web resources, branding, and translations

The canonical bundle is `web/`. HTML templates use `.html.jinja`. JSON response
templates use `.json.jinja`, including layouts and partials where applicable.

These suffixes identify MiniJinja source files. They do not identify finished HTML or
JSON. Every template, including external overrides, must use one of these two suffixes.

Configure your editor's Jinja support for these patterns. HTML templates retain
automatic HTML escaping. JSON templates use `tojson` for dynamic values. Their
rendered response bodies are checked as valid JSON objects. No separate response JSON
Schema is required.

Templates, English translations, CSS, JavaScript, workers, and WASM are embedded into
the binary. The server remains in `crates/rilgate-gateway` and delegates presentation to the
`rilgate-web` crate.

Configure `site.name`, `site.url`, `site.base_path`, and `site.stylesheet` to customize public
branding and URLs. `security.auth.cookie` and `site.i18n.cookie` control cookie names. Templates receive
only public `rilgate`, `page`, and `challenge` context, plus the negotiated
`locale` and selected client translations. Signing keys and pool credentials are
excluded.

With `site.resources.directory` unset, Rilgate runs entirely from embedded resources. Set it to an
operator directory to override individual files under `templates/`, `static/`, and
`i18n/`. An empty directory is seeded once when `site.resources.seed` is `if_empty`.

Meaningful existing files, including hidden operator files, prevent automatic
initialization. Existing destinations are never overwritten, including during
simultaneous startup. Unwritable empty roots log a warning and use the embedded bundle.
Symlinks within the override tree are rejected.

Translations are flat-key JSON objects and overlay embedded translations per key. With
`site.i18n.supported` unset or empty, the served locales are discovered from the loaded
translation files. To offer Vietnamese:

1. Add `i18n/vi.json` inside `site.resources.directory`.
2. Reload the snapshot through watch mode, SIGHUP, or a restart.

Changes to the repository's `web/i18n/` need a rebuild of the binary.

An explicit `site.i18n.supported` list selects the served locales. It can hide a translated
locale or advertise a locale that uses fallback translations. The list must include
`site.i18n.default`.

Both the custom snapshot and the embedded fallback must be able to resolve the default
locale. Keep `en` as the default for automatic discovery with the shipped
bundle. A filesystem-only default locale requires an explicit list that also works
without its override file. Locale tags use BCP 47 casing.

Locale priority is `?lang=`, the configured language cookie, `Accept-Language`, then
`site.i18n.default`. Lookup tries the selected locale and its parents, the configured default
and its parents, then English. Missing keys return the key and are reported when
`site.i18n.strict` is enabled, which turns them into validation errors at load time.
Translation values are text and are HTML-escaped. Selected status strings are serialized
safely into the page for JavaScript.

The served locale set, visible translations for each locale, and public site details are
resolved once per snapshot. They are not resolved per request. A reload changes them.

Templates and translations are loaded and validated in memory. Static overrides are
streamed from private snapshot backing files. This keeps their bytes and ETags
consistent without retention of large images in RAM. Resources support ETag revalidation
and HEAD requests. The previous `/static/challenge.js` and `/static/hash-worker.js` resource paths, plus
`/static/js/hash-worker.js` and `/static/randomx/*`, remain aliases for the canonical `/static/js/` paths.

Enable `site.resources.watch.enabled` for recursive, debounced reloads. A candidate snapshot must compile,
load translations, and render successfully before atomic publication. Invalid reloads
retain the last working snapshot.

Requests that hold an earlier snapshot finish normally. At most one builder runs at a
time. Watcher events are coalesced while it runs.

Watch is disabled by default. On Unix, SIGHUP reloads web resources even when watch is
disabled. Configuration settings are read at startup. SIGHUP reloads the resource files.

```bash
rilgate web init ./custom-web
rilgate web validate ./custom-web
rilgate web diff ./custom-web
kill -HUP $(pidof rilgate)
```

Validation reports invalid templates, includes, translations, and paths.
It does not run the server.

If startup overrides are invalid, Rilgate uses the embedded snapshot.
Rendering failures use embedded page templates. The health endpoint reports web
generation, reload counters, active locales, template count, and watch status. Parser
errors and resource contents remain in local diagnostics only.

Templates are always memory-backed: `site.templates.cache` must remain `true`. The embedded
challenge templates are also rendered into an HTML shell per locale when a snapshot is
built. Requests without preview tags insert their JSON-escaped token and solver bootstrap.
Requests with Open Graph tags render through the template context.

Client translation JSON is serialized once per snapshot and locale. Challenge responses
remain `no-store`. The cache is internal to the server and never shares tokens between
requests.

A change to any template selects dynamic rendering. This preserves conditions on
`challenge.difficulty`, expiry, and other request data. Translation, branding, and static-asset
overrides can still use the embedded shell. A reload publishes new shells atomically
with the templates and translations. Use watch mode to load template edits during
development.

For development:

1. Mount a writable web directory.
2. Enable watch.

For production:

1. Initialize the web directory.
2. Mount it read-only.
3. Leave watch disabled:

```yaml
site:
  resources:
    directory: /etc/rilgate/web
    seed: never
    watch:
      enabled: false
```

Mount that directory read-only in the deployment. Site settings use the same YAML
structure in development and production. Environment variables can supply individual
values through `${VARIABLE_NAME}` string references.

## Challenge page design

Challenge, refresh, denied and error pages share a light or dark layout.
The bar shimmers while work runs and fills on success. Reduced-motion preferences
stop the shimmer and success transition. No environment provides a state preview.
Statistics occupy a separate line and use the page locale with no fraction digits.
They remain visible through verifying, pool and success when attempts exceed zero.
RandomX asks for consent before starting; details explain the check and permission.

`window.PowClient.state(name, values)` updates translated title, description and
status using `challenge.state.<name>.{title,description,status}`. Placeholder values
come from `values`. `stats(attempts, rate)` updates statistics. `status(message)`
continues to support custom status messages.

| State | Mode | Trigger |
| --- | --- | --- |
| `preparing` | `run` | Page startup and challenge request. |
| `fast` | `run` | Fast hashing. |
| `hashwx` | `run` | HashWX hashing. |
| `consent` | `start` | Wait for RandomX consent. |
| `initializing` | `run` | RandomX module and cache setup. |
| `randomx` | `run` | RandomX hashing. |
| `pool` | `run` | Pool connection and acceptance wait. |
| `verifying` | `run` | Proof verification. |
| `cookies` | `retry` | Cookies disabled, or the access cookie did not reach the gateway after verification. |
| `success` | `done` | Verification completed. |
| `expired` | `retry` | Local solver deadline exceeded. |
| `failed` | `retry` | Other failure; details remain in console. |
| `network` | `retry` | Fetch TypeError or XHR network failure. |
| `unsupported` | `none` | Missing RandomX or HashWX browser features. |
| `limited` | `none` | HTTP 429; append Retry-After text. |
| `unavailable` | `retry` | HTTP 503 after the single RandomX reload. |

Modes `run` and `done` show the bar. `start` shows the consent button, `retry`
shows the reload button, and `none` hides both actions and the bar.
The catch handler keeps diagnostic error messages in the console only.

| Element ID | Purpose |
| --- | --- |
| `title` | State title. |
| `description` | State description. |
| `bar` | Shimmer or completed track. |
| `bar-shimmer` | Running shimmer. |
| `bar-complete` | Success fill. |
| `progress` | Visually hidden progress. |
| `status` | Live state status. |
| `stats` | Attempt and hash rate line. |
| `attempts` | Localized attempt count. |
| `hashrate` | Localized hashes per second. |
| `randomx-start` | RandomX consent button. |
| `retry` | Reload button. |
| `pow-i18n` | Client translation JSON. |

Every element is optional. Missing IDs are skipped, and templates with only
`status` still receive status updates. Keep `randomx-start` to collect RandomX
consent, and `retry` for reload actions. Preserve the `.actions` wrapper,
`noscript`, honeypot link, `pow-i18n`, `challenge_bootstrap` and bundle script
when adapting the embedded challenge template. The `progress` element remains
visually hidden. The former `partials/pow-info.html.jinja` include is removed.

## Legal notices

The footer says Protected by and links to `rilgate.website_url` with the public
name. Embedded translations contain no `legal.*` keys or legal text. Add notices
under your resource directory. Review this example for your website's practices.

```json title="i18n/en.json"
{
  "legal.privacy.label": "Privacy",
  "legal.privacy.title": "Privacy information",
  "legal.privacy.body": "This website uses a browser check to manage automated traffic.\n\nThe website owner's privacy policy applies.",
  "legal.terms.label": "Terms",
  "legal.terms.url": "https://example.com/terms"
}
```

```json title="i18n/vi.json"
{
  "legal.privacy.label": "Quyền riêng tư",
  "legal.privacy.title": "Thông tin về quyền riêng tư",
  "legal.privacy.body": "Website này dùng bước kiểm tra trình duyệt để quản lý lưu lượng tự động.\n\nChính sách quyền riêng tư của chủ website vẫn áp dụng.",
  "legal.terms.label": "Điều khoản",
  "legal.terms.url": "https://example.com/terms"
}
```

For each `privacy` and `terms` entry, a missing `legal.<id>.label` hides the entry.
An existing `.url` produces a link with `rel="noopener noreferrer"`. Otherwise,
supply `.title` and `.body` for a dialog with a Close button. Blank lines in the
body produce separate escaped `<p>` paragraphs. Legal values are text.

`has_t(key)` returns a boolean using the same locale fallback chain as `t` and
never fails in strict mode. The legal partial guards dynamic keys with `has_t`,
so missing optional entries do not fail load-time translation validation.
Its ES5 inline script opens dialogs on challenge, refresh, denied and error pages.
Reload resources and run `rilgate web validate ./custom-web` after editing.

## HashWX browser requirements

HashWX uses the classic adapter in `challenge.bundle.js` to load its modern
solver. It requires WebAssembly 1.0, workers and a modern JavaScript module
runtime. The solver assets use ES2020 syntax. BigInt and SIMD are optional
browser features for HashWX; the solver uses u32 halves at the WASM boundary.

The embedded assets are under `static/js/solvers/hashwx/`:

| File | Role |
| --- | --- |
| `adapter.js` | Classic adapter, compiled into the startup bundle |
| `solver.js` | Allocates workers, combines progress and controls cancellation |
| `worker.js` | Derives seeds, compiles modules and searches nonce ranges |
| `hashwx_wasm.wasm` | Interpreter, instance generator, seed hash and WASM generator |

The shared `static/js/solvers/loader.js` imports the descriptor-selected
`window.POW_SOLVER_MODULE_URL` for HashWX and RandomX. The Fast solver stays
in the classic startup bundle for legacy browsers.

Each VM has 16392 scratch bytes. Total browser memory also includes Rust WASM
memory and stack, generated modules and JavaScript state. The default solver
uses up to two workers. All workers are terminated on success, error, expiry,
cancellation or page exit. It uses the signed batch size and covers distinct
batches in each worker. The classic adapter reports unavailable capabilities
through the localized unsupported state. It never selects another algorithm.

Rilgate's challenge response sets `frame-ancestors 'self'`. An operator or edge
can add a more restrictive CSP. Permit worker creation under `worker-src` and
permit WASM compilation in the worker's own CSP, for example with
`script-src 'self' 'wasm-unsafe-eval'` on the worker response. A network worker
has its own policy context. A page policy alone does not establish its WASM
policy.

If a generated module cannot compile after the interpreter loads, calculation
continues with that interpreter. A worker CSP that denies all WASM also denies
the interpreter's initial compilation, so calculation fails. Browser modes
that disable WASM also make HashWX unavailable.

After changing web sources or either HashWX crate, build both generated asset
sets before building or testing the gateway:

```sh
export CARGO_BUILD_BUILD_DIR="$PWD/target" CARGO_TARGET_DIR="$PWD/target"
cargo xtask build-wasm && cargo xtask build-web
```

## Open Graph template context

`page.open_graph` is a list of `{attribute, name, content}` for a protected HTML
GET challenge when `site.open_graph` is enabled. Other pages receive an empty
list. Fetched tags keep their source attribute: `property` when the element has
that attribute, otherwise `name`. Override tags use `property` when their name
contains a colon and `name` otherwise. The list preserves fetched tag order and
repeated names. `site.open_graph.tags` controls the allowlist, and
`site.open_graph.when` selects the requests that receive tags.

The embedded `layouts/base.html.jinja` includes this loop in `<head>`:

```jinja
{% for tag in page.open_graph %}
<meta {{ tag.attribute }}="{{ tag.name }}" content="{{ tag.content }}">
{% endfor %}
```

Autoescaping applies to values; the gateway supplies the attribute from the
fixed set `property` and `name`. Custom base layouts must include the loop to
render the metadata. `rilgate web diff` shows the embedded layout change.

## Response status codes

`site.responses.challenge_status` sets the HTML challenge status. It defaults to
200 and accepts 200, 401, 403, 429 or 503. JSON challenges always return 403 with
`Rilgate-Challenge: required` so API clients can identify the challenge.

A challenge page answers at the URL of the requested resource and carries
`Cache-Control: no-store`. Some caches ignore that header, for example a CDN rule
that overrides origin cache headers or a service worker that stores every 200
response. Such a cache can keep a 200 challenge page as the resource, such as a
script or a stylesheet, and serve it to clients that passed the challenge. Behind
such a cache, set `challenge_status` to 403, because most caches store a 403
response only when a rule tells them to. Also allow public static asset paths in
the policy, so that these paths never receive a challenge. See [Web frontends and
caches](production.md#web-frontends-and-caches) for policy and cache examples.

`site.responses.deny_status` sets the status for HTML and JSON policy denials.
It defaults to 403 and accepts 403, 404, 410 or 429.
