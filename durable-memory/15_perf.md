# 15 — Performance

lmprobe has explicit perf budgets and machine-checked perf gates. This chapter documents the budgets, the gates, and the known debt.

The canonical performance spec is `crates/lmprobe/ARCHITECTURE.md` § Known Perf Debt. This chapter is the user-facing summary.

## Three-tier budget (CC3)

| Tier | Target | Verbs |
|------|--------|-------|
| **Hot-cache** (sub-100 ms) | <100 ms | search, def, callers, imports, decorations, find, grep, callees, ref-with-prefilter |
| **Warm-cache** (sub-280 ms) | <280 ms | context (CC3 target was 250 ms; 4E-D RC fix added 10 % hardware-variance headroom on the gate — see § Hardware variance below) |
| **Cold-cache** (sub-2 s) | <2 s | history blame, history introduced, history commit/churn/co-change/todos |

Plus an AST-aware warm tier introduced in 4A:

| Tier | Target | Verbs |
|------|--------|-------|
| **AST-aware warm** | <300 ms | history blame, history introduced |

Numbers are measured on the ast-grep workspace (~150 .rs files in `crates/`) and the cobra fixture (~500 commits).

## Perf gates

Two `#[ignore]` test files enforce the budget; CI runs them as a separate job to avoid contention with parallel unit tests.

### `tests/perf.rs` — core hot/warm verbs

```bash
cargo test -p lmprobe --release --test perf -- --ignored
```

Sensors (live numbers + thresholds, end-of-Phase-4):

| Sensor | best (ms) | threshold (ms) |
|--------|----------:|---------------:|
| discover | 5 | 30 |
| find | 4 | 30 |
| grep | 8 | 30 |
| ref | 145 | 260 |
| callers | 15 | 50 |
| imports | 17 | 50 |
| decorations | 61 | 100 |
| search | 51 | 100 |
| def | 38 | 100 |
| context_struct (`context Pattern`) | 187 | 280 |
| context_fn (`context run`) | 220 | 280 |

Threshold = baseline + ~20 % headroom; loosening requires explicit Lead approval. `LMPROBE_PERF_RELAX=N` multiplies all thresholds at runtime (CI override for noisy hosts).

**Hardware variance — context warm-cache (4E-D RC fix).** The original CC3 target was warm-cache <250 ms for `context`. The blob_signatures cache from Phase 4A delivers 187 / 220 ms warm on the original baseline machine, well under target. RC review on a different machine measured `context_fn` at 243-253 ms across three independent runs (1/3 over the 250 ms gate) — same hardware-variance bracket the rest of the suite tolerates implicitly. Per Lead arbitration, the gate threshold was bumped from 250 → 280 ms = max(observed) × 1.10, rounded up. The CC3 *target* (sub-250 ms warm on a representative machine) is still met on the original baseline; the *gate* now absorbs ~10 % cross-machine variance so RC tagging doesn't depend on a 3 ms margin. Not a perf regression — the 14-15× blame speedup from the blob_signatures cache is intact, and `context` cost is dominated by `recent_history` which goes through that cache.

### `tests/history_perf.rs` — history family cold + warm

```bash
cargo test -p lmprobe --release --test history_perf -- --ignored --nocapture
```

For isolated repeatability, use the reproducible probe helper (default 30 runs, serial execution, timestamped log):

```bash
./scripts/history_perf_probe.sh
./scripts/history_perf_probe.sh --runs 3 --relax 1.0
```

You can tune runs and threshold multiplier if needed:

```bash
./scripts/history_perf_probe.sh --runs 10 --relax 1.1 --log-dir /tmp/history-perf-logs
```

Each probe run appends per-test lines to a timestamped log, then prints a summary for `history_blame` and `history_introduced`:
`best` (minimum), `median`, `p95`, `max`, and `mean` for both cold and warm columns.
Re-run the summary on an existing log with:

```bash
./scripts/history_perf_probe.sh --summary-only <logfile>
```

Provisions a fresh tmp `XDG_CACHE_HOME` per sensor, runs cold then warm:

| Sensor | cold (ms) | warm (ms) | cold-thresh | warm-thresh |
|--------|----------:|----------:|------------:|------------:|
| history_blame | 947 | 81 | 2000 | 300 |
| history_introduced | 942 | 85 | 2000 | 300 |
| history_commit | 141 | 43 | 2000 | 200 |
| history_churn | 131 | 50 | 2000 | 200 |
| history_co_change | 115 | 49 | 2000 | 200 |
| history_todos | 116 | 50 | 2000 | 200 |

Warm-cache numbers post-Phase-4A reflect the `blob_signatures` cache populating: 1319 → 81 ms blame (~16× speedup) closes the AST-aware perf debt.

