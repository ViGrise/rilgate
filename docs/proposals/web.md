# Rilgate Web Template & i18n Architecture

## 1. Goals

Rilgate's web layer should provide:

- Built-in templates, translations, and static assets.
- Standalone operation without external web files.
- Runtime customization without rebuilding Rilgate.
- MiniJinja as the template engine.
- Flat-key JSON translations.
- Locale negotiation using standard HTTP language preferences.
- Key-level translation fallback.
- Filesystem overrides with embedded fallback.
- Automatic initialization of an explicitly configured but empty web directory.
- Templates loaded into memory for request-time performance.
- Translations loaded into memory.
- Optional directory watching and hot reload.
- Hot reload disabled by default.
- Atomic runtime updates without interrupting active requests.
- Failed reloads must never replace the last known-good web state.
- Existing operator files must never be automatically overwritten.

---

# 2. High-Level Architecture

```text
                         ┌─────────────────────────┐
                         │   Embedded Web Bundle   │
                         │                         │
                         │ templates/              │
                         │ static/                 │
                         │ i18n/                   │
                         └────────────┬────────────┘
                                      │
                                      │ defaults / seed source
                                      ▼
                         ┌─────────────────────────┐
                         │     web.root override   │
                         │                         │
                         │ templates/              │
                         │ static/                 │
                         │ i18n/                   │
                         └────────────┬────────────┘
                                      │
                                      │ startup load
                                      │ or watch event
                                      ▼
                         ┌─────────────────────────┐
                         │     Web Snapshot        │
                         │                         │
                         │ MiniJinja Environment   │
                         │ Translation Store       │
                         │ Static Asset Metadata   │
                         └────────────┬────────────┘
                                      │
                                      │ atomic shared pointer
                                      ▼
                              ┌───────────────┐
                              │ HTTP Request  │
                              └───────┬───────┘
                                      │
                                      ▼
                           Locale Negotiation
                                      │
                                      ▼
                                MiniJinja
                                      │
                                      ▼
                                    HTML
```

The request path should not normally access the filesystem.

---

# 3. Workspace Layout

```text
rilgate/
├── crates/
│   ├── rilgate-server/
│   ├── rilgate-config/
│   ├── rilgate-pow/
│   └── rilgate-web/
│       ├── src/
│       │   ├── lib.rs
│       │   ├── manager.rs
│       │   ├── snapshot.rs
│       │   ├── renderer.rs
│       │   ├── loader.rs
│       │   ├── embedded.rs
│       │   ├── seed.rs
│       │   ├── watcher.rs
│       │   ├── context.rs
│       │   ├── assets.rs
│       │   │
│       │   └── i18n/
│       │       ├── mod.rs
│       │       ├── loader.rs
│       │       ├── locale.rs
│       │       ├── store.rs
│       │       └── translator.rs
│       │
│       └── web/
│           ├── templates/
│           │   ├── layouts/
│           │   │   └── base.html
│           │   │
│           │   ├── pages/
│           │   │   ├── challenge.html
│           │   │   ├── denied.html
│           │   │   └── error.html
│           │   │
│           │   └── partials/
│           │       ├── pow-info.html
│           │       └── footer.html
│           │
│           ├── static/
│           │   ├── css/
│           │   │   └── rilgate.css
│           │   ├── js/
│           │   │   └── challenge.js
│           │   └── images/
│           │
│           └── i18n/
│               └── en.json
│
├── config/
│   └── rilgate.yml
└── Cargo.toml
```

`crates/rilgate-web/web/` is the canonical built-in web source.

Its contents are embedded into the Rilgate binary at build time.

---

# 4. Runtime Filesystem Layout

When an override directory is configured:

```text
/etc/rilgate/web/
├── templates/
│   ├── layouts/
│   │   └── base.html
│   ├── pages/
│   │   ├── challenge.html
│   │   ├── denied.html
│   │   └── error.html
│   └── partials/
│       ├── pow-info.html
│       └── footer.html
│
├── static/
│   ├── css/
│   │   └── rilgate.css
│   ├── js/
│   │   └── challenge.js
│   └── images/
│
└── i18n/
    ├── en.json
    ├── vi.json
    ├── zh-CN.json
    └── ja.json
```

The filesystem acts as an override layer.

The embedded bundle always remains available as fallback.

---

# 5. Configuration

Recommended configuration:

