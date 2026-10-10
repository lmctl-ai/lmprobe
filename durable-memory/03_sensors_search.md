# 03 — Search-shaped sensors

Verbs in this chapter: `search`, `def`, `ref`, `find`, `grep`, `discover`, `list-kinds`. They walk the live filesystem, optionally parse files via tree-sitter, and emit hits. Schema-first: each verb's envelope is `lmprobe.<verb>.v1` per [08_envelope_schema.md](08_envelope_schema.md). Global flags like `--limit`, `--format`, `-l`, `-G` from [02_invocation.md](02_invocation.md) apply throughout.

## search

Structural AST query. Three input modes (priority order):

1. **Pattern mode**: `-p '<pat>' -l <lang>`. ast-grep pattern syntax with metavars (`$X`, `$$$`). For a single file with a known extension, `-l` may be omitted and the parser is inferred from the file extension. Directory pattern searches still require `-l`.
2. **Name mode**: `--name <SUBSTR>`. Substring filter on extracted declaration names.
3. **Default**: walk all declaration kinds for `--lang` (or all langs) and emit each one (useful with `--outline` for a file-outline view).

Filter flags compose on top:

| Flag | Purpose |
|------|---------|
| `--kind <portable>` | Restrict to a portable kind: `function`, `method`, `class`, `struct`, `interface`, `trait`, `enum`, `type_alias`, `const`, `macro`, `import`, `route`. Resolves to per-language tree-sitter kinds via `KindTaxonomy`. |
| `--raw-kind <ts-kind>` | Tree-sitter literal kind, bypasses the taxonomy. |
| `--has '<rule-yaml>'` | Match nodes containing a descendant matching the inline ast-grep rule. |
| `--inside '<rule-yaml>'` | Match nodes inside another node matching the rule. |
| `--precedes` / `--follows` / `--not` / `--all` / `--any` | Other relational rules. |
| `-r, --rule <FILE>` | Load a `SerializableRule` from YAML. |
| `--has-child <SUBSTR>` | Filter to nodes with a structural child whose text contains SUBSTR. Per-language semantics: code-langs match method/function/field/declaration descendants; data-langs match `TopLevelKinds::child_kinds()` (pairs / attributes / child elements). See [10_languages.md](10_languages.md). |
| `--outline` | Render body-elided declarations (`fn foo(...) { ... }`). |

Envelope: `lmprobe.search.v1` → `SearchResult { hits: Vec<SymbolHit> }`. SymbolHit has `name`, `kind`, `lang`, `file_path`, `line_start`, `content`, optional `origin_root`, optional `enclosing_decl`.
No-match JSON responses include `scan_stats { files_considered, files_scanned, files_skipped }` so agents can distinguish an empty scan from an over-restrictive query.

```bash
# Find every function/struct/etc. named Pattern.
lmprobe search --name Pattern .

# Explicit path form for non-current directories; avoids positional query/path ambiguity.
lmprobe search --name Pattern --path crates/core/src/matcher

# Full declaration bodies are the default; --body is accepted for def parity.
lmprobe search --name Pattern --body .

# AST pattern: every Rust `fn foo() -> Result<T> { ... }`.
lmprobe search -p 'fn $NAME() -> Result<$T> { $$$ }' -l rust .

# Single-file AST pattern: infer Python from `.py`.
lmprobe search -p 'def $NAME($$$): $$$' src/app.py

# Outline view of one file.
lmprobe --format markdown search --outline crates/lmprobe/src/cli.rs

# Restrict by portable kind.
lmprobe search --kind class -l python .

# Has-child structural query (data lang).
lmprobe search --kind class --has-child UserID -l yaml .
```

Large AST-pattern searches are parse-bound. A command such as `lmprobe search -p 'function $NAME($$$ARGS) { $$$BODY }' -l typescript packages/vite/src/node` must parse every selected file as TypeScript before it can prove no match, so it can look idle in short agent command budgets. Narrow the directory first, add `--type typescript` when you mean extension filtering rather than language-forcing mixed files, combine `--kind` / relational filters where possible, and use `--limit` while exploring. Do not replace a precise AST query with raw `grep` unless raw text is acceptable for the task.

