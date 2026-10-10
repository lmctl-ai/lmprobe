# 16 — Testing

lmprobe has six test tiers, each with a different purpose and runtime cost.

| Tier | File | What it asserts | Runtime |
|------|------|-----------------|---------|
| Smoke | `tests/smoke.rs` | Per-case golden output stable; 41 cases | ~6 s test runtime, ~30 s wall-clock cold (cargo machinery) |
| Deep | `tests/deep.rs` | Zero panics across 261 invocations; LANG_VERDICTS match live measurement | ~7-9 s |
| Perf (core) | `tests/perf.rs` | Hot-cache CC3 budgets; 11 sensors under threshold | ~4-5 s |
| Perf (history) | `tests/history_perf.rs` | Cold + warm history budgets | ~3 s |
| Blob cache | `tests/blob_cache.rs` | `blob_signatures` cache invariants | <1 s |
| Auth | `tests/auth.rs` (`#![cfg(feature = "serve")]`) | Bearer-token middleware | ~2 s |

The default invocation runs binary unit tests and focused integration suites,
including smoke, help, GraphQL, search, history, trace, and scanner regressions.
The table above records historical tier sizes/timings, not a complete current
test inventory. Deep and performance gates remain explicit `#[ignore]` checks.
For command-specific suites, see [commands/index.md](commands/index.md).

Auxiliary tooling: `cargo test -p xtask --locked capability_matrix` checks the
command inventory, including tuple command variants and read/write classification.
`cargo xtask capability-matrix --check` validates its generated overview.
As verified 2026-10-05, the unrelated full xtask schema-generator test fails:
`generate_schema` canonicalizes existing `schemas/*_rule.json` inputs, but this
repository has no tracked `schemas/` directory. `cargo xtask schema` reports
ENOENT. This is outside the default lmprobe suite; do not create unrelated
schema artifacts merely to make the command-inventory check pass.

```bash
cargo test -p lmprobe -p lmprobe-engine --locked        # unit + engine + non-ignored integration tests
cargo test -p lmprobe -p lmprobe-engine --locked --lib --bins # fast unit tests, including engine
cargo test -p lmprobe --test deep -- --ignored           # 29-language deep tier
cargo test -p lmprobe --test perf -- --ignored           # core perf gate
cargo test -p lmprobe --test history_perf -- --ignored   # history perf gate
cargo test -p lmprobe --test blob_cache                  # cache invariants
cargo test -p lmprobe --features serve --test auth       # auth (serve feature only)
cargo test -p lmprobe --all-features                     # everything
```

Focused unit coverage for the recent agent workflows lives beside the code:
`text_edit.rs` tests sequential/range/Unicode/regex behavior; `git_command.rs`
tests byte preservation, URL redaction and status interpretation;
`program_output.rs` tests payload contracts and atomic-output failure boundaries;
engine `file_actions.rs` tests age parsing and selection framing; engine
`file_predicate.rs` tests Boolean semantics, exact complexity budgets and Git
identity matching. Engine unit tests require naming `lmprobe-engine` explicitly;
`cargo test -p lmprobe` alone does not run dependency unit tests. Filesystem and
Git fixtures stay in temporary roots, without new dependencies or network calls.

## Smoke (`tests/smoke.rs`)

Binary-level golden harness. Every case spawns the lmprobe binary, captures stdout + exit code, diffs against `tests/goldens/<case_name>.{stdout,exit}`. 41 cases at end-of-Phase-4.

Cases are parallelised via `std::thread::scope` (Phase 3B-PB2) so a 41-case sweep runs in ~6 s test runtime. Cold wall-clock includes ~30 s cargo machinery.

Update goldens: `UPDATE_GOLDENS=1 cargo test -p lmprobe --release --test smoke`. Re-run without the env var to verify.

Modes:
- `Exact` — stdout + exit must match byte-for-byte.
- `ExactSortedLines` — exit + line-sorted stdout (for sensors with non-deterministic worker-thread ordering: `grep`, `ref`, `find`).
- `ExitOnly` — exit code only (panic-prone or noise-prone cases).

Fixture base: `$LMPROBE_QA_CORPUS/<lang>/<repo>` (the qa1 corpus). Cases skip cleanly when a fixture path is missing (the harness is tolerant of fresh checkouts).

Fixture policy:
- **Smoke fixtures** are deliberately tiny and stable for regression speed (`tests/smoke.rs` and per-crate smoke checks).
- **Deep tier** (`tests/deep.rs`) is validated primarily against real-repo style qa1 fixtures under `$LMPROBE_QA_CORPUS/<lang>/<repo>/` for language-wide signal, with two existing exceptions (`toml`, `lua`) routed to in-tree directories via `@in-tree/<lang>`.

This split keeps the smoke lane fast and deterministic while keeping deep coverage grounded in broader corpus behavior.

## Deep (`tests/deep.rs`)

Two `#[ignore]` tests:

### `deep_29_languages`

Walks every language's qa1 fixture, runs every applicable sensor, and asserts
**zero panics** (exit 101). Historical exit baseline: 261 invocations across 29 languages, exit-code histogram `{0: 166, 1: 95}` — zero 101s.

