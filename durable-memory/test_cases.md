# Manual end-to-end test cases for AI-agent execution

Each case is reproducible against a clean checkout with shell access.

## Per-case schema

Every case carries:

- **ID** — `TC-NNN` (Smoke/Sensor/Mutation/etc.) or `TC-G##` (GraphQL) or `TC-W##` (Workspace/Architectural).
- **Language(s)** — explicit language under test, `all` (cross-cutting), or `n/a` (CLI-shape / harness-only).
- **Severity** — `blocker` (release-blocking on failure), `major` (expected-to-work surface; release-blocking unless explicitly accepted), `minor` (edge-case or nice-to-have), `nit` (cosmetic).
- **Preconditions** — repo state, fixture path, prior commit, tools required.
- **Command** — the exact shell invocation.
- **Expected** — exit code, stdout/stderr shape, `stderr_allowed` flag (true if non-empty stderr is acceptable on pass).
- **Pass** — explicit assertions the agent should run.
- **Links** *(optional)* — Phase fix ID (`4D-PB1`, `5C`, etc.) or prior-cycle finding ID.

## Prerequisites

External tools used in some cases. Install before running the suite:

| Tool | Used by | Install |
|------|---------|---------|
| `bash` | every case | standard on Linux/macOS |
| `git` | mutation + cache cases | `apt install git` / `brew install git` |
| `python3` | several cases for JSON parsing | standard on most distros |
| `jq` | most JSON-asserting cases | `apt install jq` / `brew install jq`. **Skip the case** if not available, or substitute a `python3 -c 'import sys, json; …'` invocation as TC-022 / TC-014 do. |
| `sqlite3` | cache-invariant cases (TC-062, TC-063) | `apt install sqlite3` / `brew install sqlite`. Substitute via `python3 -c 'import sqlite3; …'` if unavailable. |
| `sha256sum` | byte-identity D7 cases (TC-030, TC-035, TC-G09, TC-G10) | standard on Linux; macOS users can use `shasum -a 256`. |
| `diff` | TC-025 | standard. |
| `curl` | TC-087 (`serve` smoke) | standard. |
| `cargo` (toolchain) | all `cargo test` / `cargo build` / `cargo metadata` cases | rustup-installed |

## Conventions

- Working directory is `$LMPROBE_REPO` (the workspace root) unless the case `cd`s elsewhere.
- `$LMPROBE_QA_CORPUS` is a local checkout of the qa1 reference repos (`<lang>/<repo>/` clones); `$LMPROBE_QA_HOME` is the directory that contains `qa1/`. Cases that need them are skipped when they are not available.
- The release binary is `$LMPROBE_REPO/target/release/lmprobe`. Rebuild if missing: `cargo build -p lmprobe --release`.
- Most commands use `--no-cache` where cache state would otherwise affect output, so cases are idempotent.
- `trace`/`fix` cases that produce `exit 1` (no events captured / no matches) are still **passing** if their other assertions hold — the `Pass` criteria explicitly name what's checked.
- **Pinned fixtures only.** No case performs `git clone` of external repos. Fixtures reference `qa1/repos/<lang>/<repo>/` (already pinned on disk) or in-tree `crates/lmprobe/tests/fixtures/<lang>/` paths, or use throwaway `mktemp -d` + `git init` for self-contained git tests. QA-cycle 1 hit GnuTLS / index-pack failures on full clones in WSL; the suite avoids that class entirely.
- `serve` (long-running daemon) cases are limited to `--help` + smoke-startup-then-shutdown only. No load testing.
- `trace` (mutating) cases include `--worktree` + `--exec` paths; acceptance criteria is D7 contract holding (byte-identity post-rollback verified by sha256). Throwaway fixtures only; never the user's actual checkout.

## Categories

