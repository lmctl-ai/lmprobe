# `lmprobe` — LLM User Guide

This guide is written for an LLM agent (Codex / Gemini / local models / etc.) that needs to use `lmprobe` to read or modify a codebase. It assumes you can run shell commands and parse JSON.

If you're a human reader, start with `durable-memory/index.md` for the full development-memory map. This file is a focused tour of the surface most useful to an autonomous agent.

## Agent-Facing Changes

This section focuses on exact evidence extraction for AI agents:

- `diff-context` turns git diffs into changed-line anchors, with `--around`, `--parents`, and `--body`.
- `context NAME file.svelte` can return a synthetic file/component anchor when `NAME` exactly matches the `.svelte` file stem; `SVELTE_SYNTHETIC_CONTEXT` means no full Svelte AST hierarchy was inferred.
- GraphQL exposes the same expansion primitives as the CLI for batched extraction after an agent has already parsed failure text: `context(symbol: "path.py:42", around: 8)`, `context(..., usage: true, parents: true, body: true)`, `grep(..., around: N, parents: true, body: true, role: true)`, `def(..., around: N)`, and `refs(..., role: true)`.
- GraphQL `find`/`grep` accept `paths: [...]` for multi-root scans, and `grepDirsWith(markerPattern, pattern, path, paths)` covers marker-directory AND content recipes such as "packages with `package.json` containing `login`".
- `callers --tests-only` narrows callsite evidence to likely test paths.
- `callsites --args` exposes Python call arguments; `py-slice-candidates --trace-plan` emits trace-watch hints for suspicious sliced/unsliced flows.
- `config-keys` extracts JSON/YAML/TOML key facts; add `--key full.path.to.key` to target one exact key path, and `--grep-code --around N --parents --role` for mechanical same-key code anchors.
- `async-leaks` collects JS/TS call anchors for configured teardown-like names and reports only local syntax facts such as `awaited` and `returned`.
- `crypto-sites` collects Go crypto algorithm anchors for requested names and, with `--fips-check`, attaches nearby FIPS/config/build-tag evidence lines.
- `guard-sites` collects local sensitive-action, guard-looking, and decorator/annotation anchors from user-supplied regexes, with `scan_stats` counters and explicit empty-evidence diagnostics.
- `openapi-facts`, `route-anchors`, and `http-client-calls` expose separate API spec/code/client evidence streams for agent-side comparison.
- `durable-memory/19_scanner_recipes.md` defines AI scanner recipe templates as GraphQL aliases over these primitives. `durable-memory/24_find_grep_recipes.md` maps common `fd`/`rg` style recipes to lmprobe.

Boundary: lmprobe does not parse stacktraces, prove bug causality, or infer semantic diff meaning. AI supplies exact anchors and performs the reasoning.

## What `lmprobe` is

A one-shot, AST-aware, structural code-search CLI. It walks the filesystem (respecting `.gitignore`), parses each file via `tree-sitter` through `ast-grep`, and answers structured queries about the code. Every verb returns a versioned JSON envelope so an agent can reliably consume the output.

Use `lmprobe` when you would otherwise reach for:

- `grep` / `rg` for an identifier — but `lmprobe ref` / `lmprobe def` / `lmprobe search --name` understands what an identifier is and ignores matches inside strings or comments by default.
- `git log -p -S<symbol>` to ask "when did this change?" — `lmprobe history blame <symbol>` / `lmprobe history introduced <symbol>` walk the git log AST-aware.
- `git diff` plus manual file lookup — `lmprobe diff-context --from HEAD~1 --to HEAD --around 4 --parents` returns changed-line anchors with source/structural context.
- "Where is X called from?" — `lmprobe callers X` returns structural call sites; add `--tests-only` to target likely test files.
- "What does X depend on?" / "What blast radius?" — `lmprobe impact X --depth 2`.
- "Give me everything I need to know about X" — `lmprobe context X` ships a one-shot bundle (definition + callers + callees + imports + recent history + TODOs + file outline).

It is NOT a replacement for `grep` when you genuinely want raw-text scanning (string literals, comments, doc-comments). For that, use `lmprobe grep` (line-oriented regex with the same walker) or shell out to `rg`.

## Invocation shape

