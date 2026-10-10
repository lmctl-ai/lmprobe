# 17 — GraphQL surface (`lmprobe query`)

`lmprobe query '<gql>'` runs a GraphQL document against the in-process schema and prints the response as a `lmprobe.query.v1` envelope. The schema is the **same** one the optional `serve` HTTP daemon exposes — schema-first invariant; one resolver implementation, two transports.

This chapter is the standalone reference for the GraphQL surface. Reading `index.md` plus this chapter is sufficient to use `lmprobe` as an AI agent without loading the per-verb chapters.

## Why GraphQL

The CLI is fine for one-shot human use: `lmprobe search Foo .` or `lmprobe callers blame .`. It struggles when an agent wants to combine results — find a definition, then pull its callers, then check recent history — because each step is a separate process invocation, separate JSON parse, separate context.

GraphQL solves this with composition (`def → callers → enclosing` in one round-trip), field selection (only ask for `{ filePath }` if that's all you need; saves 5–20× tokens on large result sets), variables (parameterize cached query strings), and introspection (the schema is self-describing).

The surface is one-shot like the rest of the CLI: `lmprobe query '<gql>'` parses the document, executes the resolvers on a current-thread tokio runtime, prints the envelope, exits. No daemon, no persistent state. The HTTP daemon (`lmprobe serve`, `--features serve`) is opt-in for cases where a long-running server is genuinely the right shape — see [12_optional_layers.md](12_optional_layers.md).

## `query` syntax

```bash
# Inline
lmprobe query '{ search(query: "Pattern", path: ".") { hits { filePath lineStart } } }'

# stdin
cat q.graphql | lmprobe query -

# File (with @ prefix)
lmprobe query @q.graphql

# Variables: --var KEY=VALUE (repeatable; values JSON-parsed
# with string fallback) and --vars-file path/to/vars.json
# (top-level object). Combined: file loads first, individual
# --var entries override.
lmprobe query @q.graphql --var name='"Pattern"' --var limit=10

# Operation name (when the document has multiple operations)
lmprobe query @ops.graphql --operation-name GetContext
```

`--format` flags: `json` (pretty-printed envelope, default for explicit `--format json` and Auto-on-TTY), `ndjson` (one-line envelope, Auto-on-pipe). `markdown` / `sarif` / `github` / `mermaid` / `dot` aren't applicable to GraphQL responses; they fall back to JSON with a stderr note `lmprobe: query: --format <X> not applicable to GraphQL responses; rendering as JSON`. See [07_output_formats.md](07_output_formats.md).

## Schema overview

Top-level query fields plus 7 nested under `history` expose the read-only lmprobe surfaces:

```graphql
type Query {
  # Search-shaped
  discover(path: String!, topLevelOnly: Boolean): DiscoverResult!
  find(pattern: String!, path: String!, paths: [String!]): FindResult!
  grep(pattern: String!, path: String!, paths: [String!], around: Int, parents: Boolean, body: Boolean, role: Boolean): GrepResult!
  grepDirsWith(markerPattern: String!, pattern: String!, path: String!, paths: [String!], around: Int, parents: Boolean, body: Boolean, role: Boolean): GrepResult!
  search(query: String, path: String!, ...filter args): SearchResult!
  def(name: String!, path: String!, className: String, body: Boolean, around: Int, ...): DefResult!
  refs(name: String!, path: String!, role: Boolean, ...): RefResult!
  resourceSites(path: String!, acquire: String!, release: String!, around: Int, parents: Boolean, body: Boolean): ResourceSitesResult!
  asyncLeaks(path: String!, call: [String!]!, around: Int, parents: Boolean, body: Boolean): AsyncLeaksResult!
  cryptoSites(path: String!, algorithms: [String!]!, fipsCheck: Boolean, around: Int, parents: Boolean, body: Boolean): CryptoSitesResult!
  configKeys(path: String!, key: String, grepCode: Boolean, around: Int, parents: Boolean, role: Boolean): ConfigKeysResult!
  guardSites(path: String!, sensitive: String!, guard: String!, around: Int, parents: Boolean, body: Boolean): GuardSitesResult!
  openapiFacts(path: String!): OpenApiFactsResult!
  routeAnchors(path: String!, framework: String, around: Int, parents: Boolean, body: Boolean): RouteAnchorsResult!
  httpClientCalls(path: String!, around: Int, parents: Boolean, body: Boolean): HttpClientCallsResult!
  callers(name: String!, path: String!): CallersResult!
  imports(name: String!, path: String!): ImportsResult!
  decorations(name: String!, path: String!): DecorationsResult!
  evolution(name: String!, path: String!, force: Boolean): EvolutionResult!

  # Graph-shaped
  callees(name: String!, path: String!): CalleesResult!
  supertypes(name: String!, path: String!): SupertypesResult!
  subtypes(name: String!, path: String!): SubtypesResult!
  overrides(name: String!, path: String!): OverridesResult!
  exports(name: String!, path: String!): ExportsResult!

  # Compound
  impact(symbol: String!, path: String!, direction: ImpactDirection, depth: Int, testsOnly: Boolean, excludeTests: Boolean): ImpactResult!
  context(symbol: String!, path: String!, depth: Int, importAware: Boolean, usage: Boolean, around: Int, parents: Boolean, body: Boolean): ContextResult!
  astDiff(file: String!, from: String!, to: String!, path: String): AstDiffResult!

  # Misc
  listKinds(lang: Language!): KindList!
  history: HistoryQueries!  # nested resolver
}

type HistoryQueries {  # 7 sub-fields
  blame(target: String!, path: String, follow: Boolean, semantic: Boolean): BlameResult!
  introduced(name: String!, path: String, follow: Boolean): IntroducedResult!
  commit(pattern: String, since: String, author: String, path: String): HistoryCommitResult!
  diff(pattern: String!, since: String, path: String): HistoryDiffResult!
  churn(top: Int, since: String, path: String): HistoryChurnResult!
  coChange(file: String!, since: String, top: Int, minScore: Float, excludeGlobs: [String!], fileType: String, path: String): HistoryCoChangeResult!
  todos(since: String, path: String): HistoryTodosResult!
}

type Mutation {
  fix(scope: ScopeInput!, pattern: String!, replacement: String!, dryRun: Boolean = true): FixResult!
  trace(scope: ScopeInput!, template: TraceTemplate!, marker: String!, worktree: Boolean = true, exec: String): TraceResult!
  untrace(marker: String!, worktree: Boolean = false, path: String): UntraceResult!
}
```

Exact field shapes (with all per-resolver argument lists) come from the GraphQL introspection itself:

```bash
lmprobe query '{ __schema { queryType { fields { name args { name type { name kind ofType { name } } } } } } }'
```

The mutation surface and the per-result envelopes (`SearchResult`, `BlameResult`, etc.) all derive from the D4 schema in `crates/lmprobe-types/src/schema.rs`. See [08_envelope_schema.md](08_envelope_schema.md).

## Composition (§5.1, P0)

The win agents care about most. Five CLI invocations becomes one query:

```graphql
{
  def(name: "blame_with_follow", path: ".") {
    hits {
      filePath
      lineStart
      callers(limit: 10, path: ".") {
        hits {
          name
          filePath
          enclosing { name kind }
        }
      }
      recentHistory(limit: 3, path: ".") {
        entries {
          commitHash
          author
          changeType
        }
      }
      decorations { hits { name filePath } }
    }
  }
}
```

One round-trip, one parse, one envelope. The `SymbolHit` type carries four child resolvers — `callers`, `callees`, `recentHistory`, `enclosing` — that re-use the same sensor functions the top-level resolvers call.

Each child resolver takes an explicit `path` argument (defaulting to `.`). async-graphql's `ComplexObject` resolvers can't see sibling resolver args at runtime, so the parent's `path` doesn't auto-propagate; the agent passes it through where it matters. `recentHistory(limit)` truncates post-walk so the AST-aware blame classifier still sees enough context to label `LOGIC_CHANGE` vs `FORMATTING`.

For raw `find`/`grep` style work, the primitive resolvers mirror the CLI's repeated `--path` support via optional `paths: [...]`:

```graphql
{
  grep(pattern: "process\\.env", path: "apps/web", paths: ["packages/shared"]) {
    hits { filePath lineNumber lineContent }
    exitStatus
  }
}
```

The first data-dependent raw-scan pipe is `grepDirsWith`: find marker files, take their parent directories, then grep only under those dirs. This covers the common "files under packages that have `package.json` AND contain `login`" recipe without shell orchestration:

```graphql
{
  grepDirsWith(
    markerPattern: "(^|/)package\\.json$",
    pattern: "login",
    path: ".",
    around: 2,
    parents: true
  ) {
    hits { filePath lineNumber lineContent parents { kind name lineStart lineEnd } }
    warnings { code message hint }
    exitStatus
  }
}
```

Like `find` and `grep`, `grepDirsWith` accepts optional `paths: [...]` to scan
multiple marker roots in one resolver.

See [24_find_grep_recipes.md](24_find_grep_recipes.md) for the full find/grep recipe matrix and remaining interface improvement notes.

## Aliases (§5.4)

Parallel queries in one round-trip:

```graphql
{
  pattern: search(query: "Pattern", path: "crates/core") { hits { filePath } }
  matcher: search(query: "Matcher", path: "crates/core") { hits { filePath } }
  rule:    search(query: "Rule",    path: "crates/core") { hits { filePath } }
}
```

Three sensor walks; one envelope with `data.pattern`, `data.matcher`, `data.rule`. The walker isn't shared (each runs independently), but the agent collects three results without three subprocess startups.

For scanner-style background jobs, aliases are the current contract. Prefer composing exact evidence resolvers such as `resourceSites`, `configKeys`, `guardSites`, `openapiFacts`, `routeAnchors`, and `httpClientCalls` instead of adding a broad `scan` resolver. See [19_scanner_recipes.md](19_scanner_recipes.md) for ready-to-run templates and the required evidence-only wording rules.

## Variables (§5.3)

Cache a query string and parameterize it:

```bash
cat > context.graphql <<'EOF'
query GetContext($name: String!, $depth: Int!) {
  context(symbol: $name, path: ".", depth: $depth) {
    definition { filePath lineStart }
    callers { filePath lineStart enclosing { name kind } }
    recentHistory { entries { commitHash author changeType } }
  }
}
EOF

lmprobe query @context.graphql --var name='"Pattern"' --var depth=2
```

`context(importAware: true)` is the GraphQL counterpart to `lmprobe context --import-aware`: it follows one explicit relative import/re-export hop from a file-scoped `path` when the requested symbol is not declared in that file.

For AI-parsed failure anchors, GraphQL exposes the same deterministic expansion primitives as the CLI without parsing stacktraces itself:

- `context(symbol: "path/to/file.py:42", path: ".", around: 8)` mirrors `lmprobe context path/to/file.py:42 --around 8`.
- `context(symbol: "target", path: ".", usage: true, around: 2, parents: true, body: true)` mirrors `context --usage ... --around --parents --body`.
- `grep(pattern: "...", path: ".", around: 2, parents: true, body: true, role: true)` mirrors `grep -C 2 --parents --body --role`.
- `def(..., around: 2)` and `refs(..., role: true)` mirror `def --around` and `ref --role`.

`--var` values are JSON-parsed; bare strings need explicit JSON-quoting (`--var name='"Pattern"'`). Numbers, booleans, arrays, and objects don't need quoting (`--var depth=2`, `--var debug=true`, `--var langs='["rust","python"]'`). On parse failure the value falls back to a JSON string. `--vars-file vars.json` accepts a top-level object for bulk vars. Both can combine: file loads first, individual `--var` entries override.

## Introspection

`__schema` and `__type` are free with async-graphql. Agents can self-discover the API:

```graphql
{
  __schema {
    queryType  { fields { name } }
    mutationType { fields { name } }
  }
}
```

```graphql
{ __type(name: "SymbolResult") { fields { name type { name kind } } } }
```

In production daemon mode, you can disable introspection if needed — the one-shot CLI surface keeps it on.

## Typed scalars (§5.7)

The schema uses six typed scalars beyond Int/String/Boolean. Agents get input validation server-side at the GraphQL boundary:

| Scalar | Validation | Use |
|--------|-----------|-----|
| `BlobSha` | 40-char hex | Blob identity in cache + history surfaces |
| `CommitSha` | 40-char hex | Strict commit identity (vs `String` which accepts tags / branches / partial SHAs) |
| `LineNumber` | u32 ≥ 1 | 1-based line numbers, rejects 0 |
| `FilePath` | Non-empty string | Repository-relative or absolute paths |
| `Pattern` | Non-empty string | Ast-grep pattern source (semantic validation deferred to resolver — pattern.try_new requires the language) |
| `Language` | Enum: `BASH \| C \| CPP \| C_SHARP \| ... \| YAML` (29 variants) | Language hint, mirrors `SgLang` |

**Per arbitration D**: validation is parse-time for syntactic shape (40-hex, ≥1) and resolver-time for context-dependent semantics (e.g. "is this a valid ast-grep pattern *for the given language*").

## SymbolHit child resolvers

`SymbolResult` (the GraphQL name for `SymbolHit`) carries four ComplexObject resolvers:

| Field | Args | Returns | Source |
|-------|------|---------|--------|
| `callers(limit, path)` | `limit: Int = 10`, `path: String = "."` | `CallersResult!` | `sensors::callers(self.name, ...)` |
| `callees(limit, path)` | same | `CalleesResult!` | `graph_sensors::callees(self.name, ...)` |
| `recentHistory(limit, path)` | `limit: Int = 3`, `path: String = "."` | `BlameResult!` | `history_sensors::blame_with_follow_semantic` (semantic=false, follow=false) |
| `enclosing` | — | `EnclosingDecl` | `enclosing_decl: Option<String>` + `enclosing_decl_kind: Option<String>` from the parent walk |

`enclosing.kind` is the tree-sitter literal kind (`function_item`, `class_definition`, etc.) of the containing declaration. It is populated by `callers` and by `def(className: ...)`; otherwise it is null when the parent resolver did not compute enclosing context.

## Mutations

Three mutations: `fix` (generic structural rewrite), `trace` (instrumentation, two modes), `untrace` (revert by marker). All three honor the D7 mutation-safety contract identically to the CLI verb counterparts — see [06_mutations.md](06_mutations.md) for the safety contract details (single function, two callers).

### `fix`

```graphql
mutation {
  fix(
    scope: { pattern: { p: "println!($A)", lang: RUST }, paths: ["."] }
    pattern: "println!($A)"
    replacement: "eprintln!($A)"
    dryRun: false
  ) {
    mode  # "dry_run" | "applied"
    matchesCount
    filesCount
    diffs { filePath matchesCount }
    appliedFilesCount
    auditLogPath
    patchPath
  }
}
```

`dryRun: true` (default): returns the dry-run preview. `dryRun: false`: writes the mutation through the existing `fix_apply` path — D7 dirty-tree refusal applies (`errors[].extensions.code = DIRTY_TREE`), audit log written to `$XDG_CACHE_HOME/lmprobe/fix-<ts>.log`, patch captured at `$XDG_CACHE_HOME/lmprobe/patches/fix-<ts>.patch`, worktree-isolated apply.

`scope.pattern.p` is informational today; `scope.paths[0]` is the walker root, `scope.pattern.lang` (or `scope.symbol.lang`) is the language hint, and the top-level `pattern` / `replacement` are the WHAT to rewrite.

### `trace` — transient mode

```graphql
mutation {
  trace(
    scope: { pattern: { p: "fn $NAME($$$ARGS) -> Result<$T>", lang: RUST }, paths: ["src/"] }
    template: { default: "tracing::info!(\"[TRACE-$marker] entering {}\", stringify!($NAME))" }
    marker: "rust-fn-7f3a"
    worktree: true
    exec: "cargo test -p lmprobe --test smoke"
  ) {
    events { output }
    worktreePath
  }
}
```

Provisions a fresh git worktree, instruments the matched sites, runs `exec`, then rolls back. The user's tree is byte-identical post-trace (sha256-verified by the existing 4B implementation). Reuses `sensors::trace_with_safety`.

### `trace` — persistent-marker mode

```graphql
mutation {
  trace(
    scope: { pattern: { p: "expensive_op($$$)", lang: RUST }, paths: ["src/"] }
    template: { default: "log::debug!(\"[TRACE-perf-7f3a] called from {}:{}\", file!(), line!())" }
    marker: "perf-7f3a"
    worktree: false
  ) {
    events { output }
    warnings { code message }  # contains TRACE_AUDIT_LOG
  }
}
```

Persists the instrumentation in the working tree. D7 dirty-tree refusal applies (no worktree to escape into). Audit log written to `$XDG_CACHE_HOME/lmprobe/trace-<marker>.audit.json` BEFORE any mutation, recording per-insertion `file_path` / `line_number` / `inserted_text` so `untrace` can revert deterministically.

Marker validation: `[a-zA-Z0-9_-]{1,64}` per arbitration F. Invalid markers reject with `errors[].extensions.code = INVALID_MARKER`.

The two contradictory mode combos surface as `INVALID_TRACE_MODE`:
- `worktree: true + exec: null` (transient with no exec to run).
- `worktree: false + exec: <cmd>` (persistent + auto-rollback are mutually exclusive).

### `untrace`

```graphql
mutation {
  untrace(marker: "perf-7f3a") {
    mode  # "audit_log" | "regex_fallback"
    revertedCount
    revertedSites { filePath lineNumber revertedText }
    warnings { code message }  # AUDIT_LOG_MISSING when fallback used
  }
}
```

Reads `$XDG_CACHE_HOME/lmprobe/trace-<marker>.audit.json` if present (`mode: "audit_log"`); falls back to a regex scan that deletes any line containing the literal `[TRACE-<marker>]` token (`mode: "regex_fallback"`, with an `AUDIT_LOG_MISSING` warning). D7 dirty-tree refusal applies.

## D7 contract on the GraphQL surface

The same safety-contract function is called from both surfaces. From [06_mutations.md](06_mutations.md):

| Check | CLI exit / stderr | GraphQL `errors[].extensions.code` |
|-------|-------------------|-------------------------------------|
| Dirty working tree on apply | exit 2 + DIRTY_TREE | `DIRTY_TREE` |
| Not a git repository | exit 2 + NOT_GIT_REPO | `NOT_GIT_REPO` |
| Mutation under `--workspace` | exit 2 + WORKSPACE_READ_ONLY | `WORKSPACE_READ_ONLY` |
| Audit log write failed | exit 2 + AUDIT_LOG_WRITE_FAILED | (refusal) |
| Worktree provisioning failed | exit 2 + WORKTREE_PROVISION_FAILED | (refusal) |

Audit logs always live at `$XDG_CACHE_HOME/lmprobe/`, regardless of which surface invoked the mutation.

## Exit-code semantics (per arbitration I)

`lmprobe query` maps the envelope's `exit_status` to a process exit code identical to the rest of the CLI:

| Envelope `exit_status` | Process exit code | Meaning |
|------------------------|-------------------|---------|
| `matched` | 0 | The query completed and returned at least one hit. |
| `no_match` | 1 (collapsible to 0 with `--exit-zero-on-no-match`) | The query completed; no hits. |
| `error` | 2 | `errors[]` is non-empty (parse error, resolver error, validation error, D7 refusal). |
| grep `partial: true` | direct CLI exits 3; GraphQL clients inspect the field | The resolver returned usable evidence but incomplete coverage; inspect `warnings`. This is an additive field, not an `ExitStatus` enum variant. |

`exit_status` is based on resolver execution state, not the selected response fields. A no-hit resolver still makes the top-level query `no_match` even if the selection set asks only for scalar metadata such as `schema`. For introspection-only documents where no lmprobe resolver runs, `query` falls back to a conservative response-shape inference.

Errors mirror to stderr by default in the standard `lmprobe: query [<CODE>]: <message>` format. `-q / --quiet` suppresses the mirror; `errors[]` and `warnings[]` still ship on the wire.

## Surface equality vs the CLI

For every read sensor, the GraphQL response data has the same shape as the CLI envelope, modulo two channel cosmetic differences:

- **Field names**: CLI is snake_case (`file_path`); async-graphql auto-camelCases (`filePath`). The schema is camelCase by convention.
- **Enum values**: CLI ships `exit_status: "matched"`; async-graphql emits `exitStatus: "MATCHED"` (SCREAMING_SNAKE_CASE). Coverage partiality is an additive `partial` boolean, not an enum value.

**Surface naming**: `ref` (CLI) ↔ `refs` (GraphQL). This is the only verb that renames between surfaces — `query '{ ref(...) }'` will fail with "unknown field"; use `refs(name: ..., path: ...)`. All other verbs use the same name on both surfaces.

Past those, the data is byte-equal post-canonicalization. The surface-equality test (`crates/lmprobe/tests/graphql_surface_equality.rs`) asserts this on 5 read sensors + 2 D7 diagnostic-equivalence cases.

## Read next

- [06_mutations.md](06_mutations.md) for the D7 mutation-safety contract details.
- [08_envelope_schema.md](08_envelope_schema.md) for the per-verb envelope shapes the GraphQL surface returns.
- [12_optional_layers.md](12_optional_layers.md) for the long-running HTTP daemon (`lmprobe serve`).
- [14_diagnostics.md](14_diagnostics.md) for the diagnostic-code reference table (`DIRTY_TREE`, `INVALID_MARKER`, etc.).
- [24_find_grep_recipes.md](24_find_grep_recipes.md) for common `find`/`grep` translations and GraphQL internal-pipe recipes.