```yaml
web:
  # Internal Rilgate URL namespace.
  base_path: /.well-known/rilgate

  # Optional filesystem override directory.
  #
  # If omitted:
  #   only embedded resources are used.
  #
  # If configured but empty:
  #   built-in resources are copied into it once.
  #
  # Existing files are never overwritten automatically.
  root: /etc/rilgate/web

  seed:
    # Seed built-in web resources when root contains
    # no regular user files.
    empty_directory: true

  watch:
    # Watch web.root for changes and reload templates,
    # translations, and asset metadata automatically.
    #
    # Disabled by default for production predictability.
    enabled: false

    # Collapse bursts of filesystem events into one reload.
    debounce: 300ms

  templates:
    # Templates are loaded and compiled into memory.
    cache: true

    strict: true
    recursion_limit: 64
    fuel: 50000

  i18n:
    default_locale: en

    supported_locales:
      - en
      - vi
      - zh-CN
      - ja

    strict: false
```

Minimal configuration:

```yaml
web:
  root: /etc/rilgate/web
```

Default behavior when `web.root` is not configured:

```text
embedded templates
embedded translations
embedded static assets
watch disabled
```

---

# 6. Defaults

Recommended defaults:

```text
web.root                    = unset
web.seed.empty_directory    = true
web.watch.enabled           = false
web.watch.debounce          = 300ms

web.templates.cache         = true
web.templates.strict        = true
web.templates.recursion     = 64
web.templates.fuel          = 50000

web.i18n.default_locale     = en
web.i18n.strict             = false
```

Templates should always be memory-backed in normal operation.

There should not be a "read template from disk on every request" production mode.

---

# 7. Startup Flow

```text
                     Rilgate starts
                           │
                           ▼
                  web.root configured?
                    │             │
                   no            yes
                    │             │
                    │             ▼
                    │      resolve + validate root
                    │             │
                    │             ▼
                    │       root exists?
                    │         │       │
                    │        no      yes
                    │         │       │
                    │         ▼       ▼
                    │     create   inspect
                    │         │       │
                    │         └───┬───┘
                    │             ▼
                    │      contains user files?
                    │         │          │
                    │        no         yes
                    │         │          │
                    │         ▼          │
                    │       seed         │
                    │      built-ins     │
                    │         │          │
                    └─────────┴──────────┘
                              │
                              ▼
                     Build Web Snapshot
                              │
                ┌─────────────┼──────────────┐
                ▼             ▼              ▼
          Templates       Translations    Assets
                │             │              │
                └─────────────┼──────────────┘
                              ▼
                        validate all
                              │
                              ▼
                       publish snapshot
                              │
                              ▼
                   watch.enabled == true?
                       │             │
                      no            yes
                       │             │
                       ▼             ▼
                     serve      start watcher
```

---

# 8. Empty Directory Seeding

If:

```yaml
web:
  root: /etc/rilgate/web
```

and the directory contains no regular user files:

```text
/etc/rilgate/web/
```

Rilgate initializes it with the embedded web bundle.

Result:

```text
/etc/rilgate/web/
├── templates/
│   ├── layouts/
│   ├── pages/
│   └── partials/
│
├── static/
│   ├── css/
│   ├── js/
│   └── images/
│
├── i18n/
│   └── en.json
│
└── .rilgate-web-version
```

This allows:

```bash
mkdir -p ./web
```

followed by:

```yaml
web:
  root: ./web
```

and the first Rilgate startup automatically gives the operator an editable web tree.

---

# 9. Definition of an Empty Web Root

"Empty" should mean:

```text
no meaningful regular files recursively
```

Directories by themselves do not count.

For example:

```text
web/
├── templates/
├── static/
└── i18n/
```

is still considered uninitialized.

Files that may be ignored:

```text
.DS_Store
Thumbs.db
```

But:

```text
templates/pages/challenge.html
```

means the web root is operator-managed and must not be automatically seeded.

---

# 10. Seeding Ownership Rule

The ownership model is:

```text
Embedded bundle
    = Rilgate-owned

Empty override root
    = Rilgate may initialize

Non-empty override root
    = operator-owned

Existing files
    = never overwritten automatically
```

Rilgate upgrades must not silently modify customized files.

---

# 11. Safe Seeding

Built-in files should be created safely:

```text
embedded bytes
      │
      ▼
temporary/exclusive file
      │
      ▼
write
      │
      ▼
flush
      │
      ▼
atomic rename
```

Never overwrite existing destinations.

Use exclusive creation semantics where possible:

```text
create_new = true
```

This also prevents two simultaneously starting Rilgate processes from overwriting each other's output.

---

# 12. Embedded Web Bundle

The binary should contain:

```text
EmbeddedWeb
├── templates
├── static
└── i18n
```

Conceptual API:

```rust
EmbeddedWeb::get("templates/pages/challenge.html")
EmbeddedWeb::get("static/css/rilgate.css")
EmbeddedWeb::get("i18n/en.json")
```

It should also support iteration:

```rust
EmbeddedWeb::entries()
```

