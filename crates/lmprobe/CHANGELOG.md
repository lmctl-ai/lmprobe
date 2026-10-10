# lmprobe changelog

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added
- lmapi mode (lmapi protocol 6.1, via the lmrust SDK): `lmprobe --schema` publishes a
  `cli.schema.v1` document generated from the clap tree plus `crates/lmprobe/lmapi/overlay.toml`;
  63 snake_case operations (`git_status`, `grep_text`, `def_symbol`, `edit_text`, ...) run with
  flags or `--input-json` input, `--readonly`/`--dryrun`/`--confirm` policy, lmapi envelopes and
  exit codes (no_match/partial exit 0 with the status in the result). Legacy invocations are
  unchanged. Repo root `cli.schema.dev.json`/`cli.schema.latest.json` channel files. Depends on
  lmrust v0.1.1 with `default-features = false` so legacy JSON keeps its sorted key order.
- `grep --inside-kind <kind,...>` filters text hits by parsed AST ancestor
  kinds, enabling one-shot containment scans such as DAO reads inside Java
  loop nodes. Filtered-empty results now report `no_match`.
- `list-kinds` now accepts the global `-l/--lang` option when the positional
  language argument is omitted.
- `search --body` is accepted for symbol search parity with `def --body`.
  `search` already returns full declaration content by default; the flag makes
  agent recipes explicit without changing the envelope shape.
- GraphQL `find` and `grep` now accept `paths: [String!]` for multi-root scans,
  matching the CLI's repeated `--path` support.
- GraphQL `grepDirsWith(markerPattern, pattern, path, paths)` adds an
  in-process marker-directory AND content pipe for recipes such as
  "packages with `package.json` containing `login`".
- `secrets`, `secrets-recipes`, and `secrets-sources` CLI surfaces are now
  reflected in the smoke help goldens, along with `query --recipe` and
  `query --all-shapes`.
- `.github/workflows/release-binaries.yml` builds native release artifacts for
  Linux x86_64/aarch64, macOS Intel/ARM, and Windows x64, plus `SHA256SUMS`.
- A dependency-free static manual site under `site/` documents install,
  search, GraphQL composition, history, safe mutation, secrets, and scenario
  recipes.

### Changed
- Internal project documentation moved from `docs/` to `durable-memory/`;
  `README.md` remains the public entry point.
- GraphQL `find`, `grep`, and `grepDirsWith` now return a `PATH_NOT_FOUND`
  error envelope for missing explicit roots, matching the CLI read-verb
  behavior.

### Fixed
- `search --name <SYMBOL> .` no longer emits the
  `SEARCH_NAME_POSITIONAL_AS_PATH` warning on the canonical current-directory
  recipe. Path-looking non-dot positionals still warn and auto-swap; use
  `--path <PATH>` for those.
- Windows MSVC builds now link the required `advapi32` symbols for `libgit2`
  and set a larger test/binary stack for GraphQL-heavy CLI paths.
- Release-binary checksum generation now supports targeted runs that upload
  only one archive type.
- Windows CI tests now handle native path separators, escaped GraphQL path
  literals, Windows-safe test shims, and Windows-specific smoke coverage.
- `trace_watch_json` integration fixtures now use `tempfile::TempDir` instead
  of process-id temporary paths, matching release-CI fixture isolation guidance.

## [0.1.1] — 2026-05-06

### Added
- `diff-context`: changed-line anchors from `git diff <from> <to>`, with optional `--around`, `--parents`, and `--body` expansion.
- GraphQL parity for agent extraction primitives: `context(... usage/around/parents/body)`, `grep(... around/parents/body/role)`, `def(... around)`, and `refs(... role)`.
- `callers --tests-only` for path-based test target discovery.
- Python callsite argument extraction via `callsites --args`.
- Python sliced/unsliced flow evidence via `py-slice-candidates`, including `--trace-plan`.
- Runtime trace watch evidence via `trace --watch ... --json-trace`.
- Agent-facing guide updates in `durable-memory/llmuser_guide.md`.

### Fixed
- Preserved unsupported-language diagnostic-only status for `callers --tests-only`.
- `diff-context` now reads target revision source for `--to HEAD`, so dirty worktree content does not overwrite changed-line evidence.
- `diff-context` handles paths containing spaces in diff headers.

### Known Follow-Ups
- Go selector method calls are not yet included by `callers`.
- Python/TypeScript `callees` can still report internal `PATTERN_PARSE_FAILED` for some targets.
- TypeScript/JavaScript `overrides` can still report internal `PATTERN_PARSE_FAILED`.
- TypeScript relation queries may duplicate `export class` wrappers.

## [0.1.0] — Phase 4E (1.0 RC)

