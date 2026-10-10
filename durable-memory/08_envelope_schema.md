# 08 — Envelope schema (D4)

The schema-first principle: every sensor emits a `serde::Serialize` envelope declared in `crates/lmprobe-types/src/schema.rs`. CLI markdown / JSON / GraphQL all render from the same shape — no per-channel drift.

## Envelope shape

Every envelope has the same three fields plus a per-verb item collection:

```rust
pub struct <Verb>Result {
    pub schema: String,                   // "lmprobe.<verb>.v1"
    pub <items_field>: Vec<<ItemType>>,   // hits / projects / paths / nodes / etc.
    #[serde(default)]
    pub warnings: Vec<Diagnostic>,
    #[serde(default)]
    pub exit_status: ExitStatus,
}
```

The macro `envelope!(Name, "lmprobe.x.v1", field_name: Vec<Type>)` in `crates/lmprobe-types/src/schema.rs` generates this struct shape. Bespoke envelopes (Evolution, Trace, Impact, Context, BlameResult, FixResult) are hand-written when they need first-class context fields beyond a single Vec.

## Schema namespace versioning

The `schema` string has the format `lmprobe.<verb>.v<N>`. Examples:

- `lmprobe.search.v1`
- `lmprobe.def.v1`
- `lmprobe.history_blame.v1`
- `lmprobe.context.v1`
- `lmprobe.ast_diff.v1`
- `lmprobe.fix.v1`

Bumping `vN` is the only allowed way to change a field's *meaning*. New optional fields are additive within v1 — `#[serde(default, skip_serializing_if = "Option::is_none")]` keeps old envelopes deserialisable. Pinned GraphQL clients can detect breaking changes by checking the schema string.

`v2` bumps require explicit Lead arbitration. Throughout phases 0-4, no envelope has bumped past v1.

## Full envelope inventory

| Verb | Schema | Envelope struct | Items field |
|------|--------|-----------------|------------|
| discover | `lmprobe.discover.v1` | `DiscoverResult` | `projects: Vec<Project>` |
| list-kinds | `lmprobe.list_kinds.v1` | `ListKindsResult` | `rows: Vec<PortableKindRow>` |
| search | `lmprobe.search.v1` | `SearchResult` | `hits: Vec<SymbolHit>` |
| def | `lmprobe.def.v1` | `DefResult` | `hits: Vec<SymbolHit>` |
| ref | `lmprobe.ref.v1` | `RefResult` | `hits: Vec<GrepHit>` |
| callers | `lmprobe.callers.v1` | `CallersResult` | `hits: Vec<SymbolHit>` |
| callsites | `lmprobe.callsites.v1` | `CallsitesResult` | `callsites: Vec<CallsiteArgsHit>` |
| imports | `lmprobe.imports.v1` | `ImportsResult` | `hits: Vec<SymbolHit>` |
| decorations | `lmprobe.decorations.v1` | `DecorationsResult` | `hits: Vec<SymbolHit>` |
| grep | `lmprobe.grep.v1` | `GrepResultEnvelope` | `hits: Vec<GrepHit>` |
| find | `lmprobe.find.v1` | `FindResult` | `paths: Vec<String>` |
| py-slice-candidates | `lmprobe.py_slice_candidates.v1` | `PySliceCandidatesResult` | `candidates: Vec<PySliceCandidate>` |
| diff-context | `lmprobe.diff_context.v1` | `DiffContextResult` | `anchors: Vec<DiffContextAnchor>` |
| resource-sites | `lmprobe.resource_sites.v1` | `ResourceSitesResult` | `sites: Vec<ResourceSite>` |
| async-leaks | `lmprobe.async_leaks.v1` | `AsyncLeaksResult` | `sites: Vec<AsyncLeakSite>` |
| crypto-sites | `lmprobe.crypto_sites.v1` | `CryptoSitesResult` | `sites: Vec<CryptoSite>` |
| config-keys | `lmprobe.config_keys.v1` | `ConfigKeysResult` | `keys: Vec<ConfigKeyRecord>` |
| guard-sites | `lmprobe.guard_sites.v1` | `GuardSitesResult` | `candidates: Vec<GuardSiteCandidate>` |
| openapi-facts | `lmprobe.openapi_facts.v1` | `OpenApiFactsResult` | `operations: Vec<OpenApiOperationFact>` |
| route-anchors | `lmprobe.route_anchors.v1` | `RouteAnchorsResult` | `routes: Vec<RouteAnchor>` |
| http-client-calls | `lmprobe.http_client_calls.v1` | `HttpClientCallsResult` | `calls: Vec<HttpClientCallAnchor>` |
| callees | `lmprobe.callees.v1` | `CalleesResult` | `hits: Vec<SymbolHit>` |
| supertypes | `lmprobe.supertypes.v1` | `SupertypesResult` | `hits: Vec<SymbolHit>` |
| subtypes | `lmprobe.subtypes.v1` | `SubtypesResult` | `hits: Vec<SymbolHit>` |
| overrides | `lmprobe.overrides.v1` | `OverridesResult` | `hits: Vec<SymbolHit>` |
| exports | `lmprobe.exports.v1` | `ExportsResult` | `hits: Vec<SymbolHit>` |
| impact | `lmprobe.impact.v1` | `ImpactResult` | `nodes`, `edges` |
| context | `lmprobe.context.v1` | `ContextResult` | `definition`, `callers`, `callees`, `imports`, `recent_history`, `todos_inline`, `file_outline`, `usages?: Vec<GrepHit>`, `source_context?: Vec<LineContext>` |
| evolution | `lmprobe.evolution.v1` | `EvolutionResult` | `symbol`, `milestones: Vec<Milestone>` |
| history blame | `lmprobe.history_blame.v1` | `BlameResult` | `entries`, `follows: Vec<FileRename>`, `source_context?: Vec<LineContext>` |
| history introduced | `lmprobe.history_introduced.v1` | `IntroducedResult` | `entries: Vec<BlameEntry>` |
| history commit | `lmprobe.history_commit.v1` | `HistoryCommitResult` | `hits: Vec<CommitHit>` |
| history diff | `lmprobe.history_diff.v1` | `HistoryDiffResult` | `hits: Vec<DiffHit>` |
| history churn | `lmprobe.history_churn.v1` | `HistoryChurnResult` | `rows: Vec<ChurnEntry>` |
| history co-change | `lmprobe.history_co_change.v1` | `HistoryCoChangeResult` | `rows: Vec<CoChangeEntry>` |
| history todos | `lmprobe.history_todos.v1` | `HistoryTodosResult` | `hits: Vec<TodoEntry>` |
| fix | `lmprobe.fix.v1` | `FixResult` | `mode`, `diffs: Vec<FileDiff>`, `matches_count`, `applied_files_count`, `patch_path?`, `audit_log_path?` |
| ast-diff | `lmprobe.ast_diff.v1` | `AstDiffResult` | `nodes: Vec<NodeDiff>` |
| trace | `lmprobe.trace.v1` | `TraceRunResult` | `events: Vec<TraceEvent>`, `worktree_path?`, `patch_path?` |

