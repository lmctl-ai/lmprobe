# 05 — History verbs

`lmprobe history <subcommand>` is the git-history-aware family. Plus `ast-diff <file> --from --to` and the top-level `evolution` (which has an alias under `history`).

All history verbs read from a shared XDG SQLite cache documented in [09_cache.md](09_cache.md). The cache is incremental: only commits since the last cached HEAD are walked on subsequent invocations.

## Subcommand list

| Subcommand | Purpose |
|------------|---------|
| `history blame <target>` | Per-symbol or per-line blame, AST-aware. |
| `history introduced <name>` | First AST appearance of a symbol. |
| `history commit -p <pattern>` | Find commits matching `-p` in message OR diff. |
| `history diff -p <pattern>` | Structural before/after pairs over a time window. |
| `history churn` | Files with the most logic-changing commits. |
| `history co-change <file>` | Files most often changing alongside the named one. |
| `history todos` | TODO/FIXME/XXX/HACK markers added in window. |
| `history evolution <name>` | Alias for top-level `evolution` — discoverable under `history`. |
| `evolution <name>` | Top-level: per-symbol milestone history (the alias's target). |
| `ast-diff <file> --from <rev> [--to <rev>]` | Structural diff between two revisions of a file. |
| `diff-context --from <rev> --to <rev>` | New-side changed-line anchors from a git diff, with optional source/structural context. |

## history blame

```
lmprobe history blame <symbol-or-file:line> [PATH] [--follow] [--semantic] [--around N]
```

Two forms:
1. **Symbol form** (`history blame Pattern .`): walk the file's history, classify each touching commit's effect on the named declaration.
2. **Line form** (`history blame src/lib.rs:42 .`): standard `git blame` of one line, AST-aware. Absolute `PATH:LINE` is accepted when the file is inside the target repo and is normalized to a repo-relative path before blame runs.

Default change-type taxonomy:
- `INTRODUCED` — symbol's first appearance.
- `LOGIC_CHANGE` — signature changed.
- `FORMATTING` — body changed but signature didn't.

With `--semantic`, `LOGIC_CHANGE` is broken down further:
- `BODY_REWRITE` — skeleton unchanged but content_hash differs.
- `PARAM_ADDED` / `PARAM_REMOVED` / `PARAM_RENAMED` — paren-balanced top-level argument count.
- `RETURN_TYPE_CHANGED` — text after `->` differs.
- `LOGIC_CHANGE` — fallback when none of the above fit.

With `--follow`, the walk crosses **whole-file renames** detected by libgit2's similarity threshold (50% default). The envelope's `follows: Vec<FileRename>` lists each rename hop.

With `--around N`, line-form targets include an optional `source_context: Vec<LineContext>` window around the target line. The field is omitted by default and is not populated for symbol-form blame.

Envelope: `lmprobe.history_blame.v1` → `BlameResult { entries: Vec<BlameEntry>, follows: Vec<FileRename>, source_context?: Vec<LineContext>, ... }`.

```bash
# Symbol form (no follow).
lmprobe history blame Pattern .

# Cross-rename — finds renames into pattern.rs from older locations.
lmprobe history blame Pattern --follow .

# Semantic taxonomy.
lmprobe history blame run --semantic .

# Per-line blame.
lmprobe history blame crates/lmprobe/src/lib.rs:42 .
lmprobe history blame /abs/path/to/repo/crates/lmprobe/src/lib.rs:42 /abs/path/to/repo
lmprobe --format json history blame crates/lmprobe/src/lib.rs:42 --around 3 .
```

Cross-symbol-name renames (e.g. `Pattern` → `AstPattern` in the same file) are **not** detected — that's the genuinely-research case, still deferred.

## history introduced

First-appearance commit. Same walk as `blame_symbol` but filtered to the `INTRODUCED` entry only.

```bash
lmprobe history introduced Pattern .
lmprobe history introduced Pattern --follow .   # cross-rename
lmprobe history introduced TodoEntry --lang rust .
```

Envelope: `lmprobe.history_introduced.v1` → `IntroducedResult { entries: Vec<BlameEntry> }`.

## history commit

Find commits matching either a message-substring or an AST-pattern in the diff.

| Flag | Purpose |
|------|---------|
| `-p, --pattern <PAT>` | ast-grep pattern run against each commit's diff. Optional — without it, list every commit in window. |
| `--since <DURATION>` | `30d`, `12h`, `2w`, `90m`, `45s`. |
| `--author <SUBSTR>` | Case-insensitive author substring filter. |

Envelope: `lmprobe.history_commit.v1` → `HistoryCommitResult { hits: Vec<CommitHit> }`.

```bash
lmprobe history commit -p 'fn $NAME' --since 30d .
lmprobe history commit --since 90d --author "HerringtonDarkholme" .
```

## history diff

Structural diff snippets matching `-p <pattern>` over a time window. For each commit in window, pairs the matched-pattern's before/after text from the commit's parent → new-tree diff.

```bash
lmprobe history diff -p 'pub fn $NAME' --since 60d .
```

Envelope: `lmprobe.history_diff.v1` → `HistoryDiffResult { hits: Vec<DiffHit> }`. Each `DiffHit` carries `commit_hash`, `author`, `timestamp`, `file_path`, `before`, `after`.

## history churn

Files with the most logic-changing commits.

| Flag | Default | Purpose |
|------|---------|---------|
| `--top <N>` | 20 | Top-N entries returned. |
| `--since <DURATION>` | unset | Restrict to a time window. |

```bash
lmprobe history churn --top 10 --since 90d .
```

Envelope: `lmprobe.history_churn.v1` → `HistoryChurnResult { rows: Vec<ChurnEntry> }`.

## history co-change

Files most often changing alongside the named file. Cap of 1000 commits (Lead arbitration in Phase 3A); when exceeded emits `HISTORY_TRUNCATED`.

```bash
lmprobe history co-change Cargo.toml --since 90d .
lmprobe history co-change Cargo.toml --top 20 --min-score 0.2 .
lmprobe history co-change Cargo.toml --exclude-glob '.github/**' --type rust .
```

Envelope: `lmprobe.history_co_change.v1` → `HistoryCoChangeResult { rows: Vec<CoChangeEntry> }`. `score` is Jaccard-ish: `co_occurrences / total_commits_touching_target`.

Filtering is output-only: `--min-score <FLOAT>` removes rows below the existing score, `--exclude-glob <GLOB>` removes matching path rows using the same glob syntax as the global walker excludes, `--type <LANG>` keeps rows whose paths match that file type, and `--top <N>` truncates the remaining sorted rows. None of these flags change how `score` is calculated.

## history todos

TODO/FIXME/XXX/HACK markers added in the time window. Walks each commit's diff; captures `+`-side lines only (added markers, not pre-existing).

```bash
lmprobe history todos --since 90d .
```

Envelope: `lmprobe.history_todos.v1` → `HistoryTodosResult { hits: Vec<TodoEntry> }`. Each `TodoEntry { commit_hash, author, timestamp, file_path, line_number, marker, text }`. Marker is `TODO | FIXME | XXX | HACK`.

## history evolution

Alias for top-level `evolution`. Identical semantics — passes through `--no-cache`/`--force`/`--regex` so the alias path is byte-identical to the original verb.

```bash
lmprobe history evolution Pattern .
lmprobe evolution Pattern .              # same output
lmprobe --no-cache history evolution Execute /path/to/cobra
```

## evolution (top-level)

Per-symbol milestone history. Walks oldest-first (TIME | REVERSE) so the first appearance is INTRODUCED. Subsequent SIGNATURE_CHANGE entries surface every signature shift through the file's history.

| Flag | Purpose |
|------|---------|
| `--regex` | Substring match (default exact). |
| `--force` | Bypass dirty-tree refusal (still requires git repo). |

Envelope: `lmprobe.evolution.v1` → `EvolutionResult { symbol, milestones: Vec<Milestone> }`. Persists to `symbol_milestones` table — see [09_cache.md](09_cache.md).

```bash
lmprobe evolution Pattern .
```

## ast-diff

Structural diff between two commit revisions of a single file.

```bash
lmprobe ast-diff crates/core/src/matcher/pattern.rs --from c9b450d8 --to HEAD
```

For each declaration extracted from the file at `--from` and `--to`:
- `ADDED` — present at to, absent at from.
- `REMOVED` — present at from, absent at to.
- `MODIFIED` — present at both with different signatures.
- `UNCHANGED` — same signature both sides.

Envelope: `lmprobe.ast_diff.v1` → `AstDiffResult { nodes: Vec<NodeDiff> }`. Each `NodeDiff { kind, name, change_type, from_signature, to_signature, from_line, to_line }`.

`--from` is required. `--to` defaults to `HEAD`.

Reuses 4A's `blob_signatures` cache so warm-cache lookups read from SQLite instead of re-parsing.

## diff-context

Changed-line anchor query over a git diff. It parses `git diff --unified=0 --no-color <from> <to> -- <path>` and emits one anchor per new-side changed line.

```bash
lmprobe diff-context --from HEAD~1 --to HEAD --around 4 --parents --body .
lmprobe --format json diff-context --from main --to HEAD src/
```

Envelope: `lmprobe.diff_context.v1` → `DiffContextResult { anchors: Vec<DiffContextAnchor> }`.

Each anchor includes:

- `file_path`, `line_number`, `line_content`
- `anchor_kind: "changed_line"`
- hunk metadata: `old_start`, `old_lines`, `new_start`, `new_lines`
- `source_context` when `--around N` is requested
- `parents` when `--parents` is requested
- `body` when `--body` is requested

Boundary: this is deterministic changed-line evidence only. It does not infer causality, bug likelihood, moved-code semantics, or AST-level behavioral changes. Deleted-only hunks have no new-side source anchor in the first implementation slice.

## Cross-cutting

- **Cache**: every history verb refreshes the `repo_commits` cache for the repo before querying. First invocation populates ~1k commits in <1s on most repos. See [09_cache.md](09_cache.md).
- **Performance**: trivial verbs (commit/churn/co-change/todos) hit <50ms warm-cache; AST-aware verbs (blame/introduced) hit <100ms warm-cache thanks to `blob_signatures` (Phase 4A). See [15_perf.md](15_perf.md) for thresholds.
- **Workspace**: history verbs do **not** fan out under `--workspace` (their per-root semantics need design work). They run against the first root only when `--workspace` is set; mutation verbs are explicitly refused. See [13_workspace.md](13_workspace.md).
- **Diagnostics**: `HISTORY_TRUNCATED` (co-change cap), `HISTORY_SHALLOW` (shallow/grafted checkout), `DEPRECATED_CACHE_PATH` (legacy DB fallback). See [14_diagnostics.md](14_diagnostics.md).

## Read next

- [06_mutations.md](06_mutations.md) for `trace` and `fix --apply` (the mutating verbs).
- [09_cache.md](09_cache.md) for cache schema details.
