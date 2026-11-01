# Web compiler and response encoding architecture

Status: proposed.

This proposal extends the web runtime with inline CSS, shell-mode templates, HTML
minification, HTTP compression, reload hardening and, when operators need it, a
runtime compiler for operator JavaScript. It builds on the existing build-time
ES5 pipeline and keeps its central constraint: no JavaScript compiler runs in the
gateway for embedded resources, and the deployment remains a Rust binary without
Node, npm or network package resolution at runtime.

Rilgate is a proof-of-work reverse proxy: every human visitor must pass the
challenge page, so a browser that cannot run it is locked out of the protected
site. Browser reach is therefore the primary constraint, and every proposal here
preserves the current browser floor.

## 1. Current implementation and implications

### Browser floor

The embedded fast challenge targets IE11, Safari and iOS 9+, Chrome 49
(including Windows XP), Firefox 52 ESR, Android 4.4 WebView, Samsung Internet 4
and KaiOS 2.5 ([configuration](../configuration.md#challenge-browser-support)).

| Requirement | Current handling |
| --- | --- |
| ES2015+ syntax | Lowered to ES5 at build time |
| `Promise` | `web/static/js/challenge/promise.js`, installed only when missing |
| `fetch`, `AbortController`, `AbortSignal.timeout` | XMLHttpRequest fallback with its own timeout |
| `TextEncoder` | Local UTF-8 encoder in the fast worker |
| `Worker` | Time-sliced hashing on the page, with progress and cancellation |
| JavaScript disabled | Localized `<noscript>` message |

RandomX requires ES modules, workers, BigInt, WebAssembly SIMD and about 256 MiB
for its cache. The classic adapter checks these before loading anything and shows
the localized retry controls on an incapable browser; the client never switches a
RandomX challenge to the fast algorithm.

### Build-time ES5 pipeline

```text
web/static/js/**                readable sources (seeded, diffed, edited)
        |
cargo xtask build-web           SWC, ES5 target, inline helpers, strict IIFE per file
        |
crates/rilgate-web/generated/   ignored by Git, embedded into the binary
        |
WebSnapshot::build              joins members into js/challenge.bundle.js
        |
challenge page                  <script defer src=".../js/challenge.bundle.js">
```

The [development guide](../development.md#browser-resource-generation) describes
the workflow.

| Area | Repository evidence | Implication |
| --- | --- | --- |
| Compiler | `crates/xtask/src/web.rs` compiles each startup member with SWC and validates every output and the bundle with an ES5-only parser | Build-time preparation has a home; new embedded transforms belong in this pass. |
| Generated output | `crates/rilgate-web/generated/` with `manifest.json` (order, source SHA-256, compiler identity), built by `scripts/build.sh`, `scripts/test.sh`, the container build and CI | Generated artifacts are build outputs with one source of truth in `web/`. |
| Assembly | `crates/rilgate-web/src/resources/bundle.rs` rejects stale outputs and uses an ES5 member when the resolved bytes match the embedded digest, otherwise the operator's bytes as authored | Seeded but unedited directories keep the floor; edited JavaScript does not. |
| WASM revision | `crates/rilgate-web/src/runtime/snapshot.rs` substitutes `__WASM_REVISION__` after assembly in the solver, bundle and fast worker | The placeholder never passes through the compiler. |
| RandomX loading | `web/static/js/solvers/randomx/adapter.js` inserts `loader.js` as a module script after capability checks | The ES5 bundle contains no `import()` and needs no `eval`. |
| Seeding | `crates/rilgate-web/src/resources/seed.rs` copies `web/` byte for byte | Seeded and diffed files stay readable sources. |
| Configuration | `crates/rilgate-config/src/assembly/resolve/web.rs` builds `WebConfig` without `pow.browser.solver_module_path` | A runtime compiler cannot identify the configured solver module. |
| Stylesheet | `web/templates/layouts/base.html.jinja` links `rilgate.stylesheet_url`; `web/static/css/rilgate.css` is 379 bytes | Inlining removes one render-blocking request. |
| Shell cache | `snapshot.rs` builds per-locale challenge shells only when no template differs from the embedded set | Any template override disables shell caching for every page. |
| Asset HTTP | `crates/rilgate-web/src/assets/response.rs` serves strong ETags, HEAD, 304 and `max-age=0, must-revalidate`; no content encoding | Encoding must be negotiated before validators are evaluated. |
| HEAD bodies | `crates/rilgate-gateway/src/proxy/gate.rs` empties HEAD bodies before the local writer in `crates/rilgate-gateway/src/transport/server.rs` computes lengths | Encoded HTML must be prepared before body suppression. |
| Reload | `crates/rilgate-web/src/runtime/manager.rs` builds and publishes inside one `reload()` call, and startup builds the embedded set two or three times | Expensive preparation needs a publication guard and a shared artifact cache. |
| Capture | `snapshot.rs` reads files one by one and `crates/rilgate-web/src/assets/metadata.rs` freezes each override independently | A reload can combine files from different moments. |

## 2. Decisions

1. Keep the browser floor above as a requirement for every embedded resource.
2. Prepare embedded resources at build time in the `cargo xtask build-web` pass.
   A broken embedded resource fails the build, and startup cost and binary size
   stay unchanged.
3. Keep one classic ES5 bundle for every browser. Differential
   `module`/`nomodule` serving is rejected: Safari 10.1 executes both bundles and
   older Edge fetches module scripts repeatedly.
4. Inline small stylesheets through a template helper and keep external mode.
5. Cache rendered shells through an explicit restricted template contract and keep
   dynamic MiniJinja rendering for request-dependent templates.
6. Precompress static assets. Do not compress challenge HTML per request in the
   first release.
7. Keep stable asset URLs with revalidation until an artifact lifecycle exists.
8. Build a runtime compiler for operator JavaScript only when operators need
   edited scripts to keep the floor, and only after reload hardening.

## 3. CSS preparation and inline policy

- Add a `web_stylesheet()` template helper that emits an inline style element or
  the existing link, and use it in the default layout. `rilgate.stylesheet_url`
  stays available to customized templates.
- Minify the embedded stylesheet in the build-time pass with Lightning CSS.
  Invalid embedded CSS fails the build.
- Inline an operator stylesheet only when it is at most 4 KiB, has no `@import`,
  and contains no case-insensitive closing style delimiter. Otherwise keep it
  external and report the reason in diagnostics. HTML entity escaping is not an
  equivalent transformation inside style raw text.
- Deduplicate the prepared stylesheet across locales.

## 4. Template and HTML caching contract

| Cache | Contents | Lifetime |
| --- | --- | --- |
| Compiled template | MiniJinja program | Snapshot |
| Prepared shell | Locale-specific HTML before and after the bootstrap | Snapshot |
| Prepared asset | Minified and encoded representations | Content and settings identity |
| Full challenge response | Request-specific HTML containing the token | Request only |

### Page modes

- **Customization scope.** A page counts as customized when the page or any
  layout, partial or macro it reaches differs from the embedded set. When the
  dependency set cannot be determined statically, the page counts as customized.
- **`shell`.** Used by unchanged pages, and by customized pages the operator opts
  in by page path. The shell context contains `rilgate`, `locale`, `page`
  metadata including `page.title`, the `t` translation function,
  `client_i18n_json`, asset helpers, and exactly one bootstrap insertion point.
  Request-dependent values (`challenge`, token, difficulty, expiry) are excluded,
  and shell rendering uses strict undefined handling so a shell that reads them
  fails validation. Requests only serialize escaped bootstrap values and join the
  shell.
- **`dynamic`.** Default for customized pages. Executes the cached MiniJinja
  program with public request data on each request, which supports conditions on
  difficulty, expiry and other challenge data.

This replaces the rule that any changed template disables every challenge shell,
without a general-purpose partial evaluator for Jinja.

### HTML minification

```text
shell:   render with sentinel -> inject stylesheet -> minify -> verify one sentinel -> split -> store
request: insert escaped bootstrap -> send

dynamic: render request context -> send
```

Minify embedded templates in the build-time pass and opted-in shell pages at
snapshot build, with a conservative parser-based profile that preserves raw-text
elements, JSON script values and significant whitespace. Disable the minifier's
own JavaScript and CSS optimizers. Dynamic pages are not minified per request,
and JSON response templates never pass through the HTML minifier.

## 5. HTTP encoding and validators

### Static assets

- Precompress the challenge bundle, fast worker, RandomX solver, loader, worker
  and WASM with gzip and Brotli at snapshot build, keeping a variant only when it
  is smaller than identity. `application/wasm` keeps its type.
- Follow [RFC 9110 sections 8.4, 8.8, 12.5.3 and 15.4.5](https://www.rfc-editor.org/rfc/rfc9110.html):
  - Parse `Accept-Encoding` with weights, wildcard and `q=0`.
  - Select a representation before evaluating `If-None-Match`, with a strong
    ETag derived from that representation's bytes.
  - Merge `Accept-Encoding` into existing `Vary` values without dropping locale
    negotiation fields or duplicating tokens.
  - Keep length, `Content-Encoding` and validators consistent for GET, HEAD and
    304.
  - When no acceptable encoding exists, send identity. Section 12.5.3 allows
    disregarding the preference, and a 406 only breaks unusual clients.
- Leave proxied upstream responses, WebSocket traffic and API JSON unchanged.

### Challenge HTML

Per-request compression of challenge HTML is out of the first release. The page
saves about 1-2 KiB under gzip, and it is served mostly to suspected automated
clients that choose their own `Accept-Encoding`, so compression would spend CPU
on the requests the gateway least wants to serve. Revisit it with the
measurements in section 9.

If HTML encoding is added later, prepare the encoded representation before HEAD
bodies are discarded, through one path shared by challenge, denied, iframe and
gateway error pages. Challenge HTML stays `Cache-Control: no-store`.

## 6. Reload hardening and resource bounds

Required before any preparation that is expensive at reload time.

- **Capture consistency.** Read a revision manifest with expected source digests
  and retry the capture on mismatch, or document that overrides must be updated
  in compatible order. Capture consistency is separate from publication
  atomicity.
- **Publication guard.** Separate candidate construction from publication and
  check deadlines under the reload lock immediately before `current.store()`, so
  an abandoned build cannot publish late. A blocking build cannot be cancelled by
  dropping its handle.
- **Shared artifact cache.** Share prepared stylesheets, shells and encoded
  variants across the fallback, active and invalid-override startup builds, keyed
  by content and settings. Active snapshots keep their references independently
  of cache eviction.
- **Bounds.** Limit rendered template output bytes during rendering, minifier
  input size, captured override bytes and files, and temporary storage for
  backing files and encoded variants across active and retired snapshots. When
  storage is exhausted, omit optional encodings before rejecting a reload.
- **Diagnostics.** Track preparation stage duration, input and output sizes,
  cache hits, page mode counts, failure stage and encoding skips, without
  exposing template source or resource contents in public health responses.

## 7. Asset identity

Stable URLs keep `max-age=0, must-revalidate`, so a page and its later asset
requests can span reload generations. Content-addressed immutable URLs require
retained dependency closures, bounded retention, defined missing-version
behavior, restart behavior and consistency across replicas. This matters most for
RandomX assets, which load after consent. Never serve different bytes at an old
digest URL.

## 8. Runtime compiler for operator JavaScript

Operator-edited startup members are served as authored, without the floor. A
runtime compiler keeps the floor for them and has these preconditions:

- Run the `cargo xtask build-web` pipeline inside the snapshot build, sharing the
  artifact cache and publication guard from section 6, with bounded CPU time and
  output size.
- Pass the configured RandomX solver path into `WebConfig`, and through
  `crates/rilgate-gateway/src/runtime/app.rs` and `crates/rilgate/src/cli/web.rs`,
  so the solver module is distinguished from classic scripts.
- Keep the source contract the build pass enforces: startup members are classic
  scripts wrapped in strict IIFEs, with no imports, exports, top-level await or
  `require`. Report anything else with path and location, and keep serving the
  previous snapshot.

Two related decisions belong to server policy and are outside this proposal: a
server-authorized fast challenge for browsers without RandomX capability, since
the capability report comes from the client, and a no-JavaScript challenge, which
trades away most of the proof's cost.

## 9. Verification

| Area | Required evidence |
| --- | --- |
| CSS | Inline threshold, `@import` and delimiter handling, external fallback, invalid embedded CSS failing the build |
| Templates | Customization scope through layouts and partials, strict shell context, opt-in shell pages, dynamic difficulty and expiry branches, token isolation |
| HTML | Sentinel survives minification; DOM, text, spacing and escaped script data stay equivalent |
| HTTP | gzip and Brotli decoding, `q=0`, wildcard, identity fallback, GET/HEAD parity, encoded ETags, 304, `Vary` |
| Reload | Mixed captures retried or rejected, late builds never publish, artifact reuse across startup builds, storage limits |
| Browser floor | Existing ES5 syntax gate and API-removal tests keep passing with every change |

Measure before enabling each optimization by default: cold startup, reload, RSS,
binary size, HTML and static transfer bytes, startup requests and time until fast
work starts, using `crates/rilgate-web/examples/asset-throughput.rs` and the
gateway perf scripts.

## 10. Delivery sequence

1. **CSS and page preparation:** `web_stylesheet()`, customization scope, shell
   mode with the explicit context, build-time HTML minification.
2. **Static encoding:** precompressed variants, encoded validators and the shared
   artifact cache.
3. **Reload hardening:** capture consistency, publication guard and bounds.
4. **Runtime compiler for operator JavaScript:** only when operators need it,
   after step 3.

Each optimization is enabled by default only after its correctness and
performance gates pass.