## Item shapes

### `TraceEvent`

Legacy trace events expose `timestamp` and `output`. Python watch mode (`trace --watch ... --json-trace`) preserves those fields and adds optional structured runtime evidence fields: `var`, `type`, `id`, `len`, `shape`, and `repr`.

### `SymbolHit`

The most-used shape. Used by search/def/callers/callees/imports/exports/decorations/supertypes/subtypes/overrides.

```rust
pub struct SymbolHit {
    pub name: String,
    pub kind: String,         // tree-sitter literal kind (e.g. "function_item")
    pub lang: String,         // SgLang Debug repr (e.g. "Rust")
    pub file_path: String,
    pub line_start: u32,
    pub content: String,      // matched node's text
    pub origin_root: Option<String>,    // §4.7 workspace tag (None outside --workspace)
    pub enclosing_decl: Option<String>, // enclosing decl name when populated (callers, def --class)
    pub enclosing_decl_kind: Option<String>, // tree-sitter kind for enclosing_decl
    pub source_context: Vec<LineContext>, // omitted when empty
}
```

### `GrepHit`

Line-shaped. Used by `ref`, `grep`. In JSON, the matched source line is `line_content`; v1 does not define a `line` alias.

```rust
pub struct GrepHit {
    pub file_path: String,
    pub line_number: usize,
    pub line_content: String,
    pub before_context: Vec<LineContext>, // omitted when empty
    pub after_context: Vec<LineContext>,  // omitted when empty
    pub parents: Vec<StructuralParent>,   // omitted when empty
    pub body: Option<StructuralBody>,      // omitted when absent
    pub role: Option<String>,              // omitted when absent
    pub origin_root: Option<String>,    // §4.7 workspace tag
}

pub struct LineContext {
    pub line_number: usize,
    pub line_content: String,
}

pub struct StructuralParent {
    pub kind: String,
    pub name: Option<String>,
    pub line_start: usize,
    pub line_end: usize,
}

pub struct StructuralBody {
    pub kind: String,
    pub name: Option<String>,
    pub line_start: usize,
    pub line_end: usize,
    pub content: String,
}
```

For `grep`, `-A` / `-B` / `-C` populate `before_context` and/or `after_context` with bounded surrounding source lines. `grep --parents` populates `parents` for parseable files, `grep --body` populates the innermost containing structural body when one exists, and `grep --role` / `ref --role` populate optional Python-first local role labels. Without expansion flags, compact JSON/NDJSON hits omit those fields.

