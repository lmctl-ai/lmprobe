# 06 — Mutation verbs

The two verbs that modify state: `trace` (reversible debug-log injection + exec + rollback) and `fix` (AST-aware refactor — dry-run by default, `--apply` writes through).

Both honour the D7 mutation-safety contract: refuse a dirty git tree without `--force`, refuse a non-git directory, and write an audit log to the XDG cache before any mutation. See `crates/lmprobe/ARCHITECTURE.md` § D7 for the canonical spec.

Mutation verbs **refuse** when run under `--workspace`. See [13_workspace.md](13_workspace.md).

## trace

Surgically inject debug logs, execute a command, capture stdout/stderr `[TRACE]` lines, and roll back.

```
lmprobe trace <query> [PATH] --exec <CMD> [--template <TPL>] [--force] [--worktree [PATH]] [--keep-patch [PATH]]
lmprobe trace <query> [PATH] --watch value,total --json-trace [--shape] [--len] [--id] [--repr-limit N] --exec <CMD>
```

| Flag | Default | Purpose |
|------|---------|---------|
| `<query>` | — | ast-grep pattern matched against every supported file. Each match site is rewritten with the per-language `TraceTemplate`. |
| `--exec <CMD>` | required | Shell command to run after instrumentation (e.g. `cargo test`, `pytest -x`). Captured stdout+stderr scanned for `[TRACE]` prefix. |
| `--template <TPL>` | per-language default | Override the per-language `TraceTemplate`. The metavar in the template is `$VAR` (binds to the matched site). |
| `--force` | off | Bypass the dirty-tree / non-git refusal. |
| `--worktree [PATH]` | unset | Provision a fresh `git worktree add --detach` and run instrument → exec → rollback inside it. User's tree is byte-identical post-trace. Default location: `$XDG_CACHE_HOME/lmprobe/worktrees/trace-<ts>/`. |
| `--keep-patch [PATH]` | unset | Capture `git diff --no-color` of every instrumented file BEFORE rollback, written to `$XDG_CACHE_HOME/lmprobe/patches/trace-<ts>.patch` by default. Survives rollback. |
| `--watch <NAMES>` | unset | Python/Bash/R watch mode. Comma-separated names are probed after matched Python statements, complete Bash statement lines, or complete `.R`/`.r` source lines. Python/Bash names must be simple identifiers; R also accepts dotted names such as `sales.total`. Requires `--json-trace`. Expression-level anchors are not yet guaranteed safe. |
| `--json-trace` | off | Parse emitted `[TRACE_JSON]` payloads into structured `TraceEvent` fields. |
| `--shape`, `--len`, `--id` | off | Add optional shape, length, and object identity metadata for watched values when available. Bash currently supports `--len` for scalars and arrays; `--shape` and `--id` are omitted. R supports `--len` via `length()` and `--shape` via `dim()`; `--id` is emitted as null/omitted. |
| `--repr-limit <N>` | 120 | Bound watched value `repr` payloads. |

Default template per language (from `TraceTemplate`, see [11_extending.md](11_extending.md)):

| Lang | Template |
|------|----------|
| Rust | `eprintln!("[TRACE] {} = {:?}", stringify!($VAR), $VAR);` |
| JS / TS / Tsx | `console.error("[TRACE] VAR =", $VAR);` |
| Python | `print(f"[TRACE] VAR = {$VAR}", file=sys.stderr)` |
| Java | `System.err.println("[TRACE] VAR = " + $VAR);` |
| Go | `fmt.Fprintln(os.Stderr, "[TRACE] VAR =", $VAR)` |

Languages without a template (XML/YAML/JSON/TOML/CSS) emit `NO_TRACE_TEMPLATE` and refuse — pass `--template` to override.

Envelope: `lmprobe.trace.v1` → `TraceRunResult { events: Vec<TraceEvent>, worktree_path?: String, patch_path?: String, ... }`. Watch-mode events keep the legacy `output` string and add optional structured fields: `var`, `type`, `id`, `len`, `shape`, `repr`. Bash watch events emit `var`, `type`, and `repr`, plus `len` when requested; `shape` and `id` are intentionally absent for now. R watch events are line-based only and use base R to emit `var`, `type`, `repr`, optional `len`, optional `shape`, and null/omitted `id`.

Bash watch-mode JSON escaping is intentionally small in this first slice: it
escapes backslash, double quote, newline, carriage return, and tab. Other raw
C0 control bytes inside watched values can make that event fail JSON parsing
and disappear from the event list. Avoid colorized/control-character output in
watched Bash values until a fuller shell-side JSON encoder lands.

