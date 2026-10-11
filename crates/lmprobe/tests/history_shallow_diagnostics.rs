use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

struct Fixture {
  root: PathBuf,
}

impl Drop for Fixture {
  fn drop(&mut self) {
    let _ = fs::remove_dir_all(&self.root);
  }
}

fn unique_dir(prefix: &str) -> PathBuf {
  let suffix = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| {
      // Unique within the process: parallel tests can read the same clock value.
      static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
      d.as_nanos() * 1024 + N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as u128
    })
    .unwrap_or(0);
  std::env::temp_dir().join(format!("lmprobe-{prefix}-{}-{suffix}", std::process::id()))
}

fn git(repo: &Path, args: &[&str]) {
  let out = Command::new("git")
    .args(args)
    .current_dir(repo)
    .output()
    .expect("spawn git");
  assert!(
    out.status.success(),
    "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
    args,
    String::from_utf8_lossy(&out.stdout),
    String::from_utf8_lossy(&out.stderr)
  );
}

fn commit(repo: &Path, message: &str) {
  git(repo, &["add", "."]);
  git(repo, &["commit", "-qm", message]);
}

fn shallow_fixture() -> Fixture {
  let root = unique_dir("history-shallow");
  let source = root.join("source");
  let shallow = root.join("shallow");
  fs::create_dir_all(&source).expect("mkdir source");
  git(&source, &["init", "-q"]);
  git(&source, &["config", "user.email", "t@t"]);
  git(&source, &["config", "user.name", "t"]);

  fs::write(source.join("target.txt"), "target 1\n").expect("write target");
  fs::write(source.join("peer.txt"), "peer 1\n").expect("write peer");
  commit(&source, "initial");

  fs::write(source.join("target.txt"), "target 1\ntarget 2\n").expect("write target");
  fs::write(source.join("peer.txt"), "peer 1\npeer 2\n").expect("write peer");
  commit(&source, "target and peer");

  let source_url = format!("file://{}", source.display());
  let out = Command::new("git")
    .args([
      "clone",
      "--depth",
      "1",
      "-q",
      &source_url,
      shallow.to_str().unwrap(),
    ])
    .output()
    .expect("spawn git clone");
  assert!(
    out.status.success(),
    "git clone failed\nstdout:\n{}\nstderr:\n{}",
    String::from_utf8_lossy(&out.stdout),
    String::from_utf8_lossy(&out.stderr)
  );
  Fixture { root }
}

fn run_json(fix: &Fixture, repo: &Path, args: &[&str]) -> (serde_json::Value, String, i32) {
  let xdg = fix.root.join("xdg");
  fs::create_dir_all(&xdg).expect("mkdir xdg");
  let out = Command::new(binary())
    .args(args)
    .env("XDG_CACHE_HOME", &xdg)
    .current_dir(repo)
    .output()
    .expect("spawn lmprobe");
  let stdout = String::from_utf8_lossy(&out.stdout);
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    String::from_utf8_lossy(&out.stderr).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

fn has_history_shallow_warning(envelope: &serde_json::Value) -> bool {
  envelope["warnings"].as_array().is_some_and(|warnings| {
    warnings
      .iter()
      .any(|warning| warning["code"] == "HISTORY_SHALLOW")
  })
}

#[test]
fn shallow_history_churn_and_co_change_emit_diagnostics() {
  let fix = shallow_fixture();
  let repo = fix.root.join("shallow");

  let (churn, stderr, exit) = run_json(&fix, &repo, &["--format", "json", "history", "churn", "."]);
  assert_eq!(exit, 0, "stderr:\n{stderr}\nchurn:\n{churn:#}");
  assert!(
    has_history_shallow_warning(&churn),
    "history churn should warn for shallow checkouts:\n{churn:#}"
  );

  let (co_change, stderr, exit) = run_json(
    &fix,
    &repo,
    &[
      "--format",
      "json",
      "history",
      "co-change",
      "target.txt",
      ".",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nco_change:\n{co_change:#}");
  assert!(
    has_history_shallow_warning(&co_change),
    "history co-change should warn for shallow checkouts:\n{co_change:#}"
  );
}