```
lmprobe [GLOBAL_FLAGS] <VERB> [VERB_FLAGS] [<QUERY>] [<PATH>]
```

- `<QUERY>` is the symbol / pattern / module name; verb-specific.
- `<PATH>` defaults to `.` (the current directory) for verbs that walk the workspace.
- Global flags compose with every verb. The most useful ones for an agent:
  - `--format json` — always pass this. Default is `auto` (markdown on TTY, NDJSON when piped); JSON is the most reliable shape to parse.
  - `-l <lang>` — force the parser for every file (overrides extension + shebang detection). Use this when the path you query has files in multiple languages and you only care about one.
  - `-t <lang>` (a.k.a. `--type`) — file-type filter. Walker-side: only walks files matching that extension class. Different from `-l`: `-l` *forces* the parser, `-t` *filters* the walker.
  - `--limit <N>` — cap the primary result list.
  - `--exit-zero-on-no-match` — set if you want exit 0 for empty results (otherwise empty = exit 1).
  - `--workspace <ws.yaml>` — fan out across multiple repos. Read-only verbs only.

For batched extraction, use `lmprobe query`. GraphQL aliases let you submit several exact anchors in one request, for example a failure frame plus related `grep` and `refs` queries. For raw scan composition, `grepDirsWith` can find marker directories and grep under them in one resolver. lmprobe still expects the agent to parse messy failure output into exact paths, lines, and symbols first.

## Verb table

The result-producing verbs below are the agent-facing surface. Group them by purpose:

### Discovery (start here when entering an unfamiliar tree)

| Verb | What it does | When to use |
|---|---|---|
| `discover` | Find project markers (Cargo.toml, package.json, pyproject.toml, …) and project hierarchy. `--top-level-only` suppresses nested projects for compact monorepo maps. | First call when you've never seen the repo — it tells you what languages/packages exist. |
| `find` | Path-regex match over file paths; repeat `--path` for multiple roots. | Locating a file you've heard about: `lmprobe find 'main\.rs$' .`. |
| `list-kinds` | Print the portable kind taxonomy for a language. | Use when you need a `--kind` value: `lmprobe list-kinds rust`. |

### Search (find a symbol or pattern)

