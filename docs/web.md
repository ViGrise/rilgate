# Web resources

For configurable JSON challenge responses and same-origin iframe completion, see
[AJAX and API integration](api-integration.md).

## Web resources, branding, and translations

The canonical bundle is `web/`. HTML templates use `.html.jinja`; JSON response
templates use `.json.jinja`, including layouts and partials where applicable.
These suffixes identify MiniJinja source files rather than finished HTML or JSON.
Every template, including external overrides, must use one of these two suffixes.
Configure your editor's Jinja support for these patterns. HTML templates retain
automatic HTML escaping; JSON templates use `tojson` for dynamic values and their
rendered response bodies are checked as valid JSON objects. No separate response
JSON Schema is required.

Templates, English translations,
CSS, JavaScript, workers, and WASM are embedded into the binary. The server remains
in `crates/rilgate-gateway` and delegates presentation to the `rilgate-web` crate.

Configure `site.name`, `site.url`, `site.base_path`, and
`site.stylesheet` to customize public branding and URLs. `security.auth.cookie`
and `site.i18n.cookie` control cookie names. Templates receive only
public `rilgate`, `page`, and `challenge` context, plus the negotiated `locale`
and selected client translations. Signing keys and pool credentials are excluded.

With `site.resources.directory` unset, Rilgate runs entirely from embedded resources. Set it to an
operator directory to override individual files under `templates/`, `static/`,
and `i18n/`. An empty directory is seeded once when `site.resources.seed`
is `if_empty`. Meaningful existing files, including hidden operator files, prevent
automatic initialization. Existing destinations are never overwritten, including
during simultaneous startup. Unwritable empty roots log a warning and use the
embedded bundle. Symlinks within the override tree are rejected.

Translations are flat-key JSON objects and overlay embedded translations per key.
With `site.i18n.supported` unset or empty, the served locales are discovered
from the loaded translation files. Add `i18n/vi.json` inside `site.resources.directory` and reload
the snapshot through watch mode, SIGHUP, or a restart to offer Vietnamese.
Changing the repository's `web/i18n/` instead requires rebuilding the binary.

An explicit `site.i18n.supported` list selects the served locales. It can
hide a translated locale or advertise a locale that uses fallback translations.
The list must include `site.i18n.default`. Both the custom snapshot and the
embedded fallback must be able to resolve the default locale. Keep `en` as the
default for automatic discovery with the shipped bundle; a filesystem-only
default locale requires an explicit list that also works without its override
file. Locale tags use BCP 47 casing.

Locale priority is `?lang=`, the configured language cookie, `Accept-Language`,
then `site.i18n.default`. Lookup tries the selected locale and its parents,
the configured default and its parents, then English. Missing keys return the key
and are reported when `site.i18n.strict` is enabled, which turns them into
validation errors at load time. Translation values are text and are
HTML-escaped. Selected status strings are serialized safely into the page for
JavaScript.

The served locale set, the visible translations for each locale, and public site
details are all resolved once per snapshot rather than per request, so a reload is
what changes them.

Templates and translations are loaded and validated in memory. Static overrides
are streamed from private snapshot backing files, keeping their bytes and ETags
consistent without retaining large images in RAM. Resources support ETag
revalidation and HEAD requests. The previous `/static/challenge.js` and
`/static/hash-worker.js` resource paths, plus `/static/js/hash-worker.js` and
`/static/randomx/*`, remain aliases for the canonical `/static/js/` paths.

Enable `site.resources.watch.enabled` for recursive, debounced reloads. A candidate snapshot
must compile, load translations, and render successfully before atomic publication.
Invalid reloads retain the last working snapshot; requests holding an earlier
snapshot finish normally. At most one builder runs at a time, and watcher events
are coalesced while it runs. Watch is disabled by default. On Unix, SIGHUP reloads
web resources even when watch is disabled. Configuration settings are read at
startup; SIGHUP reloads the resource files.

```bash
rilgate web init ./custom-web
rilgate web validate ./custom-web
rilgate web diff ./custom-web
kill -HUP $(pidof rilgate)
```

Validation reports invalid templates, includes, translations, and paths without
running the server. Invalid startup overrides fall back to the embedded snapshot.
Rendering failures use embedded page templates. The health endpoint reports web
generation, reload counters, active locales, template count, and watch status;
parser errors and resource contents remain in local diagnostics only.

Templates are always memory-backed: `site.templates.cache` must remain `true`.
The embedded challenge templates are also rendered into an HTML shell per locale
when a snapshot is built. Each request inserts only its JSON-escaped token and
solver bootstrap. Client translation JSON is serialized once per snapshot and
locale. Challenge responses remain `no-store`; the cache is internal to the
server and never shares tokens between requests.

Changing any template selects dynamic rendering, preserving conditions on
`challenge.difficulty`, expiry, and other request data. Translation, branding,
and static-asset overrides can still use the embedded shell. Reloading publishes
new shells atomically with the templates and translations.
Use watch mode to load template edits during development.

For development, mount a writable web directory and enable watch. For production,
initialize it first, mount it read-only, and leave watch disabled:

```yaml
site:
  resources:
    directory: /etc/rilgate/web
    seed: never
    watch:
      enabled: false
```

Mount that directory read-only in the deployment. Site settings use the same
YAML structure in development and production. Environment variables can supply
individual values through `${VARIABLE_NAME}` string references.
