# Web resources

## Web resources, branding, and translations

The canonical bundle is `web/`. Templates, English translations,
CSS, JavaScript, workers, and WASM are embedded into the binary. The server remains
in `crates/rilgate-gateway` and delegates presentation to the `rilgate-web` crate.

Configure `web.name`, `web.website_url`, `web.base_path`, and
`web.stylesheet_path` to customize public branding and URLs. `pow.auth_cookie_name`
and `web.i18n.language_cookie_name` control cookie names. Templates receive only
public `rilgate`, `page`, and `challenge` context, plus the negotiated `locale`
and selected client translations. Signing keys and pool credentials are excluded.

With `web.root` unset, Rilgate runs entirely from embedded resources. Set it to an
operator directory to override individual files under `templates/`, `static/`,
and `i18n/`. An empty directory is seeded once when `web.seed.empty_directory`
is enabled. Meaningful existing files, including hidden operator files, prevent
automatic initialization. Existing destinations are never overwritten, including
during simultaneous startup. Unwritable empty roots log a warning and use the
embedded bundle. Symlinks within the override tree are rejected.

Translations are flat-key JSON objects and overlay embedded translations per key.
The locales Rilgate serves are the tags of the translation files present, so
adding `web/i18n/vi.json` (or an override of the same name) is all that is needed
to offer Vietnamese; no configuration change or restart of the decision is
involved. Set `web.i18n.supported_locales` only to restrict that set to a subset
of what has translations, which is how a translated locale stays unadvertised.
The declared list must include `web.i18n.default_locale`, and startup fails if the
default locale has no translations. Locale tags use BCP 47 casing.

Locale priority is `?lang=`, the configured language cookie, `Accept-Language`,
then `web.i18n.default_locale`. Lookup tries the selected locale and its parents,
the configured default and its parents, then English. Missing keys return the key
and are reported when `web.i18n.strict` is enabled, which turns them into
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

Enable `web.watch.enabled` for recursive, debounced reloads. A candidate snapshot
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

For development, mount a writable web directory and enable watch. For production,
initialize it first, mount it read-only, and leave watch disabled:

```yaml
services:
  rilgate:
    volumes:
      - ./custom-web:/etc/rilgate/web:ro
```

Each scalar web setting has a corresponding uppercase environment override, such
as `WEB_NAME`, `WEB_ROOT`, or `WEB_WATCH_ENABLED`.
`WEB_I18N_SUPPORTED_LOCALES` accepts a comma-separated list and, when set,
restricts the served locales to it. `web.base_path` takes precedence over the
legacy `pow.internal_prefix` setting when explicitly configured.