| Verb | What it does | When to use |
|---|---|---|
| `search` | Structural AST query. Three modes: pattern (`-p '<ast-grep pattern>'`), name (`--name <substr>`), or default (walk all decl kinds). | Pattern-based queries (`fn $NAME() -> Result<$T> { $$$ }`); name-based scans (`--name Pattern`). |
| `def` | Find a symbol's definition, **exact-match** by default. `--class <NAME>` narrows common method names to a containing class/type; `--body` returns full declaration bodies; `--around N` adds line-numbered source context around each definition hit. | "Where is `process_request` defined?" — `lmprobe def process_request .` |
| `ref` | Find AST-identifier-node references (NOT inside strings/comments). `--role` adds optional local role labels for JSON/NDJSON hits. | "Where is `process_request` referenced?" — broader than `callers`; includes type uses, mentions in arg lists, etc. |
| `callsites` | Python-first call argument extraction with `--args`, including common multiline AST call nodes. Use `--around N` for `source_context`; JSON includes `callee_text`, `positional_args`, and `keyword_args`. | Inspect how `foo(a, b, key=value)`, `Form(default=None, ...)`, or `obj.foo(x, flag=True)` is invoked without dynamic dispatch claims. |
| `grep` | Textual regex search (line-oriented). Same walker as the AST verbs; repeated `--path` narrows to explicit roots; `-A/-B/-C` add flat source context, `--parents` adds enclosing class/function metadata, `--body` adds the innermost enclosing declaration/block body, and `--role` adds local role labels for parseable Python hits. | When you need raw text including comments/strings (TODO scanning, license checks). |
| `py-slice-candidates` | Python-only low-confidence evidence finder for `Y_chunk = Y[sl]` forwarded with related unsliced `Y_*` values. Add `--trace-plan` for `candidate_id` and `trace_plan { statement_pattern, watch_variables, suggested_line_number, warnings }`; it does not execute tests. | Triage suspicious sliced/unsliced argument flows without claiming a bug. |
| `diff-context` | New-side changed-line anchors from a git diff. Add `--around N`, `--parents`, and `--body` to expand each changed line like other anchors. Deleted-only hunks have no source anchor. | Review a local commit or candidate fix: `lmprobe diff-context --from HEAD~1 --to HEAD --around 3 --parents .`. |
| `resource-sites` | Go/Rust/JS/TS/TSX evidence scanner for user-supplied acquire/release regexes. Add `--around N`, `--parents`, and `--body` for local context. JS/TS sites may include optional `acquire_timing`, `release_lifecycle`, and `optional_acquire` evidence labels for lazy assignment, dynamic import, and page/dispose lifecycle syntax. | Gather local acquire/release/defer/error-return anchors without claiming ownership or control-flow completeness. |
| `async-leaks` | JS/TS/TSX evidence scanner for literal call names supplied with repeated `--call NAME`. Add `--around N`, `--parents`, and `--body` for local context. JSON reports `awaited` and `returned` booleans for syntax only. | Gather teardown call anchors such as `worker.stop()` / `worker?.stop()` without claiming Promise types, leaks, or cleanup correctness. |
| `crypto-sites` | Go-first evidence scanner for repeated `--algorithm NAME` values such as `sha1`, `md5`, or `des`. Add `--fips-check`, `--around N`, `--parents`, and `--body` for nearby config/source/structural context. | Gather crypto algorithm anchors, imports, and local FIPS/config mentions for review without making pass/fail claims. |
| `config-keys` | Extract JSON/YAML/TOML key facts. Add `--key <KEY_PATH>` to keep one exact key path before optional `--grep-code` code-tier anchoring, with optional `--around N`, `--parents`, and `--role`. | Gather config/code evidence for an agent to investigate sync questions; anchors are facts, not proof of data flow. |
| `guard-sites` | Scan code files for user-supplied sensitive-action regexes and nearby guard-looking regexes. Add `--around N`, `--parents`, and `--body` for local source/structural context. JSON includes `scan_stats { files_scanned, anchors_found, guard_hits, unsupported_files }` and warnings such as `NO_ANCHORS_FOUND` or `ANCHORS_WITHOUT_GUARD`. | Gather concrete local anchors for a permission review; absence of nearby guard evidence is not proof by itself. |
| `openapi-facts` | Parse OpenAPI JSON/YAML `paths` facts without full validation: method, path, operation ID, required params, and easy response schema refs. | Load spec-side API facts before comparing them yourself. |
| `route-anchors` | Extract literal route declarations, especially FastAPI decorators, with optional `--around N`, `--parents`, and `--body`. | Load server-side route anchors; dynamic routes still require inspection. |
| `http-client-calls` | Extract literal Python `requests`/`httpx` and JS/TS `fetch`/`axios` client call anchors, with optional context/parents/body. | Load client-side method/path/url evidence without claiming compatibility. |

`search` has a **CLI gotcha**: `lmprobe search --name Pattern .` is the quiet canonical current-directory recipe, but other positional paths can be ambiguous because the first non-flag positional fills `[QUERY]` first. Current builds auto-swap likely paths and emit `SEARCH_NAME_POSITIONAL_AS_PATH`; prefer the explicit path form `lmprobe search --name Pattern --path <PATH>` for non-current directories, or GraphQL: `lmprobe query 'query { search(query: "...", path: "...") { hits { name filePath } } }'`.

For annotated Python functions, prefer an explicit return annotation metavariable: `lmprobe search -p 'def __str__($$$) -> $RET: $$$' -l python`. If an unannotated Python def pattern has no hits, `search` retries that annotated shape once and reports `PYTHON_TYPED_DEF_PATTERN_FALLBACK`.

For single-file pattern searches, `search -p '<pattern>' path/to/file.py` can infer the language from a known file extension. Directory pattern searches still require `-l/--lang` to avoid guessing across mixed-language trees.

`search --strictness signature` does not support ellipsis/metavariable rest patterns (`$$$`) because signature normalization needs a fixed node sequence. The command returns `PATTERN_STRICTNESS_ELLIPSIS_UNSUPPORTED` instead of panicking; retry with default strictness or a fixed-arity signature pattern.

`grep` uses Rust regex syntax, not POSIX basic regex. Alternation is bare `|`; `foo\|bar` searches for the literal pipe form. If that likely mistake produces no hits, lmprobe emits `REGEX_LITERAL_PIPE_NO_MATCH`.

