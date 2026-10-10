# `lmprobe` Architecture

**Status:** canonical (2026-05-01). Supersedes the earlier pre-fork
planning documents, which are not part of this repository.

This document is the load-bearing reference for D1–D7. One paragraph
per decision.

## Feature Layering

The lightweight CLI is the product. GraphQL is part of the default
core surface (`lmprobe query '<gql>'` runs in-process via a
current-thread tokio runtime). The HTTP daemon `serve` is the only
opt-in layer.

| Feature | Default | What it adds | Extra deps pulled in |
| ------- | ------- | ------------ | -------------------- |
| (none)  | yes     | All CLI verbs (`search`, `def`, `ref`, `callers`, `imports`, `decorations`, `evolution`, `find`, `grep`, `discover`, `trace`, `list-kinds`, `fix`, `impact`, `context`, `history *`) plus `lmprobe query '<gql>'` for one-shot GraphQL execution. `serve` parses but errors at runtime. | core, config, language, ignore, anyhow, serde, serde_json, serde_yaml, clap, toml, regex, chrono, git2, rusqlite, async-graphql, tokio (rt + macros) |
| `serve` | no      | `serve [--addr …] [--auth-token …]` — long-running GraphQL HTTP daemon over axum with bearer-token middleware (constant-time compare via `subtle`). | async-graphql-axum, axum, tokio (rt-multi-thread + net), subtle |

Build matrix:

```
cargo build -p lmprobe                       # lightweight CLI + `query`
cargo build -p lmprobe --features serve      # adds the HTTP daemon
cargo build -p lmprobe --all-features        # everything
```

Running `lmprobe serve` on a default build returns:

> the `serve` subcommand requires `--features serve`; rebuild with `cargo build --features serve`

### Test matrix

```
cargo test -p lmprobe                                # smoke (deep + perf are #[ignore])
cargo test -p lmprobe --features serve --test auth   # bearer-token integration test
cargo test -p lmprobe --all-features                 # everything
cargo test -p lmprobe --test deep -- --ignored       # 29-language deep tier
cargo test -p lmprobe --test perf -- --ignored       # CC3 perf gate
cargo test -p lmprobe --test history_perf -- --ignored  # 3A history perf gate
```

`tests/auth.rs` is `#![cfg(feature = "serve")]`-gated so the
default test run skips it cleanly (0 tests run when the feature is
absent).

### CC2 — smoke tier wall-clock target

CC2's "smoke <30 s" applies to **warm-cache test wall-clock** (i.e.
`cargo test --test smoke` with deps + lmprobe already linked).
Cold runs (after `cargo clean -p lmprobe`) include ~30 s of cargo
machinery — link, test-binary compile, harness startup — that's
fixed cargo overhead, not smoke-tier work. Today's numbers
(end-of-Phase-3-PB2):

| Mode                              | Wall-clock | Test runtime line |
| --------------------------------- | ---------- | ----------------- |
| Cold (`cargo clean -p lmprobe`)  | ~37 s      | 6.5 s             |
| Warm (back-to-back run)           | ~6.6 s     | 6.5 s             |

Phase 3B-PB2 parallelised the main case loop with
`std::thread::scope` (each case spawns its own lmprobe process; OS-
level concurrency cuts wall-clock ~6× on an 8-core box). Pre-PB2
warm wall-clock was ~12 s; post-PB2 ~6.6 s. Trace/pipe special
cases stay sequential — they provision PID-named tmp dirs and the
gain wasn't worth the harness complexity.

### Perf gate isolation

`tests/perf.rs` and `tests/history_perf.rs` are timing-sensitive.
Run them isolated (their own `cargo test --test perf` invocation,
or in a dedicated CI step) — running them concurrently with
smoke / unit tests can push samples over threshold under load.
Either run with `--test-threads=1` or split into separate cargo
test invocations. `LMPROBE_PERF_RELAX=N` loosens every threshold
by a multiplier for noisy CI hosts.

## D1 — Workspace split (Phase 5A)

