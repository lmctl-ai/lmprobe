# 09 — Cache (D2 / D6)

The XDG SQLite cache is the only persistent state lmprobe writes. The working tree is **never** written to. Cache invariants: content-addressable, only-grows, no invalidation logic — git blob SHAs and commit SHAs are immutable.

This chapter is the canonical user-facing description; the source of truth for the schema is `crates/lmprobe/src/history.rs` and `crates/lmprobe/ARCHITECTURE.md` § Cache Schema.

## Location

| Environment | Cache path |
|-------------|------------|
| `$XDG_CACHE_HOME` set | `$XDG_CACHE_HOME/lmprobe/history.db` |
| `$XDG_CACHE_HOME` unset, `$HOME` set | `$HOME/.cache/lmprobe/history.db` |
| Both unset | `.cache/lmprobe/history.db` (rare; tests) |

The cache directory is created on demand. Read by every history-family verb; written by `evolution`, `history *` verbs, and `blob_signatures` from `history blame` (Phase 4A).

### Legacy path

Pre-§0.10 lmprobe wrote to `<root>/.lmprobe/history.db` (in the working tree). That path is **read-only** for back-compat — when an XDG cache miss occurs and the legacy path exists, `evolution` reads from it and emits `DEPRECATED_CACHE_PATH` with a hint to delete `<root>/.lmprobe/`.

New writes always go to XDG. Migration: delete `<root>/.lmprobe/` when convenient; the next invocation rebuilds from git.

## Tables

Four tables. All managed by `init_history_db()` in `history.rs`.

### `symbol_milestones`

Per-symbol evolution history. Phase 1.8.

| Column | Type | Notes |
|--------|------|-------|
| `symbol_name` | TEXT | The queried symbol's identifier. |
| `commit_hash` | TEXT | git OID (40 hex chars). |
| `author` | TEXT | Author name from the commit. |
| `timestamp` | TEXT | RFC 3339. |
| `change_type` | TEXT | `INTRODUCED` / `SIGNATURE_CHANGE`. |
| `signature` | TEXT | AST-elided skeleton (`fn name(args) -> R { ... }`). |

Primary key: `(symbol_name, commit_hash)`.

Read/write: `evolution` and `history evolution` (alias).

### `repo_commits`

Repo-wide commit metadata. Phase 3A.

| Column | Type | Notes |
|--------|------|-------|
| `repo_root` | TEXT | Canonicalised absolute path to the repo. |
| `schema_version` | INTEGER | Cache-key version (currently `2`). |
| `commit_hash` | TEXT | |
| `author` | TEXT | |
| `email` | TEXT | |
| `timestamp` | INTEGER | UNIX seconds. |
| `message` | TEXT | First line of the commit message. |
| `files_changed` | TEXT | JSON-encoded `Vec<String>`; populated lazily on query. |

Primary key: `(repo_root, schema_version, commit_hash)`. Index `idx_repo_commits_ts ON (repo_root, schema_version, timestamp)` for time-windowed queries.

Read by every `history` verb. Refreshed on each invocation by `refresh_commit_cache` (which is a no-op when HEAD matches the cached HEAD).

### `repo_head`

Last-walked HEAD per repo. Drives the incremental refresh.

| Column | Type | Notes |
|--------|------|-------|
| `repo_root` | TEXT | |
| `schema_version` | INTEGER | |
| `head_sha` | TEXT | The commit walked up to in the last refresh. |
| `updated_at` | INTEGER | UNIX seconds. |

Primary key: `(repo_root, schema_version)`.

The refresh logic walks newest-first and **stops** at the previously-cached HEAD. So a repo that gained 3 commits since the last lmprobe invocation processes only those 3 commits, never the full history.

### `blob_signatures`

Per-(blob, language, symbol) AST signature + content hash. Phase 4A. The cache that closed the AST-aware blame perf debt (1319ms → 81ms warm).

| Column | Type | Notes |
|--------|------|-------|
| `blob_sha` | TEXT | git blob OID. |
| `language` | TEXT | SgLang Debug repr. |
| `sensor_version` | INTEGER | Bump when sensor result shape changes. |
| `engine_version` | INTEGER | Bump when ast_grep_core dep version changes. |
| `schema_version` | INTEGER | Cache-key version. |
| `symbol_name` | TEXT | The blame query's target. |
| `content_hash` | TEXT | fnv1a64 of the matched node's text — used to filter formatting commits. |
| `signature` | TEXT | AST-elided skeleton. Empty string is the "symbol absent in this blob" sentinel. |