### Graph (relationships between symbols)

| Verb | What it does | When to use |
|---|---|---|
| `callers` | Call sites of a function/method. Use `--tests-only` to keep likely test-path callsites. | "Who calls `process_request`?" |
| `callees` | What a function/method calls (1-hop). | "What does `process_request` depend on?" |
| `imports` | Import / use / require declarations of a module name. | "Where is `clap` imported?" — `lmprobe imports clap .` |
| `exports` | Re-exports of a symbol (Rust `pub use`, JS `export {}`, etc.). | "Which crates re-export `Pattern`?" |
| `decorations` | Annotation / attribute / decorator usages. | `lmprobe decorations Override -l java` for `@Override`. |
| `supertypes` / `subtypes` | Class/trait inheritance. | "What does `UserRepository` extend?" / "Who implements `Matcher`?" |
| `overrides` | Method overrides (Java `@Override`, Kotlin `override fun`, etc.). | "Where is `equals` overridden?" |
| `impact` | Recursive callers/callees BFS. Returns a DAG with depth + confidence labels. Use `--tests-only` / `--exclude-tests` to focus the DAG by test paths. | Blast-radius analysis: `lmprobe impact Pattern --depth 3 .` |
| `context` | One-shot bundle. Default `--def` anchors on definitions and never falls back to usages; `--usage` returns usage/reference anchors with optional `--around`, `--parents`, and `--body`. | The "tell me everything about X in one call" verb. Often your second call after `discover`. |

### History (git-aware queries)

| Verb | What it does | When to use |
|---|---|---|
| `history blame <symbol-or-path:line>` | Per-symbol blame — like `git blame` but anchored on the symbol — or per-line blame for `PATH:LINE`. Absolute in-repo `PATH:LINE` is accepted. Use `--around N` with `PATH:LINE` for bounded line-numbered JSON context. | "Who last touched this function or line?" |
| `history introduced <symbol>` | First commit that introduced a symbol. | "When was this added?" |
| `history commit` | Recent commits in a window (`--since 180d`). | "What's been changing in the last 6 months?" |
| `history churn` | Top-N most-churned files. | "Where's the activity?" |
| `history co-change <file>` | Files that change together. Add output-only `--exclude-glob <GLOB>` and `--type <LANG>` to reduce path noise without changing scores. | "What else do I need to update?" |
| `history todos` | TODO/FIXME/XXX/HACK markers across the repo. | "What's left to do?" |
| `history diff` / `ast-diff` | Structural diff between two revisions. | "What changed structurally between rev1 and rev2?" |
| `evolution <symbol>` | Cross-rename, AST-aware history walk for one symbol. | "Trace this symbol back through renames." |

### Mutation (write side — read 06_mutations.md before using)

| Verb | What it does | When to use |
|---|---|---|
| `trace` | Surgically inject debug logs, run tests, rollback. | Diagnostic: "what does this code do at runtime?" — auto-rolls back unless `--keep-patch`. |
| `fix --dry-run` | Preview an AST-aware refactor (read-only by default). | Always run `--dry-run` first. |
| `fix --apply` | Apply the refactor to disk. | After `--dry-run` looks right. |

Mutation verbs refuse to run under `--workspace` (multi-repo mode is read-only).

## The envelope (what every verb returns)

Every verb's `--format json` output is a versioned envelope. The shape:

```json
{
  "schema": "lmprobe.<verb>.v1",
  "hits": [ /* SymbolHit, GrepHit, or verb-specific */ ],
  "warnings": [
    { "code": "<DIAGNOSTIC_CODE>", "message": "...", "lang": "Rust", "hint": "..." }
  ],
  "exit_status": "matched" | "no_match" | "error"
}
```

- `schema` is the version namespace. Stable across the v1 lifetime (additive only — new optional fields with `#[serde(default)]`).
- `hits` is the primary result vector. Composite verbs (`context`, `impact`) ship multiple sub-vectors instead.
- `warnings` carries non-fatal diagnostics. **Always inspect this** — it tells you whether `NOT_IMPLEMENTED` skipped a language, whether a pattern failed to parse, etc.
- `exit_status` mirrors the process exit code (see below).
- With explicit `--format json`, non-error warnings are envelope-only by default to keep stdout/stderr capture clean. Use `-v/--verbose` to also mirror them to stderr. Error-status diagnostics still print to stderr unless `-q/--quiet` is set.
- A few diagnostic-only unsupported-language scans can return `exit_status: "matched"` with an empty primary result and `NOT_IMPLEMENTED` warnings. Treat non-empty `hits`/`nodes`/`anchors` as the evidence; treat `matched` as the process-level status, not proof that a relation was found.

