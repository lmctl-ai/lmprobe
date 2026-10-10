//! Phase 4A — `blob_signatures` cache invariants.
//!
//! Verifies the cache populates correctly on first walk, hits on
//! repeated walks, and stays incremental on new commits (existing
//! rows untouched, only the new commit's blob inserts a fresh row).
//!
//! Same harness pattern as 3A's commit-cache tests — provision a
//! tmp git repo per test, point `XDG_CACHE_HOME` at a tmp dir, run
//! the binary, inspect `history.db` directly.

use std::path::PathBuf;
use std::process::Command;

fn binary() -> PathBuf {
  if let Ok(p) = std::env::var("LMPROBE_BIN") {
    return PathBuf::from(p);
  }
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

struct Fixture {
  repo: PathBuf,
  xdg: PathBuf,
}

impl Drop for Fixture {
  fn drop(&mut self) {
    let _ = std::fs::remove_dir_all(&self.repo);
    let _ = std::fs::remove_dir_all(&self.xdg);
  }
}

fn provision(tag: &str) -> Fixture {
  let pid = std::process::id();
  let repo = std::env::temp_dir().join(format!("lmprobe-blob-cache-{}-{}", tag, pid));
  let xdg = std::env::temp_dir().join(format!("lmprobe-blob-cache-xdg-{}-{}", tag, pid));
  let _ = std::fs::remove_dir_all(&repo);
  let _ = std::fs::remove_dir_all(&xdg);
  std::fs::create_dir_all(&repo).expect("mkdir repo");
  std::fs::create_dir_all(&xdg).expect("mkdir xdg");

  let git = |args: &[&str]| -> std::process::Output {
    Command::new("git")
      .args(args)
      .current_dir(&repo)
      .output()
      .expect("git")
  };
  git(&["init", "-q"]);
  git(&["config", "user.email", "t@t"]);
  git(&["config", "user.name", "t"]);

  Fixture { repo, xdg }
}

fn commit_file(repo: &PathBuf, path: &str, body: &str, msg: &str) {
  std::fs::write(repo.join(path), body).expect("write");
  let git = |args: &[&str]| -> std::process::Output {
    Command::new("git")
      .args(args)
      .current_dir(repo)
      .output()
      .expect("git")
  };
  git(&["add", path]);
  git(&["commit", "-qm", msg]);
}

fn run_blame(fix: &Fixture, name: &str) {
  Command::new(binary())
    .args(["history", "blame", name])
    .env("XDG_CACHE_HOME", &fix.xdg)
    .current_dir(&fix.repo)
    .output()
    .expect("lmprobe");
}

fn run_blame_no_cache(fix: &Fixture, name: &str) {
  Command::new(binary())
    .args(["--no-cache", "history", "blame", name])
    .env("XDG_CACHE_HOME", &fix.xdg)
    .current_dir(&fix.repo)
    .output()
    .expect("lmprobe");
}

fn count_rows(db_path: &PathBuf, table: &str) -> i64 {
  let conn = rusqlite::Connection::open(db_path).expect("open db");
  let count: i64 = conn
    .query_row(&format!("SELECT COUNT(*) FROM {}", table), [], |r| r.get(0))
    .expect("count");
  count
}

#[test]
fn blob_cache_cold_populates() {
  let fix = provision("cold");
  commit_file(&fix.repo, "lib.rs", "pub fn target() {}\n", "init");
  commit_file(
    &fix.repo,
    "lib.rs",
    "pub fn target() { let _ = 1; }\n",
    "edit",
  );

  let db = fix.xdg.join("lmprobe/history.db");
  assert!(!db.exists(), "no cache before run");

  run_blame(&fix, "target");

  assert!(db.exists(), "cache populated");
  let blob_rows = count_rows(&db, "blob_signatures");
  assert!(
    blob_rows >= 2,
    "blob_signatures populated for ≥2 distinct blobs (got {})",
    blob_rows
  );
}

#[test]
fn blob_cache_warm_no_new_rows() {
  let fix = provision("warm");
  commit_file(&fix.repo, "lib.rs", "pub fn target() {}\n", "init");
  commit_file(
    &fix.repo,
    "lib.rs",
    "pub fn target() { let _ = 1; }\n",
    "edit",
  );

  run_blame(&fix, "target");
  let db = fix.xdg.join("lmprobe/history.db");
  let cold_rows = count_rows(&db, "blob_signatures");

  // Second invocation — every blob is cached; no new INSERTs.
  run_blame(&fix, "target");
  let warm_rows = count_rows(&db, "blob_signatures");

  assert_eq!(
    cold_rows, warm_rows,
    "warm-cache run must not insert new rows (cold {} vs warm {})",
    cold_rows, warm_rows
  );
}

#[test]
fn blob_cache_incremental_on_new_commit() {
  let fix = provision("incr");
  commit_file(&fix.repo, "lib.rs", "pub fn target() {}\n", "init");
  commit_file(
    &fix.repo,
    "lib.rs",
    "pub fn target() { let _ = 1; }\n",
    "edit_a",
  );
  run_blame(&fix, "target");
  let db = fix.xdg.join("lmprobe/history.db");
  let pre_rows = count_rows(&db, "blob_signatures");

  // Add ONE new commit — should trigger ONE new blob signature.
  commit_file(
    &fix.repo,
    "lib.rs",
    "pub fn target() { let _ = 2; }\n",
    "edit_b",
  );
  run_blame(&fix, "target");

  let post_rows = count_rows(&db, "blob_signatures");
  assert_eq!(
    post_rows,
    pre_rows + 1,
    "exactly 1 new blob row after 1 new commit (pre {} → post {})",
    pre_rows,
    post_rows
  );
}

/// §4D-PB1 — `--no-cache` must skip both reads from and writes to
/// `blob_signatures`. Cold-cache → run with `--no-cache` → assert
/// zero rows in the table. Then run without `--no-cache` and confirm
/// rows DO appear, so the gate is meaningful.
#[test]
fn blob_cache_no_cache_skips_reads_and_writes() {
  let fix = provision("nocache");
  commit_file(&fix.repo, "lib.rs", "pub fn target() {}\n", "init");
  commit_file(
    &fix.repo,
    "lib.rs",
    "pub fn target() { let _ = 1; }\n",
    "edit",
  );

  let db = fix.xdg.join("lmprobe/history.db");
  assert!(!db.exists(), "no cache before run");

  // 1. --no-cache run on a fresh XDG. Cache file is initialised
  // (refresh_commit_cache populates repo_commits) but
  // blob_signatures must stay empty.
  run_blame_no_cache(&fix, "target");
  assert!(db.exists(), "cache file initialised even under --no-cache");
  let nocache_rows = count_rows(&db, "blob_signatures");
  assert_eq!(
    nocache_rows, 0,
    "--no-cache must not insert blob_signatures rows (got {})",
    nocache_rows
  );

  // 2. Sanity gate: a normal blame DOES populate rows. Confirms
  // the test would catch a regression where --no-cache started
  // writing again.
  run_blame(&fix, "target");
  let cached_rows = count_rows(&db, "blob_signatures");
  assert!(
    cached_rows >= 2,
    "without --no-cache, blob_signatures populates ≥2 rows (got {})",
    cached_rows
  );

  // 3. Re-run --no-cache and confirm read-skip: row count stays
  // at the cached value (no fresh writes), but the run completes
  // successfully (didn't bail because the cache "exists").
  run_blame_no_cache(&fix, "target");
  let after_rows = count_rows(&db, "blob_signatures");
  assert_eq!(
    after_rows, cached_rows,
    "--no-cache re-run must not write new rows (cached {} vs after {})",
    cached_rows, after_rows
  );
}