Phase 0 §0.3 split the binary's single-file 670-LOC layout into
eight modules (`schema`, `cli`, `sensors`, `langs`, `index`,
`history`, `output`, `error`). Phase 5A promoted those modules
to dedicated workspace crates so per-language Coder sessions can
own one crate each and the dispatch cycle is resolved cleanly:

| Crate | Owns | Depends on |
|-------|------|------------|
| `lmprobe-types` | D4 envelope shapes (schema.rs), typed scalars (gql_scalars.rs), error aliases (error.rs). Pure data; no parser deps beyond `ast-grep-language` for the `Language` ↔ `SgLang` bridge. | serde, async-graphql (derives), chrono, ast-grep-language |
| `lmprobe-lang-core` | D5 trait definitions + per-language impl structs (during 5A; impls migrate to per-lang crates over 5B). | lmprobe-types, ast-grep-core, ast-grep-language |
| `lmprobe-registry` | Per-language dispatch fan-out (`name_extractor_for(SgLang)` etc.). Each dispatch fn has `xtask:per-lang:<fn>:start/:end` marker regions where xtask inserts per-language arms. **Resolves the W2 cycle**: lang-core has no dep on per-lang crates; registry depends on both. | lmprobe-lang-core, lmprobe-types, all `lmprobe-lang-*` crates as they're added |
| `lmprobe-engine` | Sensors (search, def, callers, history, …) + cache + walk helpers. Sensors call `lmprobe_registry::*_for(lang)` for dispatch — never `match lang {…}` in sensor code. | lmprobe-registry, lmprobe-lang-core, lmprobe-types, ast-grep-{core,config,language}, ignore, git2, rusqlite, async-graphql, … |
| `lmprobe-lang-rust` | Rust D5 impls (5A.3 first extraction). | lmprobe-lang-core, lmprobe-types, ast-grep-core, ast-grep-language |
| `lmprobe` (binary) | clap CLI + dispatcher + GraphQL Query/Mutation/build_schema + output renderers + `--workspace` fan-out. 4 source files (cli.rs, output.rs, workspace.rs, main.rs). | All four foundation crates + lmprobe-lang-rust |
| `xtask` | `cargo xtask new-lang <name>` + `cargo xtask check-lang [<name>]` + workspace tooling (schema, release). | toml_edit, serde_json, anyhow |

Dependency arrows are acyclic at every step: `binary → engine →
registry → {lang-core, lmprobe-lang-*} → types`. Adding a 30th
language (post-1.0) is one xtask invocation + one Coder session.

## D2 — Symbol-index location & cache role

Live sensors are stateless: they parse the filesystem on demand. The
SQLite cache memoizes **immutable Git blobs only** (history,
evolution, blame). Cache key is the tuple
`(blob_sha, language, sensor_version, engine_version, schema_version)`;
the live symbol index is never persisted. The pre-Phase-0 design
that wrote to `<root>/.lmprobe/history.db` is superseded — the cache
now lives at `$XDG_CACHE_HOME/lmprobe/` (see D6).

## D3 — Pattern engine

`lmprobe` reuses `ast_grep_core::Pattern` directly for every
structural query. The pre-Phase-0 in-house `--has-X` substring engine
was a truth-in-labelling violation (R1 A5, QA §2.10) and is removed
in Phase 1.2 in favour of the relational rules (`has`, `inside`,
`precedes`, `follows`) defined in `crates/config/src/rule_config.rs`.

## D4 — Output schema

Program mode (`--json`, `--outfile`, `--infile`) adds a separate
`lmprobe.result.v1` transport with source_schema, status, payload_format, payload,
metadata, and diagnostics. Legacy sensor/GraphQL schema meanings and
`--format json` output stay intact. Request-scoped GraphQL completion records
coverage/diagnostics before projection. Output preflight does not create temp
files before scans or dirty-tree checks; atomic file staging occurs after result
serialization. See `durable-memory/program-json.md` and the linked JSON Schema.