### `PySliceCandidate`

Evidence-only low-confidence candidate emitted by `py-slice-candidates`. Fields include `file_path`, optional `enclosing_class` / `enclosing_function`, `sliced_base_variable`, `derived_sliced_variable`, `slice_expression`, `related_unsliced_variable`, `call_expression_line`, `slice_line_number`, `call_line_number`, `slice_source_line`, `call_source_line`, and `confidence`.

With `--trace-plan`, each candidate also includes `candidate_id` and `trace_plan { file_path, statement_pattern, watch_variables, suggested_line_number, enclosing_function?, warnings? }`. These fields are omitted by default.

### `CallsiteArgsHit`

Evidence-only callsite argument item emitted by `callsites --args`. Fields include `file_path`, `line_number`, `callee_text`, `positional_args`, `keyword_args: Vec<{ name, value }>`, and optional `source_context`.

### `DiffContextAnchor`

Evidence-only changed-line anchor emitted by `diff-context`. It represents a new-side line from `git diff --unified=0`, not a semantic diff conclusion.

Fields include `file_path`, `line_number`, `line_content`, `anchor_kind: "changed_line"`, hunk metadata (`old_start`, `old_lines`, `new_start`, `new_lines`), and optional `source_context`, `parents`, and `body` when `--around`, `--parents`, or `--body` are requested.

### `ResourceSite`

Evidence-only local acquire/release grouping emitted by `resource-sites`. Fields include `file_path`, optional `enclosing`, `acquire_calls`, `release_calls`, `defer_calls`, `error_branches`, optional JS/TS classifications (`acquire_timing`, `release_lifecycle`, `optional_acquire`), optional `source_context`, `parents`, `body`, and `confidence: "local-evidence-only"`.

For Go scans, `error_branches` includes return anchors after a matched acquire and before the first matched release/defer in the same enclosing declaration, excluding the immediate acquire-failure guard.

For JavaScript, TypeScript, and TSX scans, classification fields are omitted unless syntactic evidence is present. They describe local source shape only: lazy assignment/fallback acquisition, dynamic import, event-handler acquisition, and page/dispose-style release anchors.

### `AsyncLeakSite`

Evidence-only JS/TS call-anchor item emitted by `async-leaks`. Fields include `file_path`, `line_number`, `line_content`, `callee`, `awaited`, `returned`, optional `source_context`, optional `parents`, optional `body`, and `confidence: "syntax-evidence-only"`.

`async-leaks` matches configured call names such as `stop`, including direct member calls and optional-chained calls like `worker.stop()` / `worker?.stop()`. The booleans only describe local syntax; the command does not prove Promise return types or cleanup correctness.

### `CryptoSite`

Evidence-only Go crypto algorithm anchor emitted by `crypto-sites`. Fields include `file_path`, `line_number`, `line_content`, normalized `algorithm`, `match_text`, `import_paths`, optional `nearby_guards: Vec<CryptoGuardAnchor>`, optional `source_context`, optional `parents`, optional `body`, and `confidence: "syntax-evidence-only"`.

`CryptoGuardAnchor` fields include `match_text`, `line_number`, and `line_content`. With `--fips-check`, these anchors mechanically collect nearby FIPS/config/build-tag mentions; they are not pass/fail decisions.

### `ConfigKeyRecord`

Evidence-only config key fact emitted by `config-keys`. Fields include `file_path`, `lang`, `key_path`, leaf `key`, best-effort `line_number`, `value_kind`, optional scalar `value_summary`, and `code_anchors: Vec<ConfigCodeAnchor>`. `config-keys --key <KEY_PATH>` filters records before `code_anchors` are attached; it does not change the envelope shape.

`ConfigCodeAnchor` fields include `file_path`, `line_number`, `line_content`, optional `role`, optional `parents: Vec<StructuralParent>`, and optional `source_context: Vec<LineContext>`. Anchors are lexical code references to the same leaf key text, not data-flow findings.

### `GuardSiteCandidate`

Evidence-only local sensitive-action / guard-anchor item emitted by `guard-sites`. Fields include `file_path`, `sensitive: GuardCallSite`, `nearby_guards: Vec<GuardCallSite>`, `decorations: Vec<GuardCallSite>`, optional `parents`, optional `body`, optional `source_context`, and `confidence: "local-evidence-only"`.

`GuardCallSite` fields include `match_text`, `line_number`, and `line_content`.

### API Fact Shapes

`OpenApiOperationFact` is emitted by `openapi-facts` and includes `file_path`, `method`, `path`, optional `operation_id`, `required_params`, and `response_schemas`.

`RouteAnchor` is emitted by `route-anchors` and includes `file_path`, `method`, `path`, `line_number`, `line_content`, optional `handler`, optional `source_context`, optional `parents`, optional `body`, and `confidence: "literal-evidence-only"`.