`SymbolHit` shape (the most common payload):

```json
{
  "name": "process_request",
  "kind": "function_item",
  "lang": "Rust",
  "file_path": "crates/foo/src/lib.rs",
  "line_start": 42,
  "content": "fn process_request(...) -> Result<()> { ... }",
  "enclosing_decl": "handle_main",
  "source_context": [
    { "line_number": 41, "line_content": "#[instrument]" },
    { "line_number": 42, "line_content": "fn process_request(...) -> Result<()> {" }
  ]
}
```

`enclosing_decl` is the *caller's own decl name* on `callers`/`callees` hits — so on a callers result, it tells you which function the call site sits inside.
`source_context` is omitted by default and appears only for source-expansion modes such as `def --around N`.

`GrepHit` shape, used by `grep` and `ref`:

```json
{
  "file_path": "crates/foo/src/lib.rs",
  "line_number": 42,
  "line_content": "let value = compute();",
  "role": "assignment"
}
```

Use `line_content` for the matched source line. There is no `line` field in the v1 JSON schema. `role` is present only when requested with `grep --role` / `ref --role`.

For no-match `search` and `grep` JSON responses, inspect `scan_stats`:

```json
{
  "scan_stats": {
    "files_considered": 12,
    "files_scanned": 12,
    "files_skipped": 0
  }
}
```

`files_considered` counts file entries that survived root/glob/ignore filtering; `files_scanned` counts files read by the sensor; `files_skipped` counts considered files skipped before scanning.

## Exit codes

| Code | Meaning | Agent action |
|---|---|---|
| 0 | matched (or `--exit-zero-on-no-match` set) | Parse `hits`. |
| 1 | no_match | Result was empty. Check `warnings` for `NOT_IMPLEMENTED` etc. before concluding the answer is "nothing exists". |
| 2 | error | Bad invocation, refusal, GraphQL validation error, or sensor error. Check stderr and `warnings`/`errors`. |
| 101 | panic | Process panicked. Always a bug — capture stderr and report. |

Pass `--exit-zero-on-no-match` if you want to handle the empty-result case in your own logic instead of via shell-exit branching.

## Critical diagnostic codes

Inspect `warnings[].code` after every call. The codes you care about most:

| Code | Meaning |
|---|---|
| `NOT_IMPLEMENTED` | The verb isn't wired for this language. The verb still ran across other languages. Use `-l <lang>` to scope, or fall back to `lmprobe ref` / `lmprobe grep`. |
| `CYTHON_NOT_SUPPORTED` | A `.pyx` file was selected for structural analysis. Use `grep` / `find`, or force `-l python` only for best-effort parsing. |
| `QUOTED_INPUT` | Your `<QUERY>` arrived with surrounding quotes (`"fmt"`); they were stripped. Often a shell-escaping issue — re-run without the inner quotes. |
| `PATTERN_PARSE_FAILED` | An ast-grep pattern failed to parse for the queried language. Check syntax. |
| `RULE_PARSE_FAILED` | An inline rule YAML (`--has`, `--inside`) failed to parse. |
| `FILE_METADATA_FAILED` | `secrets` could not read metadata for a selected file before scanning it. Treat the result set as partial unless you intentionally scoped around it. |
| `FILE_TOO_LARGE` | A selected file exceeded the configured size cap and was skipped. Treat the result set as partial unless you intentionally scoped around it. |
| `FILE_READ_FAILED` | `grep` or `secrets` could not open or fully read a selected file. Hits from other files may be valid, but the scan was partial. |
| `WALK_ENTRY_ERROR` | `grep` could not visit a filesystem entry. Treat the result set as partial unless you intentionally scoped around it. |
| `WALK_ERROR` | `secrets` could not visit a filesystem entry. Treat the result set as partial unless you intentionally scoped around it. |
| `DEPRECATED_POSITIONAL_QUERY` | You used `search '<pattern>'` instead of `search -p '<pattern>'`. Migrate. |
| `SEARCH_NAME_POSITIONAL_AS_PATH` | `search --name Foo path/to/dir` looked like a path/query swap; lmprobe fixed it and warned. `search --name Foo .` is quiet; prefer `search --name Foo --path path/to/dir` for non-current directories. |
| `PYTHON_TYPED_DEF_PATTERN_FALLBACK` | Python unannotated `def` pattern missed; lmprobe retried an annotated-return pattern. |
| `PATTERN_STRICTNESS_ELLIPSIS_UNSUPPORTED` | `search --strictness signature` was used with `$$$` or an ellipsis-style pattern. Retry without signature strictness or use fixed arity. |
| `REGEX_PARSE_FAILED` | A `grep` / `find` Rust regex failed to parse. The envelope has `exit_status: "error"` plus a syntax message and hint. |
| `REGEX_LITERAL_PIPE_NO_MATCH` | `grep` saw a likely POSIX `\|` alternation mistake that produced no hits. Use bare `|` for alternation. |
| `CONTEXT_LOCATION_RESOLVED` | `context PATH:LINE` resolved the line anchor to the nearest containing declaration. Informational. |
| `IMPACT_DEPTH_CAPPED` | You asked for `impact --depth >10`; capped to 10. |
| `SYMBOL_NOT_FOUND` | Verbs that anchor on an existing definition (e.g. `evolution`) couldn't find one. |
| `WORKSPACE_READ_ONLY` | You ran a mutation verb under `--workspace`. Refused by design. |