Primary key: `(blob_sha, language, sensor_version, engine_version, schema_version, symbol_name)`.

Read/written by `blame_symbol_with_follow_semantic` (and transitively by `context`'s `recent_history`). Walks are wrapped in a single SQLite transaction so per-blob INSERT autocommit doesn't dominate cold-walk cost.

## Cache key tuples

Two cache surfaces:

1. **Blob-keyed**: `(blob_sha, language, sensor_version, engine_version, schema_version, symbol_name)` — stable forever per blob; only grows. The `blob_signatures` table is the canonical example.
2. **Repo-wide**: `(repo_root, schema_version)` for `repo_head`; `(repo_root, schema_version, commit_hash)` for `repo_commits`. Invalidates only when HEAD advances. The incremental refresh re-walks `[old_head, new_head]` only.

Both surfaces are content-addressable through git: blob SHAs and commit SHAs never change for the same content. There is no invalidation logic anywhere — old rows just become unused after a `schema_version` bump.

## Version constants

In `history.rs`:

```rust
pub const SCHEMA_VERSION: u32 = 2;
pub const SENSOR_VERSION: u32 = 1;
pub const ENGINE_VERSION: u32 = 1;
```

| Constant | Bump trigger |
|----------|-------------|
| `SCHEMA_VERSION` | Adding/removing/renaming a column on any cache table. Existing rows stay readable; new queries hit fresh rows at the new version. |
| `SENSOR_VERSION` | Sensor result shape changes for a given input (e.g. signature representation changes). |
| `ENGINE_VERSION` | ast_grep_core dep upgrade. Cached results from older engine versions are not read. |

Pre-Phase-4A, `SENSOR_VERSION` and `ENGINE_VERSION` were `#[allow(dead_code)]` reservations. Phase 4A populated `blob_signatures`, dropping the reservation marker.

## Cache lifecycle

### Cold (first run)

```
$ rm -rf ~/.cache/lmprobe
$ lmprobe history blame Pattern .
# walks the file's commit history, populates blob_signatures + repo_commits + repo_head
```

Cold-cache target: <2 s for trivial verbs (commit/churn/co-change/todos), <2 s for AST-aware verbs (blame/introduced) on a ~500-commit repo. See [15_perf.md](15_perf.md).

### Warm (subsequent runs against same HEAD)

```
$ lmprobe history blame Pattern .
# refresh_commit_cache: 0 new commits walked (HEAD matches).
# blame: every blob is cached; SQLite read.
```

Warm targets: <50 ms for trivial verbs; <300 ms for AST-aware blame/introduced; <250 ms for `context`. Live numbers on cobra: ~85 ms blame, ~30 ms churn.

### Incremental (new commit since last walk)

```
$ git pull   # 5 new commits land
$ lmprobe history blame Pattern .
# refresh_commit_cache walks 5 new commits (stops at old cached HEAD).
# blame walks 0–5 new blobs depending on whether the file's blob changed.
```

The `repo_head` row is updated to the new HEAD on success.

## `--no-cache`

Global flag. Skips both reads and writes to the cache for the invocation.

```bash
lmprobe --no-cache history blame Pattern .
lmprobe --no-cache evolution Pattern .
```

Use cases:
- Smoke-test a fresh-tree behavior.
- Pin a smoke golden that mustn't depend on cached state.
- Diagnose a perf regression — `--no-cache` measures cold-cache.

`--no-cache` does **not** prevent the verb from reading git itself; it just skips the SQLite layer.

## Cache size

Each `repo_commits` row is roughly 200 bytes (commit metadata + small JSON for files_changed). A 10k-commit repo's `repo_commits` table is ~2 MB. `blob_signatures` rows are ~150 bytes each; with one row per (blob, lang, queried-symbol), the cache grows linearly with **distinct queried symbols** times **blob touches**. Practical sizes: tens of MB after months of use across many repos.

The cache is content-addressable, so blowing it away is safe — the next invocation rebuilds.

## Read next

- [05_history.md](05_history.md) for the verbs that read this cache.
- [15_perf.md](15_perf.md) for the perf gates that exercise cold/warm/incremental.
- [16_testing.md](16_testing.md) for the cache-invariant tests (`tests/blob_cache.rs`).
