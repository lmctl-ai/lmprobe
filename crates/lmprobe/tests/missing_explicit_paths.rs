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

fn fixture() -> Fixture {
  let suffix = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_nanos())
    .unwrap_or(0);
  let root = std::env::temp_dir().join(format!(
    "lmprobe-missing-explicit-paths-{}-{suffix}",
    std::process::id()
  ));
  let _ = std::fs::remove_dir_all(&root);
  std::fs::create_dir_all(&root).expect("mkdir fixture");
  std::fs::write(root.join("present.txt"), "anything\n").expect("write fixture");
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

fn assert_path_not_found(envelope: &serde_json::Value, stderr: &str, exit: i32) {
  assert_eq!(
    exit, 2,
    "missing explicit path should be an error:\n{envelope:#}\nstderr:\n{stderr}"
  );
  assert_eq!(
    envelope.get("exit_status").and_then(|v| v.as_str()),
    Some("error"),
    "missing explicit path should set envelope status to error:\n{envelope:#}"
  );
  assert!(
    envelope
      .get("warnings")
      .and_then(|v| v.as_array())
      .is_some_and(|warnings| warnings.iter().any(|warning| {
        warning.get("code").and_then(|v| v.as_str()) == Some("PATH_NOT_FOUND")
          && warning
            .get("message")
            .and_then(|v| v.as_str())
            .is_some_and(|message| message.contains("does-not-exist"))
      })),
    "missing explicit path should emit PATH_NOT_FOUND:\n{envelope:#}"
  );
}

#[test]
fn grep_missing_explicit_path_reports_error() {
  let fix = fixture();
  let (envelope, stderr, exit) = run(
    &fix.root,
    &["--format", "json", "grep", "anything", "does-not-exist"],
  );

  assert_path_not_found(&envelope, &stderr, exit);
  assert!(
    envelope
      .get("hits")
      .and_then(|v| v.as_array())
      .is_some_and(|hits| hits.is_empty()),
    "grep should not report hits for a missing explicit path:\n{envelope:#}"
  );
}

#[test]
fn find_missing_explicit_path_reports_error() {
  let fix = fixture();
  let (envelope, stderr, exit) = run(
    &fix.root,
    &["--format", "json", "find", "present", "does-not-exist"],
  );

  assert_path_not_found(&envelope, &stderr, exit);
  assert!(
    envelope
      .get("paths")
      .and_then(|v| v.as_array())
      .is_some_and(|paths| paths.is_empty()),
    "find should not report paths for a missing explicit path:\n{envelope:#}"
  );
}

#[test]
fn grep_missing_repeatable_path_reports_error() {
  let fix = fixture();
  let (envelope, stderr, exit) = run(
    &fix.root,
    &[
      "--format",
      "json",
      "grep",
      "anything",
      "--path",
      "present.txt",
      "--path",
      "does-not-exist",
    ],
  );

  assert_path_not_found(&envelope, &stderr, exit);
  assert!(
    envelope
      .get("hits")
      .and_then(|v| v.as_array())
      .is_some_and(|hits| hits.is_empty()),
    "grep should not mix valid hits into a missing-path error:\n{envelope:#}"
  );
}

#[test]
fn find_missing_repeatable_path_reports_error() {
  let fix = fixture();
  let (envelope, stderr, exit) = run(
    &fix.root,
    &[
      "--format",
      "json",
      "find",
      "present",
      "--path",
      "present.txt",
      "--path",
      "does-not-exist",
    ],
  );

  assert_path_not_found(&envelope, &stderr, exit);
  assert!(
    envelope
      .get("paths")
      .and_then(|v| v.as_array())
      .is_some_and(|paths| paths.is_empty()),
    "find should not mix valid paths into a missing-path error:\n{envelope:#}"
  );
}