R watch mode is intentionally not AST-backed because this repo has no R
`SgLang`. It only instruments `.R`/`.r` files when the trimmed trace query
exactly equals a trimmed source line, then inserts a base-R `local({ ... })`
emitter after that line. R watch names may be simple dotted names matching
`[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)*`; the runtime probe passes
the name as a quoted string to `exists()`/`get()`.

R watch queries must target standalone statement lines. Without an R AST, the
instrumenter now uses a conservative guard and skips likely continuations:
lines ending with a comma, an infix operator, or an open delimiter, plus lines
whose prefix context still has unmatched parentheses, brackets, or strings. It
does not treat ordinary unmatched `{ ... }` block braces as a continuation, so
assignment lines inside `if`/`for`/function bodies can still be watched. If all
matching R lines are skipped, the trace returns normal `no_match` instead of
injecting a temporary file that is likely to fail parsing.

Avoid watching an R function's implicit final return expression. R returns the
last evaluated expression in a function, and inserting a probe after that final
expression can change the temporary function's return value. Prefer watching
assignment lines or explicit intermediate values.

R watch JSON uses base R `encodeString`, not a full JSON library. Common
numbers, vectors, matrices, data frames, and ordinary strings are fine, but
exotic control bytes can produce non-JSON escapes and make an event disappear
from the parsed event list. This is the same failure class as the current Bash
control-byte limitation.

### trace worktree mode (recommended)

The default in-place mode is preserved for back-compat. For new use, prefer `--worktree`:

```bash
lmprobe trace 'let $VAR = $RHS' . --exec 'cargo test --no-run' --worktree --keep-patch
```

What happens:
1. Provision `$XDG_CACHE_HOME/lmprobe/worktrees/trace-<ts>/` via `git worktree add --detach`.
2. Walk the worktree, find every match of `let $VAR = $RHS`.
3. Rewrite each match using the per-language template.
4. Run `cargo test --no-run` in the worktree.
5. Capture lines containing `[TRACE]` from stdout+stderr; emit as `TraceEvent`.
6. Capture `git diff` to `trace-<ts>.patch` (BEFORE rollback).
7. Restore from per-file backup (rollback).
8. `git worktree remove --force` the worktree on success.
9. Emit `WORKTREE_USED` diagnostic for transparency.

User's working tree is **never touched** under `--worktree`. SHA256 verification: identical pre/post.

### trace failure modes

| Diagnostic | When |
|------------|------|
| `DIRTY_TREE` | Git tree has uncommitted changes and `--worktree` not set and `--force` not set. |
| `NOT_GIT_REPO` | `--worktree` set but the path isn't inside a git repo. |
| `NO_TRACE_TEMPLATE` | Language has no default template; pass `--template`. |
| `TRACE_AUDIT_LOG` | (Informational) audit log written to XDG. |
| `WORKTREE_USED` | (Informational) instrumentation ran in a worktree. |
| `WORKTREE_PRESERVED` | Exec failed; worktree NOT cleaned up so you can inspect. Locator in the message. |
| `WORKSPACE_READ_ONLY` | `--workspace` was set; mutation verbs refuse. Exit 2. |

## fix

AST-aware structural refactor. Dry-run by default; `--apply` writes through after worktree-isolated rewrite.

```
lmprobe fix -p '<pattern>' -r '<replacement>' [PATH] [-l <lang>] [--apply] [--force]
```

| Flag | Default | Purpose |
|------|---------|---------|
| `-p, --pattern <PAT>` | required | ast-grep source pattern; metavars (`$X`, `$$$`) allowed. |
| `-r, --replacement <REPL>` | required | Replacement template referencing the pattern's metavars. |
| `-l, --lang <LANG>` | unset | Restrict to one language. Without this, every file whose extension lmprobe recognises is candidate (useful for cross-lang patterns like `console.log($X)`). |
| `--apply` | off (dry-run) | Write the rewrite through to the user's tree. Provisions a worktree, applies in worktree, captures patch, `git apply --whitespace=nowarn` to user tree. |
| `--force` | off | Bypass dirty-tree refusal under `--apply`. |

Envelope: `lmprobe.fix.v1` → `FixResult { mode: "dry_run" | "applied", diffs: Vec<FileDiff>, matches_count, files_count, patch_path?, audit_log_path?, applied_files_count }`.

### Dry-run

Read-only. Walks the workspace, runs ast-grep `replace_all(pattern, replacement)`, emits per-file unified diffs.

