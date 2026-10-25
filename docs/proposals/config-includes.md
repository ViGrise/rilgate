# Configuration file includes

Status: implemented on `feat/config-includes`.

## Scope

Add `!include` at any YAML value position so operators can split configuration
sections, maps, lists and scalar values into local files. Operators can mix
inline values and included files, including single rules and lists of rules.
Existing single-file configurations keep their behavior.

YAML provides custom tag syntax; resolving `!include` is a Rilgate convention,
not a built-in YAML file-loading feature. Implement resolution once in the
configuration loader so every section uses the same behavior without dedicated
fields such as `policy.file` or `server.file`.

This proposal covers loading at startup and configuration commands. Remote
includes, glob patterns, directory discovery and automatic reload
are outside its scope.

## Configuration

### Split sections

```yaml
# rilgate.yml
server: !include ./config/server.yml
security: !include ./config/security.yml
site: !include ./config/site.yml
pow: !include ./config/pow.yml
policy: !include ./config/policy.yml
```

Each included file contains the value for that position, without repeating its
section name:

```yaml
# config/server.yml
listen: ":8080"
upstream:
  url: "http://127.0.0.1:3000"
```

```yaml
# config/security.yml
secret: "${RILGATE_SECURITY_SECRET}"
```

### Mix inline and included map values

```yaml
# config/policy.yml
mode: custom
default_action: challenge
ip_sets:
  internal-bots:
    cidrs:
      - "192.0.2.10/32"
  external-bots: !include ./external-bots.yml
rules: !include ./rules.yml
```

```yaml
# config/external-bots.yml
cidrs: !include ./external-bot-cidrs.yml
```

```yaml
# config/external-bot-cidrs.yml
- "198.51.100.0/24"
- "2001:db8::1/128"
```

### Mix inline rules, single-rule files and rule lists

```yaml
# config/rules.yml
- name: allow-health
  when:
    path:
      exact: /health
  action: allow
- !include ./rules/external-bot-rule.yml
- !include ./rules/blocked-bot-rules.yml
```

```yaml
# config/rules/external-bot-rule.yml
name: allow-external-bots
when:
  client_ip:
    in_sets: [external-bots]
action: allow
```

```yaml
# config/rules/blocked-bot-rules.yml
- name: deny-badbot
  when:
    user_agent:
      contains: BadBot
  action: deny
- name: deny-scraper
  when:
    user_agent:
      contains: ExampleScraper
  action: deny
```

The resolved `rules` value is a flat list of four rules in declaration order.
The same mechanism applies to other lists, including CIDRs, signing keys and
mining pools.

## Resolution semantics

| Include position | Included value | Result |
| --- | --- | --- |
| A mapping value or the document root | Mapping, sequence or scalar | Replace that node with the resolved value |
| A sequence item | Mapping or scalar | Insert one item at that position |
| A sequence item | Sequence | Insert its items into the containing sequence in order |

Resolve nested includes before inserting the resulting value. Sequence
insertion applies only when the original item is a direct `!include` node.
Ordinary nested sequences preserve their structure. An empty included sequence
inserts zero items; final schema validation still applies.

An included mapping replaces its value position. The `<<` key explicitly merges
a mapping or a list of mappings into the containing mapping. The final resolved
type must satisfy the existing schema for that position.

### Map merging

`<<` works in every mapping, including the root and included documents:

```yaml
policy:
  <<: [!include ./base-policy.yml, !include ./site-policy.yml]
  default_action: deny
```

- The resolved base must be a mapping or a list of mappings. Inline maps and
  document-local YAML aliases are accepted. A direct include in the list that
  resolves to a list of maps inserts its items using sequence insertion rules.
  Invalid types report the source file and file-local path.
- Bases apply in list order, later bases winning. Inline sibling keys win over
  all bases, regardless of declaration position.
- Maps merge recursively. Lists, scalars and explicit null replace previous
  values whole; the schema determines whether a final null is valid.