Coverage-affecting warnings such as `FILE_METADATA_FAILED`, `FILE_TOO_LARGE`,
`FILE_READ_FAILED`, `WALK_ERROR`, and `WALK_ENTRY_ERROR` make the direct CLI
process exit with code 3 when the underlying verb did not otherwise error.
Supported envelopes also set top-level `partial: true`. Hits are still evidence,
but the scan is not complete proof. `exit_status` remains `matched` or
`no_match` so strict clients that know the original enum do not break.

## Exact Failure And Diff Evidence

When debugging a failing test, split responsibilities:

1. The AI reads the messy failure output and extracts exact anchors such as `src/service.py:42`, `Service.handle`, or `Y_norm_squared`.
2. lmprobe returns deterministic source evidence for those anchors.
3. The AI compares evidence and decides the root cause.

Useful commands:

```bash
# Source around an exact frame/cursor location.
lmprobe --format json context src/service.py:42 --around 8

# Usage anchors with structural expansion.
lmprobe --format json context --usage target_symbol . --around 2 --parents --body

# Batched GraphQL extraction for multiple AI-parsed anchors.
lmprobe query '
query {
  frame: context(symbol: "src/service.py:42", path: ".", around: 8) {
    sourceContext { lineNumber lineContent }
    warnings { code message }
  }
  refs: grep(pattern: "Y_norm_squared", path: ".", around: 2, parents: true, body: true, role: true) {
    hits { filePath lineNumber lineContent role parents { kind name } }
  }
}'

# Changed-line anchors from two git revisions.
lmprobe --format json diff-context --from HEAD~1 --to HEAD --around 4 --parents --body .
```

`diff-context` returns new-side changed-line anchors only. Deleted-only hunks are intentionally not represented as source anchors in v1.

## Common LLM workflows

### Workflow 1 — Onboarding to an unfamiliar repo

```bash
# Step 1: what's here?
lmprobe --format json discover --top-level-only . | jq '.projects[] | {name, project_type, path}'

# Step 2: explore the top-level structure of one package
lmprobe --format json search --outline crates/foo/src/lib.rs

# Step 3: pick a symbol and pull its full context
lmprobe --format json context process_request crates/foo

# Or anchor on the line you are already editing.
lmprobe --format json context crates/foo/src/lib.rs:42
```

### Workflow 2 — "Where is this function used?"

```bash
# AST-aware references (recommended; ignores comments/strings).
lmprobe --format json ref process_request .

# Just the call sites (narrower).
lmprobe --format json callers process_request .

# Recursive blast-radius.
lmprobe --format json impact process_request --depth 3 .
```

### Workflow 3 — "When did this regression land?"