so the same source can be used for:

- startup rendering;
- filesystem seeding;
- fallback;
- validation;
- future `rilgate web init`.

---

# 13. In-Memory Web Snapshot

The core runtime object should be an immutable snapshot:

```rust
pub struct WebSnapshot {
    templates: TemplateRuntime,
    translations: TranslationStore,
    assets: AssetIndex,
    generation: u64,
}
```

A snapshot represents one internally consistent version of the entire web UI.

Requests should use one snapshot from beginning to end.

They must never see:

```text
new template
+
old translation store
```

during a reload.

---

# 14. Snapshot Manager

The application owns a manager:

```rust
pub struct WebManager {
    current: ArcSwap<WebSnapshot>,
}
```

Conceptually:

```text
                    ┌─────────────────┐
requests ──────────►│ current snapshot│
                    └────────┬────────┘
                             │
                             │ atomic swap
                             ▼
                    ┌─────────────────┐
watcher/reload ────►│  new snapshot   │
                    └─────────────────┘
```

`ArcSwap`, `RwLock<Arc<_>>`, or an equivalent atomic shared-state mechanism can be used.

`ArcSwap` is preferable for a read-heavy HTTP workload because normal request lookup can avoid global write locking.

---

# 15. Request-Time Template Performance

Templates must not be loaded from disk on each request.

Startup:

```text
filesystem / embedded
        ↓
resolve templates
        ↓
parse + compile
        ↓
MiniJinja Environment
        ↓
store in WebSnapshot
```

Request:

```text
request
   ↓
Arc<WebSnapshot>
   ↓
env.get_template(...)
   ↓
render
```

No template filesystem read occurs in the normal request path.

---

# 16. Template Resolution

At snapshot build time:

```text
filesystem override
        ↓
embedded fallback
```

Example:

```text
pages/challenge.html
        │
        ▼
/etc/rilgate/web/templates/pages/challenge.html
        │
     exists?
     │    │
    yes   no
     │    │
     ▼    ▼
 override embedded
```

All resulting template source is then loaded into MiniJinja memory.

---

# 17. Partial Overrides

An operator can override only:

```text
templates/pages/challenge.html
static/css/rilgate.css
i18n/en.json
```

while keeping built-in:

```text
layouts/base.html
partials/footer.html
pages/error.html
```

No full copy is required.

---

# 18. MiniJinja Runtime

Recommended production setup:

```rust
let mut env = Environment::new();

env.set_auto_escape_callback(|name| {
    if name.ends_with(".html") {
        AutoEscape::Html
    } else {
        AutoEscape::None
    }
});

env.set_undefined_behavior(
    UndefinedBehavior::Strict
);

env.set_recursion_limit(64);
env.set_fuel(Some(50_000));
```

Templates are added to this environment while constructing the snapshot.

After publication, the environment is treated as immutable.

---

# 19. Template Security Model

Templates are:

```text
trusted operator configuration
```

Rendered data is:

```text
untrusted
```

Do not expose:

```text
ApplicationState
full Config
PoolConfig
PoolCredentials
SigningKeys
EnvironmentVariables
Database handles
Filesystem handles
Network clients
Raw HTTP Request objects
```

Expose only explicit public context types.

---

# 20. Template Context

Example:

```rust
pub struct TemplateContext {
    pub rilgate: PublicRilgateInfo,
    pub page: PageInfo,
    pub challenge: PublicChallengeInfo,
}
```

Possible fields:

```text
rilgate.name
rilgate.version
rilgate.base_path
rilgate.static_url

page.title
page.description

challenge.id
challenge.algorithm
challenge.difficulty
challenge.expires_at
challenge.token
```

Never expose:

```text
pool.password
private wallet data
signing_key
auth_signing_key
environment secrets
internal upstream credentials
```

---

# 21. Template Example

```jinja
{% extends "layouts/base.html" %}

{% block content %}

<main class="rilgate-challenge">

  <h1>
    {{ t("challenge.title") }}
  </h1>

  <p>
    {{ t("challenge.description") }}
  </p>

  <div
    id="rilgate-challenge"
    data-token="{{ challenge.token }}"
    data-algorithm="{{ challenge.algorithm }}"
    data-difficulty="{{ challenge.difficulty }}">
  </div>

  {% include "partials/pow-info.html" %}

</main>

{% endblock %}
```

---

# 22. i18n Directory

Translations use flat JSON keys:

```text
i18n/
├── en.json
├── vi.json
├── zh-CN.json
└── ja.json
```

Example:

