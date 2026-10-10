# 14 — Diagnostics & exit codes

Every `Diagnostic` is a stable identifier (the `code`) plus a human message and an optional `hint`. Codes are part of the v1-additive schema contract: new codes can be added without bumping `vN`; renaming or removing a code requires v2.

This chapter is the canonical reference. The source of truth is `Diagnostic::new(...)` call sites across `crates/lmprobe/src/`.

## Diagnostic shape

```rust
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    pub hint: Option<String>,
    pub lang: Option<String>,
}
```

Every envelope has `warnings: Vec<Diagnostic>`. The CLI mirrors them to stderr (suppress with `-q`); JSON envelope ships them on the wire regardless.

## Diagnostic-code reference

Grouped by source.

### Discovery / walking

| Code | Source | Meaning |
|------|--------|---------|
| `FILE_METADATA_FAILED` | `secrets` | A selected file's metadata could not be read before scan. Coverage-affecting. |
| `FILE_TOO_LARGE` | `walk_pre_check` / `secrets` | A selected file exceeded the configured size cap and was skipped. Coverage-affecting. |
| `FILE_READ_FAILED` | `grep` / `secrets` | A selected file could not be opened or could not be read completely. Hits from other files may still be valid, but callers must treat the result set as partial. |
| `WALK_ENTRY_ERROR` | `grep` | The filesystem walker failed to visit an entry. Grep continues where possible and emits this warning so callers can tell traversal was incomplete. |
| `WALK_ERROR` | `secrets` | The filesystem walker failed to visit an entry. Secrets scanning continues where possible and emits this warning so callers can tell traversal was incomplete. |
| `CYTHON_NOT_SUPPORTED` | `walk_pre_check` | A `.pyx` file was selected for structural analysis without a forced language override. Use `grep` / `find`, or force `-l python` for best-effort Python parsing. |
| `DEPRECATED_POSITIONAL_QUERY` | `cli.rs` Search dispatch | `lmprobe search '<pattern>'` (positional) is deprecated; use `-p '<pattern>' -l <lang>` for pattern mode or `--name '<glob>'` for name mode. |
| `QUOTED_INPUT` | `imports` | Module name had outer ASCII quotes; stripped before pattern match. (Closes QA §2.2 panic.) |
| `GITIGNORE_PATHS_EXCLUDED` | `find` / `grep` | The default (ignore-respecting) walk silently dropped this many files via `.gitignore`/`.ignore`/global-ignore rules. Dogfood finding (2026-09): an empty or short result with no such warning reads as "nothing is there," not "nothing was looked at" — this makes the silent default explicit. Hint always points at `--no-ignore`. Count only, not a file list (the `ignore` crate doesn't expose which entries it drops, so the count comes from diffing two metadata-only walks — one respecting ignore rules, one without — not from a full match-aware second scan). Fires on match or no-match. On `grep`'s no-match responses the same count is also mirrored into `scan_stats.files_ignored` (see `08_envelope_schema.md`) — round two of the same finding (2026-09): `scan_stats.files_skipped: 0` on its own read as "nothing was excluded," which was false when this warning's population wasn't in `files_skipped` at all. |

### Pattern engine

| Code | Source | Meaning |
|------|--------|---------|
| `PATTERN_PARSE_FAILED` | search / def / ref / callers / etc. | An ast-grep pattern failed to compile for the language. Renders the language label so you can see which lang's grammar didn't accept the pattern. |
| `PATTERN_STRICTNESS_ELLIPSIS_UNSUPPORTED` | search pattern mode | `--strictness signature` was combined with an ellipsis/multi-metavariable pattern such as `$$$`; lmprobe refuses this combination before ast-grep core can panic. |
| `RULE_PARSE_FAILED` | search relational rules | A YAML rule (`--has`, `--inside`, etc.) didn't deserialise as a `SerializableRule`. |

### Regex search

| Code | Source | Meaning |
|------|--------|---------|
| `REGEX_PARSE_FAILED` | grep / find | A Rust regex pattern failed to parse. The result envelope has `exit_status: "error"`, no matches, and a hint with Rust-regex recovery guidance. |
| `REGEX_LITERAL_PIPE_NO_MATCH` | grep / find | Pattern contains `\|` and returned zero matches. Rust regex uses bare `|` for alternation; `\|` matches a literal pipe. |

`resource-sites` and `guard-sites` also use `REGEX_PARSE_FAILED` for their user-supplied regex flags and return an error-status envelope rather than a top-level anyhow error.

`async-leaks` uses literal `--call` names rather than regexes. It emits `ASYNC_LEAKS_NO_CALLS` with `exit_status: "error"` if the GraphQL surface receives an empty call list.

`crypto-sites` uses repeated literal `--algorithm` values. It emits `CRYPTO_SITES_NO_ALGORITHMS` with `exit_status: "error"` if the GraphQL surface receives an empty algorithm list.

`context SYMBOL file.svelte` can emit `SVELTE_SYNTHETIC_CONTEXT` when normal definition lookup fails and lmprobe resolves `SYMBOL` as a synthetic component/file anchor by exact file-stem match. This is matched evidence, but it explicitly means Svelte structural parsing is not implemented and callers/callees/parents/body are not invented.

### Config key extraction

| Code | Source | Meaning |
|------|--------|---------|
| `CONFIG_PARSE_FAILED` | config-keys | A selected JSON/YAML/TOML file could not be parsed. Other config files still emit key records. |
| `CONFIG_KEYS_GREP_KEY_LIMIT` | config-keys --grep-code | The config-key/code-anchor join capped the number of unique leaf keys searched. Narrow the path and rerun for more keys. |
| `CONFIG_KEYS_GREP_ANCHOR_LIMIT` | config-keys --grep-code | The config-key/code-anchor join capped code anchors for at least one leaf key. Run `grep` directly for that key to inspect every lexical anchor. |

### Secrets provider DB

| Code | Source | Meaning |
|------|--------|---------|
| `PROVIDER_DB_SCHEMA_MISMATCH` | secrets provider DB loader | The provider DB `schema_version` is not supported by this binary. Use a matching lmprobe version or update the DB/schema together. |
| `PROVIDER_DB_UNKNOWN_FIELD` | secrets provider DB loader | The TOML contains a field the parser does not enumerate. Fix the field name or add parser + GraphQL lowering support for the new schema field. |
| `PROVIDER_DB_DEPRECATED_FIELD` | secrets provider DB loader | The TOML uses a deprecated provider DB field. Replace it before the field is removed in a future schema-version bump. |
| `SOURCE_DB_SCHEMA_MISMATCH` | secrets source DB loader | The source DB `schema_version` is not supported or the TOML violates loader validation. Use a matching lmprobe version or update the DB/schema together. |
| `SOURCE_DB_UNKNOWN_FIELD` | secrets source DB loader | The TOML contains a field the parser does not enumerate. Fix the field name or add parser + source GraphQL lowering support for the new schema field. |
| `SOURCE_DB_DEPRECATED_FIELD` | secrets source DB loader | The TOML uses a deprecated source DB field. Replace it before the field is removed in a future schema-version bump. |
| `SOURCE_DB_GENERIC_FAST_PATH_TOKEN` | secrets source DB loader | A source DB `fast_path_tokens` entry uses a generic env API such as `process.env` or `os.getenv`. Replace it with curated env var names or SDK identifiers. |

### API fact extraction

| Code | Source | Meaning |
|------|--------|---------|
| `OPENAPI_PARSE_FAILED` | openapi-facts | A selected OpenAPI JSON/YAML file could not be parsed. Other parseable specs still emit operation facts. |

### Capability traits

| Code | Source | Meaning |
|------|--------|---------|
| `NOT_IMPLEMENTED` | callers / callees / imports / exports / decorations / supertypes / subtypes / overrides | Per-language `<X>Grammar` returned an empty pattern Vec. The verb skips that language; emits the diagnostic once per (verb, lang). Cross-reference [04_sensors_graph.md](04_sensors_graph.md). |

### Context

| Code | Source | Meaning |
|------|--------|---------|
| `CONTEXT_LOCATION_RESOLVED` | context | A `PATH:LINE` context request was resolved to the nearest enclosing declaration before building the bundle. |

### Mutation safety (D7)

| Code | Source | Meaning |
|------|--------|---------|
| `DIRTY_TREE` | trace / fix --apply | `git status --porcelain` (which includes untracked) is non-empty. Bypass: `--force`. |
| `NOT_GIT_REPO` | trace / fix --apply | Path is not inside a git repository. |
| `GIT_STATUS_FAILED` | trace / fix --apply | `git status` itself errored (rare; permission issue or corrupt repo). |
| `NO_TRACE_TEMPLATE` | trace | The matched file's language has no default template. Pass `--template <TPL>`. |
| `TRACE_JSON_REQUIRED` | trace --watch | Watch mode requires `--json-trace` so runtime evidence is emitted in a stable machine-readable form. |
| `TRACE_WATCH_LANG_UNSUPPORTED` | trace --watch | Watch mode matched files outside Python, Bash, or `.R`/`.r`; structured watch probes currently support Python, Bash, and line-based R only. |
| `TRACE_WATCH_IDENTIFIER_UNSUPPORTED` | trace --watch | Watch mode currently accepts simple Python/Bash identifiers and simple dotted R names, not expressions, subscripts, or Python/Bash dotted paths. |
| `TRACE_AUDIT_LOG` | trace | (Informational) Audit log written to `$XDG_CACHE_HOME/lmprobe/trace-<ts>.log` BEFORE any mutation. |
| `WORKTREE_USED` | trace --worktree | (Informational) Instrumentation ran in an isolated git worktree; user tree untouched. |
| `WORKTREE_PRESERVED` | trace --worktree (failure) | Exec failed; worktree NOT cleaned up so you can inspect. The hint contains `git worktree remove --force` instructions. |
| `WORKTREE_PROVISION_FAILED` | trace --worktree | `git worktree add` failed. Includes git's stderr in the message. |
| `WORKTREE_CLEANUP_FAILED` | trace --worktree | `git worktree remove --force` failed; falls back to `fs::remove_dir_all`. |
| `PATCH_CAPTURE_FAILED` | trace --keep-patch / fix --apply | `git diff` or write of the patch file failed. |
| `PATCH_APPLY_FAILED` | fix --apply | `git apply --whitespace=nowarn` failed; the worktree is preserved for inspection. |
| `AUDIT_LOG_WRITE_FAILED` | fix --apply | Couldn't write the audit log; `fix --apply` aborts before any mutation. |

### Cache (D6)

| Code | Source | Meaning |
|------|--------|---------|
| `DEPRECATED_CACHE_PATH` | evolution / history * | Read from legacy `<root>/.lmprobe/history.db`; new writes go to XDG. Hint: delete `<root>/.lmprobe/`. |
| `HISTORY_TRUNCATED` | history co-change | Cap of 1000 commits exceeded; later commits not analysed. Hint: pass `--since` to narrow the window. |
| `HISTORY_SHALLOW` | history churn / history co-change | The repository is shallow or grafted, so history evidence may be incomplete. Fetch more history, for example with `git fetch --unshallow`, before relying on churn/co-change evidence. |

### Impact / context

| Code | Source | Meaning |
|------|--------|---------|
| `IMPACT_DEPTH_CAPPED` | impact | `--depth N` exceeds the hard cap of 10; clamped. |
| `SYMBOL_NOT_FOUND` | context | Symbol couldn't be located via `def`. Bundle is partial (no callers/callees/blame). |

### Workspace

| Code | Source | Meaning |
|------|--------|---------|
| `WORKSPACE_READ_ONLY` | trace / fix --apply (under `--workspace`) | Mutation verbs refuse `--workspace`. Exit 2. See [13_workspace.md](13_workspace.md). |
| `DUPLICATE_FILE_ACROSS_ROOTS` | (reserved) | Two roots produced the same canonical file path. Currently silent dedup; future work may emit this. |

### Format

| Code | Source | Meaning |
|------|--------|---------|
| (stderr-only message, no `Diagnostic`) | render_by_format | `--format mermaid` or `--format dot` requested on a non-graph verb. Falls back to JSON. See [07_output_formats.md](07_output_formats.md). |

## Exit codes

The CLI maps `ExitStatus` to process exit codes:

| Code | Meaning |
|------|---------|
| 0 | `matched` — at least one hit. |
| 1 | `no_match` — verb ran cleanly, found nothing. |
| 2 | `error` — refusal (DIRTY_TREE, NOT_GIT_REPO, NO_TRACE_TEMPLATE, missing `--lang` for pattern, WORKSPACE_READ_ONLY, etc.). |
| 3 | partial coverage — `exit_status` remains `matched` or `no_match`, but supported envelopes include `partial: true`. Set by coverage-affecting diagnostics such as `FILE_METADATA_FAILED`, `FILE_TOO_LARGE`, `FILE_READ_FAILED`, `WALK_ERROR`, or `WALK_ENTRY_ERROR`. |
| 101 | (unhandled panic) — should never happen. The deep tier asserts zero panics across 261 sensor invocations on 29-language qa1 fixtures. If you see 101, file a bug. |

`--exit-zero-on-no-match` (global flag) collapses 1 → 0. Useful for shell pipelines:

```bash
# Without --exit-zero-on-no-match: search returns 1 if no hits, breaking `&&`.
lmprobe search --name Foo . && echo "found Foo"

# With it: search returns 0 even on no-match.
lmprobe --exit-zero-on-no-match search --name Foo .
echo "search ran cleanly; check the envelope's exit_status field"
```

The envelope's `exit_status` field stays accurate regardless — the flag only changes the **process** exit code.

## Stderr mirror

By default, the CLI mirrors warnings to stderr:

```
lmprobe: <verb> [CODE]: <message> [optional language=<lang>]
```

Suppress with `-q / --quiet`. The envelope's `warnings: Vec<Diagnostic>` field still ships in JSON regardless; `-q` is purely a stderr filter.

```bash
# Verbose (default).
lmprobe history co-change Cargo.toml --since 5y . 2>&1 | grep -v summary
lmprobe: history_co_change [HISTORY_TRUNCATED]: co-change scan capped at 1000 commits ...

# Quiet (warnings hidden in stderr; envelope still has them).
lmprobe -q history co-change Cargo.toml --since 5y . --format json | jq '.warnings'
```

## Reading diagnostics in tooling

```bash
# All warnings on this run.
lmprobe --format json def NotASymbol . | jq '.warnings'

# Just the codes.
lmprobe --format json def NotASymbol . | jq '.warnings[].code'

# Fail on a specific code.
lmprobe --format json fix … --apply | jq -e '.warnings[] | select(.code == "DIRTY_TREE")' && exit 1
```

## Adding a new diagnostic

Per the v1-additive policy ([08_envelope_schema.md](08_envelope_schema.md)):

1. Pick a SCREAMING_SNAKE_CASE code that hasn't been used.
2. Construct via `Diagnostic::new("YOUR_CODE", "...message...").with_hint("...")`.
3. Document the new code in this chapter's reference table.
4. Add a smoke or unit test that asserts the code fires when expected.

Renaming or removing a code requires v2 + Lead approval.

## Read next

- [08_envelope_schema.md](08_envelope_schema.md) for the envelope shape that carries warnings.
- [16_testing.md](16_testing.md) for tests that assert specific diagnostic codes.
- [06_mutations.md](06_mutations.md) for the mutation-safety contract that emits most of the safety codes.