- Nested includes and `<<` resolve inside each base before merging.
- Keys retain first-seen order across bases, followed by new inline keys.
  Overriding a key preserves its original position.
- Each value retains the winning source, file-local path and include chain.
  The resolved output contains neither `<<` nor `!include`.
- All content consumes the existing node, depth, read and byte budgets. Cycle
  detection, environment interpolation and CLI/environment precedence remain
  unchanged. Merging runs before interpolation and overrides.

The parser resolves aliases as values and preserves `<<` keys. The loader
implements these deep-merge rules without calling `apply_merge`.

### Files and paths

- The tag argument is one nonempty literal path string.
- Relative paths resolve from the directory of the file containing the tag.
  Absolute paths are accepted.
- Include paths do not interpolate environment variables. Environment
  interpolation remains available in the resolved configuration values.
- Each included file contains exactly one nonempty YAML document. Explicit YAML `null`
  remains a value subject to schema validation.
- Nested includes are supported. Independent branches may include the same
  file more than once.
- Include resolution changes the interpretation of include paths only. Other
  path settings, such as certificate paths and asset directories, retain their
  existing behavior.
- YAML anchors and aliases remain local to their document. Cross-file
  composition uses `!include`.
- Unsupported tags produce an error.

## Loader integration

The file-loading pipeline becomes:

```text
Select the main configuration file
  -> parse YAML
  -> resolve includes recursively
  -> check structure and renamed fields
  -> interpolate environment variables
  -> apply environment and CLI overrides
  -> deserialize the configuration schema
  -> validate the resolved configuration
```

Preserve current precedence:

```text
CLI flags > environment variables > resolved YAML > defaults
```

Put the shared resolver in `crates/rilgate-config/src/loader/`, with file
selection and reading integrated through `loader::load`. Keep include handling
outside individual section schemas. Gateway, verifier and configuration commands
must consume the same resolved document.

Evaluate libraries compatible with `serde_yaml_ng` before implementing the
resolver. Reuse a library if it provides these semantics, bounded resolution and
adequate source diagnostics. Otherwise, implement a focused resolver over
`serde_yaml_ng::Value`.

Keep `parse` and `parse_with` usable for ordinary YAML strings. A local include
in a string-only API without file context must return a clear error. Built-in
includes resolve against the embedded catalogs without a file context. Add an entry
point with an explicit source path if a real caller requires it; do not resolve
relative includes implicitly from the working directory.

Carry source provenance through replacement, list insertion and interpolation
so validation can identify the originating file. Overrides retain their flag or
environment-variable source information.

## Code feasibility review

This section records the code review before implementation.

The current code supports this approach without changing the public section
schemas or policy evaluator. A local probe against the installed
`serde_yaml_ng` 0.10.0 dependency confirmed that root tags and tagged sequence
items retain both the `include` tag and path string. A sequence insertion probe
preserved declaration order. The parser also rejects multiple documents through
`from_str::<Value>`.

The implementation needs these changes at the existing boundaries:

| Current code | Required change |
| --- | --- |
| `loader::load` and `read_default` return file contents without retaining the selected path | Retain the source path alongside the text for relative includes, including default-file selection |
| `loader::parse_with` parses directly into `Value` before interpolation | Extract the shared value-to-document stage; resolve includes before passing file input to that stage |
| `loader::expand` rejects every `Value::Tagged` | Consume supported tags in the resolver and keep the rejection for unresolved or unsupported tags |
| `Document.sources` stores only override paths and source strings | Add file provenance separately from override provenance and preserve it through sequence insertion |
| `Document::resolve` annotates errors by matching textual configuration paths | Share source annotation with verifier validation and handle parent paths and cross-field errors |
| `main.rs` loads the document once before dispatching gateway and verifier commands | Keep this common entry point so command behavior stays consistent |
| `Document::display` serializes the typed document; `Secret::serialize` redacts secret values | Reuse typed output after include resolution and preserve verifier-specific output filtering |

### Diagnostic constraints