### Current threshold note (history cold flap)

`crates/lmprobe/tests/history_perf.rs` restored the `2000ms` cold threshold for
`history_blame` and `history_introduced` after a fixed-helper 30-run gate passed
for both sensors (`30/30` cold <= 2000ms, p95 <= 2000ms).

## Known Perf Debt

`crates/lmprobe/ARCHITECTURE.md` § Known Perf Debt is the canonical record. As of the current baseline:

### Resolved

- **search/def 142 → ~50 ms warm** (Phase 1A, DFS-once kind prefilter).
- **history_blame / history_introduced 1319 → 81 ms warm** (Phase 4A, `blob_signatures` cache).
- **context 297 → 187 ms warm** (Phase 4A, shared cache with blame).

### Remaining

- **`ref` 145 ms warm.** Already does a single DFS pass on identifier kinds (no walk-count win to cash in). Cost is dominated by per-file tree-sitter parsing across the ~150 `.rs` files containing the queried symbol's text.

  Two paths to <100 ms remain:
  - **D2 / Phase-3.1 blob cache extension.** Memoise parsed trees keyed on `(blob_sha, language)` (primary tuple).
  - **Tighter content prefilter** (e.g. anchor on `\b<name>\b`), which is cheaper but still parses substring-matches.

  Threshold pinned at 260 ms (~80% headroom) until one of those ships. **Loosening further requires Lead approval.**

#### Design gate note (closed planning item: `131-ref-blob-cache-perf-gate`)

- **Cache key (minimum):** `(blob_sha, language)` for parsed-tree memoization.
  `blob_sha` is stable across repositories for identical file content; `language` ensures parser behavior compatibility.
- **Schema/version compatibility note:** this is an internal cache optimization and does not alter `lmprobe.ref.v1`; keep envelope + payload versioning unchanged. The key should still participate in existing cache invalidation/version bookkeeping in the cache layer if that layer is shared with other cached parses.
- **Baseline measurement plan (before implementation):**
  - Run current `ref` warm-path baseline for at least 30 runs: 10 with cold cache, 20 with warm cache.
  - Capture `best`, `median`, `p95`, `max` plus matched-file count across fixture set.
  - Re-run after implementation with the same script and hardware.
- **Acceptance targets (post-cache):**
  - keep current gate at 260 ms during rollout,
  - target `ref` warm-path median ≤ 100 ms and p95 ≤ 140 ms on ast-grep workspace fixture,
  - target no regressions to `search/def` and no increase in `ref` cold-path cold metrics.
- **Non-goals:**
  - no CLI/API or sensor-output changes,
  - no schema or envelope field changes,
  - no runtime behavior changes outside the `ref` parser-cache layer.

## CI policy

Per `crates/lmprobe/ARCHITECTURE.md`:

- Both feature configurations (default, `serve`) build with **0 warnings**.
- Smoke + deep + perf + history_perf + blob_cache all green on every PR.
- Threshold loosening requires explicit Lead approval and a plan_corrections entry.
- Perf-gate isolation: run with `--test-threads=1` or as their own CI job to avoid contention.

## Diagnosing a perf regression

If a perf test fails:

1. **Run it isolated**: `cargo test -p lmprobe --release --test perf -- --ignored`. Contention with other tests inflates timings.
2. **Compare against baselines** in this chapter / `ARCHITECTURE.md`.
3. **Check cache state**: `rm -rf ~/.cache/lmprobe && cargo test -p lmprobe --release --test history_perf -- --ignored --nocapture` — fresh-cache run with timing rows visible.
4. **Profile a single invocation**: `time /target/release/lmprobe <verb> <args>` then `valgrind --tool=callgrind` if needed.
5. **Bisect via git**: `git bisect run cargo test -p lmprobe --release --test perf -- --ignored`.
6. **Don't just relax the threshold.** A budget loosening without explicit approval is the regression itself.

## Synthetic regression demo

Confirm the gate fires when it should:

```bash
# Temporarily tighten history_blame warm threshold to 50 ms.
sed -i 's/warm_ms: 300,/warm_ms: 50,/' crates/lmprobe/tests/history_perf.rs

cargo test -p lmprobe --release --test history_perf -- --ignored --nocapture
# ... FAILED:
#   - history_blame: warm 81>50ms

# Restore.
sed -i 's/warm_ms: 50,/warm_ms: 300,/' crates/lmprobe/tests/history_perf.rs
```

The gate works as intended.

## Read next

- [09_cache.md](09_cache.md) for the cache that backs all the warm-cache numbers.
- [16_testing.md](16_testing.md) for the full test-tier organisation.
- [05_history.md](05_history.md) for which history verbs are cold / AST-aware / trivial.