```bash
# Per-symbol blame: who last changed it.
lmprobe --format json history blame parse_config crates/foo

# Per-line blame from an editor/compiler location.
lmprobe --format json history blame /abs/path/to/repo/crates/foo/src/lib.rs:42 /abs/path/to/repo
lmprobe --format json history blame /abs/path/to/repo/crates/foo/src/lib.rs:42 --around 3 /abs/path/to/repo

# When was it introduced?
lmprobe --format json history introduced parse_config crates/foo

# Across renames.
lmprobe --format json evolution parse_config crates/foo
```

### Workflow 4 — Refactor preview

```bash
# Always dry-run first.
lmprobe --format json fix -p '$OLD.call($ARG)' -r '$OLD.call_v2($ARG)' -l rust .

# Inspect the patch hunks in the envelope before applying.
# Then:
lmprobe fix -p '$OLD.call($ARG)' -r '$OLD.call_v2($ARG)' -l rust . --apply
```

### Workflow 5 — GraphQL composition (preferred for LLM agents that compose queries)

```bash
lmprobe query '
query {
  context(symbol: "Pattern", path: ".") {
    definition { name filePath lineStart }
    callers { name filePath lineStart enclosing { name kind } }
    fileOutline { name kind lineStart }
  }
}'
```

The GraphQL surface (`lmprobe query '<gql>'`) lets you bundle multiple verbs in one round-trip, alias them (`a: callers(...) b: callees(...)`), and use variables. It also carries raw-scan composition primitives such as `grep(pattern: ..., path: ..., paths: [...])` and `grepDirsWith(markerPattern: ..., pattern: ..., path: ...)`. See `durable-memory/17_graphql.md` for the schema. **Surface equality**: every CLI verb has a 1:1 GraphQL resolver — same envelope shape, same diagnostics, same exit codes.

For AI scanner jobs, prefer documented GraphQL recipes over a custom orchestration format: `durable-memory/19_scanner_recipes.md` shows resource pairing, config/code reference, permission-guard, and API-surface evidence templates. Treat those outputs as investigation bundles, not findings.

## Per-language coverage caveats

`lmprobe` supports 29 languages. Coverage is uneven by verb:

- **Tier 1 (full coverage)**: Rust, Python, JavaScript, TypeScript, TSX, Go, Java. All read-side verbs work; mutation verbs (`fix`, `trace`) work for most.
- **Tier 2 (most read-side verbs work)**: Kotlin, Scala, Swift, C, C++, C#, Ruby, PHP, Lua, Bash, Dart.
- **Tier 3 (search/def/ref work; relational verbs partial)**: Elixir, Haskell, Nix, HCL, Solidity.
- **Tier 4 (data languages — search-only)**: HTML, CSS, JSON, YAML, XML, TOML.

When a verb returns 0 hits + `NOT_IMPLEMENTED` in `warnings`, fall back to `ref` (which works for most identifier mentions) or `grep` (raw text). The diagnostic `hint` field often tells you which phase will fix it.

For the per-language matrix in detail, see `durable-memory/10_languages.md`.

## What `lmprobe` does NOT do

- **Whole-program type analysis**: it's syntactic, not semantic. It will not tell you "this `foo()` resolves to `crate::a::b::foo`, not `crate::c::foo`". Use a language server / compiler for that.
- **Cross-language tracing**: `callers` won't follow a JS → WASM → Rust call chain.
- **Semantic equivalence**: `ast-diff` returns structural diffs; it doesn't tell you whether two refactored functions compute the same result.
- **Replace your editor's "go to definition"**: `lmprobe def` is workspace-wide and exact-name; it doesn't disambiguate by import path the way an LSP server does.
- **Run code**: `trace` injects logs and runs your test command, but `lmprobe` itself doesn't compile/execute target code. (`trace` requires `--exec` to specify what to run.)

## Performance expectations

On the lmprobe workspace itself (~50k LoC):

- `search` / `def` / `ref` / `grep` / `find` — under 100 ms hot-cache.
- `context` — 187–220 ms warm-cache.
- `history blame` / `introduced` — under 1500 ms warm-cache (cold up to 2200 ms; first-run fills `blob_signatures` cache).
- `impact --depth 3` — sub-second.

Cold-cache budget is **<2 s** for any single verb. If something takes longer, that's a perf bug — capture timings and report.