### `lang_verdicts_match`

Tier-aware probe set:
- **Code tier** (23 langs): `{def, ref, search --name, callers, imports}` — 5 probes; ≥3 hits = Ok, 1-2 = Partial, 0 = Fail.
- **Data tier** (6 langs): `{search --name, search --kind class, search --has-child, search --outline}` — 4 probes; same thresholds.

Asserts the live count matches the post_phase_2c column in `LANG_VERDICTS`. Reconcile when adding a language or when the cache / extractor changes a probe's hit count.

Current deep-verdict baseline: **27 Ok / 2 Partial / 0 Fail** against the current 29-language matrix (Phase 2 exit criterion: ≤4 PARTIAL, 0 FAIL).

The `LANG_VERDICTS` table itself is a 6-tuple `(lang, post_p0, post_p1, post_p2a, post_p2b, post_p2c)` — historical reconstructions through 2C, with the rightmost column always asserted against live measurement. See [10_languages.md](10_languages.md) § LANG_VERDICTS for the matrix.

## Perf gates

See [15_perf.md](15_perf.md). Briefly:

- `tests/perf.rs` — 11 sensors hot/warm-cache. Asserts each `best(ms) < threshold(ms) * LMPROBE_PERF_RELAX`.
- `tests/history_perf.rs` — 6 history verbs cold + warm. Provisions tmp XDG per sensor.

Both tiers should run with `--test-threads=1` or as their own CI job to avoid contention.

## Blob-cache invariants (`tests/blob_cache.rs`)

Three tests verifying the `blob_signatures` cache invariants (Phase 4A):

- `blob_cache_cold_populates` — empty XDG → blame populates ≥2 distinct blob rows for a 2-commit fixture.
- `blob_cache_warm_no_new_rows` — second invocation against same XDG inserts zero new rows.
- `blob_cache_incremental_on_new_commit` — pre-cache N rows, add 1 new commit, re-run blame → exactly N+1 rows. Confirms the cache is incremental (no full rebuild on commit advance).

Each test provisions a fresh tmp git repo + tmp `XDG_CACHE_HOME`, runs `lmprobe history blame target`, opens `history.db` directly, asserts row counts.

## Auth (`tests/auth.rs`)

`#![cfg(feature = "serve")]`-gated; default test run skips it cleanly.

Verifies the bearer-token middleware:
- No token → 401.
- Wrong token → 401.
- Right token → 200.

Constant-time compare via `subtle::ConstantTimeEq` (timing-oracle hardening).

## How to add a smoke case

1. Edit `tests/smoke.rs` `cases()` table:

   ```rust
   Case {
       name: "discover_my_fixture",
       fixture: Some("path/relative/to/qa1"),
       args: &["discover", "."],
       mode: Exact,
   },
   ```

2. Bless the golden:

   ```bash
   UPDATE_GOLDENS=1 cargo test -p lmprobe --release --test smoke
   ```

3. Re-run without `UPDATE_GOLDENS=1` to verify it passes:

   ```bash
   cargo test -p lmprobe --release --test smoke
   ```

4. Inspect the new files:

   ```bash
   cat crates/lmprobe/tests/goldens/discover_my_fixture.stdout
   cat crates/lmprobe/tests/goldens/discover_my_fixture.exit
   ```

Commit the case + goldens together.

## How to add a verdict row

When adding a language (see [11_extending.md](11_extending.md)):

1. Add to `LANGUAGES` table in `tests/deep.rs`:

   ```rust
   ("zig", "zig/some-zig-repo", "main"),
   ```

   Or for in-tree fixture:

   ```rust
   ("zig", "@in-tree/zig", "main"),
   ```

2. Add a row to `LANG_VERDICTS`:

   ```rust
   ("zig", Verdict::Fail, Verdict::Fail, Verdict::Fail, Verdict::Fail, Verdict::Fail),
   ```

   (Initial Fail values; reconcile after first measurement.)

3. Run `cargo test -p lmprobe --release --test deep -- --ignored lang_verdicts_match --nocapture`.

4. The test panics with the live verdict; reconcile the rightmost column to match.

5. Iterate until all rows match live measurement.

## CC2 — smoke wall-clock

The "smoke <30 s" CC2 contract applies to **warm-cache test wall-clock**:

| Mode | Wall-clock | Test runtime |
|------|-----------|--------------|
| Cold (`cargo clean -p lmprobe`) | ~37 s | 6.5 s |
| Warm (back-to-back) | ~6.6 s | 6.5 s |

Cargo machinery (~30 s of cold) is fixed overhead. The warm wall-clock and the test-runtime line both meet <30 s. See `crates/lmprobe/ARCHITECTURE.md` § CC2 for the canonical record.

## Read next

- [15_perf.md](15_perf.md) for perf-gate details.
- [10_languages.md](10_languages.md) for the LANG_VERDICTS matrix.
- [test_cases.md](test_cases.md) for AI-agent-runnable end-to-end test cases.