```json
{
  "challenge.title": "Checking your browser",
  "challenge.description": "This site uses a small proof-of-work challenge to protect its resources.",

  "challenge.status.preparing": "Preparing challenge…",
  "challenge.status.solving": "Solving challenge…",
  "challenge.status.verifying": "Verifying proof…",
  "challenge.success": "Verification complete",

  "error.title": "Unable to continue",
  "error.retry": "Try again",

  "pow.disclosure.title": "Proof of Work",

  "footer.powered_by": "Protected by Rilgate"
}
```

---

# 23. Translation API

MiniJinja receives one translation function:

```jinja
{{ t("challenge.title") }}
```

Interpolation:

```jinja
{{ t(
    "challenge.expires_in",
    seconds=challenge.expires_in
) }}
```

JSON:

```json
{
  "challenge.expires_in": "Challenge expires in {seconds} seconds."
}
```

Named placeholders should be preferred over positional placeholders.

Use:

```text
{seconds}
{difficulty}
{algorithm}
```

instead of:

```text
%s
%d
{}
```

---

# 24. Flat-Key Convention

Recommended namespaces:

```text
common.*
challenge.*
pow.*
error.*
footer.*
```

Examples:

```text
common.retry
common.continue

challenge.title
challenge.description
challenge.status.preparing
challenge.status.solving
challenge.status.verifying

pow.disclosure.title
pow.disclosure.description

error.generic.title
error.generic.description

footer.powered_by
```

Avoid:

```text
title
message
text1
label2
```

---

# 25. Locale Negotiation

Recommended order:

```text
explicit configured/request locale
            ↓
Rilgate language cookie
            ↓
Accept-Language
            ↓
configured default locale
            ↓
en
```

Example request:

```http
Accept-Language: vi-VN,vi;q=0.9,en;q=0.8
```

Resolution:

```text
vi-VN
 ↓
vi
 ↓
en
```

Locale identifiers should be normalized as BCP 47 tags.

Examples:

```text
en-us  → en-US
pt-br  → pt-BR
zh-cn  → zh-CN
```

---

# 26. Translation Fallback

Translation lookup should occur at key level.

Example:

```text
requested locale = vi-VN
default locale   = en
```

Resolution:

```text
custom vi-VN
embedded vi-VN

custom vi
embedded vi

custom en
embedded en
```

If still missing:

```text
return translation key itself
```

and log a warning.

---

# 27. Key-Level Override

Filesystem translation files should not replace entire embedded locale files.

They overlay individual keys.

Embedded:

```json
{
  "challenge.title": "Checking your browser",
  "challenge.description": "Please wait.",
  "challenge.success": "Verification complete"
}
```

Custom:

```json
{
  "challenge.title": "Welcome to Example"
}
```

Effective translation:

```text
challenge.title
    → custom

challenge.description
    → embedded

challenge.success
    → embedded
```

This prevents Rilgate upgrades from requiring operators to copy newly introduced translation keys.

---

# 28. In-Memory Translation Store

Translations should also be loaded into memory when the snapshot is constructed.

Conceptual structure:

```rust
TranslationStore {
    locales: HashMap<Locale, TranslationMap>,
}
```

where:

```rust
type TranslationMap =
    HashMap<String, CompiledMessage>;
```

Request-time translation lookup should not read JSON files.

Flow:

```text
startup / reload
      ↓
read JSON
      ↓
parse
      ↓
normalize
      ↓
merge fallback layers
      ↓
store in WebSnapshot
```

Request:

```text
t("challenge.title")
      ↓
in-memory lookup
```

---

# 29. Missing Translation Behavior

Production:

```text
requested key
     ↓
locale fallback
     ↓
default fallback
     ↓
embedded English
     ↓
still missing
     ↓
return key
```

Example:

```text
foo.bar
```

Log:

```text
WARN missing translation
locale=vi
key=foo.bar
```

Do not fail the HTTP request merely because a translation is missing.

Development may enable:

```yaml
web:
  i18n:
    strict: true
```

to make missing keys easier to detect.

---

# 30. No HTML in Translation Strings

Translations should be treated as plain text.

Avoid:

```json
{
  "challenge.help": "Read <a href=\"...\">documentation</a>"
}
```

and:

```jinja
{{ t("challenge.help") | safe }}
```

Prefer:

```json
{
  "challenge.help.prefix": "Read our",
  "challenge.help.link": "documentation"
}
```

with markup owned by the template:

```jinja
<p>
  {{ t("challenge.help.prefix") }}

  <a href="{{ rilgate.docs_url }}">
    {{ t("challenge.help.link") }}
  </a>
</p>
```

Security boundary:

```text
translation = text
template    = markup
```

---

# 31. Client-Side Translation Data

Challenge JavaScript may need localized status strings.

Do not require an additional translation request for a few strings.

Render selected strings into the page:

```html
<script type="application/json" id="rilgate-i18n">
  {
    "preparing": "Preparing challenge…",
    "solving": "Solving challenge…",
    "verifying": "Verifying proof…"
  }
</script>
```

The actual JSON should be produced using a proper JSON serializer.

JavaScript:

```js
const translations = JSON.parse(
  document.getElementById("rilgate-i18n").textContent,
);
```

Advantages:

```text
same negotiated locale
no additional request
no browser-side locale negotiation
no full translation dictionary exposure
```

---

# 32. Static Assets

Static resources follow the same override model:

```text
filesystem override
        ↓
embedded fallback
```

URLs:

```text
/.well-known/rilgate/static/css/rilgate.css
/.well-known/rilgate/static/js/challenge.js
/.well-known/rilgate/static/images/...
```

Templates use:

```jinja
<link
  rel="stylesheet"
  href="{{ rilgate.static_url }}/css/rilgate.css">
```

instead of hardcoding the base path.

---

# 33. Static Asset Index

During snapshot construction, Rilgate should build metadata for static files:

```rust
AssetIndex {
    entries: HashMap<String, AssetMetadata>,
}
```

Possible metadata:

```text
content type
size
ETag
last modified
source
```

Whether actual large static asset bytes should be cached entirely in RAM may be configurable later.

For v1:

```text
templates       = always memory cached
translations    = always memory cached
asset metadata  = memory cached
asset bytes     = embedded or filesystem-backed
```

This avoids loading arbitrarily large operator-provided images into RAM.

Small assets may later gain an optional memory cache.

---

# 34. Watch Mode

Rilgate may optionally watch:

```text
web.root
```

for runtime changes.

Configuration:

```yaml
web:
  watch:
    enabled: false
    debounce: 300ms
```

Default:

```text
disabled
```

This avoids unnecessary filesystem watchers and gives production deployments deterministic behavior unless explicitly enabled.

---

# 35. Watch Scope

When enabled, watch recursively:

```text
web.root/templates/
web.root/static/
web.root/i18n/
```

Relevant events:

```text
create
modify
rename
remove
```

Changes to irrelevant files may be ignored.

Examples:

```text
.DS_Store
editor swap files
temporary files
.git/
```

---

# 36. Watch Implementation

A Rust implementation may use a filesystem notification library such as `notify`.

Conceptual flow:

```text
filesystem event
      │
      ▼
event filter
      │
      ▼
debounce
      │
      ▼
reload requested
      │
      ▼
build new snapshot
```

Do not reload directly for every filesystem event.

Many editors save files as:

```text
write temporary file
rename original
rename temporary
delete old file
```

which can produce several events for one logical edit.

---

# 37. Debounce

Example:

```yaml
web:
  watch:
    enabled: true
    debounce: 300ms
```

Events:

```text
0 ms    modify challenge.html
10 ms   rename challenge.html
35 ms   modify en.json
```

should result in approximately:

```text
one reload
```

after the event burst settles.

---

# 38. Hot Reload Strategy

Never mutate the currently active MiniJinja environment in place.

Instead:

```text
change detected
      │
      ▼
build completely new WebSnapshot
      │
      ├── load templates
      ├── compile MiniJinja
      ├── load i18n
      ├── validate JSON
      ├── build asset index
      └── validate configuration
      │
      ▼
successful?
   │        │
  yes       no
   │        │
   ▼        ▼
atomic    log error
swap      keep old snapshot
```

This is critical for availability.

---

# 39. Failed Reload Behavior

Suppose the operator saves invalid MiniJinja syntax:

```jinja
{% if something
```

Rilgate should:

```text
detect change
     ↓
attempt build
     ↓
template parse fails
     ↓
log error
     ↓
discard candidate snapshot
     ↓
continue serving previous snapshot
```

Do not replace the working site with:

```text
500 Internal Server Error
```

because of an incomplete edit.

The next valid filesystem change triggers another reload attempt.

---

# 40. Invalid Translation Reload

Same principle:

```json
{
  "challenge.title": "Hello",
```

Invalid JSON must:

```text
fail candidate snapshot
keep previous snapshot
log exact file + parser error
```

---

# 41. Atomic Snapshot Publication

Snapshot changes should be atomic:

```text
generation 42
     │
     │ requests
     ▼
┌──────────────┐
│ Snapshot #42 │
└──────────────┘

reload succeeds
     │
     ▼

┌──────────────┐
│ Snapshot #43 │
└──────────────┘
```

New requests receive generation `43`.

Requests already holding an `Arc<Snapshot #42>` can finish normally.

When no references remain, old generation `42` is automatically freed.

---

# 42. Consistency During Reload

Never produce:

```text
request begins
template from generation 41
translation from generation 42
asset config from generation 43
```

