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
    "lmprobe-cython-not-supported-{}-{suffix}",
    std::process::id()
  ));
  let _ = std::fs::remove_dir_all(&root);
  std::fs::create_dir_all(root.join("src")).expect("mkdir fixture");
  std::fs::write(
    root.join("src/module.pyx"),
    "cdef int add(int left, int right):\n    return left + right\n",
  )
  .expect("write cython fixture");
  std::fs::write(
    root.join("src/python_compatible.pyx"),
    "def py_value():\n    return 1\n",
  )
  .expect("write python-compatible pyx fixture");
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

fn has_warning(envelope: &serde_json::Value, code: &str) -> bool {
  envelope
    .get("warnings")
    .and_then(|v| v.as_array())
    .is_some_and(|warnings| {
      warnings
        .iter()
        .any(|warning| warning.get("code").and_then(|v| v.as_str()) == Some(code))
    })
}

#[test]
fn structural_search_reports_cython_not_supported_for_pyx() {
  let fix = fixture();
  let (envelope, stderr, exit) = run(
    &fix.root,
    &[
      "--format",
      "json",
      "-G",
      "**/*.pyx",
      "search",
      "--outline",
      "--path",
      ".",
    ],
  );

  assert_eq!(
    exit, 1,
    "unsupported .pyx structural search should be no_match:\n{envelope:#}\nstderr:\n{stderr}"
  );
  assert_eq!(
    envelope.get("exit_status").and_then(|v| v.as_str()),
    Some("no_match")
  );
  assert!(
    has_warning(&envelope, "CYTHON_NOT_SUPPORTED"),
    "structural search should report unsupported Cython files:\n{envelope:#}"
  );
  assert!(
    !has_warning(&envelope, "DEPRECATED_POSITIONAL_QUERY"),
    "explicit --path should avoid positional query warning:\n{envelope:#}"
  );
}

#[test]
fn forced_python_language_override_suppresses_cython_diagnostic() {
  let fix = fixture();
  let (envelope, stderr, exit) = run(
    &fix.root,
    &[
      "--format",
      "json",
      "-G",
      "**/python_compatible.pyx",
      "-l",
      "python",
      "search",
      "--outline",
      "--path",
      ".",
    ],
  );

  assert_eq!(
    exit, 0,
    "forced Python parse should find the Python-compatible .pyx def:\n{envelope:#}\nstderr:\n{stderr}"
  );
  assert!(
    !has_warning(&envelope, "CYTHON_NOT_SUPPORTED"),
    "forced language override should suppress Cython unsupported diagnostic:\n{envelope:#}"
  );
}

#[test]
fn grep_and_find_do_not_emit_cython_structural_diagnostic() {
  let fix = fixture();
  let (grep_env, grep_stderr, grep_exit) = run(
    &fix.root,
    &["--format", "json", "-G", "**/*.pyx", "grep", "add", "."],
  );
  let (find_env, find_stderr, find_exit) =
    run(&fix.root, &["--format", "json", "find", r"\.pyx$", "."]);

  assert_eq!(
    grep_exit, 0,
    "grep should still scan .pyx text:\n{grep_env:#}\nstderr:\n{grep_stderr}"
  );
  assert!(
    grep_env
      .get("hits")
      .and_then(|v| v.as_array())
      .is_some_and(|hits| !hits.is_empty()),
    "grep should return .pyx text hits:\n{grep_env:#}"
  );
  assert!(
    !has_warning(&grep_env, "CYTHON_NOT_SUPPORTED"),
    "grep should not emit structural Cython diagnostic:\n{grep_env:#}"
  );

  assert_eq!(
    find_exit, 0,
    "find should still locate .pyx paths:\n{find_env:#}\nstderr:\n{find_stderr}"
  );
  assert!(
    find_env
      .get("paths")
      .and_then(|v| v.as_array())
      .is_some_and(|paths| !paths.is_empty()),
    "find should return .pyx paths:\n{find_env:#}"
  );
  assert!(
    !has_warning(&find_env, "CYTHON_NOT_SUPPORTED"),
    "find should not emit structural Cython diagnostic:\n{find_env:#}"
  );
}
