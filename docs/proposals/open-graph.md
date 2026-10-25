# Open Graph passthrough

## Scope

Link preview clients (chat apps, social networks, fediverse servers) fetch a
URL once and read its `<meta>` tags. They cannot complete a proof-of-work
challenge, so a challenged URL shows the challenge page title in the preview.
Open Graph passthrough fetches the preview tags of the requested page from the
upstream, caches them, and renders them in the `<head>` of the challenge page.
The upstream content stays behind the challenge; only an allowlisted set of
public metadata tags is exposed.

## Configuration

```yaml
site:
  open_graph:
    enabled: false
    ttl: 24h
    negative_ttl: 5m
    timeout: 2s
    max_entries: 10000
    include_query: false
    request_headers:
      user-agent: Rilgate-OpenGraph/1.0
      x-forwarded-host: "{host}"
      x-forwarded-proto: https
    tags:
      prefixes: ["og:", "twitter:", "fediverse:"]
      names: [description, keywords, author]
    when:
      all:
        - host: {in: [example.com, www.example.com]}
        - not: {path: {prefix: /admin/}}
    override: {}
```

- `enabled` defaults to `false`. When it is `false`, the gateway sends no
  prefetch request.
- `ttl` is the cache lifetime of a successful lookup, also when the page has no
  allowlisted tags. It is bounded to `1m..=7d`.