Every sensor emits a single `serde::Serialize` shape declared in
`crates/lmprobe-types/src/schema.rs` (relocated from
`crates/lmprobe/src/schema.rs` in Phase 5A). CLI markdown / JSON /
GraphQL all render from those shapes — no per-sensor render code,
no drift between channels. Schema namespaces are versioned: each envelope
carries a `schema` string of the form `lmprobe.<verb>.v<N>`. Bumping
`vN` is the only way to change a field's meaning; new optional fields
are additive within a version. Phase 0 Chunk 0B added `warnings:
Vec<Diagnostic>` and `exit_status` to every envelope additively in
v1.

## D5 — Language registry

Per-language behaviour is composed from capability traits, not a
monolithic struct. Phase 0 §0.5 installs `NameExtractor` and
`CallGrammar`; Phase 1 §1.4 adds `KindTaxonomy`; Phase 2 broadens
`ImportGrammar` and `DecorationGrammar`; Phase 3+ expands further.
Every QA blocker traces to a missing trait implementation rather
than a missing field. Languages opt into the traits they support;
the `_` arm of each registry returns a generic fallback or a
`NOT_IMPLEMENTED` diagnostic.

Phase 5A relocated the trait definitions to
`crates/lmprobe-lang-core/src/langs.rs` and split the dispatch
into a dedicated `crates/lmprobe-registry/` crate. The registry's
8 dispatch fns each have `match lang { ... }` blocks with
`xtask:per-lang:<fn>:start/:end` marker regions; `cargo xtask
new-lang <name>` inserts a `SgLang::<Lang> =>
&lmprobe_lang_<lang>::<Lang><Trait>` arm into each region. The
catch-all `_ =>` arm delegates to lang-core's monolithic dispatch
for languages that haven't been extracted to a dedicated
`lmprobe-lang-<name>` crate yet. 5A.3 extracted Rust as the first
heavy lang; 5B fans out the remaining 28.

See `durable-memory/11_extending.md` for the per-language workflow + xtask
invocations.

## D6 — Telemetry-free defaults / XDG cache

The cache lives at `$XDG_CACHE_HOME/lmprobe/` (defaulting to
`~/.cache/lmprobe/`). Working trees are never written to. The
`--no-cache` global flag opts out entirely. The legacy
`<root>/.lmprobe/history.db` path is read for back-compat (with a
`DEPRECATED_CACHE_PATH` diagnostic); new writes always go to XDG.

### Cache Schema (Phase 3A §3.1)

The SQLite database `$XDG_CACHE_HOME/lmprobe/history.db` holds four
tables. Three version constants in `crates/lmprobe/src/history.rs`
gate cache validity: `SCHEMA_VERSION` (bumped on column changes),
`SENSOR_VERSION` (per-sensor result-shape changes), `ENGINE_VERSION`
(ast_grep_core dep version). All four cache tables include the
relevant version columns so old rows stay readable but unused after
a bump.

| Table              | Key                                                                              | Purpose                                                       |
|--------------------|----------------------------------------------------------------------------------|---------------------------------------------------------------|
| `symbol_milestones`| `(symbol_name, commit_hash)`                                                     | Per-symbol evolution milestones (Phase 1.8). Back-compat unchanged. |
| `repo_commits`     | `(repo_root, schema_version, commit_hash)`                                       | Repo-wide commit metadata (author / email / ts / message). `files_changed` is JSON-encoded; populated lazily per query.                                                |
| `repo_head`        | `(repo_root, schema_version)`                                                    | Last-walked HEAD per repo. Driver of incremental refresh — only commits since the previous HEAD are re-walked.                                                        |
| `blob_signatures`  | `(blob_sha, language, sensor_version, engine_version, schema_version, symbol_name)` | Per-(blob, name) AST signature + content-hash for the blame heuristic. Reserved for Phase 4 (not yet populated; the `last_blob_oid` skip in `blame_symbol` covers same-blob de-dup at runtime). |

Cache surfaces:
- **Blob-keyed**: `(blob_sha, lang, sensor_version, engine_version, schema_version)` — stable forever per blob; only grows.
- **Repo-wide**: `(repo_root, schema_version, head_sha)` — invalidates only on HEAD advance; the incremental refresh walks `[old_head, new_head]` only, never the full revwalk.