`HttpClientCallAnchor` is emitted by `http-client-calls` and includes `file_path`, optional `method`, `url`, `line_number`, `line_content`, optional `client`, optional `source_context`, optional `parents`, optional `body`, and `confidence: "literal-evidence-only"`.

### `ScanStats`

No-match `search` and `grep` JSON envelopes include lightweight scan counters:

```rust
pub struct ScanStats {
    pub files_considered: usize,        // file entries yielded after root/glob/ignore filters
    pub files_scanned: usize,           // files read by the sensor
    pub files_skipped: usize,           // considered files skipped before scanning, e.g. FILE_TOO_LARGE
    pub files_ignored: Option<usize>,   // files the walker excluded via .gitignore/.ignore/global-ignore rules
}
```

The field is emitted as `scan_stats` only on no-match `search`/`grep` responses.

`files_skipped` does NOT count `files_ignored` — they're disjoint populations.
`files_skipped` files were considered (they passed ignore filtering, i.e. they're
part of `files_considered`) and then skipped before reading; `files_ignored`
files were excluded by the walker before ever being considered at all. Dogfood
finding (2026-09): a caller reading only `files_skipped: 0` on a `no_match`
response reasonably concludes "nothing was excluded," which is false when
`.gitignore` hid the file that would have matched — `files_ignored` is the
separate, honest counter for that. It's `Option<usize>`: `None` means the
sensor doesn't compute it (an honest "not measured," never a claimed zero),
`Some(n)` means it does. Currently only `grep` computes it; `search` always
reports `None`. See `GITIGNORE_PATHS_EXCLUDED` in `14_diagnostics.md` for the
companion warning, which fires for both `find` and `grep` regardless of
match/no-match (unlike `scan_stats`, which stays no-match-only).

### `Project`

Used by `discover`.

```rust
pub struct Project {
    pub name: String,
    pub project_type: String,     // "Cargo", "Npm", "Python/PyProject", "Lua/Neovim", etc.
    pub version: String,
    pub path: String,
    pub dependencies_count: usize,
    pub children: Vec<String>,    // §2.1 hierarchy: paths of nested projects
}
```

### `Diagnostic`

```rust
pub struct Diagnostic {
    pub code: String,             // stable identifier — see 14_diagnostics.md
    pub message: String,
    pub hint: Option<String>,
    pub lang: Option<String>,     // populated for per-language diagnostics
}
```

### `ExitStatus`

```rust
pub enum ExitStatus { Matched, NoMatch, Error }
```

The CLI maps these to process exit codes 0/1/2 (with `--exit-zero-on-no-match`
collapsing 1→0). Coverage-affecting diagnostics additively expose
`partial: true` in supported envelopes and map that condition to process exit
code 3 without adding a new `ExitStatus` variant. See
[14_diagnostics.md](14_diagnostics.md).

## v1-additive policy

Allowed within v1:
- Add a new optional field with `#[serde(default)]` and `#[serde(default, skip_serializing_if = "...")]` for `Option`/`Vec`.
- Add a new `Diagnostic` code.
- Add a new variant to a leaf enum (e.g. extending the `change_type` taxonomy on `BlameEntry`).
- Do **not** add variants to protocol-control enums like `ExitStatus` lightly; strict clients commonly deserialize those exhaustively. Prefer an additive field such as `partial: true` when the existing status enum can stay stable.

Requires v2:
- Removing a field.
- Renaming a field.
- Changing the meaning of an existing field.
- Changing a field's type to one that breaks existing serialised values.

Phase 4D and earlier added several v1-additive fields (`origin_root`, `enclosing_decl`, `worktree_path`, `patch_path`, `audit_log_path`, `applied_files_count`, `follows`, etc.). Pre-existing envelopes deserialise cleanly via `#[serde(default)]`.

## Reading envelopes

```bash
# pretty-print
lmprobe --format json search Pattern . | jq

# extract one field
lmprobe --format json search Pattern . | jq '.hits[0].file_path'

# count hits
lmprobe --format json search Pattern . | jq '.hits | length'

# check for warnings
lmprobe --format json def NotASymbol . | jq '.warnings[].code'

# pin the schema string in a script (defensive)
lmprobe --format json discover . \
  | jq -e 'select(.schema == "lmprobe.discover.v1")'
```

The `schema` field is intended to be checked by pinned tooling; if you see `lmprobe.search.v2` in the future, your tool must explicitly opt in (or fall back).

## Read next

- [09_cache.md](09_cache.md) for the SQLite cache that backs evolution + history verbs.
- [11_extending.md](11_extending.md) for adding a new envelope (new sensor).
- [14_diagnostics.md](14_diagnostics.md) for the diagnostic-code reference.