`serde_yaml_ng::Value` does not retain per-node line and column marks. Parsing
errors can report a location, but schema errors after conversion to `Value`
cannot recover exact source lines from the current representation. The initial
implementation should guarantee file, logical path and include-chain context
for errors with an identifiable source, and parser line/column where available.
Exact line locations for every post-parse validation error would require a
span-aware representation or additional parser work and are outside this scope.

Use structured path segments internally for provenance, including mapping keys
and sequence indices. Rebase included source entries to their final indices
when lists are inserted. A plain dotted-string prefix is ambiguous for keys
that contain dots, such as header names. Keep both the original file-local path
and resolved configuration path so errors can identify the source rule after
insertion.

Existing semantic validation uses `anyhow` messages, some of which describe a
parent section or multiple fields. It does not always provide one exact leaf
path. Annotate the relevant source set or include trace in these cases and keep
the original validation message. Do not promise a single exact file for an
error involving values from several files. File annotation must cover both
`resolve` and `resolve_verifier`; they currently have different error wrappers.

### Input and compatibility constraints

The current main-file reader uses `read_to_string` before checking the 1 MiB
limit. Bounded reading must apply to the main file as well as includes. A
resolved-node budget must be checked during traversal and insertion; the
existing 64-level YAML nesting check in `expand` runs too late to protect the
resolver itself. Bound resolver traversal depth as well as file-include depth.
The parser has its own recursion and alias-repetition limits. Keep those limits
and distinguish parser allocation from subsequent resolver expansion when
documenting resource guarantees.

An empty input parses as `Value::Null`, so rejecting empty included documents
while allowing explicit `null` requires a document-presence check. Preserve the
existing empty-main-file and missing-default-file behavior used by
environment-only configuration. The nonempty-file rule applies to included
fragments.

`config show` currently prunes defaults unless `--defaults` is requested, and
verifier output contains only verifier settings. Here, complete resolved output
means that includes have been consumed; these existing output choices remain.

The core include behavior is a focused loader change. Source attribution across
semantic validation is the larger integration task and must be included in the
implementation estimate. A span-aware parser migration would materially expand
that work; retain the current parser for this proposal.

### Review verification

- A scratch Rust probe linked against the installed YAML dependency confirmed
  tag/path retention, root tags, ordered sequence insertion, multiple-document
  rejection and empty-input conversion to `null`.
- `cargo test -p rilgate-config --lib --tests --offline` passed all 61 existing
  tests, including override precedence/source attribution, IP set serialization,
  secret redaction and verifier output coverage.
- These checks established parser feasibility and the configuration baseline.
  The implementation now includes end-to-end loading tests, CLI coverage and
  the multi-file distribution example.

## Errors and resource limits

Detect cycles using canonical paths on the active include stack. This detects
cycles through symlinks while allowing reuse after a branch finishes resolving.
Return an error containing the include chain.

Bound include depth, resolver traversal depth, total file reads and expanded
tree size with documented constants. Select concrete bounds during implementation based on representative
configurations. Apply the existing 1 MiB input budget to the aggregate bytes
read, including the main file and repeated reads. Enforce limits while reading
and expanding. Retain parser recursion and alias-repetition limits during YAML
parsing; resolver budgets apply to the subsequent tree traversal and insertion.

Return errors for missing or unreadable files, invalid tag arguments, empty
files, multiple YAML documents, parse failures, cycles, exceeded limits and
resolved values that fail schema validation. Never silently skip a failed
include.

Diagnostics should identify:

- The originating file and logical configuration path when identifiable, or
  the relevant source set for a cross-field validation error.
- The relevant include chain.
- Line and column when available from the parser.

Additional diagnostic context must contain paths and locations without printing
secret values or entire configuration fragments. Preserve existing secret
redaction in configuration output.

## Configuration commands and compatibility

- `config check` resolves every include before checking the configuration.
- `config show` emits a complete resolved YAML document without include tags,
  using the existing secret-redaction behavior.
- Gateway and verifier share the resolver and its error semantics.
- Main-file selection through `--config`, `RILGATE_CONFIG` and default paths
  remains unchanged.