The cache lives at `$XDG_CACHE_HOME/lmprobe/` (defaults to `~/.cache/lmprobe/`). It's content-addressable and only-grows; you can `rm -rf` it without losing correctness, you just pay the cold-cache walk on the next call. Use `--no-cache` to force a fresh walk for one invocation.

## Common pitfalls for agents

1. **Forgetting `--format json`**. Default is `auto` — markdown on TTY, NDJSON on pipe. NDJSON is parseable but the per-line shape is less predictable than the JSON envelope. Always pass `--format json`.

2. **Passing a quoted module name verbatim**. `lmprobe imports '"fmt"'` will be normalised (quotes stripped) and emit `QUOTED_INPUT`. Pass `lmprobe imports fmt` instead.

3. **Confusing `-l` with `-t`**. `-l rust` *forces* every file to parse as Rust (use when querying mixed trees). `-t rust` *filters* the walker to `*.rs` files.

4. **Over-relying on `callers`**. It only returns structural call sites — `obj.foo(args)` and `foo(args)` shapes. For type uses, member references, or constructor sites in some languages, use `ref` instead. (This gap is being closed in Phase 6D for the remaining languages.)

5. **Search positional ambiguity**. `lmprobe search --name Foo .` is quiet, but `lmprobe search --name Foo path/to/dir` is accepted with a warning when the second token looks like a path. Prefer `lmprobe search --name Foo --path path/to/dir` or GraphQL for non-current directories.

6. **Assuming `--workspace` works for mutations**. It doesn't. `--workspace` is read-only by design.

7. **Not inspecting `warnings`**. Empty `hits` doesn't always mean "nothing exists" — it might mean `NOT_IMPLEMENTED` or `PATTERN_PARSE_FAILED`. Always check `warnings[]` before concluding.

## Quick reference: useful one-liners

```bash
# Project markers in this tree.
lmprobe --format json discover . | jq -r '.projects[] | "\(.project_type)\t\(.path)"'

# Compact top-level project markers for a monorepo.
lmprobe --format json discover --top-level-only . | jq -r '.projects[] | "\(.project_type)\t\(.path)"'

# All function definitions matching a substring.
lmprobe --format json search --name parse --kind function . | jq -r '.hits[] | "\(.file_path):\(.line_start)\t\(.name)"'

# Line-numbered context around one exact definition.
lmprobe --format json def --around 3 handle_request . | jq '.hits[0].source_context'

# Full body for one exact definition.
lmprobe --format json def --body handle_request . | jq -r '.hits[0].content'

# Callers of a method, with their enclosing decl.
lmprobe --format json callers handle_request . | jq -r '.hits[] | "\(.file_path):\(.line_start)\t\(.enclosing_decl) -> \(.name)"'

# Last 3 commits touching a function.
lmprobe --format json history blame handle_request crates/foo | jq '.hits[:3]'

# One-shot context bundle, JSON.
lmprobe --format json context handle_request .

# Context bundle from a cursor location.
lmprobe --format json context src/service.ts:42

# GraphQL composition.
lmprobe query 'query { def(name: "handle_request", path: ".", body: true) { hits { filePath lineStart content } } callers(name: "handle_request", path: ".") { hits { filePath lineStart enclosing { name kind } } } }'
```

## Where to look next

When this guide isn't enough:

- `durable-memory/01_overview.md` — design doctrine, where lmprobe fits.
- `durable-memory/02_invocation.md` — every global flag with full semantics.
- `durable-memory/03_sensors_search.md` / `durable-memory/04_sensors_graph.md` / `durable-memory/05_history.md` / `durable-memory/06_mutations.md` — per-verb deep dives.
- `durable-memory/07_output_formats.md` — JSON / NDJSON / SARIF / GitHub / Mermaid / DOT.
- `durable-memory/08_envelope_schema.md` — the envelope-shape contract and v1-additive policy.
- `durable-memory/14_diagnostics.md` — the full diagnostic-code reference table.
- `durable-memory/17_graphql.md` — the GraphQL schema (`lmprobe query`).
- `durable-memory/24_find_grep_recipes.md` — common `find`/`grep` recipes and internal-pipe guidance.
- `durable-memory/test_cases.md` — categorised test cases reproducible against a clean checkout.