NOT_IMPLEMENTED behavior: when `--kind <portable>` resolves to an empty Vec for the file's language (the language doesn't expose that kind in its taxonomy), the file is skipped without diagnostic. Empty hit set with `exit_status: no_match` is the normal "found nothing" result.

Cython `.pyx` files are not structurally supported yet. If a `.pyx` file is selected by a structural walk without `-l/--lang`, lmprobe emits `CYTHON_NOT_SUPPORTED`; `grep` and `find` still scan `.pyx` text and paths normally.

> **CLI gotcha**: `search`'s clap layout is `[QUERY] [PATH]` — both optional. `lmprobe search --name Pattern .` is the quiet canonical current-directory recipe. For any other path, prefer the unambiguous `lmprobe search --name Pattern --path crates/core/src/matcher` form; path-looking positionals are accepted but emit `SEARCH_NAME_POSITIONAL_AS_PATH` so agents notice the auto-swap. GraphQL `search(query: ..., path: ...)` avoids the CLI ambiguity entirely.

## def

Find a symbol's *definition*, anchored on the full identifier. Exact-match by default; `--regex` switches to substring (pre-§0.6 behaviour).

| Flag | Default | Purpose |
|------|---------|---------|
| `--regex` | off | Substring match. |
| `--class <NAME>` | unset | Restrict method/function definitions to declarations enclosed by the named class/type/container. Useful for common method names such as `__str__`, `toString`, or `render`. |
| `--body` | off | Return full declaration bodies instead of skeletonized content. |
| `--around <N>` | unset | Add a bounded line-numbered `source_context` window around each definition hit. |
| `--scope {code\|all}` | `code` | `code` skips data-language files (JSON/YAML/TOML/XML/HTML/CSS); `all` walks every recognised lang. |

Envelope: `lmprobe.def.v1` → `DefResult { hits: Vec<SymbolHit> }`.

```bash
# Anchored exact match.
lmprobe def Pattern .

# Substring (re-enables pre-§0.6 fuzzy match).
lmprobe def configure --regex .

# Inspect implementation logic in one turn.
lmprobe --format json def --body handle_request src/service.ts

# Add flat source lines around the definition anchor.
lmprobe --format json def --around 3 handle_request src/service.ts

# Narrow a common method name to one containing class.
lmprobe def --class OpStrategy __str__ .

# Walk data languages too.
lmprobe def package --scope all .
```

`def` differs from `search --name` only in the exact-vs-substring default. Implementation lives in `sensors::def` calling `search_symbol_hits` with `exact_name: true`.

## ref

Find references to a symbol. Walks identifier nodes (not raw text) so a function-name match in a comment or string literal is excluded.

| Flag | Default | Purpose |
|------|---------|---------|
| `--scope {code\|all}` | `code` | Same semantics as `def`. |
| `--include-comments` | off | Re-include identifier matches inside comments. |
| `--include-docs` | off | Re-include identifier matches inside doc-comments. |
| `--role` | off | Add optional Python-first local role labels to JSON/NDJSON hits. |

Envelope: `lmprobe.ref.v1` → `RefResult { hits: Vec<GrepHit> }`. `GrepHit` is line-shaped: `file_path`, `line_number`, `line_content`, optional `origin_root`, optional `role`.

```bash
lmprobe ref Pattern .
lmprobe ref Pattern --include-comments .
```

`ref` is the AST-aware companion to `grep`. Use `grep` when you want raw regex against every line, including strings/comments; use `ref` when you want only the identifier-token uses.

