use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
  if let Ok(p) = std::env::var("LMPROBE_BIN") {
    return PathBuf::from(p);
  }
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

struct Fixture {
  root: PathBuf,
}

impl Drop for Fixture {
  fn drop(&mut self) {
    let _ = std::fs::remove_dir_all(&self.root);
  }
}

fn fixture(name: &str) -> Fixture {
  let suffix = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| {
      // Unique within the process: parallel tests can read the same clock value.
      static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
      d.as_nanos() * 1024 + N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as u128
    })
    .unwrap_or(0);
  let root = std::env::temp_dir().join(format!(
    "lmprobe-no-match-scan-stats-{name}-{}-{suffix}",
    std::process::id()
  ));
  let _ = std::fs::remove_dir_all(&root);
  std::fs::create_dir_all(&root).expect("mkdir fixture");
  Fixture { root }
}

fn run(root: &Path, args: &[&str]) -> (serde_json::Value, String, i32) {
  let out = Command::new(binary())
    .args(args)
    .current_dir(root)
    .output()
    .expect("spawn lmprobe");
  let stdout = String::from_utf8_lossy(&out.stdout);
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    String::from_utf8_lossy(&out.stderr).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

fn assert_scan_stats(envelope: &serde_json::Value, considered: u64, scanned: u64, skipped: u64) {
  assert_eq!(
    envelope
      .pointer("/scan_stats/files_considered")
      .and_then(|v| v.as_u64()),
    Some(considered),
    "unexpected files_considered:\n{envelope:#}"
  );
  assert_eq!(
    envelope
      .pointer("/scan_stats/files_scanned")
      .and_then(|v| v.as_u64()),
    Some(scanned),
    "unexpected files_scanned:\n{envelope:#}"
  );
  assert_eq!(
    envelope
      .pointer("/scan_stats/files_skipped")
      .and_then(|v| v.as_u64()),
    Some(skipped),
    "unexpected files_skipped:\n{envelope:#}"
  );
}

#[test]
fn grep_no_match_reports_scan_stats() {
  let fix = fixture("grep");
  std::fs::write(fix.root.join("a.txt"), "alpha\n").expect("write a.txt");
  std::fs::write(fix.root.join("b.txt"), "beta\n").expect("write b.txt");

  let (envelope, stderr, exit) = run(&fix.root, &["--format", "json", "grep", "needle", "."]);

  assert_eq!(
    exit, 1,
    "grep should be no_match:\n{envelope:#}\nstderr:\n{stderr}"
  );
  assert_eq!(
    envelope.get("exit_status").and_then(|v| v.as_str()),
    Some("no_match")
  );
  assert_scan_stats(&envelope, 2, 2, 0);
}

// Dogfood finding (2026-09): a caller who checks `scan_stats` on a
// `no_match` grep response and sees `files_skipped: 0` reasonably reads
// that as "nothing was excluded" -- a false reassurance when
// `.gitignore` silently hid the file that would have matched.
// `files_ignored` is the honest, separate counter for that population;
// this fixture reproduces the real-world case (a gitignored file that
// contains the pattern) end to end.
#[test]
fn grep_no_match_reports_files_ignored_when_gitignore_hid_the_match() {
  let fix = fixture("grep-gitignore");
  std::process::Command::new("git")
    .args(["init", "-q", "."])
    .current_dir(&fix.root)
    .status()
    .expect("git init");
  std::fs::write(fix.root.join(".gitignore"), "*.secret\n").expect("write .gitignore");
  std::fs::write(fix.root.join("keep.txt"), "alpha\n").expect("write keep.txt");
  std::fs::write(fix.root.join("hidden.secret"), "needle\n").expect("write hidden.secret");

  let (envelope, stderr, exit) = run(&fix.root, &["--format", "json", "grep", "needle", "."]);

  assert_eq!(
    exit, 1,
    "grep should be no_match (the only match is gitignored):\n{envelope:#}\nstderr:\n{stderr}"
  );
  assert_eq!(
    envelope.get("exit_status").and_then(|v| v.as_str()),
    Some("no_match")
  );
  assert_scan_stats(&envelope, 1, 1, 0);
  assert_eq!(
    envelope
      .pointer("/scan_stats/files_ignored")
      .and_then(|v| v.as_u64()),
    Some(1),
    "files_ignored should count the gitignored file that would have matched:\n{envelope:#}"
  );
  let codes: Vec<&str> = envelope["warnings"]
    .as_array()
    .expect("warnings array")
    .iter()
    .filter_map(|w| w["code"].as_str())
    .collect();
  assert!(
    codes.contains(&"GITIGNORE_PATHS_EXCLUDED"),
    "expected a GITIGNORE_PATHS_EXCLUDED warning:\n{envelope:#}"
  );
}

#[test]
fn search_no_match_reports_scan_stats() {
  let fix = fixture("search");
  std::fs::write(fix.root.join("lib.rs"), "pub fn alpha() {}\n").expect("write lib.rs");
  std::fs::write(fix.root.join("other.rs"), "pub fn beta() {}\n").expect("write other.rs");
  std::fs::write(fix.root.join("notes.txt"), "definitely_missing_symbol\n").expect("write notes");

  let (envelope, stderr, exit) = run(
    &fix.root,
    &[
      "--format",
      "json",
      "search",
      "--name",
      "definitely_missing_symbol",
      "--path",
      ".",
    ],
  );

  assert_eq!(
    exit, 1,
    "search should be no_match:\n{envelope:#}\nstderr:\n{stderr}"
  );
  assert_eq!(
    envelope.get("exit_status").and_then(|v| v.as_str()),
    Some("no_match")
  );
  assert_scan_stats(&envelope, 3, 2, 1);
}