- Existing single-file configurations retain their loading and validation
  behavior.

## Implementation plan

1. Confirm the parser's handling of tagged values and assess compatible include
   libraries against the specified behavior.
2. Retain the selected main-file path, extract the shared value-to-document
   stage and add a resolver with source context, relative path handling, cycle
   detection and resource budgets.
3. Implement node replacement and direct sequence-item insertion, preserving
   source provenance and item order.
4. Integrate resolution before interpolation and overrides in the common
   loader. Extend source annotation through gateway and verifier semantic
   validation. Ensure configuration-command entry points use the same path.
5. Add behavioral coverage for the cases below, with generated fixtures in
   system temporary directories.
6. Add a multi-file configuration example and update `docs/configuration.md`
   with syntax, path rules, precedence, limits, diagnostics and migration steps.
7. Run formatting, lint and relevant configuration, CLI, gateway and verifier
   checks. Review the diff for scope, unsupported claims and redundant comments.

## Behavioral verification

| Behavior | Defect the test must catch |
| --- | --- |
| Split and single-file configurations resolve equivalently | Values lost or changed during inclusion |
| Nested includes work from a different working directory | Paths resolved against the process directory |
| Inline and included mapping values coexist | Sibling entries lost or overwritten |
| Inline rules, one rule and rule lists preserve order | Nested rule lists or reordered policy execution |
| Included CIDRs drive policy matching | Included addresses fail to affect observable decisions |
| Ordinary nested sequences retain their shape | Unconditional sequence flattening |
| Environment and CLI overrides retain precedence | Included values override higher-priority settings |
| Independent branches reuse an included file | Reuse incorrectly reported as a cycle |
| Cycles through relative paths and symlinks fail | Recursive expansion escapes cycle detection |
| Missing files, invalid YAML and invalid resolved types fail with source context | Failed includes are skipped or errors identify the wrong file |
| Resource limits reject excessive input | Unbounded reads or expansion |
| Gateway, verifier, `config check` and `config show` agree | Different entry points load different configurations |
| Resolved output retains secret redaction | Include processing exposes secret values |

Use caller-visible outcomes for integration coverage, especially policy order
and IP membership. Avoid tests that only repeat internal traversal steps.

## Implementation

The shared resolver uses `serde_yaml_ng::Value` and an annotated intermediate
tree to retain file-local paths while inserting sequences. Source annotation
is shared by gateway and verifier validation. No additional YAML dependency is
required.

The implemented limits are 1 MiB aggregate input, 256 reads, 16 active files,
64 traversal levels and 65,536 visited value nodes including include nodes.
Repeated includes consume the budgets again. Main-file reading is bounded too.
Parser recursion and alias-repetition limits remain active.

See [configuration documentation](../configuration.md#splitting-configuration-files)
and the [multi-file example](../../config/examples/includes.yml) for use.
Exact per-node source lines after parsing, URL includes, globs and
automatic reload remain outside this implementation.

## Acceptance criteria

Every configuration value position can load a local file. Operators can mix
inline entries with included entries and insert a single rule or multiple rules
at one list position. Resolution preserves order, validates the final schema,
reports the source of errors and enforces resource bounds. Existing single-file
configuration behavior and override precedence remain compatible.

## Built-in catalogs

The resolver also accepts `!include builtin:rules/<file>.yml` and
`!include builtin:policies/<file>.yml`. The source directories `config/rules/`
and `config/policies/` are embedded at compile time. `config/examples/` contains
operator examples and is outside the embedded catalogs.

Local, built-in and inline sources share resource budgets and source attribution.
Relative references from built-in documents stay in the virtual catalog, with
normalized identities used for cycle detection. Built-in resources never fall
back to filesystem copies. String parsing supports built-in includes while
local includes continue to require file context. The default configuration
defaults to balanced mode. The production configuration selects the built-in
balanced policy. Container and release distributions carry third-party notices
alongside the binary.
