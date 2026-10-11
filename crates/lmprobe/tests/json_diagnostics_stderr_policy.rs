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
    .map(|d| {
      // Unique within the process: parallel tests can read the same clock value.
      static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
      d.as_nanos() * 1024 + N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as u128
    })
    .unwrap_or(0);
  let root = std::env::temp_dir().join(format!(
    "lmprobe-json-diagnostics-stderr-{}-{suffix}",
    std::process::id()
  ));
  let _ = std::fs::remove_dir_all(&root);
  std::fs::create_dir_all(&root).expect("mkdir fixture");
  std::fs::write(root.join("large.txt"), "needle\n").expect("write fixture");
  Fixture { root }
}

fn run_grep(root: &Path, verbose: bool) -> (serde_json::Value, String, i32) {
  let mut args = vec![
    "--format",
    "json",
    "--max-filesize",
    "1",
    "grep",
    "needle",
    ".",
  ];
  if verbose {
    args.insert(0, "-v");
  }
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

fn has_file_too_large_warning(envelope: &serde_json::Value) -> bool {
  envelope
    .get("warnings")
    .and_then(|v| v.as_array())
    .is_some_and(|warnings| {
      warnings
        .iter()
        .any(|warning| warning.get("code").and_then(|v| v.as_str()) == Some("FILE_TOO_LARGE"))
    })
}

#[test]
fn json_mode_mirrors_partial_file_too_large_warning_to_stderr() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_grep(&fix.root, false);

  assert_eq!(
    exit, 3,
    "grep should signal partial coverage:\n{envelope:#}\nstderr:\n{stderr}"
  );
  assert!(
    has_file_too_large_warning(&envelope),
    "FILE_TOO_LARGE should stay in the JSON envelope:\n{envelope:#}"
  );
  assert!(
    stderr.contains("FILE_TOO_LARGE"),
    "partial JSON diagnostics should mirror to stderr by default:\n{stderr}"
  );
}

#[test]
fn json_verbose_mirrors_nonfatal_file_too_large_warning_to_stderr() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_grep(&fix.root, true);

  assert_eq!(
    exit, 3,
    "grep should signal partial coverage:\n{envelope:#}\nstderr:\n{stderr}"
  );
  assert!(
    has_file_too_large_warning(&envelope),
    "FILE_TOO_LARGE should stay in the JSON envelope:\n{envelope:#}"
  );
  assert!(
    stderr.contains("FILE_TOO_LARGE"),
    "-v should mirror non-error JSON diagnostics to stderr:\n{stderr}"
  );
}

#[test]
fn json_error_diagnostic_still_mirrors_to_stderr() {
  let fix = fixture();
  let (envelope, stderr, exit) = run(
    &fix.root,
    &["--format", "json", "grep", "needle", "does-not-exist"],
  );

  assert_eq!(
    exit, 2,
    "missing path should remain an error:\n{envelope:#}\nstderr:\n{stderr}"
  );
  assert_eq!(
    envelope.get("exit_status").and_then(|v| v.as_str()),
    Some("error"),
    "missing path should set JSON status to error:\n{envelope:#}"
  );
  assert!(
    stderr.contains("PATH_NOT_FOUND"),
    "error-status diagnostics should still mirror to stderr:\n{stderr}"
  );
}

#[test]
fn json_quiet_suppresses_error_diagnostic_stderr_mirror() {
  let fix = fixture();
  let (envelope, stderr, exit) = run(
    &fix.root,
    &["--format", "json", "-q", "grep", "needle", "does-not-exist"],
  );

  assert_eq!(
    exit, 2,
    "missing path should remain an error:\n{envelope:#}\nstderr:\n{stderr}"
  );
  assert_eq!(
    envelope.get("exit_status").and_then(|v| v.as_str()),
    Some("error"),
    "quiet should not change JSON status:\n{envelope:#}"
  );
  assert_eq!(
    stderr, "",
    "-q should suppress even error diagnostic stderr mirrors"
  );
}