The request acquires exactly one:

```rust
Arc<WebSnapshot>
```

and uses it throughout rendering.

---

# 43. Watch Reload Logging

Useful logs:

```text
INFO web directory change detected
paths=["templates/pages/challenge.html"]
```

```text
INFO web snapshot reloaded
generation=17
templates=8
locales=4
duration_ms=12
```

On error:

```text
ERROR web snapshot reload failed
file="templates/pages/challenge.html"
error="unexpected end of template"
generation_kept=16
```

Avoid logging template contents.

---

# 44. Manual Reload

Even with watch disabled, Rilgate should eventually support manual reload.

Possible mechanisms:

```text
SIGHUP
```

or administrative command/API.

Example:

```bash
kill -HUP $(pidof rilgate)
```

Behavior:

```text
SIGHUP
  ↓
reload config if supported
  ↓
rebuild web snapshot
  ↓
atomic swap
```

This provides deterministic production updates without requiring a permanent watcher.

---

# 45. Optional Future CLI

Possible commands:

```bash
rilgate web init ./web
```

Initialize a custom web directory.

```bash
rilgate web validate ./web
```

Validate:

```text
templates
translations
paths
includes
extends
```

without running the server.

```bash
rilgate web diff ./web
```

Compare custom files against current embedded defaults.

These should reuse the same loader and validator used by normal startup.

---

# 46. Path Security

All filesystem paths must remain underneath the configured root.

Reject:

```text
..
absolute paths
NUL bytes
Windows drive prefixes
unexpected path prefixes
symlink escapes
```

Template names should use logical paths:

```text
layouts/base.html
pages/challenge.html
partials/footer.html
```

not arbitrary filesystem paths.

---

# 47. Template Loader Restrictions

Valid:

```text
layouts/*
pages/*
partials/*
```

Invalid:

```text
../../secret
/etc/passwd
C:\secret
```

After canonicalization, the resolved file must still be a descendant of:

```text
web.root/templates
```

---

# 48. Symlink Policy

Recommended production policy:

```text
symlinks inside web.root may be disabled
```

or, if allowed:

```text
resolved target must remain inside canonical web.root
```

Never allow:

```text
web/templates/foo.html
  → /etc/shadow
```

to escape the root.

---

# 49. Watcher and Symlinks

Watcher behavior should follow the same canonical-path policy.

A filesystem event must not cause Rilgate to read content outside the configured root.

---

# 50. Container Deployment

Development:

```yaml
services:
  rilgate:
    volumes:
      - ./web:/etc/rilgate/web
```

with:

```yaml
web:
  root: /etc/rilgate/web

  watch:
    enabled: true
```

Editing:

```text
./web/templates/pages/challenge.html
```

will automatically rebuild and publish a new snapshot.

---

# 51. Production Deployment

Recommended:

```yaml
services:
  rilgate:
    volumes:
      - ./web:/etc/rilgate/web:ro
```

and:

```yaml
web:
  root: /etc/rilgate/web

  watch:
    enabled: false
```

Templates and translations are loaded once into RAM at startup.

Requests then render entirely from the in-memory snapshot.

---

# 52. Seeding and Read-Only Mounts

Automatic first-run seeding requires write permission.

Therefore either:

### Initialization

```yaml
volumes:
  - ./web:/etc/rilgate/web
```

Start once, let Rilgate populate the directory.

Then switch to:

```yaml
volumes:
  - ./web:/etc/rilgate/web:ro
```

### Or CLI initialization

Future:

```bash
rilgate web init ./web
```

Then mount read-only from the first production start.

---

# 53. Read-Only Root with Empty Directory

If:

```yaml
web:
  root: /etc/rilgate/web
```

is configured,

the directory is empty,

and Rilgate cannot seed because it is read-only:

Rilgate should report a clear startup warning/error according to configuration.

Recommended behavior:

```text
seed requested
+
root empty
+
root not writable
     ↓
warning
     ↓
continue using embedded resources
```

because embedded resources are sufficient to operate safely.

Do not crash solely because customization storage is unavailable.

---

# 54. Version Tracking

Seed:

```text
.rilgate-web-version
```

Example:

```text
1
```

or:

```text
0.2.0
```

It is informational.

A newer Rilgate binary must never automatically overwrite older customized files.

---

# 55. Updating Built-In Resources

Suppose Rilgate v1 contains:

```text
challenge.title
challenge.description
```

and v2 adds:

```text
challenge.status.waiting
```

An operator's old custom `en.json` does not have to be modified.

Lookup:

```text
custom en
   ↓ missing
embedded en from current binary
   ↓ found
```

This is why translation overrides should be key-level overlays rather than complete replacements.

---