Performance characteristics (cobra fixture, ~500 commits):
- Cold-cache `history commit/churn/co-change/todos`: ~120-150ms wall (transaction-batched commit inserts; `files_changed` deferred to query time).
- Warm-cache same: ~45-50ms (SQLite read only).
- Cold-cache `history blame/introduced`: ~1.4s (walk + parse the file's commit-touches AST-aware).
- Warm-cache same: ~1.3s (cache populated; AST-parse cost dominates — `blob_signatures` cache will close this in Phase 4).

## Sensor Inventory

The CLI exposes **27 result-producing verbs** plus two interface
verbs:
- `query` (Phase 4E-A) — runs a GraphQL document against the
  in-process schema; default-built, no feature flag.
- `serve` (opt-in, `--features serve`) — wraps the same schema in
  a long-running HTTP daemon.

Phase 4E covers all 27 read sensors over GraphQL (24 top-level
fields plus the 7 `HistoryQueries` sub-fields after the
`history evolution` ↔ `evolution` alias); a few sensors are
exposed under nested types rather than top-level Query fields.
The 3-mutation surface (`fix` / `trace` / `untrace`) lands in the
same chunk.

| # | Verb | Envelope | Schema namespace |
| - | ---- | -------- | ---------------- |
| 1 | `search`      | `SearchResult`        | `lmprobe.search.v1` |
| 2 | `def`         | `DefResult`           | `lmprobe.def.v1` |
| 3 | `ref`         | `RefResult`           | `lmprobe.ref.v1` |
| 4 | `callers`     | `CallersResult`       | `lmprobe.callers.v1` |
| 5 | `imports`     | `ImportsResult`       | `lmprobe.imports.v1` |
| 6 | `decorations` | `DecorationsResult`   | `lmprobe.decorations.v1` |
| 7 | `discover`    | `DiscoverResult`      | `lmprobe.discover.v1` |
| 8 | `evolution`   | `EvolutionResult`     | `lmprobe.evolution.v1` |
| 9 | `trace`       | `TraceRunResult`      | `lmprobe.trace.v1` |
| 10 | `grep`       | `GrepResultEnvelope`  | `lmprobe.grep.v1` |
| 11 | `find`       | `FindResult`          | `lmprobe.find.v1` |
| 12 | `serve`      | (interface only — exposes the other 11 over GraphQL) | — |
| 13 | `callees`    | `CalleesResult`       | `lmprobe.callees.v1` (Phase 3B §3.3) |
| 14 | `supertypes` | `SupertypesResult`    | `lmprobe.supertypes.v1` (Phase 3B §3.3) |
| 15 | `subtypes`   | `SubtypesResult`      | `lmprobe.subtypes.v1` (Phase 3B §3.3) |
| 16 | `overrides`  | `OverridesResult`     | `lmprobe.overrides.v1` (Phase 3B §3.3) |
| 17 | `exports`    | `ExportsResult`       | `lmprobe.exports.v1` (Phase 3B §3.3) |
| 18 | `impact`     | `ImpactResult`        | `lmprobe.impact.v1` (Phase 3B §3.2) |
| 19 | `context`    | `ContextResult`       | `lmprobe.context.v1` (Phase 3B §3.4) |
| 20 | `history blame` | `BlameResult`      | `lmprobe.history_blame.v1` (Phase 3A §3.1) |
| 21 | `history introduced` | `IntroducedResult` | `lmprobe.history_introduced.v1` (Phase 3A §3.1) |
| 22 | `history commit` | `HistoryCommitResult` | `lmprobe.history_commit.v1` (Phase 3A §3.1) |
| 23 | `history diff` | `HistoryDiffResult`  | `lmprobe.history_diff.v1` (Phase 3A §3.1) |
| 24 | `history churn` | `HistoryChurnResult` | `lmprobe.history_churn.v1` (Phase 3A §3.1) |
| 25 | `history co-change` | `HistoryCoChangeResult` | `lmprobe.history_co_change.v1` (Phase 3A §3.1) |
| 26 | `history todos` | `HistoryTodosResult` | `lmprobe.history_todos.v1` (Phase 3A §3.1) |
| 27 | `fix`        | `FixResult`           | `lmprobe.fix.v1` (Phase 4B §4.1 dry-run / 4C §4.1 apply) |
| 28 | `ast-diff`   | `AstDiffResult`       | `lmprobe.ast_diff.v1` (Phase 4C §4.8) |

Phase 1.7 (Chunk 1C) lifted GraphQL parity for the first 10 read
sensors (discover / find / grep / search / def / refs / callers /
imports / decorations / evolution); Phase 4E extends that to the
full read surface (callees / supertypes / subtypes / overrides /
exports / impact / context / list_kinds / ast_diff plus the seven
`history *` resolvers under a nested `HistoryQueries` type) and
adds the 3-mutation surface (`fix` / `trace` / `untrace`). Every
resolver returns the **envelope** (with `warnings` and `exit_status`).

Phase 4E promotes GraphQL to the core surface: the same Query +
Mutation types are consumed in-process by `lmprobe query '<gql>'`
(current-thread tokio runtime, default build) and over HTTP by
`serve` (multi-thread + axum behind `--features serve`). The MCP
feature was dropped in Phase 4E — agents talk to lmprobe via the
unified GraphQL contract.

Phase 4E-D wires arbitration I exit-code mapping for the `query`
verb: errors[] non-empty → exit 2; data with no hits → exit 1
(collapsible to 0 via `--exit-zero-on-no-match`); data with hits
→ exit 0.

Auth: `serve` refuses non-loopback bind without `--auth-token`.
Loopback (127.0.0.1 / ::1 / localhost) skips auth.

See the development-memory `durable-memory/17_graphql.md` for the GraphQL surface
reference.

## D7 — Mutation-safety contract

### Standalone file deletion

`find --action delete` and piped `delete` display a preview unless an explicit
`--force` is supplied. This nonrecursive action deletes only regular files;
it intentionally does not require Git or a clean tree. It validates every
original selection entry before deduplication, preserves lexical symlink-parent
evidence, rejects incomplete framed selections, rechecks metadata/identity,
and writes an XDG audit before unlinking. Forced workspace actions are refused.
Deletion is irreversible and metadata rechecks are not filesystem locking.
See `durable-memory/commands/delete.md` for input framing and partial outcomes.

Every mutating command refuses by default unless its preconditions
hold. Phase 0 §0.9 lands the basic guard for `trace`: it refuses a
dirty working tree (`DIRTY_TREE`) AND a non-git directory
(`NOT_GIT_REPO`, Lead arbitration) unless `--force`. Pre-instrument
file hashes are audit-logged to `$XDG_CACHE_HOME/lmprobe/trace-<ts>.log`.

### Phase 4B — trace safety surface

`trace` gains two safety options layered on the §0.9 base contract:

  * **`--worktree [path]`** — provision a fresh `git worktree` (via
    `git worktree add --detach`) and run the entire instrument →
    exec → rollback cycle inside it. The user's tree is byte-
    identical post-trace (sha256 verified). Default location is
    `$XDG_CACHE_HOME/lmprobe/worktrees/trace-<ts>/`. With
    `--worktree` set, the dirty-tree refusal is moot — the worktree
    clones HEAD, not the dirty in-tree state — but a `WORKTREE_USED`
    diagnostic is emitted for transparency. On exec failure the
    worktree is preserved and a `WORKTREE_PRESERVED` diagnostic
    with its locator surfaces in the envelope; on success it's
    deleted via `git worktree remove --force`.
  * **`--keep-patch [path]`** — capture a unified `git diff --no-color`
    of every instrumented file BEFORE rollback, written to
    `$XDG_CACHE_HOME/lmprobe/patches/trace-<ts>.patch` (or the
    explicit path). The patch survives rollback so `trace`'s
    instrumentation is reviewable.

Both options are additive on the existing TraceRunResult v1 envelope
(`worktree_path: Option<String>`, `patch_path: Option<String>` —
`#[serde(default, skip_serializing_if = "Option::is_none")]` keeps
old envelopes deserialisable).

### Phase 4B — `fix` dry-run

`fix -p '<old>' -r '<new>' [-l <lang>] [PATH]` is read-only by
default — walks the workspace, runs ast-grep
`replace_all(pattern, replacement)` against every supported file,
emits `lmprobe.fix.v1` envelope with per-file `FileDiff { hunks:
Vec<DiffHunk> }`. `--format markdown` renders unified-diff output.

### Phase 4C — `fix --apply`

`fix … --apply [--force]` writes the rewrite to the user's tree
atomically:

  1. Refuse dirty tree without `--force` (mirrors trace's contract).
  2. Run dry-run first so a malformed pattern errors out before any
     mutation.
  3. Audit log to `$XDG_CACHE_HOME/lmprobe/fix-<ts>.log`.
  4. Provision worktree at HEAD via `git worktree add --detach`.
  5. Apply rewrites in the worktree; capture diff to
     `$XDG_CACHE_HOME/lmprobe/patches/fix-<ts>.patch`.
  6. `git apply --whitespace=nowarn` the patch to the user tree.
  7. Worktree cleanup via `git worktree remove --force`.

Envelope additions (v1-additive): `mode: "applied"`, `patch_path:
String`, `applied_files_count: u32`, `audit_log_path: String`. Both
new fields use `#[serde(default, skip_serializing_if = "…")]` so
old envelopes deserialise unchanged.

### Phase 4C — `ast-diff <file> --from <rev> --to <rev>`

Structural diff between two commit revisions of a single file. Per-
declaration classification: `ADDED | REMOVED | MODIFIED | UNCHANGED`.
Reuses 4A's `blob_signatures` cache so each blob's per-symbol
signature is read from SQLite on warm runs. `lmprobe.ast_diff.v1`
envelope.

### Phase 4C — Mermaid + DOT graph rendering

New `OutputFormat` variants `Mermaid` / `Dot` accepted on **graph-
shaped verbs only**: `impact`, `callers`, `callees`, `supertypes`,
`subtypes`. Other verbs requesting these formats fall through to
JSON with a stderr `format … only valid on graph-shaped verbs` note
(`render_by_format` in `cli.rs`). Mermaid output is GitHub-flavored
(`graph TD` / `graph BT` / `graph LR`); DOT is dot(1)-compatible.

### Phase 4D — Multi-repo federated mode

`lmprobe --workspace <yaml>` reads a workspace config:

```yaml
roots:
  - path: /home/me/repo1
    alias: api
  - path: /home/me/repo2
    alias: web
```

The dispatcher fans out across roots, runs the read-only verb in
each, tags every hit's `origin_root` with the alias (or basename
fallback), and merges results into one envelope. Cross-root dedup
by canonical absolute path; collisions emit
`DUPLICATE_FILE_ACROSS_ROOTS` (rare; user-error case).

Mutation verbs (`fix --apply`, `trace`) refuse `--workspace` and
emit `WORKSPACE_READ_ONLY` then `process::exit(2)`. The doctrine
rationale: cross-root mutation is too dangerous for a one-shot
CLI invocation; it would have to be staged per root, with separate
audit logs and rollback contracts. Single-root mutation works as
before.

Per-root cache namespacing already in place: 3A's `repo_commits`
and `repo_head` tables key on `(repo_root, schema_version)`, so
each root maintains its own commit cache without interference.
Blob-keyed entries (4A `blob_signatures`) are content-addressable
and shared across roots — same blob_sha → same row regardless of
which root walked it.

### Phase 4D — Faithful caller-name extraction

`SymbolHit` gains a v1-additive `enclosing_decl: Option<String>`
field. The `callers` sensor walks the matched call-site's parent
chain to the nearest declaration kind (per the language's
`KindTaxonomy::all_declaration_kinds`), runs `NameExtractor` on
that ancestor, and stamps the result onto the hit. Phase 3B's
`impact` upstream BFS reads this field instead of doing a
line-walk-back heuristic — replacing fragile shell-out string-
prefix matching with proper AST traversal.

### Phase 4C — semantic blame

`history blame --semantic` breaks the LOGIC_CHANGE bucket into
heuristic categories using string operations on the AST-skeleton:
`BODY_REWRITE` (signature unchanged but content_hash differs),
`PARAM_ADDED` / `PARAM_REMOVED` / `PARAM_RENAMED` (paren-balanced
top-level argument count), `RETURN_TYPE_CHANGED` (text after `->`).
Best-effort heuristic; languages whose skeletons don't conform fall
back to LOGIC_CHANGE. Whitespace-normalised FORMATTING vs
BODY_REWRITE discrimination requires a different blob cache shape
and is a post-1.0 refinement.

---

## Diagnostic codes (CC4)

Stable identifiers emitted on the envelope's `warnings` channel and
mirrored to stderr by the CLI:

| Code | Source | Meaning |
| --- | --- | --- |
| `NOT_IMPLEMENTED` | callers/imports/decorations | sensor has no grammar for the language |
| `QUOTED_INPUT` | imports | outer ASCII quotes were stripped from the query |
| `PATTERN_PARSE_FAILED` | callers/imports/decorations | `Pattern::try_new` rejected the pattern |
| `NO_TRACE_TEMPLATE` | trace | no default template for the language |
| `DIRTY_TREE` | trace | refuses uncommitted working tree |
| `NOT_GIT_REPO` | trace | refuses non-git directory |
| `GIT_STATUS_FAILED` | trace | `git status` failed unexpectedly |
| `TRACE_AUDIT_LOG` | trace | path of the audit log just written |
| `DEPRECATED_CACHE_PATH` | evolution | read milestones from the legacy `.lmprobe/` path |

New codes are additive within v1. Renaming or removing a code
requires a `v2` schema bump for that envelope.

---

## Known Perf Debt

A real CC3 perf gate ships in `crates/lmprobe/tests/perf.rs`. Run
with `cargo test -p lmprobe --release --test perf -- --ignored`.
Thresholds are baseline + ~20% headroom; `LMPROBE_PERF_RELAX=N`
lets noisy CI hosts loosen by a multiplier. **Any phase that loosens
a threshold requires explicit Lead approval.**

Current baselines on the ast-grep workspace (release, hot cache),
post-Phase-1A DFS-once refactor:

| Sensor      | Baseline | Threshold | Phase plan |
| ----------- | -------- | --------- | ---------- |
| discover    | 11 ms    | 30 ms     | hold       |
| find        | 8 ms     | 30 ms     | hold       |
| grep        | 13 ms    | 30 ms     | hold       |
| **ref**     | 204 ms   | 260 ms    | **dominant cost is per-file tree-sitter parse, not walk count — needs blob cache (D2 / Phase 3.1) or content prefilter** |
| callers     | 22 ms    | 50 ms     | hold       |
| imports     | 26 ms    | 50 ms     | hold       |
| decorations | 74 ms    | 100 ms    | hold       |
| search      | 90 ms    | 100 ms    | hold (Phase 0 budget) |
| def         | 69 ms    | 100 ms    | hold (Phase 0 budget) |

### Closures (post-Phase-1A)

- **Phase-0 search/def perf debt — RESOLVED.** The §1.4 KindTaxonomy
  unblocked a DFS-once kind prefilter in `search_symbol_hits` (one
  tree traversal + `HashSet<&str>` kind probe, replacing N
  `find_all(KindMatcher)` walks). `search` 146 → 90 ms; `def` 124 →
  69 ms. Both back inside the Phase 0 plan target of <100 ms;
  thresholds tightened from 175 → 100 ms.
- **Phase-0 §0.5 Elixir outstanding — RESOLVED in §1.4.**

### Remaining

- **`ref` 204 ms.** Already does a single DFS pass on identifier
  kinds (no walk-count win to cash in). Cost is dominated by
  per-file tree-sitter parsing across the ~150 .rs files in the
  workspace that contain "Pattern". Two paths to <100 ms:
  - **D2 / Phase 3.1 blob cache.** Memoise parsed trees keyed on
    `(blob_sha, language)`. Re-runs on the same git state become
    O(matched-files-changed-since-last-call).
  - Tighter content prefilter (e.g. anchor on `\b<name>\b`).
    Cheaper, lower-risk, but capped — files containing the
    substring still parse.
  Threshold pinned at 260 ms (current + ~25%) until one of those
  ships. **Loosening this further requires Lead approval.**

- **`history blame` / `history introduced` 1.3-1.4 s warm — RESOLVED
  in Phase 4A.** Populated the `blob_signatures` cache (table
  reserved in 3A); per-(blob_sha, lang, symbol) AST signature is
  read from SQLite on warm-cache instead of re-parsed. Live numbers
  on cobra: warm 81-85 ms (was 1319 ms — ~16× speedup). Cold also
  improved (1381 → 947 ms) because all blob_signatures inserts batch
  into a single SQLite transaction over the blame walk. Threshold
  tightened from 1500 → 300 ms warm; cold stays at 2000 ms.

- **`context <symbol>` 420 ms warm — RESOLVED in Phase 4A.** Same
  root cause: the bundle's `recent_history` component invoked
  `blame` which inherited the AST-aware reparse cost. The shared
  blob_signatures cache closes the gap automatically. Live numbers:
  187 ms (struct) / 215 ms (function) — both under CC3's 250 ms
  target. Threshold tightened from 500 → 250 ms.

## Phase 1 — Known Gaps

Tracked here so Reviewer's end-of-Phase-1 pass sees them
explicitly. Same convention as `## Known Perf Debt`: one bullet
each, with the phase that owns the fix.

- **MCP transport (dropped).** The Phase 1.10 stdio MCP server was
  removed in Phase 4E. Agents talk to lmprobe via the unified
  GraphQL surface (`lmprobe query` for one-shots, `serve` for the
  HTTP daemon).
- **GraphQL CommonArgs mirror.** GraphQL today accepts only the
  verb-specific args + the §1.2 relational flags on `search`. The
  walker-tuning surface (`-G/--globs`, `-j/--threads`,
  `--no-ignore`, `--max-filesize`, `-t/--type`) is CLI-only over
  GraphQL — replicating ~12 `InputObject` fields per resolver was
  judged disproportionate for Phase 1. Owner: **Phase 1.7
  follow-up** if/when a GraphQL client needs the knobs.
- **`--paths` repeatable.** Every verb takes a single positional
  `[PATH]` today. Multi-path requires per-sensor refactors (loop
  over roots, merge results). Owner: **Phase 2** alongside the
  language-coverage work.
- **`-A/-B/-C` on multi-line sensors.** Wired only on line-oriented
  sensors (`grep`, `ref`). Multi-line sensors (`search`, `def`,
  `callers`, `imports`, `decorations`) already render full
  declaration bodies; context-line semantics around them aren't a
  clean fit. Documented choice; not a regression. Owner: revisit
  if user feedback suggests a shape (e.g. context-around-the-
  declaration-name vs context-around-the-body).

## §0.5 Outstanding — Elixir KindTaxonomy dependency: **CLOSED in §1.4**

Phase 1 §1.4 closes the Elixir outstanding by combining the
`ElixirTaxonomy` (every def-macro maps to `call`) with a
disambiguating `ElixirExtractor`: when a `call` node's `target`
identifier is one of the recognised def-macros (`def`, `defp`,
`defmodule`, `defmacro`, `defprotocol`, `defimpl`, `defstruct`, …)
the extractor walks the call's first non-target child to pull the
actual symbol identifier; otherwise it returns `None` so the search
loop's name filter skips the node. End-to-end:

```
$ lmprobe def Plug $LMPROBE_QA_CORPUS/elixir/plug
#### File: ./lib/plug.ex
- **Plug** (call)
  defmodule Plug do  ...
```

All 11 §0.5 languages now reach at least PARTIAL on `def`/`search`.