- `negative_ttl` is the cache lifetime of a failed lookup (see
  [Fetch](#fetch)). It is bounded to `10s..=ttl`.
- `timeout` is the longest time a challenge response waits for a lookup. It is
  bounded to `100ms..=10s`. When it expires, the page renders without tags and
  the fetch continues in the background, so the next request gets the result.
- `max_entries` bounds the cache size, `1..=1_000_000`.
- `include_query` adds the query string to the upstream URL and the cache key.
  With the default `false`, all query variants of one path share one lookup.
- `request_headers` is a map from header name to value. The gateway sends
  exactly these headers on each prefetch request. A configured map replaces the
  default map; it does not merge with it. An empty map sends no extra header.
  - The value `{host}` placeholder expands to the request host, the same host
    the gateway uses for challenge binding. No other placeholder exists; a `{`
    followed by anything other than `host}` is a configuration error.
  - Header names are case-insensitive and stored lowercase. At most 16 headers.
    A value is at most 1024 bytes after expansion and must be a valid HTTP
    header value; an expanded value that is invalid skips that header.
  - `host`, `content-length`, `transfer-encoding`, `connection`, `upgrade`,
    `te`, `trailer`, `keep-alive` and `proxy-connection` are rejected. The Host
    header follows `server.upstream.preserve_host`, like proxied requests.
- `tags` is the allowlist of tag names the gateway keeps (see
  [Tag extraction](#tag-extraction)). `prefixes` matches names that start with
  an entry; `names` matches names exactly. Each list given replaces its
  default; a list left out keeps its default. Entries are 1 to 64 bytes from `a-z`, `0-9`, `:`, `_`, `.`
  and `-`, stored lowercase, without duplicates. At most 16 prefixes and 32
  names. Both lists empty is a configuration error.
- `when` is an optional condition that selects the requests that look up tags
  (see [Request conditions](#request-conditions)). Without `when`, every
  request described in [When a lookup happens](#when-a-lookup-happens) looks
  up tags.
- `override` is a map from tag name to content. When it is not empty, the
  gateway renders these tags and never fetches. Each name must match `tags`.
  At most 32 entries, each content at most 1024 bytes.

Client request headers are never forwarded. A prefetch request carries no
client cookie, authorization or other client header, so pages that the upstream
personalises or protects with its own authentication return only their
anonymous metadata.

## When a lookup happens

A lookup runs only for a challenge HTML response to a `GET` request to a
protected URL. `HEAD`, JSON challenge responses, the embedded challenge page
under `site.base_path`, denied pages, error pages and allowed requests never
look up tags. The lookup runs after the policy decision and the cookie check,
so authenticated clients never pay for it. When `when` is set, a request looks
up tags only when the condition matches; otherwise the page renders without
tags. `override` tags follow the same selection.

## Request conditions

`when` uses the condition grammar of policy rules (`docs/policy.md`): `all`,
`any`, `not`, `path`, `method`, `user_agent`, `headers` and `client_ip`, with
the same string matchers (`exact`, `prefix`, `contains`, `in`, `regex`) and
the same IP sets from `policy.ip_sets`. One grammar serves both, so request
information is read the same way in policy rules and here. Include and exclude
rules are expressed with `any`, `all` and `not`.

- `host` is a new condition in the shared grammar, available to policy rules
  too. It matches the request host the gateway uses for challenge binding: the
  `Host` header, or the URI authority when the header is absent, in ASCII
  lowercase and with the port when the request carries one. Policy rules get
  it from the same field of the policy request.
- `score` is rejected in `when`, because no score exists outside policy
  evaluation. The error carries the path, e.g.
  `site.open_graph.when.all[1].score: score is only available in policy rules`.
- A condition with an unknown result, for example an IP set that has not
  loaded yet, does not look up tags.

## Fetch

- The upstream URL is built by the same path mapping as proxied requests
  (`ProxyService::request_url`), without the query unless `include_query` is
  set. Upstream TLS certificates are verified against the upstream host name,
  like proxied requests.
- Method `GET` with `accept: text/html,application/xhtml+xml`.
- Redirects are followed only when the target keeps the scheme, host and port
  of the upstream URL, at most 3 times.
- The whole fetch is bounded by 10 s.
- Only status `200` with media type `text/html` or `application/xhtml+xml` is
  parsed. A response with a `content-encoding` other than `identity` is a
  failure, because the request asks for no encoding. The body read stops after 512 KiB, at `</head>`, or at the first
  `<body>` element, whichever comes first. A page that is cut by the size limit
  keeps the tags read before the limit.
- At most 16 fetches run at the same time. When no slot is free, the lookup
  returns no tags immediately and caches nothing.
- Concurrent lookups for one cache key share one fetch.

A lookup is a failure, cached for `negative_ttl`, when the connection fails,
the fetch times out, the status is not `200`, the media type is not HTML, or
the redirect limit or origin rule stops it. A failure renders the page without
tags and never fails the challenge response.

## Cache key

The key is the request host, the upstream path and, with `include_query`, the
query. The host is part of the key because the upstream can dispatch on the
Host or `x-forwarded-host` header. The cache is in memory, per process, with
size-based eviction at `max_entries` and per-entry expiry.

## Tag extraction

The parser is incremental and reads only `<meta>` elements inside `<head>`. It
skips comments and the content of `script`, `style`, `title`, `textarea`,
`noscript` and `template`, so markup inside them is never metadata. An
element is kept when its `property` or `name` attribute matches an entry of
`tags.prefixes` or `tags.names`. With the defaults, these are the `og:`,
`twitter:` and `fediverse:` prefixes and the `description`, `keywords` and
`author` names.

Names are compared in ASCII lowercase. A name with a byte outside `a-z`, `0-9`,
`:`, `_`, `.` and `-` is skipped. Content drops control characters and turns
tabs and line breaks into spaces. The document order and repeated names
(for example several `og:image` tags) are kept. At most 32 tags, each name at
most 64 bytes and each content at most 1024 bytes; longer content is truncated
at a character boundary, and tags after the 32nd are ignored.

## Rendering

`PageInfo` gains `open_graph`, a list of `{attribute, name, content}`. A
fetched tag keeps the attribute it has in the upstream page (`property` when
the element has a `property` attribute, else `name`). An override tag uses
`property` when its name contains `:` and `name` otherwise. The base layout
renders:

```jinja
{% for tag in page.open_graph %}
<meta {{ tag.attribute }}="{{ tag.name }}" content="{{ tag.content }}">
{% endfor %}
```

The values are escaped by the template autoescape. `attribute` comes only from
the fixed set `property` and `name`. Pages other than the challenge page get an
empty list. Sites that override `layouts/base.html.jinja` add the loop
themselves; `rilgate web diff` shows the change.

## Diagnostics

`rilgate_open_graph_lookups_total{result}` counts lookups with `result` one of
`hit`, `fetched`, `failed`, `timeout`, `busy` and `override`. Requests that
`when` does not select are not counted. `timeout` counts
responses that rendered before the fetch completed; `busy` counts lookups
skipped because all fetch slots were in use.

## Crates

```text
rilgate-policy   host condition; public compiled condition for use outside policy rules
rilgate-config   site.open_graph schema, validation and runtime config
rilgate-web      PageInfo.open_graph, base layout loop
rilgate-gateway  open_graph module: fetch, parse, cache; gate integration; metrics
```

`gate_with_client` becomes `async` because the challenge branch awaits the
lookup. The cache hit path does not wait on I/O.

## Deferred

Each item is additive on top of this design:

- A shared cache across instances.
- More header value placeholders.