### Added
- **GraphQL as core surface** (default-built; no `--features serve` required to use). New `lmprobe query '<gql>'` subcommand executes a one-shot GraphQL document against the in-process schema with a current-thread tokio runtime. See `durable-memory/17_graphql.md`.
- **17 missing read resolvers** exposed via GraphQL: `callees`, `supertypes`, `subtypes`, `overrides`, `exports`, `impact`, `context`, `astDiff`, `listKinds`, plus the `HistoryQueries` nested type with 7 sub-resolvers (`blame`, `introduced`, `commit`, `diff`, `churn`, `coChange`, `todos`). Joins the 10 existing resolvers from Phase 1.7 (`discover`, `find`, `grep`, `search`, `def`, `refs`, `callers`, `imports`, `decorations`, `evolution`).
- **`SymbolHit` child resolvers** for query composition (`#[ComplexObject]`): `callers(limit, path)`, `callees(limit, path)`, `recentHistory(limit, path)`, `enclosing { name kind }`. Lets agents express `def → callers → enclosing` plus `recentHistory` in one round-trip instead of N CLI subprocesses.
- **Three GraphQL mutations**: `fix(scope, pattern, replacement, dryRun = true)` — generic structural rewrite; `trace(scope, template, marker, worktree = true, exec)` — instrumentation in two modes (transient with auto-rollback, or persistent-marker with a JSON audit log); `untrace(marker)` — revert by marker, with audit-log path or regex-fallback. All three honor the D7 mutation-safety contract identically to the CLI verbs (single function, two callers).
- **Typed scalars**: `BlobSha`, `CommitSha` (40-char hex), `LineNumber` (u32 ≥ 1), `FilePath`, `Pattern`, `Language` (29-variant enum mirroring `SgLang`).
- **Surface-equality test** (`crates/lmprobe/tests/graphql_surface_equality.rs`) asserting CLI ↔ GraphQL byte-equal output (post canonical sort + camelCase ↔ snake_case normalization) on 5 read sensors and diagnostic-code equivalence on D7 mutation refusals (`DIRTY_TREE`, `WORKSPACE_READ_ONLY`).
- **Arbitration I exit-code mapping** for the `query` verb: `errors[]` non-empty → exit 2; data with no hits → exit 1 (collapsible to 0 via `--exit-zero-on-no-match`); data with hits → exit 0. Brings GraphQL into the same exit-code contract the rest of the CLI uses.
- **Hardware-variance perf threshold** for `context_struct` / `context_fn`: 250 → 280 ms warm-cache. Original CC3 sub-250 ms target met on the baseline machine; gate now tolerates ~10 % cross-machine variance. Not a perf regression — the 14-15× blame speedup from the `blob_signatures` cache (Phase 4A) is intact.
- New chapter `durable-memory/17_graphql.md` (343 lines, standalone agent-facing reference).
- 10 new GraphQL test cases in `durable-memory/test_cases.md` (TC-G01 through TC-G10): basic query, composition, variables, introspection, exit-code mapping, mutation dry-run, D7 dirty-tree refusal, trace transient/persistent + untrace round-trip.

### Removed
- **`mcp` cargo feature** and the in-tree MCP stdio JSON-RPC implementation (Phase 1.10's `crates/lmprobe/src/mcp.rs`, ~700 lines). Per user doctrine, MCP belongs as an external wrapper layer (e.g. via `lmctl`), not in the lmprobe binary. Agents now consume the unified GraphQL contract — one schema, one envelope shape, one set of resolvers — via either `lmprobe query` (subprocess, one-shot, default build) or `serve` (HTTP daemon, opt-in).

### Changed
- **`serve` reframed** as an opt-in long-running daemon mode wrapping the GraphQL schema that's already available to the CLI's `query` verb on every build. `--features serve` now adds only the daemon-specific deps (axum, async-graphql-axum, subtle, multi-thread tokio); `async-graphql` and current-thread `tokio` are part of the default build.
- **`durable-memory/12_optional_layers.md`** rewritten around the new core/opt-in split. Adds a "When to pick `serve` vs `query`" section with a >10 calls/sec heuristic.
- **`SymbolHit` envelope** gains a v1-additive `enclosing_decl_kind: Option<String>` field carrying the tree-sitter literal kind of the containing declaration (`function_item`, `class_definition`, etc.). Populated by the `callers` walk; `None` on older serialized envelopes (`#[serde(default)]`-back-compat).

### Doctrine
- **One-shot CLI is the always-default.** `--features serve` is opt-in for daemon mode; everything else (including the full GraphQL surface) ships in the default build.
- **Stateless mutations.** No handles, no two-step apply pattern. Each `lmprobe query '...'` invocation is fully self-contained; mutation safety is enforced inline via the existing D7 contract.
- **No engine-side query complexity limit.** Agents control duration via shell `timeout` or process cancellation. `serve` mode can layer this in via async-graphql middleware if a deployment needs it.
- **`mcp` belongs outside the binary.** Removing the in-tree MCP server is a doctrine decision: lmprobe's surface is GraphQL (in-process or over HTTP); MCP-style transports are an external concern.