Markdown render shows `--- a/file`, `+++ b/file`, `@@` headers, `-`/`+` lines.

```bash
lmprobe fix -p '$L.ast_grep($X)' -r '$L.parse_v2($X)' -l rust crates/lmprobe/src/history_sensors.rs
```

### Apply

Worktree-isolated atomic write:

1. **Refuse dirty tree** without `--force` (`DIRTY_TREE`).
2. **Run dry-run** to collect diffs. Bail out with `no_match` exit_status if zero matches.
3. **Audit log** written to `$XDG_CACHE_HOME/lmprobe/fix-<ts>.log` BEFORE provisioning.
4. **Provision worktree** at HEAD: `git worktree add --detach`.
5. **Apply rewrites** in the worktree, file by file. Re-runs `replace_all` against each file's current content.
6. **Capture patch** with `git diff --no-color` to `$XDG_CACHE_HOME/lmprobe/patches/fix-<ts>.patch`.
7. **Apply patch** to user's tree atomically: `git apply --whitespace=nowarn`. Failure surfaces `PATCH_APPLY_FAILED` and the worktree is preserved.
8. **Cleanup**: `git worktree remove --force`.

```bash
lmprobe fix -p 'println!($X)' -r 'eprintln!($X)' -l rust . --apply
```

### Why dry-run by default

Forcing `--apply` to be explicit means typos or pattern-mismatch surprises don't silently rewrite files. The dry-run output is reviewable; `--apply` is opt-in.

### What's not in fix

- `--confirm-each` interactive mode: not implemented. A REPL would conflict with the one-shot doctrine. If you want partial-apply, run dry-run, edit the patch, `git apply` it manually.
- `--regex` substring mode: not implemented. The pattern is always ast-grep's structural pattern; if you need text-only replace use `git ls-files | xargs sed -i`.

## D7 mutation-safety contract

Text `edit` is a separate live-file path; see [edit command](commands/edit.md).
It previews by default and requires clean Git for apply unless force is explicit.
It validates every file/operation before writes, records private recovery JSON
before the first write, then atomically replaces each file after metadata/content
checks. Force supports dirty/non-Git agent workflows using current file contents.
Batch writes are not atomic: a write failure stops the batch and reports per-file
applied flags plus recovery location. Existing `fix`/`trace` contracts below remain.

Every mutating verb refuses by default unless preconditions hold:

1. **Dirty-tree refusal**: `git status --porcelain` (which includes untracked) must be empty. Bypass with `--force`.
2. **Non-git refusal**: `git worktree add` requires a discoverable repo. Bypass with `--force` only for in-place trace; `fix --apply` always requires git.
3. **Pre-instrument audit log**: file SHAs (and now patch contents) are written to XDG before any tree mutation, so a hard crash mid-instrument leaves a recovery breadcrumb.
4. **Backup-and-restore**: trace's in-place mode keeps per-file `.bak` files until rollback completes.
5. **Atomic apply** for `fix --apply`: rewrite happens in a worktree, then `git apply` is one atomic operation against the user's tree.

The contract is enforced both in source (`check_trace_safety`, `check_fix_safety`) and in smoke goldens (`trace_refuses_nogit`, `trace_refuses_untracked` in `tests/smoke.rs`). Loosening any of these requires explicit Lead approval per `crates/lmprobe/ARCHITECTURE.md` § D7.

## Audit log format

```
# lmprobe trace audit log 20260502T013452
<fnv1a64 hash>  /abs/path/file.rs
<fnv1a64 hash>  /abs/path/other.rs
```

```
# lmprobe fix audit log 20260502T013452
# 4 matches across 1 files
crates/lmprobe/src/history_sensors.rs  hunks=4
```

The audit log is plain text by design — recoverable without any lmprobe machinery. Pair with the `.patch` file for full reconstructability.

## Read next

- [11_extending.md](11_extending.md) for adding a new `TraceTemplate` for a language.
- [13_workspace.md](13_workspace.md) for why mutation verbs refuse `--workspace`.
- [14_diagnostics.md](14_diagnostics.md) for the diagnostic-code reference.

## File deletion action

`find --action delete` and piped `lmprobe delete` preview by default. Only an
explicit --force can delete regular files. Unlike AST fix/trace, file deletion
is standalone, nonrecursive, irreversible, and does not require Git. It uses
metadata/identity rechecks, whole-selection validation, XDG audit-before-delete,
and workspace refusal. See [commands/delete.md](commands/delete.md) for the
complete contract and temporary-directory tests.