`ref` ≠ `callers`: `ref` matches any identifier-token mention (type annotations, callable invocations, pure references). `callers` is the narrower invocation-shape verb — see [04_sensors_graph.md](04_sensors_graph.md#callers) for the recognized call-shape doctrine.

`ref --role` only adds the local role label. Structural expansion flags (`--parents`, `--body`) are grep-only in this slice.

## find

Find files by path-regex match. No tree-sitter parsing.

```bash
lmprobe find 'main\.rs$' .
lmprobe find 'go\.mod$' .
lmprobe find -i 'readme' .   # case-insensitive
lmprobe find '\.(ts|tsx)$' --path apps/web --path packages/shared
```

Envelope: `lmprobe.find.v1` → `FindResult { paths: Vec<String> }`.
Results retain every path matched by the regex, then rank likely source/core roots above test, docs, fixture, sample, and example copies unless the regex itself names those areas.
Malformed regex input returns a structured `REGEX_PARSE_FAILED` diagnostic envelope with `exit_status: "error"`.

GraphQL mirrors the multi-root CLI shape with `paths: [...]`:

```graphql
{
  find(pattern: "\\.(ts|tsx)$", path: "apps/web", paths: ["packages/shared"]) {
    paths
    exitStatus
  }
}
```

## py-slice-candidates

Python-only static evidence finder for sliced variables forwarded with related unsliced variables.

```bash
lmprobe py-slice-candidates src/ package/module.py
lmprobe --format json py-slice-candidates sklearn/**/*.py
lmprobe --format json py-slice-candidates src/ --trace-plan
```

Envelope: `lmprobe.py_slice_candidates.v1` -> `PySliceCandidatesResult { candidates: Vec<PySliceCandidate> }`.
Each candidate is low-confidence evidence only. It reports the enclosing class/function when available, the sliced base variable, derived sliced variable, slice expression, related unsliced variable, call line, source lines, and line numbers. The first slice is intentionally conservative: it looks for assignments like `Y_chunk = Y[sl]` followed nearby in the same function by a call that passes both `Y_chunk` and a related unsliced `Y_*` variable as call arguments. NDJSON output emits one candidate per line plus a final summary.

`--trace-plan` additively attaches a deterministic `candidate_id` and a `trace_plan` per candidate. The plan suggests a Python statement pattern for `lmprobe trace --watch`, watch variables, source line, enclosing function when known, and warnings when a whitespace/comment-normalized statement appears more than once in the file. Suggested patterns strip trailing Python comments so they can be used as trace anchors. It does not run tests or claim a bug.

## grep

Textual regex search across files (line-oriented). Same role as `git grep` / `rg` but with lmprobe's walker (respects `.gitignore` / `--lang` / `--max-filesize`).

```bash
lmprobe grep '^fn main' .
lmprobe grep -i 'todo' .
lmprobe grep '^package main' -l go .
lmprobe grep 'login|signin' --path apps/web --path packages/shared
lmprobe --format json grep --parents 'needle' src/
lmprobe --format json grep --body 'needle' src/
lmprobe --format json grep --role 'base_config_key' src/
```

Envelope: `lmprobe.grep.v1` → `GrepResultEnvelope { hits: Vec<GrepHit> }`.
Each `GrepHit` has `file_path`, `line_number`, and `line_content`; use `line_content` for the matched source line (`line` is not a v1 field).
With `-A`, `-B`, or `-C`, grep JSON/NDJSON hits also include optional `before_context` and `after_context` arrays of `{ line_number, line_content }`; compact hits omit those fields when context is not requested.
With `--parents`, grep JSON/NDJSON hits in parseable files also include optional `parents` arrays ordered outer-to-inner, with `{ kind, name?, line_start, line_end }` for enclosing class/function-like declarations. Hits in unparseable files omit `parents`.
With `--body`, grep JSON/NDJSON hits in parseable files also include optional `body` metadata/content for the innermost enclosing structural declaration or statement block. Hits outside a structural body or in unparseable files omit `body`.
With `--role`, grep JSON/NDJSON hits include an optional mechanical local `role` string. The first slice is Python-first and uses line/anchor-local classification: `definition`, `assignment`, `read`, `call`, `argument`, `return`, `condition`, `attribute_access`, `dict_key`, `index_access`, `import`, or `unknown`.
No-match JSON responses include `scan_stats { files_considered, files_scanned, files_skipped }`.
Malformed regex input returns a structured `REGEX_PARSE_FAILED` diagnostic envelope with `exit_status: "error"`.
If grep cannot visit or fully read a selected file, it emits `WALK_ENTRY_ERROR`
or `FILE_READ_FAILED` while continuing across the rest of the tree. A matched
result with either warning is usable evidence, but not proof that the scan was
complete; the envelope reports `partial: true` and the direct CLI process exits
3 for coverage-affecting warnings (`FILE_TOO_LARGE`, `FILE_READ_FAILED`, or
`WALK_ENTRY_ERROR`). `exit_status` stays in the stable
`matched`/`no_match`/`error` enum.

GraphQL mirrors repeated `--path` with `paths: [...]` on `find`, `grep`, and
`grepDirsWith`. `grepDirsWith` is the internal-pipe resolver for the common
"marker directory AND content" recipe:

```graphql
{
  grep(pattern: "login|signin", path: "apps/web", paths: ["packages/shared"]) {
    hits { filePath lineNumber lineContent }
    exitStatus
  }
  packageLogin: grepDirsWith(
    markerPattern: "(^|/)package\\.json$",
    pattern: "login",
    path: ".",
    parents: true
  ) {
    hits { filePath lineNumber lineContent parents { kind name lineStart lineEnd } }
    exitStatus
  }
}
```

See [24_find_grep_recipes.md](24_find_grep_recipes.md) for a longer recipe
matrix for agents translating `fd`/`find` and `rg`/`grep` workflows.

## resource-sites

Evidence-only scanner for user-supplied acquire/release regexes in Go, Rust, JavaScript, TypeScript, and TSX files.

```bash
lmprobe resource-sites . --acquire 'os\.Open|File::open' --release 'Close|drop'
lmprobe --format json resource-sites src --acquire 'os\.Create' --release 'Close' --around 3 --parents --body
```

Envelope: `lmprobe.resource_sites.v1` -> `ResourceSitesResult { sites: Vec<ResourceSite> }`.
Each site groups acquire anchors by nearest enclosing declaration and reports release/defer/error-branch anchors mechanically visible in the same declaration. This is local evidence only: empty `release_calls` is not a conclusion about ownership or control flow. Optional `--around`, `--parents`, and `--body` attach the same source and structural context shapes used by other line-anchor commands.

For Go, `error_branches` also includes return statements after a matched acquire line and before the first matched release/defer line in the same enclosing declaration. The immediate acquire-failure `if err != nil { return ... }` guard is skipped so the branch list focuses on returns after the acquire has succeeded.

For JavaScript, TypeScript, and TSX, resource-sites may add optional syntactic classifications when evidence is present: `acquire_timing` (`module_load`, `lazy_callback`, `event_handler`, or `dynamic_import`), `release_lifecycle` (`pagehide`, `beforeunload`, `visibilitychange`, or `dispose_callback`), and `optional_acquire: true` for lazy/fallback-like acquisition lines such as `??=` or `import(...).catch(...)`. These are evidence labels only, not leak verdicts.

## async-leaks

Evidence-only scanner for literal JS/TS async teardown call anchors.

```bash
lmprobe async-leaks . --call stop --call close --around 2 --parents
lmprobe --format json async-leaks src/plugin.ts --call dispose --body
```

Envelope: `lmprobe.async_leaks.v1` -> `AsyncLeaksResult { sites: Vec<AsyncLeakSite> }`.
Each site reports the call anchor, source line, configured callee name, and syntax-only `awaited` / `returned` booleans. This does not prove Promise return types, resource ownership, or cleanup correctness; it only gives AI agents the local evidence needed to inspect teardown paths.

## crypto-sites

Evidence-only Go-first scanner for requested crypto algorithm anchors and nearby FIPS/config evidence.

```bash
lmprobe crypto-sites . --algorithm sha1 --fips-check --around 4
lmprobe --format json crypto-sites src --algorithm md5 --algorithm des --parents --body
```

Envelope: `lmprobe.crypto_sites.v1` -> `CryptoSitesResult { sites: Vec<CryptoSite> }`.
Each site includes `file_path`, `line_number`, `line_content`, normalized `algorithm`, matched text, `import_paths`, optional `nearby_guards`, optional `source_context`, optional `parents`, optional `body`, and `confidence: "syntax-evidence-only"`.

The first slice scans Go files and matches common aliases for requested algorithms such as `sha1`, `SHA1`, `crypto/sha1`, `sha1.New`, `md5.Sum`, and `des.NewCipher`. With `--fips-check`, nearby lines containing `fips`, `boringcrypto`, `GODEBUG`, `GOFIPS`, or Go build tags are attached as evidence only.

## config-keys

Evidence-only extractor for JSON/YAML/TOML config keys, with optional lexical code anchors.

```bash
lmprobe --format json config-keys package.json pyproject.toml settings.yaml
lmprobe --format json config-keys . --grep-code --around 2 --parents --role
lmprobe --format json config-keys pyproject.toml --key tool.pytest.ini_options.addopts --grep-code
```

Envelope: `lmprobe.config_keys.v1` -> `ConfigKeysResult { keys: Vec<ConfigKeyRecord> }`.
Each key record includes `file_path`, `lang`, `key_path`, leaf `key`, best-effort `line_number`, `value_kind`, optional scalar `value_summary`, and `code_anchors`.

`--key <KEY_PATH>` keeps only an exact config key path before optional code-anchor attachment. Use it when one key matters and a broad `--grep-code` run would waste budget or hit the key cap.

`--grep-code` searches code-tier files for escaped leaf-key text, skipping config files. When the selected path is a config file, code-anchor lookup uses that file's parent directory while config extraction stays limited to the file. Anchors are lexical/mechanical evidence only: they do not prove data flow, and an empty `code_anchors` array is only an investigation lead. `--around`, `--parents`, and `--role` attach source context, enclosing structural metadata, and mechanical role labels to those code anchors when available. The first slice caps searched keys and anchors and reports `CONFIG_KEYS_GREP_KEY_LIMIT` / `CONFIG_KEYS_GREP_ANCHOR_LIMIT` warnings when truncated.

## guard-sites

Evidence-only scanner for user-supplied sensitive-action and guard-looking regexes in code files.

```bash
lmprobe guard-sites . --sensitive 'Delete|exec|eval' --guard 'auth|authorize|allowed' --around 8 --parents
lmprobe --format json guard-sites src --sensitive 'SaveUploadedFile' --guard 'auth|middleware' --around 4 --parents --body
```

Envelope: `lmprobe.guard_sites.v1` -> `GuardSitesResult { candidates: Vec<GuardSiteCandidate> }`.
Each candidate includes a `sensitive` line anchor, `nearby_guards` within the requested `--around` window, decorator/annotation-like `decorations` directly above the enclosing declaration when available, optional `parents`, optional `body`, optional `source_context`, and `confidence: "local-evidence-only"`.

The regexes are caller-supplied heuristics. Empty `nearby_guards` means only that this local scan did not collect guard-looking lines in the requested window; inspect middleware, decorators, callers, and context before deciding what the evidence means. Malformed regexes return a structured `REGEX_PARSE_FAILED` diagnostic envelope.

## openapi-facts

Evidence-only extractor for OpenAPI/Swagger JSON/YAML operation facts. It parses the `paths` object without full schema validation.

```bash
lmprobe --format json openapi-facts openapi.json
lmprobe --format json openapi-facts specs/
```

Envelope: `lmprobe.openapi_facts.v1` -> `OpenApiFactsResult { operations: Vec<OpenApiOperationFact> }`.
Each operation fact includes `file_path`, `method`, `path`, optional `operation_id`, `required_params`, and `response_schemas` collected from easy `$ref` / schema-title evidence. Malformed selected specs emit non-fatal `OPENAPI_PARSE_FAILED` diagnostics; other parseable specs still emit operations.

## route-anchors

Evidence-only route declaration anchors from code.

```bash
lmprobe --format json route-anchors . --framework fastapi --around 2 --parents --body
lmprobe --format json route-anchors src --framework generic
```

Envelope: `lmprobe.route_anchors.v1` -> `RouteAnchorsResult { routes: Vec<RouteAnchor> }`.
The first slice recognizes Python/FastAPI decorator literals such as `@app.get("/x")` / `@router.post('/x')`, plus a generic literal `.get("/x")`, `.post(...)`, `.put(...)`, `.delete(...)`, `.patch(...)` fallback. Each route includes `method`, `path`, `file_path`, `line_number`, `line_content`, best-effort `handler`, optional `source_context`, optional `parents`, optional `body`, and `confidence: "literal-evidence-only"`.

## http-client-calls

Evidence-only HTTP client call anchors from code.

```bash
lmprobe --format json http-client-calls . --around 2 --parents --body
```

Envelope: `lmprobe.http_client_calls.v1` -> `HttpClientCallsResult { calls: Vec<HttpClientCallAnchor> }`.
The first slice recognizes literal Python `requests`/`httpx` calls and JS/TS `fetch`/`axios` calls. Each call includes `method` when available, `url`, `file_path`, `line_number`, `line_content`, optional `client`, optional `source_context`, optional `parents`, optional `body`, and `confidence: "literal-evidence-only"`.

These three API fact streams are intentionally separate. lmprobe does not compare them or claim API drift, compatibility issues, or missing routes.

## discover

Find project-marker files and project hierarchy. Recognises 22 marker types across 18 ecosystems: Cargo, npm, Maven, Gradle, Go modules, pyproject/setup.py, Gemfile, .csproj/.sln, Ant/Make/CMake, requirements.txt, composer.json, Bazel, Helm, Ansible (`site.yml`/`playbook.yml` adjacent to `roles/`), SwiftPM, Nix flakes, sbt, *.gemspec, *.rockspec, Dart pubspec, Elixir mix, Terraform `versions.tf`, Lua/Neovim `init.lua`, Web/Static `index.html`.

| Flag | Default | Purpose |
|------|---------|---------|
| `--tree` | off | Render parent → children indented hierarchy. |
| `--depth <N>` | unbounded | Cap the walk at N levels. |
| `--top-level-only` | off | Suppress nested projects and clear `children[]` for a compact monorepo map. |

Envelope: `lmprobe.discover.v1` → `DiscoverResult { projects: Vec<Project> }`. Each `Project { name, project_type, version, path, dependencies_count, children: Vec<String> }`.

```bash
# JSON of every project marker found.
lmprobe --format json discover .

# Indented tree.
lmprobe discover --tree .

# Just the top level.
lmprobe discover --tree --depth 1 .

# Compact monorepo map: only projects with no discovered parent.
lmprobe --format json discover --top-level-only .

# Nested Go package: report package context from the nearest ancestor go.mod.
lmprobe --format json discover server/etcdserver/api/membership/
```

Coverage: 27 of 29 qa1 fixture roots return ≥1 project. The two `(c)-bucket` holdouts (markdown-only spec repos, pre-package.json era code dumps) are expected misses.

## list-kinds

Print the `KindTaxonomy` registry for one language: each portable kind and the tree-sitter literal kinds it maps to. Useful for picking the right `--kind` or `--raw-kind` value.

```bash
lmprobe list-kinds rust
lmprobe list-kinds python
lmprobe list-kinds toml
```

Envelope: `lmprobe.list_kinds.v1` → `ListKindsResult { rows: Vec<PortableKindRow> }`. Each `PortableKindRow { portable: String, kinds: Vec<String> }`.

Empty `kinds: []` for a portable kind means "this language doesn't expose that kind" — `--kind <portable>` skips the file. See [10_languages.md](10_languages.md) for the full per-language matrix.

## Composability

All search-shaped verbs honour the global walker flags: `-l`, `-G`, `--no-ignore`, `-j`, `--max-filesize`, `--limit`. They produce the same `*Hit` shape across formats; choose the format with `--format` per [07_output_formats.md](07_output_formats.md).

Cross-references:
- **Multi-repo fan-out**: `--workspace` adds `origin_root` tags. See [13_workspace.md](13_workspace.md).
- **Diagnostic codes from these verbs**: `NOT_IMPLEMENTED`, `PATTERN_PARSE_FAILED`, `RULE_PARSE_FAILED`, `QUOTED_INPUT`, `FILE_TOO_LARGE`, `FILE_READ_FAILED`, `WALK_ENTRY_ERROR`, `DEPRECATED_POSITIONAL_QUERY`. See [14_diagnostics.md](14_diagnostics.md).
- **Performance**: search/def/ref/grep/find sub-100ms on the ast-grep workspace. See [15_perf.md](15_perf.md).

## Read next

- [04_sensors_graph.md](04_sensors_graph.md) for relational verbs (callers/callees/imports/exports/decorations/supertypes/subtypes/overrides/impact/context).
- [05_history.md](05_history.md) for git-history verbs.