- [Smoke](#smoke) — every verb runs cleanly on the workspace.
- [Sensor](#sensor) — per-sensor correctness.
- [History](#history) — git-history verbs.
- [Mutation](#mutation) — trace + fix; D7 mutation-safety contract; sha256 byte-identity.
- [Format](#format) — output formats.
- [Language](#language) — Tier-1 (Ok/Partial) and Tier-2 (Fail) coverage; NOT_IMPLEMENTED diagnostic verification.
- [Cache](#cache) — XDG cache invariants.
- [Multi-repo](#multi-repo) — `--workspace` mode.
- [Failure-mode](#failure-mode) — refusals + diagnostics; `no_match` vs `NOT_IMPLEMENTED` distinction; `serve` startup smoke.
- [GraphQL](#graphql) — `lmprobe query` one-shot semantics: composition, field selection, variables, introspection, typed scalars, exit-code mapping, mutations.
- [Workspace / Architectural](#workspace--architectural) — Phase 5+ workspace shape, per-lang crate isolation, xtask scaffolding, dep-graph invariants.

---

## Smoke

### TC-001 — `lmprobe --help` runs cleanly

- **Language(s)**: n/a
- **Severity**: blocker
- **Preconditions**: `target/release/lmprobe` built.
- **Command**:
  ```bash
  target/release/lmprobe --help
  ```
- **Expected**: exit 0; stdout contains `Commands:`, `search`, `history`, `fix`, `ast-diff`, `--workspace`; stderr empty.
- **Pass**: exit 0; stdout has all 5 listed strings.

### TC-002 — `cargo test -p lmprobe` (smoke tier) all green

- **Language(s)**: all
- **Severity**: blocker
- **Preconditions**: cargo toolchain present.
- **Command**:
  ```bash
  cargo test -p lmprobe --release --test smoke 2>&1 | grep -E '^test result:' | tail -1
  ```
- **Expected**: exit 0; stdout contains `test result: ok` and `0 failed` on the captured line.
- **Pass**: exit 0; captured line asserts both strings.

### TC-003 — Default build is warning-free

- **Language(s)**: n/a
- **Severity**: blocker
- **Preconditions**: cargo toolchain.
- **Command**:
  ```bash
  cargo build -p lmprobe --release 2>&1 | grep -cE '^warning:'
  ```
- **Expected**: exit 1 (grep returns 1 when no matches); stdout exactly `0`.
- **Pass**: stdout is `0`.

### TC-004 — Both feature configurations build clean

- **Language(s)**: n/a
- **Severity**: major
- **Preconditions**: cargo toolchain.
- **Command**:
  ```bash
  for f in '' '--features serve'; do
    printf '[%s] ' "$f"
    cargo build -p lmprobe --release $f 2>&1 | grep -cE '^warning:'
  done
  ```
- **Expected**: exit 0; stdout has 2 lines, each ending ` 0`.
- **Pass**: each line ends with ` 0`.

---

## Sensor

### TC-010 — `discover` finds the workspace

- **Language(s)**: all
- **Severity**: major
- **Preconditions**: workspace root contains `Cargo.toml` + multiple crate manifests.
- **Command**:
  ```bash
  target/release/lmprobe --format json discover . | jq '.projects | length'
  ```
- **Expected**: exit 0; stdout is an integer ≥ 5.
- **Pass**: stdout is a number ≥ 5.

### TC-011 — `discover --tree --depth 1` against rust/fd

- **Language(s)**: rust
- **Severity**: major
- **Preconditions**: pinned fixture at `$LMPROBE_QA_CORPUS/rust/fd/`. **Requires** `jq`. Uses `--format json` to bypass NDJSON's auto-emitted summary line (which broke the prior 2-line assertion).
- **Command**:
  ```bash
  cd $LMPROBE_QA_CORPUS/rust/fd && \
    $LMPROBE_REPO/target/release/lmprobe --format json discover --tree --depth 1 . | \
    jq '{count: (.projects | length), types: [.projects[].project_type] | sort | unique}'
  ```
- **Expected**: exit 0; stdout JSON has `count == 2` and `types` includes both `"Cargo"` and `"Make"`.
- **Pass**: count is 2; both project types present.

### TC-012 — `search --name` returns hits with envelope shape

- **Language(s)**: rust
- **Severity**: blocker
- **Preconditions**: workspace root.
- **Command**:
  ```bash
  cd $LMPROBE_REPO && \
    target/release/lmprobe --format json search --name Pattern crates/core | \
    jq '{schema, hit_count: (.hits | length), exit_status}'
  ```
- **Expected**: exit 0; stdout JSON has `schema == "lmprobe.search.v1"`, `hit_count > 0`, `exit_status == "matched"`.
- **Pass**: all three field assertions.

### TC-013 — `def` exact-match anchored

- **Language(s)**: go
- **Severity**: blocker
- **Preconditions**: pinned fixture at `qa1/repos/go/cobra/`.
- **Command**:
  ```bash
  cd $LMPROBE_QA_CORPUS/go/cobra && \
    $LMPROBE_REPO/target/release/lmprobe --no-cache def Execute . --format ndjson | \
    head -3
  ```
- **Expected**: exit 0; first 3 NDJSON lines all have `name == "Execute"` (NOT `ExecuteContext`).
- **Pass**: every name in the first 3 lines is `Execute`.
- **Links**: 0.6

### TC-014 — `callers` populates `enclosing_decl`

- **Language(s)**: rust
- **Severity**: major
- **Preconditions**: workspace.
- **Command**:
  ```bash
  cd $LMPROBE_REPO/crates/lmprobe && \
    $LMPROBE_REPO/target/release/lmprobe --format json callers blame_with_follow_semantic | \
    jq '.hits[0].enclosing_decl'
  ```
- **Expected**: exit 0; stdout is a non-null JSON string.
- **Pass**: stdout parses as a non-null string (likely `"blame"`).
- **Links**: 4D

### TC-015 — `list-kinds rust` lists portable kinds

- **Language(s)**: rust
- **Severity**: major
- **Preconditions**: workspace.
- **Command**:
  ```bash
  target/release/lmprobe --format json list-kinds rust | jq '.rows | length'
  ```
- **Expected**: exit 0; stdout is exactly `12` (PortableKind has 12 variants).
- **Pass**: stdout is `12`.

### TC-015a — `list-kinds java` returns rich kinds (post-Phase-5C)

- **Language(s)**: java
- **Severity**: blocker
- **Preconditions**: workspace.
- **Command**:
  ```bash
  target/release/lmprobe --format json list-kinds java | jq '.rows | length'
  ```
- **Expected**: exit 0; stdout is an integer ≥ 5 (java's portable kinds).
- **Pass**: stdout is a number ≥ 5.
- **Links**: 5C — pre-Phase-5C this returned 0 (CLI bypassed registry; lang-core dispatch arm was removed by Phase 5B Step-6); regression coverage for the bypass class.

### TC-015b — `list-kinds python` returns rich kinds (post-Phase-5C)

- **Language(s)**: python
- **Severity**: blocker
- **Preconditions**: workspace.
- **Command**:
  ```bash
  target/release/lmprobe --format json list-kinds python | jq '.rows | length'
  ```
- **Expected**: exit 0; stdout is an integer ≥ 4.
- **Pass**: stdout is a number ≥ 4.
- **Links**: 5C

### TC-015c — `list-kinds kotlin` returns rich kinds (post-Phase-5C)

- **Language(s)**: kotlin
- **Severity**: blocker
- **Preconditions**: workspace.
- **Command**:
  ```bash
  target/release/lmprobe --format json list-kinds kotlin | jq '.rows | length'
  ```
- **Expected**: exit 0; stdout is an integer ≥ 5.
- **Pass**: stdout is a number ≥ 5.
- **Links**: 5C

### TC-015d — `list-kinds yaml` returns rich kinds (data-tier; post-Phase-5C)

- **Language(s)**: yaml
- **Severity**: blocker
- **Preconditions**: workspace.
- **Command**:
  ```bash
  target/release/lmprobe --format json list-kinds yaml | jq '.rows | length'
  ```
- **Expected**: exit 0; stdout is an integer ≥ 2.
- **Pass**: stdout is a number ≥ 2.
- **Links**: 5C

### TC-015e — `list-kinds json` returns rich kinds (data-tier; post-Phase-5C)

- **Language(s)**: json
- **Severity**: blocker
- **Preconditions**: workspace.
- **Command**:
  ```bash
  target/release/lmprobe --format json list-kinds json | jq '.rows | length'
  ```
- **Expected**: exit 0; stdout is an integer ≥ 2.
- **Pass**: stdout is a number ≥ 2.
- **Links**: 5C

### TC-016 — Nested-composition GraphQL perf gate

- **Language(s)**: rust
- **Severity**: major
- **Preconditions**: workspace at HEAD; release build; warm-cache (run the query once first to populate, then measure on the second run). Pure perf measurement — no engine-side complexity-limit semantics per stateless doctrine.
- **Command**:
  ```bash
  cd $LMPROBE_REPO
  Q='{ def(name: "Pattern", path: "crates/core/src/matcher") { hits { name callers(limit: 5, path: "crates/core/src/matcher") { hits { name enclosing { name kind } } } recentHistory(limit: 3) { entries { commitHash } } } } }'
  target/release/lmprobe --format json query "$Q" >/dev/null 2>&1   # warm
  start=$(date +%s%N)
  target/release/lmprobe --format json query "$Q" >/dev/null 2>&1   # measure
  end=$(date +%s%N)
  ms=$(( (end - start) / 1000000 ))
  echo "warm_ms=$ms (target: <250)"
  ```
- **Expected**: exit 0; stdout shows `warm_ms=N` with N < 250.
- **Pass**: `warm_ms` < 250.
- **Links**: 4E (composition P0); CC3 perf budget.

### TC-019 — `imports` JS/TS single+double-quote parity

- **Language(s)**: typescript
- **Severity**: major
- **Preconditions**: writable tmp dir. Phase 0 §2.8 — pre-fix, JS/TS imports parsing was quote-style-sensitive. **Requires** `jq`.
- **Command**:
  ```bash
  d=/tmp/lmprobe-tc019-$$
  mkdir -p "$d"; cd "$d"
  cat > a.ts <<'EOF'
  import { Foo } from "./mod";
  import { Bar } from './mod';
  EOF
  $LMPROBE_REPO/target/release/lmprobe --format json imports './mod' . -l typescript | jq '.hits | length'
  cd /; rm -rf "$d"
  ```
- **Expected**: exit 0; stdout integer ≥ 2 (both quote styles match).
- **Pass**: stdout ≥ 2.
- **Links**: 0 (§2.8); 2.2 (ImportGrammar work)

---

## History

### TC-020 — `history blame Execute` on cobra fixture

- **Language(s)**: go
- **Severity**: major
- **Preconditions**: pinned fixture at `qa1/repos/go/cobra/` (stable multi-commit history on `cobra.go`'s `Execute` symbol — cobra was not touched by the 2026-05-02 history rewrite). **Requires** `jq`.
- **Command**:
  ```bash
  cd $LMPROBE_QA_CORPUS/go/cobra && \
    $LMPROBE_REPO/target/release/lmprobe --no-cache --format json history blame Execute . | \
    jq '{entries: (.entries | length), introducing_commit: .entries[-1].commit_hash, has_introducing: (.entries[-1].commit_hash | length > 0)}'
  ```
- **Expected**: exit 0; `entries > 0`; `introducing_commit` is a non-empty hex string.
- **Pass**: `entries >= 1` AND `introducing_commit` is a non-empty string.
- **Links**: 3A; rebased off rewrite-fragile pattern.rs fixture per QA Cycle 2 round 1 finding.

### TC-021 — `history blame Execute --follow` on cobra fixture (multi-commit)

- **Language(s)**: go
- **Severity**: major
- **Preconditions**: pinned fixture at `qa1/repos/go/cobra/`. The `Execute` symbol has many commits across `cobra.go`'s history. **Requires** `jq`.
- **Command**:
  ```bash
  cd $LMPROBE_QA_CORPUS/go/cobra && \
    $LMPROBE_REPO/target/release/lmprobe --no-cache --format json history blame Execute --follow . | \
    jq '{entries: (.entries | length), follows: (.follows | length), oldest: .entries[-1].commit_hash}'
  ```
- **Expected**: exit 0; `entries >= 5` (cobra has many touches on Execute); `oldest` is a non-empty hex string.
- **Pass**: `entries >= 5`; `oldest` non-empty.
- **Links**: 4B; rebased off rewrite-fragile pattern.rs fixture per QA Cycle 2 round 1 finding. Note: cobra's `cobra.go` may not have file-renames in its history; if `follows == 0`, that's acceptable — the assertion is on entries count, not on follows count.

### TC-022 — `history churn --top 5 --since 90d`

- **Language(s)**: all
- **Severity**: minor
- **Preconditions**: workspace at HEAD.
- **Command**:
  ```bash
  cd $LMPROBE_REPO && \
    target/release/lmprobe --no-cache --format json history churn --top 5 --since 90d . | \
    jq '.rows | length'
  ```
- **Expected**: exit 0; stdout integer ≤ 5.
- **Pass**: stdout is a number ≤ 5.

### TC-023 — `history blame --semantic` classifies multi-commit history

- **Language(s)**: go
- **Severity**: major
- **Preconditions**: pinned fixture at `qa1/repos/go/cobra/` (stable multi-commit history on `Execute`). **Requires** `jq`. Verifies the `--semantic` flag emits the richer change_type taxonomy (BODY_REWRITE / SIGNATURE_CHANGE / etc.) rather than collapsing to plain LOGIC_CHANGE/FORMATTING.
- **Command**:
  ```bash
  cd $LMPROBE_QA_CORPUS/go/cobra && \
    $LMPROBE_REPO/target/release/lmprobe --format json history blame Execute --semantic . | \
    jq '[.entries[].change_type] | unique'
  ```
- **Expected**: exit 0; stdout JSON array contains at least one of `BODY_REWRITE`, `SIGNATURE_CHANGE`, or `INTRODUCED` (the semantic taxonomy values, NOT plain `FORMATTING` which only appears under non-semantic blame).
- **Pass**: `change_type` list includes at least one semantic-taxonomy value.
- **Links**: 4C; rebased off rewrite-fragile pattern.rs fixture per QA Cycle 2 round 1 finding.

### TC-024 — `ast-diff` between two pattern.rs revisions

- **Language(s)**: rust
- **Severity**: major
- **Preconditions**: workspace; commit `c9b450d8` reachable.
- **Command**:
  ```bash
  cd $LMPROBE_REPO && \
    target/release/lmprobe --format json ast-diff crates/core/src/matcher/pattern.rs --from c9b450d8 --to HEAD | \
    jq '[.nodes[].change_type] | group_by(.) | map({type: .[0], count: length})'
  ```
- **Expected**: exit 0; includes `ADDED`, `MODIFIED`, `REMOVED`, `UNCHANGED` buckets.
- **Pass**: all four change_type values appear.
- **Links**: 4C

### TC-025 — `evolution Pattern` and `history evolution Pattern` byte-identical

- **Language(s)**: go
- **Severity**: minor
- **Preconditions**: pinned fixture at `qa1/repos/go/cobra/`.
- **Command**:
  ```bash
  cd $LMPROBE_QA_CORPUS/go/cobra && \
    diff \
      <($LMPROBE_REPO/target/release/lmprobe --no-cache --format json evolution Execute .) \
      <($LMPROBE_REPO/target/release/lmprobe --no-cache --format json history evolution Execute .)
  ```
- **Expected**: exit 0; stdout empty.
- **Pass**: exit 0 with empty stdout.

---

## Mutation

D7 mutation-safety contract is the load-bearing invariant for this category. Every mutation case asserts that either (a) the user's tree is byte-identical post-rollback (verified by sha256), or (b) the mutation refuses with an explicit diagnostic code. No silent-mutation paths are acceptable.

### TC-030 — `trace --worktree` leaves user tree byte-identical

- **Language(s)**: rust
- **Severity**: blocker
- **Preconditions**: writable tmp dir; throwaway git repo. `trace --exec 'true'` produces no `[TRACE]` lines so it returns `exit 1` (`no_match`); the assertion is on tree-byte-identity, not on trace's exit code.
- **Command**:
  ```bash
  d=/tmp/lmprobe-tc030-$$
  rm -rf "$d"; mkdir -p "$d"; cd "$d"
  git init -q; git config user.email t@t; git config user.name t
  cat > main.rs <<'EOF'
  fn main() {
      let x = 42;
      println!("hello {}", x);
  }
  EOF
  git add .; git commit -qm init
  sha_before=$(sha256sum main.rs | cut -d' ' -f1)
  XDG_CACHE_HOME=$d/xdg $LMPROBE_REPO/target/release/lmprobe --format json \
    trace 'let $VAR = $RHS' . --exec 'true' --worktree > /dev/null 2>&1
  trace_exit=$?
  sha_after=$(sha256sum main.rs | cut -d' ' -f1)
  echo "trace_exit=$trace_exit (1 = no_match expected)"
  if [ "$sha_before" = "$sha_after" ]; then
    echo "PASS: tree byte-identical"
  else
    echo "FAIL: sha differs (before=$sha_before after=$sha_after)"
  fi
  cd /; rm -rf "$d"
  ```
- **Expected**: script exit 0; stdout contains `trace_exit=1` and `PASS: tree byte-identical`.
- **Pass**: stdout contains `PASS: tree byte-identical`.
- **Links**: 4B (D7)

### TC-031 — `trace --keep-patch` produces a unified diff

- **Language(s)**: rust
- **Severity**: major
- **Preconditions**: writable tmp dir; throwaway git repo.
- **Command**:
  ```bash
  d=/tmp/lmprobe-tc031-$$
  rm -rf "$d"; mkdir -p "$d"; cd "$d"
  git init -q; git config user.email t@t; git config user.name t
  printf 'fn main() { let x = 42; }\n' > main.rs
  git add .; git commit -qm init
  XDG_CACHE_HOME=$d/xdg $LMPROBE_REPO/target/release/lmprobe \
    trace 'let $VAR = $RHS' . --exec 'true' --worktree --keep-patch > /dev/null 2>&1
  trace_exit=$?
  echo "trace_exit=$trace_exit (1 = no_match expected)"
  patch_files=$(ls "$d/xdg/lmprobe/patches/" 2>/dev/null)
  if [ -n "$patch_files" ]; then
    echo "PATCH_PRESENT: $patch_files"
    head -3 "$d/xdg/lmprobe/patches/"trace-*.patch
  else
    echo "FAIL: no patch file"
  fi
  cd /; rm -rf "$d"
  ```
- **Expected**: exit 0; stdout has `PATCH_PRESENT:` and `diff --git`.
- **Pass**: both strings present.

### TC-032 — `fix --apply` mutates atomically

- **Language(s)**: rust
- **Severity**: blocker
- **Preconditions**: writable tmp dir; throwaway git repo. **Requires** `jq` (or substitute `python3 -c 'import sys, json; …'`).
- **Command**:
  ```bash
  d=/tmp/lmprobe-tc032-$$
  rm -rf "$d"; mkdir -p "$d"; cd "$d"
  git init -q; git config user.email t@t; git config user.name t
  printf 'fn main() { println!("hi"); }\n' > main.rs
  git add .; git commit -qm init
  XDG_CACHE_HOME=$d/xdg $LMPROBE_REPO/target/release/lmprobe --format json \
    fix -p 'println!($X)' -r 'eprintln!($X)' -l rust . --apply 2>/dev/null | \
    jq '{mode, applied_files_count, has_audit: (.audit_log_path != null), has_patch: (.patch_path != null)}'
  echo "--- file after ---"
  cat main.rs
  cd /; rm -rf "$d"
  ```
- **Expected**: exit 0; stdout JSON has `mode == "applied"`, `applied_files_count >= 1`, `has_audit: true`, `has_patch: true`; main.rs after the run contains `eprintln!`.
- **Pass**: JSON shows applied; file contains `eprintln!`.
- **Links**: 4C (D7)

### TC-033 — `fix --apply` refuses dirty tree

- **Language(s)**: rust
- **Severity**: blocker
- **Preconditions**: writable tmp dir; throwaway git repo with one untracked file. **Requires** `jq`.
- **Command**:
  ```bash
  d=/tmp/lmprobe-tc033-$$
  rm -rf "$d"; mkdir -p "$d"; cd "$d"
  git init -q; git config user.email t@t; git config user.name t
  printf 'fn main() {}\n' > main.rs
  git add .; git commit -qm init
  echo 'extra' >> main.rs
  XDG_CACHE_HOME=$d/xdg $LMPROBE_REPO/target/release/lmprobe --format json \
    fix -p 'fn $X()' -r 'fn renamed_$X()' -l rust . --apply 2>/dev/null | \
    jq '{exit_status, codes: [.warnings[].code]}'
  cd /; rm -rf "$d"
  ```
- **Expected**: exit 0 (jq); JSON has `exit_status: "error"`; `codes` includes `"DIRTY_TREE"`.
- **Pass**: stdout shows `DIRTY_TREE`.
- **Links**: D7

### TC-034 — `trace` refuses non-git directory

- **Language(s)**: rust
- **Severity**: blocker
- **Preconditions**: writable tmp dir without `.git/`. **Requires** `jq`.
- **Command**:
  ```bash
  d=/tmp/lmprobe-tc034-$$
  rm -rf "$d"; mkdir -p "$d"; cd "$d"
  printf 'fn main() {}\n' > main.rs
  $LMPROBE_REPO/target/release/lmprobe --format json \
    trace 'fn main() { $$$ }' . --exec 'true' 2>/dev/null | \
    jq '{exit_status, codes: [.warnings[].code]}'
  cd /; rm -rf "$d"
  ```
- **Expected**: exit 0; stdout JSON has `exit_status: "error"`; `codes` includes `NOT_GIT_REPO`.
- **Pass**: stdout shows `NOT_GIT_REPO`.

### TC-035 — `trace --worktree exec panics` rolls back; tree byte-identical

- **Language(s)**: rust
- **Severity**: blocker
- **Preconditions**: writable tmp dir; throwaway git repo. Verifies the D7 contract holds even when the `--exec` command itself fails — instrumentation persists ONLY in the worktree which is destroyed; the user's tree is untouched.
- **Command**:
  ```bash
  d=/tmp/lmprobe-tc035-$$
  rm -rf "$d"; mkdir -p "$d"; cd "$d"
  git init -q; git config user.email t@t; git config user.name t
  cat > main.rs <<'EOF'
  fn main() { let y = 7; println!("{}", y); }
  EOF
  git add .; git commit -qm init
  sha_before=$(sha256sum main.rs | cut -d' ' -f1)
  XDG_CACHE_HOME=$d/xdg $LMPROBE_REPO/target/release/lmprobe \
    trace 'let $VAR = $RHS' . --exec 'false' --worktree > /dev/null 2>&1
  exec_exit=$?
  sha_after=$(sha256sum main.rs | cut -d' ' -f1)
  echo "exec_exit=$exec_exit"
  [ "$sha_before" = "$sha_after" ] && echo "PASS: D7 byte-identical post-failed-exec" || echo "FAIL: drift"
  cd /; rm -rf "$d"
  ```
- **Expected**: script exit 0; stdout has `PASS: D7 byte-identical post-failed-exec`.
- **Pass**: D7 byte-identity assertion holds even though the exec'd command failed.
- **Links**: D7

### TC-037 — `trace --template` syntax validation refuses before write

- **Language(s)**: rust
- **Severity**: blocker
- **Preconditions**: writable tmp dir; throwaway git repo. Verifies `trace` validates the template syntax BEFORE writing any instrumentation — invalid template → refusal with diagnostic, no broken-code state. **Requires** `jq` for output parsing (optional).
- **Command**:
  ```bash
  d=/tmp/lmprobe-tc037-$$
  rm -rf "$d"; mkdir -p "$d"; cd "$d"
  git init -q; git config user.email t@t; git config user.name t
  cat > main.rs <<'EOF'
  fn main() { let x = 42; println!("{}", x); }
  EOF
  git add .; git commit -qm init
  sha_pre=$(sha256sum main.rs | cut -d' ' -f1)
  XDG_CACHE_HOME=$d/xdg $LMPROBE_REPO/target/release/lmprobe \
    trace 'let $V = $E' . --template 'invalid-rust ((( syntax' --exec 'true' --worktree > /dev/null 2>&1
  trace_exit=$?
  sha_post=$(sha256sum main.rs | cut -d' ' -f1)
  echo "trace_exit=$trace_exit"
  [ "$sha_pre" = "$sha_post" ] && echo "PASS: file untouched" || echo "FAIL: file mutated"
  cd /; rm -rf "$d"
  ```
- **Expected**: script exit 0; stdout has `PASS: file untouched`. trace_exit non-zero (refusal exit code).
- **Pass**: file byte-identical post-attempt; trace did NOT write instrumentation.
- **Links**: D7 (refuse-before-write principle)

---

## Format

### TC-040 — `--format mermaid` on impact

- **Language(s)**: rust
- **Severity**: major
- **Preconditions**: workspace at HEAD.
- **Command**:
  ```bash
  cd $LMPROBE_REPO && \
    target/release/lmprobe --format mermaid impact Pattern --depth 2 . 2>/dev/null
  ```
- **Expected**: exit 0; stdout starts with ` ```mermaid ` and contains `graph TD` plus `-->`.
- **Pass**: both strings present.
- **Links**: 4C

### TC-041 — `--format dot` on subtypes

- **Language(s)**: rust
- **Severity**: major
- **Preconditions**: workspace.
- **Command**:
  ```bash
  cd $LMPROBE_REPO && \
    target/release/lmprobe --format dot subtypes Matcher . 2>/dev/null | head -3
  ```
- **Expected**: exit 0; stdout begins with `digraph` and contains `rankdir=LR`.
- **Pass**: stdout begins with `digraph`.

### TC-042 — `--format mermaid` on a non-graph verb falls back

- **Language(s)**: n/a
- **Severity**: minor
- **Preconditions**: workspace; `stderr_allowed: true`.
- **Command**:
  ```bash
  cd $LMPROBE_REPO && \
    target/release/lmprobe --format mermaid def Pattern . 2>&1 >/dev/null | head -1
  ```
- **Expected**: exit 0 or 1; stderr contains `format Mermaid is only valid on graph-shaped verbs`.
- **Pass**: stderr message present.

---

## Language

This category splits into **Tier-1** (Ok / Partial verdicts — the language is wired and produces results) and **Tier-2** (Fail verdicts — the language is recognized but at least one sensor returns a `NOT_IMPLEMENTED` diagnostic). Tier-2 cases verify the diagnostic fires explicitly rather than silent-empty.

### TC-050 — `lang_verdicts_match` test green (LANG_VERDICTS byte-equal pre/post Phase 5)

- **Language(s)**: all
- **Severity**: blocker
- **Preconditions**: workspace; `cargo` toolchain.
- **Command**:
  ```bash
  cargo test -p lmprobe --release --test deep -- --ignored lang_verdicts_match
  ```
- **Expected**: exit 0; stdout contains `test lang_verdicts_match ... ok`.
- **Pass**: exit 0.
- **Links**: 5B

### TC-051 — Tier-1: Decorations on Kotlin in-tree fixture

- **Language(s)**: kotlin
- **Severity**: major
- **Preconditions**: in-tree fixture `crates/lmprobe/tests/fixtures/kotlin/`.
- **Command**:
  ```bash
  cd $LMPROBE_REPO && \
    target/release/lmprobe --format ndjson decorations Inject crates/lmprobe/tests/fixtures/kotlin | \
    head -2
  ```
- **Expected**: exit 0; stdout NDJSON contains `@Inject` and `@field:Inject`.
- **Pass**: both substrings present.

### TC-052 — Tier-1: `--has-child` on YAML fixture (data-tier)

- **Language(s)**: yaml
- **Severity**: major
- **Preconditions**: pinned fixture at `qa1/repos/yaml/ansible-examples/lamp_simple/`.
- **Command**:
  ```bash
  cd $LMPROBE_QA_CORPUS/yaml/ansible-examples/lamp_simple && \
    $LMPROBE_REPO/target/release/lmprobe --format json \
    search --kind class --has-child name | \
    jq '.hits | length'
  ```
- **Expected**: exit 0; stdout integer > 0.
- **Pass**: stdout is a positive number.

### TC-053 — Tier-2: NOT_IMPLEMENTED diagnostic on bash callers

- **Language(s)**: bash
- **Severity**: blocker
- **Preconditions**: any bash file or the workspace itself. Bash has no `CallGrammar` impl; `callers` should explicitly return `NOT_IMPLEMENTED` rather than silently returning empty. **Requires** `jq`.
- **Command**:
  ```bash
  d=/tmp/lmprobe-tc053-$$
  mkdir -p "$d"; cd "$d"
  printf '#!/bin/bash\nfunction greet() { echo hi; }\ngreet\n' > script.sh
  $LMPROBE_REPO/target/release/lmprobe --format json callers greet . -l bash | \
    jq '{exit_status, codes: [.warnings[].code]}'
  cd /; rm -rf "$d"
  ```
- **Expected**: exit 0 (jq); stdout JSON `codes` includes `NOT_IMPLEMENTED`.
- **Pass**: `NOT_IMPLEMENTED` present in warning codes.
- **Links**: 0.7

### TC-054 — Tier-2: NOT_IMPLEMENTED appears in envelope and stderr

- **Language(s)**: bash
- **Severity**: major
- **Preconditions**: as TC-053. Verifies the silent-empty path was closed in 0.7 — the diagnostic appears in BOTH the envelope's `warnings[]` array AND on stderr (suppressible via `-q`).
- **Command**:
  ```bash
  d=/tmp/lmprobe-tc054-$$
  mkdir -p "$d"; cd "$d"
  printf '#!/bin/bash\necho hi\n' > script.sh
  $LMPROBE_REPO/target/release/lmprobe callers any . -l bash 2>&1 >/dev/null | grep NOT_IMPLEMENTED | head -1
  cd /; rm -rf "$d"
  ```
- **Expected**: exit 0; stderr contains `NOT_IMPLEMENTED` line.
- **Pass**: stderr line includes `NOT_IMPLEMENTED`.
- **Links**: 0.7

### TC-053b — Tier-1: Nix `def` returns correct symbol or explicit NOT_IMPLEMENTED

- **Language(s)**: nix
- **Severity**: major
- **Preconditions**: writable tmp dir. Cycle 1 had Nix as FAIL; Phase 2C/5C should produce correct extraction OR an explicit `NOT_IMPLEMENTED` diagnostic. Silent-empty is a fail. **Requires** `jq`.
- **Command**:
  ```bash
  d=/tmp/lmprobe-tc053b-$$
  mkdir -p "$d"; cd "$d"
  cat > flake.nix <<'EOF'
  { description = "demo";
    outputs = inputs: {
      lib = { foo = x: x + 1; };
    };
  }
  EOF
  $LMPROBE_REPO/target/release/lmprobe --format json def foo . -l nix | \
    jq '{exit_status, hit_count: (.hits | length), warnings: [.warnings[].code]}'
  cd /; rm -rf "$d"
  ```
- **Expected**: exit 0; stdout JSON with EITHER (a) `hit_count >= 1` (correct extraction) OR (b) `NOT_IMPLEMENTED` in warnings (explicit diagnostic).
- **Pass**: NOT silent-empty — at least one of: `hit_count >= 1`, OR `NOT_IMPLEMENTED` in warnings.
- **Links**: 2C; 5C; QA Cycle 1 finding (Nix FAIL).

### TC-054b — Tier-1: Solidity `def` returns correct symbol or explicit NOT_IMPLEMENTED

- **Language(s)**: solidity
- **Severity**: major
- **Preconditions**: writable tmp dir. Cycle 1 had Solidity as FAIL; Phase 2C/5C should produce correct extraction OR explicit `NOT_IMPLEMENTED`. **Requires** `jq`.
- **Command**:
  ```bash
  d=/tmp/lmprobe-tc054b-$$
  mkdir -p "$d"; cd "$d"
  cat > Token.sol <<'EOF'
  // SPDX-License-Identifier: MIT
  pragma solidity ^0.8.0;
  contract Token {
    function transfer(address to, uint256 amount) public returns (bool) { return true; }
  }
  EOF
  $LMPROBE_REPO/target/release/lmprobe --format json def Token . -l solidity | \
    jq '{exit_status, hit_count: (.hits | length), warnings: [.warnings[].code]}'
  cd /; rm -rf "$d"
  ```
- **Expected**: exit 0; stdout JSON with EITHER `hit_count >= 1` OR `NOT_IMPLEMENTED` in warnings.
- **Pass**: NOT silent-empty.
- **Links**: 2C; 5C; QA Cycle 1 finding (Solidity FAIL).

### TC-055 — TOML table headers (`[package]`, `[features]`) are searchable

- **Language(s)**: toml
- **Severity**: major
- **Preconditions**: workspace's own `Cargo.toml` (has `[workspace]`, `[profile.release]`, `[workspace.package]`, `[workspace.dependencies]` table headers). Phase 2C work — TOML table headers should be first-class structural targets, not literal `[` strings. **Requires** `jq`.
- **Command**:
  ```bash
  $LMPROBE_REPO/target/release/lmprobe --format json search '' $LMPROBE_REPO/Cargo.toml -l toml --kind table | \
    jq '[.hits[].name] | unique | length'
  ```
- **Expected**: exit 0; stdout integer ≥ 2 (multiple distinct table names in the workspace Cargo.toml).
- **Pass**: stdout ≥ 2.
- **Links**: 2C (TopLevelKinds + TOML extractor)

---

## Cache

### TC-060 — `blob_cache` invariants

- **Language(s)**: all
- **Severity**: blocker
- **Preconditions**: cargo toolchain.
- **Command**:
  ```bash
  cargo test -p lmprobe --release --test blob_cache
  ```
- **Expected**: exit 0; stdout contains `4 passed`.
- **Pass**: both assertions.
- **Links**: 4A

### TC-061 — Cold + warm history_perf gate

- **Language(s)**: all
- **Severity**: blocker
- **Preconditions**: workspace at HEAD; release build.
- **Command**:
  ```bash
  cargo test -p lmprobe --release --test history_perf -- --ignored
  ```
- **Expected**: exit 0; stdout contains `test history_perf_gate ... ok`.
- **Pass**: exit 0.
- **Links**: 3A, 4A

### TC-062 — Incremental commit-cache demo

- **Language(s)**: all
- **Severity**: blocker
- **Preconditions**: writable tmp dir; throwaway git repo. **Requires** `sqlite3`.
- **Command**:
  ```bash
  d=/tmp/lmprobe-tc062-$$
  rm -rf "$d"; mkdir -p "$d"; cd "$d"
  git init -q; git config user.email t@t; git config user.name t
  for i in 1 2 3 4 5; do
    echo "fn f$i() {}" > "f$i.rs"
    git add .; git commit -qm "c$i" >/dev/null
  done
  XDG_CACHE_HOME=$d/xdg $LMPROBE_REPO/target/release/lmprobe --format ndjson \
    history churn --top 5 . > /dev/null 2>&1
  n_before=$(sqlite3 "$d/xdg/lmprobe/history.db" \
    "SELECT COUNT(*) FROM repo_commits;")
  echo "fn new_one() {}" > new.rs
  git add .; git commit -qm c6 >/dev/null
  XDG_CACHE_HOME=$d/xdg $LMPROBE_REPO/target/release/lmprobe --format ndjson \
    history churn --top 6 . > /dev/null 2>&1
  n_after=$(sqlite3 "$d/xdg/lmprobe/history.db" \
    "SELECT COUNT(*) FROM repo_commits;")
  echo "before=$n_before after=$n_after delta=$((n_after - n_before))"
  cd /; rm -rf "$d"
  ```
- **Expected**: exit 0; stdout `before=5 after=6 delta=1`.
- **Pass**: delta is exactly 1.
- **Links**: 3A

### TC-063 — `--no-cache` skips blob_signatures reads + writes

- **Language(s)**: all
- **Severity**: blocker
- **Preconditions**: writable tmp dir. **Requires** `sqlite3`. Regression test for the doc-implementation gap fixed in 4D-PB1.
- **Command**:
  ```bash
  d=/tmp/lmprobe-tc063-$$
  rm -rf "$d"; mkdir -p "$d"; cd "$d"
  git init -q; git config user.email t@t; git config user.name t
  printf 'pub fn target() {}\n' > lib.rs
  git add .; git commit -qm init
  printf 'pub fn target() { let _ = 1; }\n' > lib.rs
  git add .; git commit -qm edit
  XDG_CACHE_HOME=$d/xdg $LMPROBE_REPO/target/release/lmprobe \
    --no-cache history blame target . > /dev/null 2>&1
  n_nocache=$(sqlite3 "$d/xdg/lmprobe/history.db" \
    "SELECT COUNT(*) FROM blob_signatures;")
  XDG_CACHE_HOME=$d/xdg $LMPROBE_REPO/target/release/lmprobe \
    history blame target . > /dev/null 2>&1
  n_cached=$(sqlite3 "$d/xdg/lmprobe/history.db" \
    "SELECT COUNT(*) FROM blob_signatures;")
  echo "nocache_rows=$n_nocache cached_rows=$n_cached"
  cd /; rm -rf "$d"
  ```
- **Expected**: exit 0; stdout `nocache_rows=0 cached_rows=2` (or higher cached).
- **Pass**: nocache_rows is 0 AND cached_rows ≥ 2.
- **Links**: 4D-PB1

### TC-064 — Cache version-tuple bump invalidates blob_signatures rows

- **Language(s)**: all
- **Severity**: major
- **Preconditions**: writable tmp dir; throwaway git repo. **Requires** `sqlite3`. Verifies the D2 invariant — when `sensor_version`, `engine_version`, or `schema_version` increments, old rows stay readable but new lookups generate fresh rows; old rows aren't reused for the new version-tuple.
- **Command**:
  ```bash
  d=/tmp/lmprobe-tc064-$$
  rm -rf "$d"; mkdir -p "$d"; cd "$d"
  git init -q; git config user.email t@t; git config user.name t
  printf 'pub fn foo() {}\n' > lib.rs
  git add .; git commit -qm init
  printf 'pub fn foo() { let _ = 1; }\n' > lib.rs
  git add .; git commit -qm edit
  XDG_CACHE_HOME=$d/xdg $LMPROBE_REPO/target/release/lmprobe history blame foo . > /dev/null 2>&1
  n_pre=$(sqlite3 "$d/xdg/lmprobe/history.db" "SELECT COUNT(*) FROM blob_signatures;")
  # Mutate sensor_version to simulate a bump.
  sqlite3 "$d/xdg/lmprobe/history.db" "UPDATE blob_signatures SET sensor_version = sensor_version - 1;"
  XDG_CACHE_HOME=$d/xdg $LMPROBE_REPO/target/release/lmprobe history blame foo . > /dev/null 2>&1
  n_post=$(sqlite3 "$d/xdg/lmprobe/history.db" "SELECT COUNT(*) FROM blob_signatures;")
  echo "pre=$n_pre post=$n_post (post should be > pre — fresh version-tuple rows added; old rows untouched)"
  cd /; rm -rf "$d"
  ```
- **Expected**: exit 0; stdout `pre=N post=M` with M > N.
- **Pass**: post > pre (new rows written for current version-tuple; old rows preserved).
- **Links**: D2 (cache key invariant)

---

## Multi-repo

### TC-070 — `--workspace` fan-out tags origin_root

- **Language(s)**: all
- **Severity**: major
- **Preconditions**: workspace.
- **Command**:
  ```bash
  cat > /tmp/ws-tc070.yaml <<'EOF'
  roots:
    - path: $LMPROBE_REPO/crates/lmprobe-engine
      alias: engine
    - path: $LMPROBE_REPO/crates/core
      alias: core
  EOF
  $LMPROBE_REPO/target/release/lmprobe --workspace /tmp/ws-tc070.yaml \
    --format json search --name new --limit 200 2>/dev/null | \
    jq '[.hits[] | .origin_root] | group_by(.) | map({root: .[0], count: length})'
  rm /tmp/ws-tc070.yaml
  ```
- **Expected**: exit 0; stdout array contains `{root: "core", count: > 0}`.
- **Pass**: at least one entry has `"root": "core"`.
- **Links**: 4D

### TC-071 — `--workspace` refuses fix --apply

- **Language(s)**: rust
- **Severity**: blocker
- **Preconditions**: workspace; `stderr_allowed: true`.
- **Command**:
  ```bash
  cat > /tmp/ws-tc071.yaml <<'EOF'
  roots:
    - path: $LMPROBE_REPO/crates/lmprobe-engine
      alias: engine
  EOF
  $LMPROBE_REPO/target/release/lmprobe --workspace /tmp/ws-tc071.yaml \
    fix -p 'foo($X)' -r 'bar($X)' --apply 2>&1 >/dev/null | head -1
  echo "exit: $?"
  rm /tmp/ws-tc071.yaml
  ```
- **Expected**: exit 0 (head); stderr contains `WORKSPACE_READ_ONLY`; lmprobe's process exit code is 2.
- **Pass**: both stderr substring and exit-code 2.
- **Links**: 4D (D7)

---

## Failure-mode

### TC-080 — `def NotASymbol` returns no_match exit 1 (no kinds match)

- **Language(s)**: rust
- **Severity**: blocker
- **Preconditions**: workspace.
- **Command**:
  ```bash
  cd $LMPROBE_REPO && \
    target/release/lmprobe def NotASymbol crates/lmprobe ; echo "exit=$?"
  ```
- **Expected**: stdout ends with `exit=1` (process exit 1; envelope `exit_status: no_match`).
- **Pass**: stdout ends with `exit=1`.
- **Links**: 0.7 (no_match vs NOT_IMPLEMENTED distinction)

### TC-080a — Distinction: `no_match` (exit 1) vs `NOT_IMPLEMENTED` (exit 0 + diagnostic)

- **Language(s)**: rust + bash
- **Severity**: blocker
- **Preconditions**: workspace. The two cases must produce DIFFERENT exit codes despite both having empty hits. **Requires** `jq`.
- **Command**:
  ```bash
  echo "--- no_match (rust callers, no such symbol) ---"
  cd $LMPROBE_REPO && \
    target/release/lmprobe --format json callers ZzZNoSuchFunction crates/lmprobe -l rust | \
    jq -r '"exit_status=" + .exit_status'
  exit_no_match=$(target/release/lmprobe callers ZzZNoSuchFunction crates/lmprobe -l rust >/dev/null 2>/dev/null; echo $?)
  echo "process_exit=$exit_no_match (1 expected)"
  echo "--- NOT_IMPLEMENTED (bash callers, sensor not wired) ---"
  d=/tmp/lmprobe-tc080a-$$
  mkdir -p "$d"; cd "$d"
  printf '#!/bin/bash\necho hi\n' > script.sh
  $LMPROBE_REPO/target/release/lmprobe --format json callers any . -l bash | \
    jq -r '"exit_status=" + .exit_status + " has_NOT_IMPLEMENTED=" + ([.warnings[].code] | any(. == "NOT_IMPLEMENTED") | tostring)'
  exit_not_impl=$($LMPROBE_REPO/target/release/lmprobe callers any . -l bash >/dev/null 2>/dev/null; echo $?)
  echo "process_exit=$exit_not_impl (0 expected — diagnostic surfaces but exit_status is matched)"
  cd /; rm -rf "$d"
  ```
- **Expected**: no_match has `exit_status=no_match` and process exit 1; NOT_IMPLEMENTED has `exit_status=matched` (or similar) WITH `has_NOT_IMPLEMENTED=true` and process exit 0.
- **Pass**: distinct exit codes (1 vs 0); NOT_IMPLEMENTED diagnostic surfaces in warnings.
- **Links**: 0.7

### TC-081 — `--exit-zero-on-no-match` collapses 1→0

- **Language(s)**: rust
- **Severity**: major
- **Preconditions**: workspace.
- **Command**:
  ```bash
  cd $LMPROBE_REPO && \
    target/release/lmprobe --exit-zero-on-no-match def NotASymbol crates/lmprobe ; \
    echo "exit=$?"
  ```
- **Expected**: stdout ends with `exit=0`.
- **Pass**: stdout ends with `exit=0`.

### TC-082 — `-p` without `-l` errors

- **Language(s)**: rust
- **Severity**: major
- **Preconditions**: workspace; `stderr_allowed: true`.
- **Command**:
  ```bash
  cd $LMPROBE_REPO && \
    target/release/lmprobe search -p 'fn $X' . 2>&1 ; echo "exit=$?"
  ```
- **Expected**: stderr contains `requires `-l``; final line is `exit=2`.
- **Pass**: stderr substring present; exit 2.

### TC-083 — Bare `lmprobe` with no verb prints help

- **Language(s)**: n/a
- **Severity**: minor
- **Preconditions**: `target/release/lmprobe` built; `stderr_allowed: true`.
- **Command**:
  ```bash
  target/release/lmprobe ; echo "exit=$?"
  ```
- **Expected**: stderr has `Usage:`; exit 2.
- **Pass**: both.

### TC-084 — `serve` on default build refuses

- **Language(s)**: n/a
- **Severity**: blocker
- **Preconditions**: default build (no `--features serve`); `stderr_allowed: true`.
- **Command**:
  ```bash
  target/release/lmprobe serve 2>&1 | head -1 ; echo "exit=$?"
  ```
- **Expected**: stderr contains `requires `--features serve``; exit non-zero.
- **Pass**: stderr substring present.

### TC-018 — `callers` cross-language no-panic regression

- **Language(s)**: rust + xml (mixed)
- **Severity**: blocker
- **Preconditions**: pinned fixture at `qa1/repos/xml/dom4j/` (mixes XML, Java, Maven sidecars). Phase 0 §2.1 blocker — pre-fix `callers` panicked with `MultipleNode` when crossing into a language without a CallGrammar.
- **Command**:
  ```bash
  $LMPROBE_REPO/target/release/lmprobe callers servlet $LMPROBE_QA_CORPUS/xml/dom4j 2>&1 | tail -3
  echo "exit=$?"
  ```
- **Expected**: exit 0 or 1 (not 101); no `panicked at` substring in output.
- **Pass**: exit code is 0 or 1; no `panic` substring.
- **Links**: 0 (§2.1 panic fix); QA REPORT §2.1 blocker anchor.

### TC-085 — Broken-pipe (`cmd | head`) does not panic

- **Language(s)**: rust
- **Severity**: blocker
- **Preconditions**: workspace; release build. Phase 0 §2.14 blocker — pre-fix, `lmprobe ... | head` panicked with `failed printing to stdout: Broken pipe`.
- **Command**:
  ```bash
  $LMPROBE_REPO/target/release/lmprobe ref Root $LMPROBE_REPO/crates/core 2>&1 | head -1
  echo "exit=$?"
  $LMPROBE_REPO/target/release/lmprobe ref Root $LMPROBE_REPO/crates/core 2>&1 | head -3 | grep -ci 'panic\|broken pipe' || echo "no_panic_substring"
  ```
- **Expected**: exit 0 or 141 (SIGPIPE); first line of `ref Root` output emerges cleanly; output free of `panic` / `broken pipe` substrings.
- **Pass**: stdout/stderr free of `panic` / `broken pipe` substring; first line of `ref Root` content emerges.
- **Links**: 0 (§2.14 SIGPIPE fix)

### TC-086 — `imports` with quoted-input does not panic

- **Language(s)**: go
- **Severity**: blocker
- **Preconditions**: pinned fixture at `qa1/repos/go/cobra/`. Phase 0 §2.2 — pre-fix, `lmprobe imports '"fmt"' ...` panicked.
- **Command**:
  ```bash
  $LMPROBE_REPO/target/release/lmprobe imports '"fmt"' $LMPROBE_QA_CORPUS/go/cobra 2>&1 | tail -2
  echo "exit_quoted=$?"
  $LMPROBE_REPO/target/release/lmprobe imports 'fmt' $LMPROBE_QA_CORPUS/go/cobra >/dev/null 2>&1
  echo "exit_unquoted=$?"
  ```
- **Expected**: both exit codes 0 or 1 (not 101); no `panicked at` substring.
- **Pass**: no panic; both quoted and unquoted invocations exit cleanly.
- **Links**: 0 (§2.2 quoted-input fix); QA REPORT §2.2 blocker anchor.

### TC-087 — `\|` regex foot-gun: silent-zero → `REGEX_LITERAL_PIPE_NO_MATCH` warning

- **Language(s)**: n/a (cross-cutting; `lang: null` on the warning envelope)
- **Severity**: major
- **Preconditions**: writable tmp dir; **requires** `jq`.
- **Command**:
  ```bash
  L=$LMPROBE_REPO/target/release/lmprobe
  d=/tmp/lmprobe-tc087-$$
  rm -rf "$d"; mkdir -p "$d"
  cat > "$d/sample.txt" <<'EOF'
  this line has TODO marker
  this line has FIXME marker
  literal a|b separator
  EOF

  # L1: \| with zero hits -> warning fires; envelope no_match
  $L --format json grep 'TODO\|FIXME' "$d" 2>/dev/null | jq -r '
    if .exit_status == "no_match" and ([.warnings[].code] | index("REGEX_LITERAL_PIPE_NO_MATCH")) then
      "L1=PASS"
    else "L1=FAIL: \(.exit_status) warnings=\([.warnings[].code])" end'

  # L2: correct alt -> matched, no warning
  $L --format json grep 'TODO|FIXME' "$d" 2>/dev/null | jq -r '
    if .exit_status == "matched" and (.hits | length) == 2 and (.warnings | length) == 0 then
      "L2=PASS"
    else "L2=FAIL: \(.exit_status) hits=\(.hits|length) warnings=\(.warnings|length)" end'

  # L3: find verb covered too
  $L --format json find 'foo\|bar' "$d" 2>/dev/null | jq -r '
    if ([.warnings[].code] | index("REGEX_LITERAL_PIPE_NO_MATCH")) then
      "L3=PASS"
    else "L3=FAIL" end'

  # L4: --help carries Rust-regex-flavor doctrine note (both verbs)
  $L grep --help 2>&1 | grep -qE 'alternation is `\|`|literal pipe' && echo "L4_grep=PASS" || echo "L4_grep=FAIL"
  $L find --help 2>&1 | grep -qE 'alternation is `\|`|literal pipe' && echo "L4_find=PASS" || echo "L4_find=FAIL"

  # E1: \| AND non-empty result -> warning does NOT fire (gating on zero-result)
  $L --format json grep 'a\|b' "$d" 2>/dev/null | jq -r '
    if .exit_status == "matched" and (.warnings | length) == 0 and (.hits | length) >= 1 then
      "E1=PASS"
    else "E1=FAIL: \(.exit_status) warnings=\(.warnings | length) hits=\(.hits|length)" end'

  # E2: GraphQL surface inherits via envelope; resolver carries warnings[]
  $L --format json query '{ grep(pattern: "TODO\\|FIXME", path: "'"$d"'") { warnings { code } exitStatus } }' 2>/dev/null | \
    jq -r '
    if (.data.grep.exitStatus == "NO_MATCH") and
       ([.data.grep.warnings[].code] | index("REGEX_LITERAL_PIPE_NO_MATCH")) then
      "E2=PASS"
    else "E2=FAIL" end'

  # E3: -q/--quiet suppresses stderr but envelope warnings[] still ships
  out=$($L -q --format json grep 'TODO\|FIXME' "$d" 2>/tmp/tc087_e3.stderr)
  has_warn=$(echo "$out" | jq -r '[.warnings[].code] | index("REGEX_LITERAL_PIPE_NO_MATCH") != null')
  stderr_size=$(wc -c </tmp/tc087_e3.stderr)
  if [ "$has_warn" = "true" ] && [ "$stderr_size" = "0" ]; then echo "E3=PASS"; else echo "E3=FAIL: warn=$has_warn stderr_size=$stderr_size"; fi

  rm -rf "$d" /tmp/tc087_e3.stderr
  ```
- **Expected**: exit 0 (the script's last command sets the exit); stdout has all 8 PASS markers (`L1=PASS L2=PASS L3=PASS L4_grep=PASS L4_find=PASS E1=PASS E2=PASS E3=PASS`).
- **Pass**: every check above is `PASS`.
- **Links**: Phase 6A.1 (`fix(grep,find): warn on POSIX-BRE \| alternation foot-gun`); QA battle-test cross-cutting Issue #5; battle-test Pattern D.

### TC-088 — Phase 6A meta-CLI doctrine: `--version`, `-p` audit note, `--name <SUBSTR>` semantics

- **Language(s)**: n/a
- **Severity**: minor
- **Preconditions**: workspace at HEAD; release build.
- **Command**:
  ```bash
  L=$LMPROBE_REPO/target/release/lmprobe

  # #2 — --version reports `lmprobe X.Y.Z (commit SHORTSHA)`
  $L --version 2>&1 | grep -qE '^lmprobe [0-9]+\.[0-9]+\.[0-9]+ \(commit [0-9a-f]{7}\)$' && echo "V=PASS" || echo "V=FAIL: $($L --version 2>&1)"

  # #5 — top-level --help carries Phase 6A #5 doctrine block on `-p` / `--pattern`
  $L --help 2>&1 | grep -qE '`-p` */ *`--pattern` is always pattern' && echo "P=PASS" || echo "P=FAIL"

  # #3 — search --help describes `--name <SUBSTR>` (substring) semantics, not `<GLOB>`
  $L search --help 2>&1 | grep -qE 'Case-insensitive substring filter' && echo "N1=PASS" || echo "N1=FAIL"
  $L search --help 2>&1 | grep -q '<SUBSTR>' && echo "N2=PASS" || echo "N2=FAIL"
  ```
- **Expected**: exit 0; stdout has `V=PASS P=PASS N1=PASS N2=PASS`.
- **Pass**: every check is PASS.
- **Links**: Phase 6A items #2 (`6627f4d4`), #3 (`6eb74a85`), #5 (`0ab4df86`).

### TC-089 — `-v/--verbose` stderr-gating doctrine

- **Language(s)**: n/a
- **Severity**: major
- **Preconditions**: writable tmp dir.
- **Command**:
  ```bash
  L=$LMPROBE_REPO/target/release/lmprobe
  d=/tmp/lmprobe-tc089-$$
  rm -rf "$d"; mkdir -p "$d"
  echo 'fn x() {}' > "$d/a.rs"
  echo '<x/>' > "$d/c.xml"

  # E1 — default mode: cross-lang NOT_IMPLEMENTED walk-noise gated (stderr empty)
  $L --format json --no-cache callers x "$d" 2>/tmp/tc089_e1.stderr >/dev/null
  s=$(wc -c </tmp/tc089_e1.stderr); [ "$s" = "0" ] && echo "E1=PASS" || echo "E1=FAIL: stderr-size=$s"

  # E2 — -v mode: same call now surfaces NOT_IMPLEMENTED on stderr
  $L -v --format json --no-cache callers x "$d" 2>/tmp/tc089_e2.stderr >/dev/null
  grep -qE 'NOT_IMPLEMENTED' /tmp/tc089_e2.stderr && echo "E2=PASS" || echo "E2=FAIL"

  # E3 — user-facing diagnostic (REGEX_LITERAL_PIPE_NO_MATCH) still surfaces on default (NOT walk-noise)
  $L --format json grep 'TODO\|FIXME' "$d" 2>/tmp/tc089_e3.stderr >/dev/null
  grep -q 'REGEX_LITERAL_PIPE_NO_MATCH' /tmp/tc089_e3.stderr && echo "E3=PASS" || echo "E3=FAIL"

  # E4 — -q suppresses everything; envelope warnings[] still ships
  out=$($L -q --format json grep 'TODO\|FIXME' "$d" 2>/tmp/tc089_e4.stderr)
  s=$(wc -c </tmp/tc089_e4.stderr)
  has=$(echo "$out" | jq -r '[.warnings[].code] | index("REGEX_LITERAL_PIPE_NO_MATCH") != null')
  if [ "$s" = "0" ] && [ "$has" = "true" ]; then echo "E4=PASS"; else echo "E4=FAIL: stderr=$s warn=$has"; fi

  rm -rf "$d" /tmp/tc089_*.stderr
  ```
- **Expected**: exit 0; stdout has `E1=PASS E2=PASS E3=PASS E4=PASS`.
- **Pass**: every check is PASS.
- **Links**: Phase 6A item #4 (`2649cdf0`); QA battle-test cross-cutting Issue #14.

### TC-090 — `search --name X PATH` precedence: positional auto-swap warns

- **Language(s)**: n/a
- **Severity**: major
- **Preconditions**: writable tmp dir.
- **Command**:
  ```bash
  L=$LMPROBE_REPO/target/release/lmprobe
  d=/tmp/lmprobe-tc090-$$
  rm -rf "$d"; mkdir -p "$d/sub"
  echo 'fn TodoItem() {}' > "$d/sub/a.rs"

  # E1 — single positional + --name set: positional reinterpreted as PATH; warn fires
  out=$($L --format json search --name TodoItem "$d" 2>/tmp/tc090.stderr)
  hits=$(echo "$out" | jq -r '.hits | length')
  has_warn=$(echo "$out" | jq -r '[.warnings[].code] | index("SEARCH_NAME_POSITIONAL_AS_PATH") != null')
  if [ "$hits" -ge 1 ] && [ "$has_warn" = "true" ]; then echo "E1=PASS"; else echo "E1=FAIL: hits=$hits warn=$has_warn"; fi

  # E2 — stderr line carries the warning code + actionable hint
  grep -q 'SEARCH_NAME_POSITIONAL_AS_PATH' /tmp/tc090.stderr && \
    grep -q 'reinterpreted positional' /tmp/tc090.stderr && \
    echo "E2=PASS" || echo "E2=FAIL"

  rm -rf "$d" /tmp/tc090.stderr
  ```
- **Expected**: exit 0; stdout has `E1=PASS E2=PASS`.
- **Pass**: every check is PASS.
- **Links**: Phase 6A item #6 (`c4bb8050`); QA battle-test cross-cutting Issue #13.

### TC-091 — TS `exports` works without `PATTERN_PARSE_FAILED`

- **Language(s)**: typescript, tsx
- **Severity**: major
- **Preconditions**: pinned fixture under `qa1/battle_test/repos/tsx/recharts/` (TSX-heavy with many `export {…}` declarations) OR any TS/TSX repo with re-exports.
- **Command**:
  ```bash
  L=$LMPROBE_REPO/target/release/lmprobe
  REPO=$LMPROBE_QA_HOME/qa1/battle_test/repos/tsx/recharts
  [ -d "$REPO" ] || REPO=$LMPROBE_QA_CORPUS/typescript/zod

  # E1 — `exports default` runs cleanly: exit_status is matched OR no_match (NOT error);
  # no PATTERN_PARSE_FAILED in errors[]
  out=$($L --format json exports default "$REPO" 2>/dev/null)
  bad=$(echo "$out" | jq -r '[.errors[]?.code] | index("PATTERN_PARSE_FAILED") != null')
  status=$(echo "$out" | jq -r '.exit_status')
  if [ "$bad" != "true" ] && [ "$status" != "error" ]; then echo "E1=PASS"; else echo "E1=FAIL: status=$status pattern_parse_failed=$bad"; fi
  ```
- **Expected**: exit 0; stdout has `E1=PASS`.
- **Pass**: `errors[]` does NOT contain `PATTERN_PARSE_FAILED`; `exit_status` is not `error`.
- **Links**: Phase 6A item #7 (`5d4cd169`); QA battle-test top-issue #12.

### TC-092 — `-l <lang>` force-parser + shebang detection (BACK-COMPAT BREAK)

- **Language(s)**: rust, bash
- **Severity**: blocker (back-compat break is intentional; verifies migration path)
- **Preconditions**: writable tmp dir.
- **Command**:
  ```bash
  L=$LMPROBE_REPO/target/release/lmprobe
  d=/tmp/lmprobe-tc092-$$
  rm -rf "$d"; mkdir -p "$d/bin"
  echo 'fn rust_only() {}' > "$d/a.rs"
  echo 'def python_only(): pass' > "$d/b.py"
  cat > "$d/bin/myscript" <<'EOF'
  #!/usr/bin/env bash
  function helper() { echo hi; }
  EOF
  chmod +x "$d/bin/myscript"

  # NEW: `-l rust` forces Rust parser; finds rust_only in a.rs (and walks all files)
  out=$($L -l rust --format json search --name rust_only "$d" 2>/dev/null)
  hits=$(echo "$out" | jq -r '.hits | length')
  [ "$hits" -ge 1 ] && echo "L1=PASS" || echo "L1=FAIL: hits=$hits"

  # MIGRATION: `--type rust` filters walker to .rs only (the OLD `-l rust` semantics)
  out=$($L --type rust --format json search --name rust_only "$d" 2>/dev/null)
  hits=$(echo "$out" | jq -r '.hits | length')
  [ "$hits" = "1" ] && echo "L2=PASS" || echo "L2=FAIL: hits=$hits"

  # SHEBANG with explicit `-l bash`: extensionless `bin/myscript` is parsed; helper found
  out=$($L -l bash --format json search --name helper "$d/bin" 2>/dev/null)
  hits=$(echo "$out" | jq -r '.hits | length')
  files=$(echo "$out" | jq -r '[.hits[].file_path] | unique | length')
  [ "$hits" -ge 1 ] && [ "$files" = "1" ] && echo "S1=PASS" || echo "S1=FAIL: hits=$hits files=$files"

  # SHEBANG without `-l`: shebang detection alone makes the script searchable
  out=$($L --format json search --name helper "$d/bin" 2>/dev/null)
  hits=$(echo "$out" | jq -r '.hits | length')
  [ "$hits" -ge 1 ] && echo "S2=PASS" || echo "S2=FAIL: hits=$hits"

  rm -rf "$d"
  ```
- **Expected**: exit 0; stdout has `L1=PASS L2=PASS S1=PASS S2=PASS`.
- **Pass**: every check is PASS.
- **Back-compat note**: prior to Phase 6A, `lmprobe -l rust .` filtered the walker to `.rs` files; now it forces the Rust parser on every walked file. **Agents that relied on the old extension-filter must migrate to `--type rust .`**.
- **Links**: Phase 6A item #8 (`ba9884f5`); QA battle-test cross-cutting Issue #11; cycle-1 wishlist (shebang).

### TC-093 — `--exclude-glob` flag composes with `--globs` and is cumulative

- **Language(s)**: n/a
- **Severity**: minor
- **Preconditions**: writable tmp dir.
- **Command**:
  ```bash
  L=$LMPROBE_REPO/target/release/lmprobe
  d=/tmp/lmprobe-tc093-$$
  rm -rf "$d"; mkdir -p "$d"
  echo 'fn x() {}' > "$d/a.rs"
  echo 'def py(): pass' > "$d/b.py"
  echo 'function js() {}' > "$d/c.js"

  # E1 — bare grep finds 1 (b.py)
  bare=$($L --format json grep py "$d" 2>/dev/null | jq -r '.hits | length')
  [ "$bare" = "1" ] && echo "E1=PASS" || echo "E1=FAIL: hits=$bare"

  # E2 — --exclude-glob '*.py' excludes the only match -> 0 hits
  exc=$($L --format json --exclude-glob '*.py' grep py "$d" 2>/dev/null | jq -r '.hits | length')
  [ "$exc" = "0" ] && echo "E2=PASS" || echo "E2=FAIL: hits=$exc"

  # E3 — composes with --globs include
  comp=$($L --format json --globs '*.py' --exclude-glob '*test*' grep py "$d" 2>/dev/null | jq -r '.hits | length')
  [ "$comp" = "1" ] && echo "E3=PASS" || echo "E3=FAIL: hits=$comp"

  # E4 — cumulative: multiple --exclude-glob args excluding .py and .js leaves a.rs only
  cum_files=$($L --format json --exclude-glob '*.py' --exclude-glob '*.js' grep . "$d" 2>/dev/null | jq -r '[.hits[].file_path] | unique | .[0]')
  case "$cum_files" in
    *a.rs) echo "E4=PASS" ;;
    *) echo "E4=FAIL: file=$cum_files" ;;
  esac

  rm -rf "$d"
  ```
- **Expected**: exit 0; stdout has `E1=PASS E2=PASS E3=PASS E4=PASS`.
- **Pass**: every check is PASS.
- **Links**: Phase 6A item #9 (`0c2b76c3`); QA battle-test enhancement #16.

### TC-094 — `evolution` partial-clone detection: `EVOLUTION_PARTIAL_CLONE` warning + graceful degrade

- **Language(s)**: n/a
- **Severity**: blocker
- **Preconditions**: writable tmp dir; `git`, `jq`, network access for fresh-clone test (skip M3+M8 if offline). Pinned partial-clone fixture at `$LMPROBE_QA_HOME/qa1/battle_test/repos/rust/tracing/` (`--filter=blob:none`).
- **Command**:
  ```bash
  L=$LMPROBE_REPO/target/release/lmprobe
  PARTIAL=$LMPROBE_QA_HOME/qa1/battle_test/repos/rust/tracing
  FULL=$LMPROBE_REPO

  # M1 — directory-walk on partial clone: warning + envelope (NOT raw libgit2 errno)
  out=$($L --format json evolution worker_thread "$PARTIAL" 2>/dev/null)
  has=$(echo "$out" | jq -r '[.warnings[].code] | index("EVOLUTION_PARTIAL_CLONE") != null')
  status=$(echo "$out" | jq -r '.exit_status')
  if [ "$has" = "true" ] && [ "$status" != "error" ]; then echo "M1=PASS"; else echo "M1=FAIL: status=$status warn=$has"; fi

  # M2 — single-file path on partial clone: same warning surfaces
  out=$($L --format json evolution Worker "$PARTIAL/tracing-appender/src/worker.rs" 2>/dev/null)
  has=$(echo "$out" | jq -r '[.warnings[].code] | index("EVOLUTION_PARTIAL_CLONE") != null')
  [ "$has" = "true" ] && echo "M2=PASS" || echo "M2=FAIL"

  # M4 — baseline FULL clone: warning does NOT fire (regression guard)
  out=$($L --format json evolution Pattern "$FULL/crates/core/src/matcher/pattern.rs" 2>/dev/null)
  has=$(echo "$out" | jq -r '[.warnings[].code] | index("EVOLUTION_PARTIAL_CLONE") != null')
  [ "$has" = "false" ] && echo "M4=PASS" || echo "M4=FAIL"

  # M5 — warning bypasses -v gating (surfaces on default mode)
  $L --format json evolution worker_thread "$PARTIAL" 2>/tmp/tc094_m5.stderr >/dev/null
  grep -q EVOLUTION_PARTIAL_CLONE /tmp/tc094_m5.stderr && echo "M5=PASS" || echo "M5=FAIL"

  # M6 — -q suppresses stderr; envelope warnings[] still ships
  out=$($L -q --format json evolution worker_thread "$PARTIAL" 2>/tmp/tc094_m6.stderr)
  s=$(wc -c </tmp/tc094_m6.stderr)
  has=$(echo "$out" | jq -r '[.warnings[].code] | index("EVOLUTION_PARTIAL_CLONE") != null')
  if [ "$s" = "0" ] && [ "$has" = "true" ]; then echo "M6=PASS"; else echo "M6=FAIL: stderr=$s warn=$has"; fi

  # M7 — GraphQL surface: warning in data.evolution.warnings[].code
  out=$($L --format json query '{ evolution(name: "worker_thread", path: "'"$PARTIAL"'") { warnings { code } exitStatus milestones { commitHash } } }' 2>/dev/null)
  has=$(echo "$out" | jq -r '[.data.evolution.warnings[].code] | index("EVOLUTION_PARTIAL_CLONE") != null')
  [ "$has" = "true" ] && echo "M7=PASS" || echo "M7=FAIL"

  # M8 — shallow clone (--depth 1) also detected (covers is_shallow() branch)
  TC8=/tmp/lmprobe-tc094-shallow-$$
  rm -rf "$TC8"
  if git clone --depth 1 --quiet https://github.com/clap-rs/clap.git "$TC8" 2>/dev/null; then
    out=$($L --format json evolution Arg "$TC8" 2>/dev/null)
    has=$(echo "$out" | jq -r '[.warnings[].code] | index("EVOLUTION_PARTIAL_CLONE") != null')
    [ "$has" = "true" ] && echo "M8=PASS" || echo "M8=FAIL"
    rm -rf "$TC8"
  else
    echo "M8=SKIP_NO_NETWORK"
  fi

  rm -f /tmp/tc094_*.stderr
  ```
- **Expected**: exit 0; stdout has `M1=PASS M2=PASS M4=PASS M5=PASS M6=PASS M7=PASS` and `M8=PASS` (or `M8=SKIP_NO_NETWORK` in offline environments). M3 (fresh-clone-up-front) is intentionally not in this TC because the underlying detection covers M1/M2/M8 paths instead — see "Out of scope" note below.
- **Pass**: every M check is PASS (M8 may SKIP if offline).
- **Out of scope (doctrine note)**: `evolution <nonexistent>` short-circuits with `Error: Symbol not found in current HEAD` on BOTH full and partial clones (same legacy code path). This is independent of partial-clone detection and was not part of the round-2 RCA's three targeted modes (cache-hit, blob:none blind spot, mid-walk crash). If/when the bare-error short-circuit gets migrated to the envelope-with-warnings pattern, this TC could grow an M3 case.
- **Links**: Phase 6A item #1 round-2 (`e026bf7b`); QA battle-test cross-cutting Issue #4; cycle-3 Pattern C.

### TC-095 — `history blame` walks real history with real line ranges

- **Language(s)**: go, python
- **Severity**: major
- **Preconditions**: pinned fixtures at `qa1/repos/go/cobra/` and `qa1/repos/python/markupsafe/`. **Requires** `jq`. Cobra's `command.go` has 268 commits; `Execute` symbol has substantial history. Markupsafe's `__init__.py` has 75 commits; `escape` symbol has substantial history. Cycle-3 battle-test surfaced "1-entry cap" on real-world files in 7+ langs.
- **Command**:
  ```bash
  L=$LMPROBE_REPO/target/release/lmprobe

  # M1 — cobra Execute on command.go: ≥30 entries, real line ranges (no zeros)
  out=$($L --no-cache --format json history blame Execute $LMPROBE_QA_CORPUS/go/cobra/command.go 2>/dev/null)
  entries=$(echo "$out" | jq -r '.entries | length')
  zeros=$(echo "$out" | jq -r '[.entries[] | select(.line_start == 0 or .line_end == 0)] | length')
  if [ "$entries" -ge 30 ] && [ "$zeros" = "0" ]; then echo "M1=PASS"; else echo "M1=FAIL: entries=$entries zeros=$zeros"; fi

  # M2 — markupsafe escape on __init__.py: ≥10 entries, real line ranges
  out=$($L --no-cache --format json history blame escape $LMPROBE_QA_CORPUS/python/markupsafe/src/markupsafe/__init__.py 2>/dev/null)
  entries=$(echo "$out" | jq -r '.entries | length')
  zeros=$(echo "$out" | jq -r '[.entries[] | select(.line_start == 0 or .line_end == 0)] | length')
  if [ "$entries" -ge 10 ] && [ "$zeros" = "0" ]; then echo "M2=PASS"; else echo "M2=FAIL: entries=$entries zeros=$zeros"; fi

  # M5 — every recoverable line range is ≥ 1 (legacy 0,0 placeholder is gone)
  c1=$($L --no-cache --format json history blame Execute $LMPROBE_QA_CORPUS/go/cobra/command.go 2>/dev/null | jq '[.entries[] | select(.line_start == 0 or .line_end == 0)] | length')
  c2=$($L --no-cache --format json history blame escape $LMPROBE_QA_CORPUS/python/markupsafe/src/markupsafe/__init__.py 2>/dev/null | jq '[.entries[] | select(.line_start == 0 or .line_end == 0)] | length')
  if [ "$c1" = "0" ] && [ "$c2" = "0" ]; then echo "M5=PASS"; else echo "M5=FAIL: cobra-zeros=$c1 markupsafe-zeros=$c2"; fi
  ```
- **Expected**: exit 0; stdout has `M1=PASS M2=PASS M5=PASS`.
- **Pass**: every check is PASS.
- **Links**: Phase 6B item #1 (`1137d418`); QA battle-test cross-cutting Issue #3 (1-entry cap); QA battle-test Pattern B.

### TC-096 — `context.recent_history` returns multiple entries; honors `--recent-history-limit`

- **Language(s)**: go
- **Severity**: major
- **Preconditions**: pinned fixture at `qa1/repos/go/cobra/` (Execute symbol on `command.go` has 37 reachable history entries). **Requires** `jq`. Cycle-3 battle-test surfaced "context recent_history truncated to 1 entry" on real-world files; round-1 fix lifted the cap to default 3; round-2 (`13fdd4eb`) added the dedicated `--recent-history-limit <N>` flag (mirrors GraphQL `recentHistory(limit:N)`).
- **Command**:
  ```bash
  L=$LMPROBE_REPO/target/release/lmprobe
  PATHF=$LMPROBE_QA_CORPUS/go/cobra/command.go

  # M3 — default recent_history depth > 1 (cycle-3 cap fixed; default stays 3)
  default_n=$($L --no-cache --format json context Execute "$PATHF" 2>/dev/null | jq -r '.recent_history | length')
  if [ "$default_n" -gt 1 ]; then echo "M3=PASS"; else echo "M3=FAIL: default_n=$default_n"; fi

  # M4 — --recent-history-limit 5 produces exactly 5 entries
  five_n=$($L --no-cache --format json context Execute "$PATHF" --recent-history-limit 5 2>/dev/null | jq -r '.recent_history | length')
  if [ "$five_n" = "5" ]; then echo "M4=PASS"; else echo "M4=FAIL: yields=$five_n (expected 5)"; fi

  # M4LO — --recent-history-limit 1 clamps DOWN to exactly 1 entry
  one_n=$($L --no-cache --format json context Execute "$PATHF" --recent-history-limit 1 2>/dev/null | jq -r '.recent_history | length')
  if [ "$one_n" = "1" ]; then echo "M4LO=PASS"; else echo "M4LO=FAIL: yields=$one_n (expected 1)"; fi

  # M4HI — --recent-history-limit 50 gracefully caps at all-available (37 on this fixture)
  many_n=$($L --no-cache --format json context Execute "$PATHF" --recent-history-limit 50 2>/dev/null | jq -r '.recent_history | length')
  if [ "$many_n" = "37" ]; then echo "M4HI=PASS"; else echo "M4HI=FAIL: yields=$many_n (expected 37 — all-available cap)"; fi

  # M4ZERO — --recent-history-limit 0 is rejected at parse time (clap "must be ≥ 1")
  $L --format json context Execute "$PATHF" --recent-history-limit 0 >/dev/null 2>/tmp/tc096_zero.stderr
  ec=$?
  if [ "$ec" -ne 0 ] && grep -qE 'must be . 1|must be ≥ 1|invalid value .0.' /tmp/tc096_zero.stderr; then echo "M4ZERO=PASS"; else echo "M4ZERO=FAIL: exit=$ec stderr=$(cat /tmp/tc096_zero.stderr | head -1)"; fi
  rm -f /tmp/tc096_zero.stderr

  # M3R — recent_history entries carry real (non-zero) line ranges
  zeros=$($L --no-cache --format json context Execute "$PATHF" 2>/dev/null | jq -r '[.recent_history[] | select(.line_start == 0 or .line_end == 0)] | length')
  if [ "$zeros" = "0" ]; then echo "M3R=PASS"; else echo "M3R=FAIL: zero-line-range-entries=$zeros"; fi

  # M4G — GraphQL parity: SymbolResult.recentHistory(limit: N) honors the same N
  g_five=$($L --format json query "{ def(name: \"Execute\", path: \"$(dirname $PATHF)\") { hits { recentHistory(limit: 5, path: \"$(dirname $PATHF)\") { entries { commitHash } } } } }" 2>/dev/null | jq -r '.data.def.hits[0].recentHistory.entries | length')
  if [ "$g_five" = "5" ]; then echo "M4G=PASS"; else echo "M4G=FAIL: GraphQL recentHistory(limit:5) yields=$g_five"; fi
  ```
- **Expected**: exit 0; stdout has `M3=PASS M4=PASS M4LO=PASS M4HI=PASS M4ZERO=PASS M3R=PASS M4G=PASS`.
- **Pass**: every check is PASS.
- **Doctrine note (Phase 6B round 2 doc shift)**: `--limit <N>` on `context` is intentionally a no-op on the composite envelope's sub-lists; sub-list flags (`--recent-history-limit`, future analogous) own their slices. `--limit` remains the primary-hits cap on non-composite verbs (search/grep/find/etc.). Documented in `--limit` `--help` longabout.
- **Links**: Phase 6B item #2 round-2 (`13fdd4eb`); QA battle-test enhancement #7; cycle-3 Pattern B.

### TC-097 — `BlameEntry.lineStart` / `lineEnd` are nullable Int (GraphQL typing) — BACK-COMPAT BREAK

- **Language(s)**: n/a (GraphQL schema)
- **Severity**: major
- **Preconditions**: workspace at HEAD; release build with GraphQL surface. **Requires** `jq`.
- **Command**:
  ```bash
  L=$LMPROBE_REPO/target/release/lmprobe

  # M7 — schema introspection: BlameEntry.lineStart/.lineEnd are SCALAR Int with ofType:null (nullable)
  out=$($L --format json query '{ __type(name: "BlameEntry") { fields { name type { name kind ofType { name kind } } } } }' 2>/dev/null)
  ls_kind=$(echo "$out" | jq -r '.data.__type.fields[] | select(.name == "lineStart") | .type.kind')
  ls_oftype=$(echo "$out" | jq -r '.data.__type.fields[] | select(.name == "lineStart") | .type.ofType')
  le_kind=$(echo "$out" | jq -r '.data.__type.fields[] | select(.name == "lineEnd") | .type.kind')
  if [ "$ls_kind" = "SCALAR" ] && [ "$ls_oftype" = "null" ] && [ "$le_kind" = "SCALAR" ]; then echo "M7=PASS"; else echo "M7=FAIL: lineStart kind=$ls_kind ofType=$ls_oftype lineEnd kind=$le_kind"; fi

  # M7B — commitHash is still NON_NULL String! (regression guard)
  cm_kind=$(echo "$out" | jq -r '.data.__type.fields[] | select(.name == "commitHash") | .type.kind')
  cm_inner=$(echo "$out" | jq -r '.data.__type.fields[] | select(.name == "commitHash") | .type.ofType.kind')
  if [ "$cm_kind" = "NON_NULL" ] && [ "$cm_inner" = "SCALAR" ]; then echo "M7B=PASS"; else echo "M7B=FAIL: commitHash kind=$cm_kind inner=$cm_inner"; fi

  # M7E — end-to-end blame query with lineStart selection executes; returns Int values
  out=$($L --format json query '{ history { blame(target: "Execute", path: "$LMPROBE_QA_CORPUS/go/cobra/command.go") { entries { commitHash lineStart lineEnd } } } }' 2>/dev/null)
  n=$(echo "$out" | jq -r '.data.history.blame.entries | length')
  if [ "$n" -gt 0 ]; then echo "M7E=PASS"; else echo "M7E=FAIL: entries=$n"; fi
  ```
- **Expected**: exit 0; stdout has `M7=PASS M7B=PASS M7E=PASS`.
- **Pass**: every check is PASS.
- **Back-compat note**: prior to Phase 6B, `BlameEntry.lineStart` and `lineEnd` were `Int!` (NON_NULL). After Phase 6B, both are nullable `Int` to honestly represent the unrecoverable case (cross-rename hop, blob mismatch) instead of the schema-violating `(0, 0)` placeholder per chapter 17 §`LineNumber ≥ 1`. **Clients selecting `lineStart`/`lineEnd` under non-null assumptions must handle `null`** (display fallback, skip during aggregation). Same posture as Phase 6A item #8 (`-l` deprecation). See release notes for the introducing release.
- **Links**: Phase 6B item #3 (`1137d418`); chapter 17 §`LineNumber ≥ 1`; QA battle-test cross-cutting Issue #3 (line-range 0,0).

### TC-098 — `blob_signatures` cache version-bump invariants (sensor_version 1→2)

- **Language(s)**: n/a (cache layer)
- **Severity**: major (operational; affects upgrade UX)
- **Preconditions**: writable tmp dir; pinned fixture at `qa1/repos/go/cobra/`. **Requires** `jq`. Optional: `sqlite3` CLI (else uses `python3 -c 'import sqlite3'`).
- **Command**:
  ```bash
  L=$LMPROBE_REPO/target/release/lmprobe
  TC=/tmp/lmprobe-tc098-$$
  rm -rf "$TC"; mkdir -p "$TC"

  # Cold run: builds fresh DB at sensor_version=2 with line_start/line_end columns
  XDG_CACHE_HOME=$TC $L --format json history blame Execute $LMPROBE_QA_CORPUS/go/cobra/command.go >/dev/null 2>/dev/null
  DB="$TC/lmprobe/history.db"
  [ -f "$DB" ] && echo "M9DB=PASS" || echo "M9DB=FAIL: no DB at $DB"

  # M9COL — schema has line_start, line_end columns (nullable INTEGER)
  if command -v sqlite3 >/dev/null; then
    cols=$(sqlite3 "$DB" "PRAGMA table_info(blob_signatures)" | awk -F'|' '$2 ~ /^line_(start|end)$/ {print $2"="$3}' | sort | tr '\n' ' ')
  else
    cols=$(python3 -c "
import sqlite3
c = sqlite3.connect('$DB')
print(' '.join(sorted(f'{r[1]}={r[2]}' for r in c.execute('PRAGMA table_info(blob_signatures)').fetchall() if r[1] in ('line_start','line_end'))))
")
  fi
  case "$cols" in
    *"line_end=INTEGER"*"line_start=INTEGER"*) echo "M9COL=PASS" ;;
    *"line_start=INTEGER"*"line_end=INTEGER"*) echo "M9COL=PASS" ;;
    *) echo "M9COL=FAIL: cols=[$cols]" ;;
  esac

  # M9SV — fresh rows are at sensor_version=2
  if command -v sqlite3 >/dev/null; then
    sv=$(sqlite3 "$DB" "SELECT MIN(sensor_version), MAX(sensor_version), COUNT(*) FROM blob_signatures" 2>/dev/null | tr '|' ' ')
  else
    sv=$(python3 -c "
import sqlite3
c = sqlite3.connect('$DB')
mn, mx, n = c.execute('SELECT MIN(sensor_version), MAX(sensor_version), COUNT(*) FROM blob_signatures').fetchone()
print(mn, mx, n)
")
  fi
  set -- $sv
  if [ "$1" = "2" ] && [ "$2" = "2" ] && [ "$3" -gt 0 ]; then echo "M9SV=PASS"; else echo "M9SV=FAIL: min=$1 max=$2 count=$3"; fi

  # M9W — second invocation reuses cache (warm < 500ms)
  start=$(date +%s%N)
  XDG_CACHE_HOME=$TC $L --format json history blame Execute $LMPROBE_QA_CORPUS/go/cobra/command.go >/dev/null 2>/dev/null
  end=$(date +%s%N)
  warm_ms=$(( (end - start) / 1000000 ))
  if [ "$warm_ms" -lt 500 ]; then echo "M9W=PASS"; else echo "M9W=FAIL: warm_ms=$warm_ms"; fi

  rm -rf "$TC"
  ```
- **Expected**: exit 0; stdout has `M9DB=PASS M9COL=PASS M9SV=PASS M9W=PASS`.
- **Pass**: every check is PASS.
- **Operational note**: `SENSOR_VERSION: 1 → 2` upgrade does NOT require manual cache deletion. Old v1 rows are filtered by sensor_version WHERE-clause; ALTER TABLE adds the new nullable columns to existing DBs. First run after upgrade pays a one-time cold-rewalk cost; subsequent runs are warm-cached.
- **Links**: Phase 6B item #3 (`1137d418`); cache invariants TC-064.

### TC-099 — Phase 6C cross-cutting `imports` invariants (no-panic, NOT_IMPLEMENTED gone, GraphQL parity)

- **Language(s)**: all 11 newly-wired (c, cpp, csharp, php, rust, solidity, nix, hcl, scala, swift, elixir, go)
- **Severity**: blocker
- **Preconditions**: pinned cycle-1 fixtures at `qa1/repos/<lang>/<repo>/`. **Requires** `jq`.
- **Command**:
  ```bash
  L=$LMPROBE_REPO/target/release/lmprobe

  # M4 — cross-lang walk doesn't panic (regression guard for cycle-1 §2.1)
  $L --no-cache --format json imports import $LMPROBE_QA_CORPUS 2>/tmp/tc099_m4.stderr >/dev/null
  ec=$?; panics=$(grep -c "panicked" /tmp/tc099_m4.stderr)
  if [ "$ec" -ne 101 ] && [ "$panics" = "0" ]; then echo "M4=PASS"; else echo "M4=FAIL: exit=$ec panics=$panics"; fi

  # M3 — NOT_IMPLEMENTED is GONE for all 11 newly-wired langs (single-file scoped)
  fails=0
  for pair in "c:qa1/repos/c/sds/sds.c:stdio.h" \
              "cpp:qa1/repos/cpp/stb/stb_image.h:stdarg.h" \
              "csharp:qa1/repos/csharp/commandline/src/CommandLine/Properties/Resources.Designer.cs:System" \
              "php:qa1/repos/php/PHP-Parser/lib/PhpParser/Lexer.php:PhpParser" \
              "rust:qa1/repos/rust/fd/src/main.rs:std" \
              "solidity:qa1/repos/solidity/solmate/src/tokens/ERC20.sol:./Owned.sol" \
              "nix:qa1/repos/nix/nixos-generators/configuration.nix:lib" \
              "scala:qa1/repos/scala/scopt/jvm/src/main/scala/scopt/OParser.scala:scopt" \
              "elixir:qa1/repos/elixir/plug/lib/plug.ex:Plug" \
              "go:qa1/repos/go/cobra/command.go:fmt"; do
    lang=${pair%%:*}; rest=${pair#*:}; file=$LMPROBE_QA_HOME/${rest%:*}; query=${rest##*:}
    [ -e "$file" ] || continue
    ni=$($L --no-cache --format json imports "$query" "$file" 2>/dev/null | jq -r "[.warnings[]? | select(.code == \"NOT_IMPLEMENTED\" and ((.lang // \"\") | ascii_downcase | startswith(\"$lang\")))] | length")
    [ "$ni" = "0" ] || { echo "M3=FAIL: lang=$lang NI_count=$ni"; fails=$((fails+1)); }
  done
  [ "$fails" = "0" ] && echo "M3=PASS"

  # M5 — GraphQL parity for Rust/C#/Go (one canonical query per lang, identical result)
  for triple in "rust:qa1/repos/rust/fd:std" "csharp:qa1/repos/csharp/commandline:System" "go:qa1/repos/go/cobra:fmt"; do
    lang=${triple%%:*}; rest=${triple#*:}; root=$LMPROBE_QA_HOME/${rest%:*}; query=${rest##*:}
    cli=$($L --no-cache --format json imports "$query" "$root" 2>/dev/null | jq -r '.hits | length')
    gq=$($L --format json query "{ imports(name: \"$query\", path: \"$root\") { hits { name } } }" 2>/dev/null | jq -r '.data.imports.hits | length')
    [ "$cli" = "$gq" ] && [ "$cli" != "0" ] || { echo "M5=FAIL: lang=$lang cli=$cli gq=$gq"; fails=$((fails+1)); }
  done
  [ "$fails" = "0" ] && echo "M5=PASS"

  rm -f /tmp/tc099_m4.stderr
  ```
- **Expected**: exit 0; stdout has `M4=PASS M3=PASS M5=PASS`.
- **Pass**: every check is PASS.
- **Links**: Phase 6C cross-cutting (`c7abcd4e`); QA battle-test top product issue #2 (imports per-language grammars unwired); cycle-1 §2.1 (no-panic regression).

### TC-100 — Atomic-lang `imports` (C, Cpp, Nix, Swift): canonical full-form match

- **Language(s)**: c, cpp, nix, swift
- **Severity**: major
- **Preconditions**: pinned fixtures. **Requires** `jq`. Doctrine: atomic langs use exact-or-contains semantics; bare-root-crate prefix-match does NOT apply.
- **Command**:
  ```bash
  L=$LMPROBE_REPO/target/release/lmprobe

  # C — #include "stdio.h" canonical
  c=$($L --no-cache --format json imports "stdio.h" $LMPROBE_QA_CORPUS/c/sds 2>/dev/null | jq '.hits | length')
  [ "$c" -ge 1 ] && echo "C=PASS" || echo "C=FAIL: hits=$c"

  # Cpp — both <stdarg.h> and <math.h> patterns
  c1=$($L --no-cache --format json imports "stdarg.h" $LMPROBE_QA_CORPUS/cpp/stb 2>/dev/null | jq '.hits | length')
  c2=$($L --no-cache --format json imports "math.h" $LMPROBE_QA_CORPUS/cpp/stb 2>/dev/null | jq '.hits | length')
  [ "$c1" -ge 1 ] && [ "$c2" -ge 1 ] && echo "CPP=PASS" || echo "CPP=FAIL: stdarg=$c1 math=$c2"

  # Nix — `lib` (matches `with lib;` / `inherit (lib) ...` / lib refs)
  n=$($L --no-cache --format json imports lib $LMPROBE_QA_CORPUS/nix/nixos-generators 2>/dev/null | jq '.hits | length')
  [ "$n" -ge 1 ] && echo "NIX=PASS" || echo "NIX=FAIL: hits=$n"

  # Swift — Foundation (the cycle-3 battle-test repro that returned 0; now should be 70+)
  s=$($L --no-cache --format json imports Foundation $LMPROBE_QA_CORPUS/swift/Alamofire 2>/dev/null | jq '.hits | length')
  [ "$s" -ge 50 ] && echo "SWIFT=PASS" || echo "SWIFT=FAIL: hits=$s (expected ≥50; cycle-3 had 0)"

  # No-duplicate-emission regression guard (Swift was 2× per battle-test §11)
  uniq=$($L --no-cache --format json imports CoreServices $LMPROBE_QA_CORPUS/swift/Alamofire 2>/dev/null | jq '[.hits[]|{f:.file_path,l:.line_start}] | unique | length')
  total=$($L --no-cache --format json imports CoreServices $LMPROBE_QA_CORPUS/swift/Alamofire 2>/dev/null | jq '.hits | length')
  [ "$uniq" = "$total" ] && echo "SWIFT_DEDUPE=PASS" || echo "SWIFT_DEDUPE=FAIL: total=$total unique=$uniq"
  ```
- **Expected**: exit 0; stdout has `C=PASS CPP=PASS NIX=PASS SWIFT=PASS SWIFT_DEDUPE=PASS`.
- **Pass**: every check is PASS.
- **Links**: Phase 6C atomic langs (`43f0e340`/`d2496bd4`/`77624641`/`e909b5be`); cycle-3 battle-test §C/Cpp/Nix/Swift `imports` gaps.

### TC-101 — Hierarchical-prefix-match doctrine (Rust, C#, Scala, Solidity)

- **Language(s)**: rust, csharp, scala, solidity
- **Severity**: blocker (doctrine — Designer Lead's clarification: bare-root query must find all sub-path uses)
- **Preconditions**: pinned fixtures. **Requires** `jq`.
- **Command**:
  ```bash
  L=$LMPROBE_REPO/target/release/lmprobe

  # Rust — std must be a prefix-match root: `imports std` finds all `use std::...`
  bare=$($L --no-cache --format json imports std $LMPROBE_QA_CORPUS/rust/fd 2>/dev/null | jq '.hits | length')
  one=$($L --no-cache --format json imports "std::io" $LMPROBE_QA_CORPUS/rust/fd 2>/dev/null | jq '.hits | length')
  full=$($L --no-cache --format json imports "std::io::Write" $LMPROBE_QA_CORPUS/rust/fd 2>/dev/null | jq '.hits | length')
  if [ "$bare" -ge "$one" ] && [ "$one" -ge "$full" ] && [ "$full" -ge 1 ]; then echo "RUST_PFX=PASS"; else echo "RUST_PFX=FAIL: bare=$bare one-seg=$one full=$full (expected bare ≥ one ≥ full ≥ 1)"; fi
  # Rust battle-test reprods: imports clap (root-crate) and crossbeam_channel (underscored)
  c=$($L --no-cache --format json imports clap $LMPROBE_QA_CORPUS/rust/fd 2>/dev/null | jq '.hits | length')
  cc=$($L --no-cache --format json imports crossbeam_channel $LMPROBE_QA_HOME/qa1/battle_test/repos/rust/tracing 2>/dev/null | jq '.hits | length')
  [ "$c" -ge 1 ] && [ "$cc" -ge 1 ] && echo "RUST_BT=PASS" || echo "RUST_BT=FAIL: clap=$c crossbeam_channel=$cc"

  # C# — System must prefix-match: `imports System` ⊇ `imports System.Linq`
  bare=$($L --no-cache --format json imports System $LMPROBE_QA_CORPUS/csharp/commandline 2>/dev/null | jq '.hits | length')
  sub=$($L --no-cache --format json imports "System.Linq" $LMPROBE_QA_CORPUS/csharp/commandline 2>/dev/null | jq '.hits | length')
  if [ "$bare" -ge "$sub" ] && [ "$bare" -ge 1 ]; then echo "CS_PFX=PASS"; else echo "CS_PFX=FAIL: bare=$bare sub=$sub"; fi

  # Scala — scopt prefix
  s=$($L --no-cache --format json imports scopt $LMPROBE_QA_CORPUS/scala/scopt 2>/dev/null | jq '.hits | length')
  [ "$s" -ge 1 ] && echo "SCALA=PASS" || echo "SCALA=FAIL: hits=$s"

  # Solidity — path-form imports work
  so=$($L --no-cache --format json imports ERC20 $LMPROBE_QA_CORPUS/solidity/solmate 2>/dev/null | jq '.hits | length')
  [ "$so" -ge 1 ] && echo "SOL=PASS" || echo "SOL=FAIL: hits=$so"
  ```
- **Expected**: exit 0; stdout has `RUST_PFX=PASS RUST_BT=PASS CS_PFX=PASS SCALA=PASS SOL=PASS`.
- **Pass**: every check is PASS.
- **Links**: Phase 6C hierarchical langs (`35a91997`/`919a38b9`/`738d3424`/`e48fcb42`); Designer Lead Doctrine #2 (prefix-match clarification); battle-test top issue #2.

### TC-102 — HCL `imports` doctrine (BLOCK-NAME ANCHOR + DEDUPE + LINE_START)

- **Language(s)**: hcl
- **Severity**: blocker
- **Preconditions**: writable tmp dir. **Requires** `jq`. Designer Lead Doctrine #1: in HCL, `module = anchor`; `imports <module-name>` matches the block; `imports <source-path>` matches the same block via its source value; 1 hit per statement; `line_start` populated (block start line).
- **Command**:
  ```bash
  L=$LMPROBE_REPO/target/release/lmprobe
  TC=/tmp/lmprobe-tc102-$$
  rm -rf "$TC"; mkdir -p "$TC/modules/vpc"
  cat > "$TC/main.tf" <<'EOF'
  module "vpc" {
    source = "./modules/vpc"
    name   = "demo"
  }
  EOF

  # M1 — module name is the anchor: `imports vpc` returns 1 hit (the block)
  hits_anchor=$($L --no-cache --format json imports vpc "$TC" 2>/dev/null | jq '.hits | length')
  if [ "$hits_anchor" = "1" ]; then echo "M1=PASS"; else echo "M1=FAIL: imports-vpc-yields=$hits_anchor (expected 1)"; fi

  # M2 — source path also matches the same block: `imports './modules/vpc'` returns 1 hit
  hits_path=$($L --no-cache --format json imports './modules/vpc' "$TC" 2>/dev/null | jq '.hits | length')
  if [ "$hits_path" = "1" ]; then echo "M2=PASS"; else echo "M2=FAIL: imports-source-path-yields=$hits_path (expected 1; round-1 over-emitted at 2; round-2 partial at c54d319e under-emits at 0)"; fi

  # M3 — line_start populated (block start line; non-null, ≥ 1)
  null_or_zero=$($L --no-cache --format json imports vpc "$TC" 2>/dev/null | jq '[.hits[] | select(.line_start == null or .line_start == 0)] | length')
  if [ "$null_or_zero" = "0" ]; then echo "M3=PASS"; else echo "M3=FAIL: hits-with-null-or-zero-line_start=$null_or_zero"; fi

  rm -rf "$TC"
  ```
- **Expected**: exit 0; stdout has `M1=PASS M2=PASS M3=PASS`.
- **Pass**: every check is PASS.
- **Future-additive (not in 6C scope)**: `line_end` populated on `SymbolHit` requires cross-cutting schema expansion (`SymbolHit.line_end` field). Per Designer Lead, this is deferred to a separate phase. TC-102 does NOT gate on `line_end`; if/when the schema lands, the marker can be added back.
- **Round status**:
  - Round-1 (`c7abcd4e`): all three FAIL — `imports vpc` → 0 (anchor broken); `imports './modules/vpc'` → 2 (dedupe over-emit); `line_start` was null on hits.
  - Round-2 partial (`c54d319e`): M1 PASS (`imports vpc` → 1), M3 PASS (`line_start = 1`); **M2 FAIL** (under-emit: source-path query now returns 0 hits — the round-2 dedupe collapsed both query-form aliases into anchor-only).
- **Links**: Phase 6C HCL (`14d3d9d1`, `c54d319e`); Designer Lead Doctrine #1; QA battle-test cross-cutting Issue #2.

### TC-103 — PHP `imports` hierarchical-prefix-match + false-positive guard

- **Language(s)**: php
- **Severity**: blocker
- **Preconditions**: pinned fixture at `qa1/repos/php/PHP-Parser/`. **Requires** `jq`. Doctrine: `imports PhpParser` (bare-root) finds all `use PhpParser\…\X;` statements (≥ `imports PhpParser\Node` ≥ `imports PhpParser\Node\Expr` ≥ 1). Plus a controlled-fixture false-positive guard.
- **Command**:
  ```bash
  L=$LMPROBE_REPO/target/release/lmprobe
  ROOT=$LMPROBE_QA_CORPUS/php/PHP-Parser

  # M1 — prefix-match envelope on real fixture
  bare=$($L --no-cache --format json imports PhpParser "$ROOT" 2>/dev/null | jq '.hits | length')
  sub=$($L --no-cache --format json imports 'PhpParser\Node' "$ROOT" 2>/dev/null | jq '.hits | length')
  full=$($L --no-cache --format json imports 'PhpParser\Node\Expr' "$ROOT" 2>/dev/null | jq '.hits | length')

  if [ "$bare" -ge "$sub" ] && [ "$sub" -ge "$full" ] && [ "$full" -ge 1 ]; then
    echo "M1=PASS"
  else
    echo "M1=FAIL: bare=$bare sub=$sub full=$full (expected bare ≥ sub ≥ full ≥ 1)"
  fi

  # M2 — controlled fixture: 4 use-form variants ALL match `imports Foo`
  TC=/tmp/lmprobe-tc103-$$
  rm -rf "$TC"; mkdir -p "$TC"
  cat > "$TC/test.php" <<'PHP'
  <?php
  use Foo;
  use Foo\Bar;
  use Foo\Bar\Baz;
  use Foo\Bar as Quux;
  use Other;
  PHP
  m2=$($L --no-cache --format json imports Foo "$TC" 2>/dev/null | jq '.hits | length')
  if [ "$m2" = "4" ]; then echo "M2=PASS"; else echo "M2=FAIL: imports-Foo-yields=$m2 (expected 4)"; fi

  # M3 — false-positive guard: `imports Other` returns exactly 1 (not 5)
  m3=$($L --no-cache --format json imports Other "$TC" 2>/dev/null | jq '.hits | length')
  if [ "$m3" = "1" ]; then echo "M3=PASS"; else echo "M3=FAIL: imports-Other-yields=$m3 (expected 1)"; fi

  rm -rf "$TC"
  ```
- **Expected**: exit 0; stdout has `M1=PASS M2=PASS M3=PASS`.
- **Pass**: every check is PASS.
- **Round status**:
  - Round-1 (`c7abcd4e`): M1 FAIL — `imports PhpParser` yields 13 < `imports PhpParser\Node` yields 98 (longest-prefix-only).
  - Round-2 partial (`c54d319e`): all PASS — bare=751, sub=557, full=148 (envelope holds); 4 use-form variants all match; false-positive guard tight.
- **Links**: Phase 6C PHP (`f424a1ae`, `c54d319e`); Designer Lead Doctrine #2; battle-test top issue #2.

### TC-104 — Elixir `imports` hierarchical-prefix-match (round-1 FAIL)

- **Language(s)**: elixir
- **Severity**: blocker
- **Preconditions**: pinned fixture at `qa1/repos/elixir/plug/`. **Requires** `jq`.
- **Command**:
  ```bash
  L=$LMPROBE_REPO/target/release/lmprobe
  ROOT=$LMPROBE_QA_CORPUS/elixir/plug

  bare=$($L --no-cache --format json imports Plug "$ROOT" 2>/dev/null | jq '.hits | length')
  sub=$($L --no-cache --format json imports "Plug.Conn" "$ROOT" 2>/dev/null | jq '.hits | length')

  if [ "$bare" -ge "$sub" ] && [ "$bare" -ge 1 ]; then echo "M1=PASS"; else echo "M1=FAIL: bare=$bare sub=$sub (expected bare ≥ sub ≥ 1)"; fi
  ```
- **Expected**: exit 0; stdout has `M1=PASS`.
- **Pass**: M1 = PASS.
- **Round-1 status (`c7abcd4e`)**: FAIL — `imports Plug` yields 0; `imports Plug.Conn` yields 39. Bare-root completely missing for Elixir hierarchical names. Round-2 should land prefix-match symmetric to Rust/C#.
- **Links**: Phase 6C Elixir (`958f6745`); Designer Lead Doctrine #2; battle-test top issue #2.

### TC-105 — Go `imports` prefix-match (round-1 FAIL)

- **Language(s)**: go
- **Severity**: blocker
- **Preconditions**: pinned fixture at `qa1/repos/go/cobra/`. **Requires** `jq`. Doctrine: Go import paths are slash-separated; `imports github.com` (domain prefix), `imports github.com/spf13` (org prefix), `imports github.com/spf13/pflag` (full) should all match the same `import "github.com/spf13/pflag"` statements.
- **Command**:
  ```bash
  L=$LMPROBE_REPO/target/release/lmprobe
  ROOT=$LMPROBE_QA_CORPUS/go/cobra

  # Standard library — bare crate name canonical (was 28 in round-1)
  fmt=$($L --no-cache --format json imports fmt "$ROOT" 2>/dev/null | jq '.hits | length')
  [ "$fmt" -ge 10 ] && echo "M1_STD=PASS" || echo "M1_STD=FAIL: fmt-hits=$fmt"

  # Domain-prefix: `imports github.com` should cover everything under it
  dom=$($L --no-cache --format json imports github.com "$ROOT" 2>/dev/null | jq '.hits | length')
  # Org-prefix: `imports github.com/spf13` should cover spf13 packages
  org=$($L --no-cache --format json imports "github.com/spf13" "$ROOT" 2>/dev/null | jq '.hits | length')
  # Full path: `imports github.com/spf13/pflag` (round-1: 6 hits)
  full=$($L --no-cache --format json imports "github.com/spf13/pflag" "$ROOT" 2>/dev/null | jq '.hits | length')
  # Bare basename: `imports pflag` (battle-test repro; round-1: 0)
  bare=$($L --no-cache --format json imports pflag "$ROOT" 2>/dev/null | jq '.hits | length')

  # Doctrine: dom ≥ org ≥ full ≥ 1; bare ≥ full
  if [ "$dom" -ge "$org" ] && [ "$org" -ge "$full" ] && [ "$full" -ge 1 ]; then echo "M2_PFX=PASS"; else echo "M2_PFX=FAIL: dom=$dom org=$org full=$full"; fi
  if [ "$bare" -ge "$full" ]; then echo "M3_BARE=PASS"; else echo "M3_BARE=FAIL: bare=$bare full=$full (expected bare ≥ full)"; fi

  # Cycle-1 §2.2 regression guard — quoted input does NOT panic
  $L --no-cache --format json imports '"fmt"' "$ROOT" 2>&1 | grep -q "panicked" && echo "M4_QUOTED=FAIL: panic" || echo "M4_QUOTED=PASS"
  ```
- **Expected**: exit 0; stdout has `M1_STD=PASS M2_PFX=PASS M3_BARE=PASS M4_QUOTED=PASS`.
- **Pass**: every check is PASS.
- **Round-1 status (`c7abcd4e`)**: M1_STD PASS; M2_PFX FAIL (`github.com`/`github.com/spf13` both yield 0; only EXACT path works); M3_BARE FAIL (`pflag` yields 0; bare basename unsupported); M4_QUOTED PASS (cycle-1 §2.2 fix preserved).
- **Links**: Phase 6C Go (`c7abcd4e`); Designer Lead Doctrine #2; battle-test top issue #2; cycle-1 §2.2 panic regression guard.

---

### TC-106 — `context PATH:LINE` resolves to a declaration anchor

- **Language(s)**: typescript
- **Severity**: major
- **Preconditions**: release build; writable tmp dir; `git`. **Requires** `jq`.
- **Command**:
  ```bash
  L=$LMPROBE_REPO/target/release/lmprobe
  d=$(mktemp -d)
  cd "$d" && git init -q && git config user.email test@example.com && git config user.name test
  cat > "$d/app.ts" <<'EOF'
  export function outer() {
    const value = inner();
    return value;
  }
  function inner() { return 1; }
  EOF
  cd "$d" && git add app.ts && git commit -qm init
  out=$($L --format json context "$d/app.ts:2" 2>/dev/null)
  jq -r '"status=\(.exit_status) symbol=\(.symbol) def=\(.definition.name) resolved=\(([.warnings[].code] | index("CONTEXT_LOCATION_RESOLVED")) != null)"' <<<"$out"
  rm -rf "$d"
  ```
- **Expected**: exit 0; stdout exactly `status=matched symbol=outer def=outer resolved=true`.
- **Pass**: location syntax resolves before the bundle is built, and `CONTEXT_LOCATION_RESOLVED` is present.
- **Links**: battle-test context cursor-location ergonomics.

### TC-107 — `discover --top-level-only` returns a compact monorepo map

- **Language(s)**: n/a
- **Severity**: major
- **Preconditions**: release build; writable tmp dir. **Requires** `jq`.
- **Command**:
  ```bash
  L=$LMPROBE_REPO/target/release/lmprobe
  d=$(mktemp -d)
  mkdir -p "$d/apps/web" "$d/crates/core"
  printf '[package]\nname = "root"\nversion = "0.1.0"\n' > "$d/Cargo.toml"
  printf '{"name":"web"}\n' > "$d/apps/web/package.json"
  printf '[package]\nname = "core"\nversion = "0.1.0"\n' > "$d/crates/core/Cargo.toml"
  full=$($L --format json discover "$d" 2>/dev/null | jq '.projects | length')
  compact=$($L --format json discover --top-level-only "$d" 2>/dev/null | jq '.projects | length')
  children=$($L --format json discover --top-level-only "$d" 2>/dev/null | jq '[.projects[].children[]?] | length')
  echo "full=$full compact=$compact children=$children"
  rm -rf "$d"
  ```
- **Expected**: exit 0; stdout exactly `full=3 compact=1 children=0`.
- **Pass**: nested projects are suppressed and returned top-level projects have empty `children[]`.
- **Links**: battle-test monorepo discovery ergonomics.

### TC-108 — `def --body` returns implementation bodies

- **Language(s)**: typescript
- **Severity**: major
- **Preconditions**: release build; writable tmp dir. **Requires** `jq`.
- **Command**:
  ```bash
  L=$LMPROBE_REPO/target/release/lmprobe
  d=$(mktemp -d)
  cat > "$d/app.ts" <<'EOF'
  export function outer() {
    const value = inner();
    return value;
  }
  function inner() { return 1; }
  EOF
  skel=$($L --format json def outer "$d" 2>/dev/null | jq -r '.hits[0].content')
  body=$($L --format json def --body outer "$d" 2>/dev/null | jq -r '.hits[0].content')
  case "$skel|$body" in
    *"{ ... }"*"const value = inner()"*) echo "BODY=PASS" ;;
    *) echo "BODY=FAIL skel=$skel body=$body" ;;
  esac
  rm -rf "$d"
  ```
- **Expected**: exit 0; stdout exactly `BODY=PASS`.
- **Pass**: default `def` remains skeletonized, while `--body` includes implementation content.
- **Links**: battle-test full-definition retrieval ergonomics.

---

## GraphQL

`lmprobe query` is the one-shot GraphQL CLI (Phase 4E). Reference: [17_graphql.md](17_graphql.md). The cases in this category cover the six rich-feature pillars individually plus mutation safety on the GraphQL surface.

### Composition

### TC-G01 — `query` runs a basic search (envelope shape)

- **Language(s)**: rust
- **Severity**: blocker
- **Preconditions**: workspace at HEAD; release build.
- **Command**:
  ```bash
  target/release/lmprobe --format json query '{ search(query: "Pattern", path: "crates/core/src/matcher") { hits { name filePath } } }' \
    | jq '.data.search.hits | length'
  ```
- **Expected**: exit 0; stdout integer ≥ 1.
- **Pass**: stdout is a positive integer.
- **Links**: 4E

### TC-G02 — Composition: `def → callers → enclosing` in one round-trip

- **Language(s)**: rust
- **Severity**: blocker
- **Preconditions**: in-tree composition fixture at `crates/lmprobe/tests/fixtures/composition/`.
- **Command**:
  ```bash
  cd crates/lmprobe && $LMPROBE_REPO/target/release/lmprobe --format json query '{
    def(name: "compute_total", path: "tests/fixtures/composition") {
      hits { name filePath callers(limit: 5, path: "tests/fixtures/composition") {
        hits { name filePath enclosing { name kind } }
      } }
    }
  }' | jq '.data.def.hits[0].callers.hits | length'
  ```
- **Expected**: exit 0; stdout `2` (the fixture has two callers); each caller's `enclosing.kind == "function_item"`.
- **Pass**: stdout is `2`.
- **Links**: 4E (composition P0)

### Field selection

### TC-G02a — Field selection skips unrequested fields (token-cost reduction)

- **Language(s)**: rust
- **Severity**: minor
- **Preconditions**: workspace.
- **Command**:
  ```bash
  target/release/lmprobe --format json query '{ search(query: "Pattern", path: "crates/core/src/matcher") { hits { name } } }' \
    | jq '.data.search.hits[0] | keys'
  ```
- **Expected**: exit 0; stdout JSON array of length 1 containing only `["name"]` — `filePath`, `lineStart`, `content`, etc. are absent because they weren't selected.
- **Pass**: keys array has only `name`.

### Variables

### TC-G03 — `query` with `--var` (JSON-parsed values)

- **Language(s)**: lua
- **Severity**: major
- **Preconditions**: in-tree fixture at `crates/lmprobe/tests/fixtures/lua/`. **Requires** `jq`.
- **Command**:
  ```bash
  target/release/lmprobe --format json query 'query D($p: String!) { discover(path: $p) { projects { name } } }' \
    --var p='"crates/lmprobe/tests/fixtures/lua"' \
    | jq -r '.data.discover.projects[0].name'
  ```
- **Expected**: exit 0; stdout exactly `lua`.
- **Pass**: stdout is `lua`.

### TC-G11 — `query -` stdin + `--vars-file` invocation

- **Language(s)**: rust
- **Severity**: major
- **Preconditions**: workspace at HEAD; release build. **Requires** `jq`. Verifies the multi-line stdin path with vars-from-file matches the inline `--var` envelope shape.
- **Command**:
  ```bash
  cd $LMPROBE_REPO
  cat > /tmp/lmprobe-tc-g11-vars.json <<'EOF'
  { "name": "Pattern", "path": "crates/core/src/matcher" }
  EOF
  cat > /tmp/lmprobe-tc-g11-query.gql <<'EOF'
  query LookupSym($name: String!, $path: String!) {
    search(query: $name, path: $path) {
      hits { name filePath }
    }
  }
  EOF
  target/release/lmprobe --format json query - --vars-file /tmp/lmprobe-tc-g11-vars.json \
    < /tmp/lmprobe-tc-g11-query.gql | jq '.data.search.hits | length'
  rm -f /tmp/lmprobe-tc-g11-vars.json /tmp/lmprobe-tc-g11-query.gql
  ```
- **Expected**: exit 0; stdout integer ≥ 1 (Pattern is defined; envelope shape matches inline `--var` invocation in TC-G03).
- **Pass**: stdout is a positive integer.
- **Links**: 4E (variables P0)

### Introspection

### TC-G04 — `__schema` self-discovery

- **Language(s)**: n/a
- **Severity**: major
- **Preconditions**: workspace.
- **Command**:
  ```bash
  target/release/lmprobe --format json query '{ __schema { queryType { fields { name } } } }' \
    | jq -r '.data.__schema.queryType.fields[].name' | sort | head -3
  ```
- **Expected**: exit 0; 3 alphabetic top-level resolver names (e.g. `astDiff`, `callees`, `callers`).
- **Pass**: 3 lines, each a valid resolver name.

### TC-G18 — `query` and `serve` schema parity

- **Language(s)**: n/a
- **Severity**: major
- **Preconditions**: workspace; cargo toolchain; `--features serve` build available; `curl` + `jq` available.
- **Command**:
  ```bash
  cd $LMPROBE_REPO
  cargo build -p lmprobe --release --features serve 2>&1 | tail -1
  cli_fields=$(target/release/lmprobe --format json query '{ __schema { queryType { fields { name } } } }' | \
    jq -r '.data.__schema.queryType.fields[].name' | sort)
  target/release/lmprobe serve --addr 127.0.0.1:18118 &
  PID=$!
  sleep 0.5
  serve_fields=$(curl -s -X POST http://127.0.0.1:18118/graphql -H 'Content-Type: application/json' \
    -d '{"query":"{ __schema { queryType { fields { name } } } }"}' | \
    jq -r '.data.__schema.queryType.fields[].name' | sort)
  kill -TERM "$PID" 2>/dev/null; wait "$PID" 2>/dev/null
  if [ "$cli_fields" = "$serve_fields" ]; then echo "PASS: schema parity"; else echo "FAIL"; diff <(echo "$cli_fields") <(echo "$serve_fields"); fi
  ```
- **Expected**: exit 0; stdout `PASS: schema parity`.
- **Pass**: identical top-level field set across `query` and `serve` schemas.
- **Links**: 4E

### Typed scalars

### TC-G04a — Typed scalar: `Language` enum coerces to `SgLang`

- **Language(s)**: rust
- **Severity**: major
- **Preconditions**: workspace.
- **Command**:
  ```bash
  target/release/lmprobe --format json query '{ listKinds(lang: RUST) { rows { portable kinds } } }' \
    | jq '.data.listKinds.rows | length'
  ```
- **Expected**: exit 0; stdout integer (≥ 1; matches `list-kinds rust` CLI count).
- **Pass**: stdout is a positive integer.
- **Links**: 4E (typed scalars P1)

### Aliases (parallel queries)

### TC-G04b — Aliases: multiple resolvers in one round-trip

- **Language(s)**: rust + lua
- **Severity**: minor
- **Preconditions**: workspace.
- **Command**:
  ```bash
  target/release/lmprobe --format json query '{
    rust: search(query: "Pattern", path: "crates/core/src/matcher") { hits { name } }
    lua_d: discover(path: "crates/lmprobe/tests/fixtures/lua") { projects { name } }
  }' | jq '{rust_hits: (.data.rust.hits | length), lua_root: .data.lua_d.projects[0].name}'
  ```
- **Expected**: exit 0; stdout has both `rust_hits > 0` and `lua_root == "lua"`.
- **Pass**: both assertions.

### Exit-code mapping

### TC-G05 — `query` exit code on no_match

- **Language(s)**: rust
- **Severity**: blocker
- **Preconditions**: workspace.
- **Command**:
  ```bash
  target/release/lmprobe query '{ search(query: "ZzZSymbolDoesNotExist", path: ".") { hits { filePath } } }' >/dev/null 2>/dev/null ; echo $?
  target/release/lmprobe --exit-zero-on-no-match query '{ search(query: "ZzZSymbolDoesNotExist", path: ".") { hits { filePath } } }' >/dev/null 2>/dev/null ; echo $?
  ```
- **Expected**: stdout `1\n0`.
- **Pass**: first invocation 1; second 0.
- **Links**: 4E (arbitration I)

### TC-G06 — `query` exit code on errors[]-non-empty

- **Language(s)**: n/a
- **Severity**: blocker
- **Preconditions**: workspace.
- **Command**:
  ```bash
  target/release/lmprobe query '{ nope }' >/dev/null 2>/dev/null ; echo $?
  ```
- **Expected**: stdout `2`.
- **Pass**: stdout is `2`.
- **Links**: 4E (arbitration I)

### Mutation safety (D7 on GraphQL surface)

### TC-G07 — `fix` mutation dry-run does not write

- **Language(s)**: rust
- **Severity**: blocker
- **Preconditions**: writable tmp dir; throwaway git repo. **Requires** `jq`.
- **Command**:
  ```bash
  tmp=$(mktemp -d) ; cd "$tmp"
  cat > main.rs <<'EOF'
  fn main() { println!("hello"); println!("world"); }
  EOF
  git init -q && git add main.rs && git -c user.email=t@t -c user.name=t commit -qm init
  target/release/lmprobe --format json query 'mutation {
    fix(scope: { pattern: { lang: RUST }, paths: ["."] },
        pattern: "println!($A)", replacement: "eprintln!($A)", dryRun: true)
      { mode matchesCount } }' | jq -r '.data.fix.mode'
  grep -c 'eprintln!' main.rs
  ```
- **Expected**: exit 0; stdout `dry_run\n0`.
- **Pass**: `mode == "dry_run"`; eprintln! count is 0.
- **Links**: D7

### TC-G08 — `fix` mutation refuses dirty tree

- **Language(s)**: rust
- **Severity**: blocker
- **Preconditions**: writable tmp dir; throwaway git repo with one untracked file. **Requires** `jq`.
- **Command**:
  ```bash
  tmp=$(mktemp -d) ; cd "$tmp"
  echo 'fn main() { println!("a"); }' > main.rs
  git init -q && git add main.rs && git -c user.email=t@t -c user.name=t commit -qm init
  echo "scratch" > untracked.txt
  target/release/lmprobe --format json query 'mutation {
    fix(scope: { pattern: { lang: RUST }, paths: ["."] },
        pattern: "println!($A)", replacement: "eprintln!($A)", dryRun: false)
      { mode } }' | jq -r '.errors[0].code'
  target/release/lmprobe query 'mutation { fix(scope: { pattern: { lang: RUST }, paths: ["."] }, pattern: "println!($A)", replacement: "eprintln!($A)", dryRun: false) { mode } }' >/dev/null 2>/dev/null ; echo $?
  ```
- **Expected**: exit 0 then 2; stdout `DIRTY_TREE\n2`.
- **Pass**: `errors[0].code == "DIRTY_TREE"`; process exit 2.
- **Links**: D7

### TC-G09 — `trace` transient mode auto-rolls back (D7 byte-identity)

- **Language(s)**: rust
- **Severity**: blocker
- **Preconditions**: writable tmp dir; throwaway git repo; `sha256sum` available.
- **Command**:
  ```bash
  tmp=$(mktemp -d) ; cd "$tmp"
  cat > main.rs <<'EOF'
  fn main() { println!("hi"); }
  EOF
  git init -q && git add main.rs && git -c user.email=t@t -c user.name=t commit -qm init
  sha_pre=$(sha256sum main.rs | awk '{print $1}')
  target/release/lmprobe query 'mutation {
    trace(scope: { pattern: { p: "println!($A)", lang: RUST }, paths: ["."] },
          template: { default: "tracing::info!(\"[TRACE-x] hit\");" },
          marker: "x", worktree: true, exec: "true")
      { schema } }' >/dev/null
  sha_post=$(sha256sum main.rs | awk '{print $1}')
  [ "$sha_pre" = "$sha_post" ] && echo "byte-identical" || echo "DRIFT"
  ```
- **Expected**: exit 0; stdout `byte-identical`.
- **Pass**: stdout exactly `byte-identical`.
- **Links**: D7

### TC-G10 — `trace` persistent + `untrace` round-trip

- **Language(s)**: rust
- **Severity**: blocker
- **Preconditions**: writable tmp dir; throwaway git repo; `sha256sum` and `jq` available.
- **Command**:
  ```bash
  tmp=$(mktemp -d) ; cd "$tmp"
  cat > main.rs <<'EOF'
  fn main() {
      println!("hello");
      println!("world");
  }
  EOF
  git init -q && git add main.rs && git -c user.email=t@t -c user.name=t commit -qm init
  sha_pre=$(sha256sum main.rs | awk '{print $1}')
  target/release/lmprobe query 'mutation {
    trace(scope: { pattern: { p: "println!($A)", lang: RUST }, paths: ["."] },
          template: { default: "// [TRACE-rt-1]" },
          marker: "rt-1", worktree: false)
      { events { output } } }' >/dev/null
  git add . && git -c user.email=t@t -c user.name=t commit -qm "post-trace" -q
  target/release/lmprobe query 'mutation { untrace(marker: "rt-1") { mode revertedCount } }' \
    | jq -r '.data.untrace.mode + " " + (.data.untrace.revertedCount | tostring)'
  sha_post=$(sha256sum main.rs | awk '{print $1}')
  [ "$sha_pre" = "$sha_post" ] && echo "byte-identical" || echo "DRIFT"
  ```
- **Expected**: exit 0; last 2 lines `audit_log 2\nbyte-identical`.
- **Pass**: untrace mode is `audit_log`; revertedCount is 2; sha256 byte-identical.
- **Links**: 4E (D7)

### Surface equality

### TC-G16 — CLI/GraphQL surface-equality (TC mirror of `tests/graphql_surface_equality.rs`)

- **Language(s)**: rust
- **Severity**: major
- **Preconditions**: workspace at HEAD. Mirrors the engine-side `tests/graphql_surface_equality.rs` Layer-1 read-parity check at the QA-agent surface.
- **Command**:
  ```bash
  cd $LMPROBE_REPO
  cli=$(target/release/lmprobe --format json def Pattern crates/core/src/matcher | \
    jq '[.hits[] | {name, file_path, line_start}] | sort_by(.file_path, .line_start)')
  gql=$(target/release/lmprobe --format json query '{ def(name: "Pattern", path: "crates/core/src/matcher") { hits { name filePath lineStart } } }' | \
    jq '[.data.def.hits[] | {name, file_path: .filePath, line_start: .lineStart}] | sort_by(.file_path, .line_start)')
  if [ "$cli" = "$gql" ]; then echo "PASS: surface-equal"; else echo "FAIL"; diff <(echo "$cli") <(echo "$gql"); fi
  ```
- **Expected**: exit 0; stdout `PASS: surface-equal`.
- **Pass**: CLI and GraphQL data byte-equal modulo wrapping (after canonical sort).
- **Links**: 4E (engine-side surface-equality test)

### Diagnostics

### TC-G17 — NOT_IMPLEMENTED maps to `warnings[]`, not `errors[]`

- **Language(s)**: bash
- **Severity**: blocker
- **Preconditions**: writable tmp dir with a bash file. Bash has no `CallGrammar`; `callers` against a bash-only path should produce `warnings[].code = NOT_IMPLEMENTED` in the envelope, NOT a top-level `errors[]` entry. Distinction matters because `errors[]` triggers process exit 2 while NOT_IMPLEMENTED is exit 0 with a diagnostic per Phase 0 §0.7 routing. Path-based language detection — the `callers` resolver does not accept a `lang:` arg, so we route by directory contents (only-bash files in tmp dir). **Requires** `jq`.
- **Command**:
  ```bash
  d=/tmp/lmprobe-tc-g17-$$
  mkdir -p "$d"; cd "$d"
  printf '#!/bin/bash\nfunction helper() { echo hi; }\nhelper\n' > script.sh
  $LMPROBE_REPO/target/release/lmprobe --format json query "{ callers(name: \"helper\", path: \"$d\") { hits { name } warnings { code } exitStatus } }" | \
    jq '{has_errors: ((.errors // []) | length > 0), warning_codes: [.data.callers.warnings[].code], exit_status: .data.callers.exitStatus}'
  $LMPROBE_REPO/target/release/lmprobe query "{ callers(name: \"helper\", path: \"$d\") { hits { name } } }" >/dev/null 2>/dev/null ; echo "process_exit=$?"
  cd /; rm -rf "$d"
  ```
- **Expected**: stdout JSON has `has_errors: false`; `warning_codes` includes `NOT_IMPLEMENTED`; process exit 0.
- **Pass**: NOT_IMPLEMENTED in warnings; process exit 0 (NOT 2).
- **Links**: 4E (arbitration I); 0.7; QA Cycle 2 round 1 designer arbitration (path-based detection vs `lang:` arg that doesn't exist on the `callers` resolver).

---

## Workspace / Architectural

Phase 5 split the codebase into 4 foundation crates + 29 per-language crates + 1 binary + xtask = 35 workspace members. These cases verify the workspace shape, per-language isolation, and architectural invariants from the Phase 5 design.

### TC-W01 — Workspace member count is 38

- **Language(s)**: n/a
- **Severity**: major
- **Preconditions**: workspace; cargo toolchain. **Requires** `jq`. The 38 = 3 ast-grep upstream forks (`crates/{config,core,language}`) + 4 lmprobe foundation (`lmprobe-{types,lang-core,registry,engine}`) + 1 lmprobe binary (`lmprobe`) + 29 lmprobe-lang-* + 1 xtask.
- **Command**:
  ```bash
  cargo metadata --format-version 1 --no-deps | jq '.workspace_members | length'
  ```
- **Expected**: exit 0; stdout `38`.
- **Pass**: stdout is `38`.
- **Links**: 5A, 5B (W1).

### TC-W02 — Per-language test suites all green

- **Language(s)**: all
- **Severity**: major
- **Preconditions**: workspace; cargo toolchain. Uses cargo's exit code as the per-crate gate (cargo test returns 0 only if all tests pass; trailing-line grep approach was unreliable due to cargo's stderr-positioned summary).
- **Command**:
  ```bash
  cd $LMPROBE_REPO && \
    failed=0
    for L in $(ls -d crates/lmprobe-lang-* | grep -v lang-core | sed 's|.*/||; s|lmprobe-lang-||'); do
      cargo test -p "lmprobe-lang-$L" --release >/dev/null 2>&1 || { echo "FAIL: $L"; failed=$((failed+1)); }
    done
    echo "failed_count=$failed"
  ```
- **Expected**: exit 0; stdout ends with `failed_count=0`.
- **Pass**: zero failed crates across all 29 langs.
- **Links**: 5B.

### TC-W03 — Cargo dep isolation: no `async-graphql` in lang crates

- **Language(s)**: all
- **Severity**: blocker
- **Preconditions**: workspace; cargo toolchain. Verifies W4 (Cargo-level isolation) — no per-language crate transitively depends on `async-graphql` because that would defeat the lightweight per-lang doctrine.
- **Command**:
  ```bash
  cd $LMPROBE_REPO && \
    leaks=0
    for L in $(ls -d crates/lmprobe-lang-* | grep -v lang-core | sed 's|.*/||'); do
      n=$(cargo tree -p "$L" 2>/dev/null | grep -cE 'async-graphql|lmprobe-types' || true)
      [ "$n" != "0" ] && { echo "LEAK: $L has $n hits"; leaks=$((leaks+1)); }
    done
    echo "leak_count=$leaks"
  ```
- **Expected**: exit 0; stdout ends with `leak_count=0`.
- **Pass**: zero per-lang crates leak `async-graphql` or `lmprobe-types`.
- **Links**: 5C (W4)

### TC-W04 — `cargo xtask check-lang` clean across 29 crates

- **Language(s)**: all
- **Severity**: major
- **Preconditions**: workspace; xtask built.
- **Command**:
  ```bash
  cd $LMPROBE_REPO && cargo xtask check-lang 2>&1 | tail -1
  ```
- **Expected**: exit 0; stdout contains `clean (29 crate(s) verified)` (or equivalent count).
- **Pass**: clean message present.
- **Links**: 5A

### TC-W05 — `cargo xtask new-lang` round-trip (idempotency check)

- **Language(s)**: n/a
- **Severity**: minor
- **Preconditions**: workspace; xtask built; clean working tree (no uncommitted changes). The case scaffolds a real SgLang variant (one already extracted, e.g. `python`), then immediately reverts, verifying:
  1. The scaffold succeeds even though the dir already exists with the production crate's contents.
  2. The revert restores the workspace cleanly.
  
  Choose a lang with NO open per-lang work. Currently `python` is fine.
- **Command**:
  ```bash
  cd $LMPROBE_REPO && \
    git stash --include-untracked >/dev/null 2>&1
    # NOTE: production xtask refuses to overwrite an existing crate; this case
    # validates that behavior. Expect exit non-zero with a clear error.
    out=$(cargo xtask new-lang python 2>&1)
    echo "$out" | grep -qE 'exists|already' && echo "PASS: refused to overwrite" || echo "FAIL: did not refuse"
    git stash pop --quiet 2>/dev/null || true
  ```
- **Expected**: stdout `PASS: refused to overwrite`.
- **Pass**: xtask correctly refuses to overwrite an existing crate.
- **Links**: 5A

### TC-W06 — Lang-core has no concrete-lang dispatch arms (post-Phase-5C cleanup)

- **Language(s)**: n/a
- **Severity**: major
- **Preconditions**: workspace at HEAD post-Phase-5C.
- **Command**:
  ```bash
  cd $LMPROBE_REPO && \
    grep -E 'SgLang::' crates/lmprobe-lang-core/src/langs.rs | wc -l
  ```
- **Expected**: exit 0; stdout `1` (the only remaining hit is the categorical `lang_tier` data-tier `|`-group; no per-lang trait dispatch arms remain).
- **Pass**: stdout is `1`.
- **Links**: 5B Step-6, 5C

### TC-W07 — CLI routes through `lmprobe-registry` (Phase 5C bypass closed)

- **Language(s)**: n/a
- **Severity**: blocker
- **Preconditions**: workspace at HEAD post-Phase-5C.
- **Command**:
  ```bash
  cd $LMPROBE_REPO && \
    grep -nE 'lmprobe_lang_core::langs::|crate::langs::' crates/lmprobe/src/ -r | grep -vE 'PortableKind|SgLang' | wc -l
  ```
- **Expected**: exit 0; stdout `0` (all CLI dispatch goes through `lmprobe-registry`; no direct lang-core call paths remain except the `PortableKind`/`SgLang` type re-exports).
- **Pass**: stdout is `0`.
- **Links**: 5C

### TC-W08 — `serve --features serve` startup-then-shutdown smoke

- **Language(s)**: n/a
- **Severity**: major
- **Preconditions**: workspace; cargo toolchain; `curl`. Builds with `--features serve` and verifies the daemon binds, accepts one introspection query, and exits cleanly when killed.
- **Command**:
  ```bash
  cd $LMPROBE_REPO && \
    cargo build -p lmprobe --release --features serve 2>&1 | tail -1
    target/release/lmprobe serve --addr 127.0.0.1:18101 &
    PID=$!
    sleep 0.5
    code=$(curl -s -o /dev/null -w '%{http_code}' \
      -X POST http://127.0.0.1:18101/graphql \
      -H 'Content-Type: application/json' \
      -d '{"query": "{ __schema { queryType { name } } }"}')
    kill -TERM "$PID" 2>/dev/null
    wait "$PID" 2>/dev/null
    echo "http_code=$code"
  ```
- **Expected**: exit 0; stdout has `http_code=200`.
- **Pass**: curl got 200 from the introspection query.
- **Links**: 4E

### TC-W09 — D5 default-impl `NOT_IMPLEMENTED` invariant

- **Language(s)**: bash
- **Severity**: blocker
- **Preconditions**: workspace; `bash` does not override `InheritanceGrammar` (or another D5 trait) — the lang crate inherits the default-impl from `lmprobe-lang-core` which returns empty/None. Sensor must surface `NOT_IMPLEMENTED` (not silent-empty) when the default fires. Generalizes: any `(lang, trait)` pair where the lang doesn't override default behavior. **Requires** `jq`.
- **Command**:
  ```bash
  d=/tmp/lmprobe-tc-w09-$$
  mkdir -p "$d"; cd "$d"
  cat > script.sh <<'EOF'
  #!/bin/bash
  function helper() { echo hi; }
  function caller() { helper; }
  EOF
  $LMPROBE_REPO/target/release/lmprobe --format json supertypes helper . -l bash | \
    jq '{exit_status, hit_count: (.hits | length), codes: [.warnings[].code]}'
  cd /; rm -rf "$d"
  ```
- **Expected**: exit 0; stdout JSON shows `NOT_IMPLEMENTED` in `codes` (default-impl returned empty → sensor surfaces NOT_IMPLEMENTED diagnostic, NOT silent-empty hits).
- **Pass**: `NOT_IMPLEMENTED` present in warning codes.
- **Links**: 5A (W6 default-impl pattern); 0.7 (silent-empty → diagnostic transition).

---

## End

Total: 86 cases (Smoke 4 + Sensor 16 + History 6 + Mutation 7 + Format 3 + Language 8 + Cache 5 + Multi-repo 2 + Failure-mode 9 + GraphQL 17 + Workspace/Architectural 9).

New cases must populate all schema fields (ID, Language(s), Severity, Preconditions, Command, Expected, Pass, optional Links). Cases must be deterministic against a known fixture state — every fixture path is either in `qa1/repos/<lang>/<repo>/` (pinned), `crates/lmprobe/tests/fixtures/<lang>/` (in-tree), or a throwaway `mktemp -d` + `git init`. **No `git clone` of remote repos** is permitted in any case (QA cycle 1 hit GnuTLS / index-pack failures in WSL; the suite avoids that class entirely).

External tools beyond `bash`/`git`/`cargo` are listed in [§ Prerequisites](#prerequisites) or noted inline as "**Requires** `<tool>`".