# 56. Module Responsibilities

## `embedded.rs`

Responsible for:

```text
embedded templates
embedded translations
embedded static assets
resource enumeration
```

---

## `seed.rs`

Responsible for:

```text
detect uninitialized root
create directories
seed built-ins
exclusive file creation
version marker
never overwrite user files
```

---

## `loader.rs`

Responsible for:

```text
filesystem override lookup
canonical path enforcement
embedded fallback
template source collection
```

---

## `snapshot.rs`

Responsible for:

```text
immutable WebSnapshot
generation number
template runtime
translation store
asset index
```

---

## `manager.rs`

Responsible for:

```text
current snapshot ownership
atomic publication
manual reload
reload coordination
```

---

## `watcher.rs`

Responsible for:

```text
filesystem watch
event filtering
debouncing
triggering reload
```

It must not directly mutate the active snapshot.

---

## `renderer.rs`

Responsible for:

```text
MiniJinja configuration
template compilation
HTML rendering
t() registration
autoescaping
fuel
recursion limits
strict undefined handling
```

---

## `context.rs`

Responsible for:

```text
public template context
conversion from internal state
ensuring secrets never reach templates
```

---

## `i18n/locale.rs`

Responsible for:

```text
BCP 47 parsing
normalization
Accept-Language negotiation
parent-locale fallback
```

---

## `i18n/loader.rs`

Responsible for:

```text
embedded JSON loading
filesystem JSON loading
validation
key-level overlay
```

---

## `i18n/store.rs`

Responsible for:

```text
immutable in-memory translation maps
locale fallback chains
compiled/interpolated messages
```

---

## `i18n/translator.rs`

Responsible for:

```text
t(key)
t(key, args)
missing-key handling
translation lookup
```

---

## `assets.rs`

Responsible for:

```text
static asset routing
filesystem override
embedded fallback
content type
ETag
cache headers
asset metadata
```

---

# 57. Runtime Request Flow

```text
HTTP Request
     │
     ▼
WebManager.current()
     │
     ▼
Arc<WebSnapshot>
     │
     ├───────────────┐
     │               │
     ▼               ▼
Locale          Template context
Negotiation
     │               │
     └───────┬───────┘
             ▼
         MiniJinja
             │
             │ t(...)
             ▼
     TranslationStore
             │
             ▼
       Rendered HTML
             │
             ▼
          Response
```

No normal template or translation filesystem access occurs here.

---

# 58. Watch Reload Flow

```text
         Filesystem
             │
             ▼
          Watcher
             │
             ▼
        event filter
             │
             ▼
          debounce
             │
             ▼
       Reload Request
             │
             ▼
     ┌─────────────────┐
     │ Snapshot Builder│
     └────────┬────────┘
              │
      ┌───────┼──────────┐
      ▼       ▼          ▼
 templates   i18n       assets
      │       │          │
      └───────┼──────────┘
              ▼
           validate
              │
          ┌───┴───┐
          │       │
        valid   invalid
          │       │
          ▼       ▼
      atomic    discard
       swap       │
          │       ▼
          │     log
          ▼
     generation +1
```

---

# 59. Performance Characteristics

Normal request path:

```text
filesystem access       = none for templates/i18n
template parsing        = none
JSON parsing            = none
locale tables           = RAM
template bytecode/state = RAM
```

Only:

```text
snapshot pointer lookup
locale negotiation
hash-map translation lookup
template render
```

are required.

---

# 60. Reload Cost

Reload may be more expensive because Rilgate intentionally reconstructs the entire consistent web state.

This is preferable to incremental mutation because reload events should be rare compared with HTTP requests.

Design preference:

```text
expensive and safe reload
+
cheap request path
```

rather than:

```text
cheap reload
+
locking / filesystem checks on every request
```

---

# 61. Concurrency Model

Multiple HTTP workers:

```text
worker 1 ─┐
worker 2 ─┼──► Arc<WebSnapshot #12>
worker 3 ─┘
```

Watcher builds:

```text
WebSnapshot #13
```

independently.

After validation:

```text
current.store(snapshot_13)
```

New requests use `#13`.

Existing requests may continue using `#12`.

No global rendering pause is required.

---

# 62. Reload Storm Protection

Watcher should coalesce reload requests.

If another event arrives while a reload is already running:

```text
reload running
+
new change
     ↓
mark pending
```

After reload:

```text
pending?
  │
 yes
  ↓
run one additional reload
```

Do not spawn an unlimited number of concurrent snapshot builders.

Recommended invariant:

```text
max concurrent web reloads = 1
```

---

# 63. Startup Validation

Unlike hot reload, startup has no previous snapshot.

Therefore invalid operator templates should produce a clear result.

Recommended strategy:

```text
custom override invalid
       │
       ▼
log exact error
       │
       ▼
fallback to embedded resource where possible
```

For errors affecting the complete custom snapshot, optionally fall back to a pure embedded snapshot.

This ensures Rilgate can still display a challenge/error page instead of failing completely due to presentation customization.

---

# 64. Health / Diagnostics

Expose basic diagnostic state internally:

```text
web generation
last successful reload
last reload error
active locales
template count
watch enabled
```

Example logs/metrics:

```text
rilgate_web_generation 12
rilgate_web_reload_total 4
rilgate_web_reload_failures_total 1
```

Do not expose template source or translation contents through diagnostics.

---

# 65. Recommended Production Security Boundary

```text
                   TRUSTED
                      │
                      ▼
            operator web directory
                      │
                      ▼
              Snapshot Builder
                      │
                      ▼
                 MiniJinja
                      ▲
                      │
              whitelisted data
                      │
                   UNTRUSTED
                      ▲
                      │
                 HTTP request
```

The directory itself is considered administrator-controlled.

Remote users must never be allowed to upload or edit templates through the public Rilgate web API.

---

# 66. Recommended v1 Configuration

A practical production configuration:

```yaml
web:
  base_path: /.well-known/rilgate

  root: /etc/rilgate/web

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

    supported_locales:
      - en
```

Development:

```yaml
web:
  root: ./web

  watch:
    enabled: true

  i18n:
    default_locale: en

    supported_locales:
      - en
      - vi
```

---

# 67. Final Architecture

```text
                         ┌──────────────────────────┐
                         │      Rilgate Binary      │
                         │                          │
                         │ Embedded Templates       │
                         │ Embedded i18n             │
                         │ Embedded Static Assets   │
                         └────────────┬─────────────┘
                                      │
                           fallback / seed
                                      │
                                      ▼
                 ┌──────────────────────────────────┐
                 │          web.root                │
                 │                                  │
                 │ templates/                       │
                 │ static/                          │
                 │ i18n/                            │
                 └────────────────┬─────────────────┘
                                  │
                        startup / filesystem event
                                  │
                                  ▼
                    ┌────────────────────────┐
                    │    Snapshot Builder    │
                    │                        │
                    │ resolve overrides      │
                    │ parse templates        │
                    │ compile MiniJinja      │
                    │ parse translations     │
                    │ merge translations     │
                    │ index static assets    │
                    │ validate everything    │
                    └───────────┬────────────┘
                                │
                                ▼
                    ┌────────────────────────┐
                    │      WebSnapshot       │
                    │                        │
                    │ MiniJinja Environment  │
                    │ Translation Store      │
                    │ Asset Index            │
                    │ Generation             │
                    └───────────┬────────────┘
                                │
                           atomic swap
                                │
                                ▼
                    ┌────────────────────────┐
                    │      WebManager        │
                    │                        │
                    │ current Arc<Snapshot>  │
                    └───────────┬────────────┘
                                │
                          every request
                                │
                                ▼
                  ┌─────────────────────────────┐
                  │     Locale Negotiation      │
                  └──────────────┬──────────────┘
                                 │
                                 ▼
                        MiniJinja Render
                                 │
                                 ▼
                              HTML
```

Optional watch path:

```text
web.root change
      │
      ▼
filesystem watcher
      │
      ▼
debounce
      │
      ▼
build candidate snapshot
      │
      ├── invalid
      │      ↓
      │   discard
      │   keep current
      │
      └── valid
             ↓
         atomic swap
```

---

# 68. Core Design Rules

```text
1. Embedded web resources are always available.

2. An explicitly configured empty web.root may be initialized
   automatically from embedded resources.

3. Once web.root contains user files, it is operator-owned.

4. Rilgate never automatically overwrites customized files.

5. Filesystem templates are resolved and loaded into RAM.

6. Translation JSON is parsed and stored in RAM.

7. HTTP requests do not read template/i18n files from disk.

8. Filesystem watch is optional and disabled by default.

9. Hot reload builds a completely new immutable snapshot.

10. A snapshot is published only after full validation succeeds.

11. Reload errors leave the previous known-good snapshot active.

12. Translation overrides happen per key, not per file.

13. HTML belongs to templates; translation values remain text.

14. Templates receive only explicitly whitelisted public data.

15. All filesystem access is constrained to canonical web.root.

16. Active requests keep their existing snapshot during reload.

17. At most one web snapshot reload runs concurrently.

18. Performance-critical reads happen from memory.
```

This provides Rilgate with a customizable web system that is fast enough for a gateway hot path, safe against partially written templates during development, simple to customize through ordinary files, and capable of live updates without coupling request rendering to filesystem I/O.
