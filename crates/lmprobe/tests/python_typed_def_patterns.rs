use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

fn fixture_dir(name: &str) -> PathBuf {
  let dir = std::env::temp_dir().join(format!("lmprobe-{name}-{}", std::process::id()));
  let _ = fs::remove_dir_all(&dir);
  fs::create_dir_all(&dir).expect("create tmp dir");
  dir
}

fn write_model(dir: &PathBuf, source: &str) {
  fs::write(dir.join("model.py"), source).expect("write fixture");
}

fn run_search(dir: &PathBuf, pattern: &str) -> (serde_json::Value, i32) {
  let out = Command::new(binary())
    .args(["--format", "json", "search", "-p", pattern, "-l", "python"])
    .current_dir(dir)
    .output()
    .expect("spawn lmprobe");
  let stdout = String::from_utf8_lossy(&out.stdout);
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    out.status.code().unwrap_or(-1),
  )
}

#[test]
fn annotated_python_def_pattern_matches_with_return_metavar() {
  let dir = fixture_dir("python-typed-def-explicit");
  write_model(
    &dir,
    "class User:\n    def __str__(self) -> str:\n        return \"user\"\n",
  );
  let (envelope, exit) = run_search(&dir, "def __str__($$$) -> $RET: $$$");
  let _ = fs::remove_dir_all(&dir);

  assert_eq!(exit, 0, "envelope:\n{envelope:#}");
  assert_eq!(
    envelope
      .get("hits")
      .and_then(|v| v.as_array())
      .map(Vec::len),
    Some(1),
    "envelope:\n{envelope:#}",
  );
  assert_eq!(
    envelope.pointer("/hits/0/kind").and_then(|v| v.as_str()),
    Some("function_definition"),
    "envelope:\n{envelope:#}",
  );
}

#[test]
fn unannotated_python_def_pattern_falls_back_to_typed_shape() {
  let dir = fixture_dir("python-typed-def-fallback");
  write_model(
    &dir,
    "class User:\n    def __str__(self) -> str:\n        return \"user\"\n",
  );
  let (envelope, exit) = run_search(&dir, "def __str__($$$): $$$");
  let _ = fs::remove_dir_all(&dir);

  assert_eq!(exit, 0, "envelope:\n{envelope:#}");
  assert_eq!(
    envelope
      .get("hits")
      .and_then(|v| v.as_array())
      .map(Vec::len),
    Some(1),
    "envelope:\n{envelope:#}",
  );
  assert!(
    envelope
      .get("warnings")
      .and_then(|v| v.as_array())
      .is_some_and(|warnings| warnings.iter().any(|warning| {
        warning.get("code").and_then(|v| v.as_str()) == Some("PYTHON_TYPED_DEF_PATTERN_FALLBACK")
      })),
    "fallback should be reported in warnings:\n{envelope:#}",
  );
}

#[test]
fn unannotated_python_def_fallback_handles_annotated_parameters() {
  let dir = fixture_dir("python-typed-def-param-annotation");
  write_model(&dir, "def f(x: int) -> str:\n    return str(x)\n");
  let (envelope, exit) = run_search(&dir, "def f($$$): $$$");
  let _ = fs::remove_dir_all(&dir);

  assert_eq!(exit, 0, "envelope:\n{envelope:#}");
  assert_eq!(
    envelope
      .get("hits")
      .and_then(|v| v.as_array())
      .map(Vec::len),
    Some(1),
    "envelope:\n{envelope:#}",
  );
  assert!(
    envelope
      .get("warnings")
      .and_then(|v| v.as_array())
      .is_some_and(|warnings| warnings.iter().any(|warning| {
        warning.get("code").and_then(|v| v.as_str()) == Some("PYTHON_TYPED_DEF_PATTERN_FALLBACK")
      })),
    "fallback should be reported in warnings:\n{envelope:#}",
  );
}

#[test]
fn unannotated_python_def_fallback_preserves_no_match_when_typed_shape_misses() {
  let dir = fixture_dir("python-typed-def-no-match");
  write_model(&dir, "def other() -> str:\n    return \"other\"\n");
  let (envelope, exit) = run_search(&dir, "def __str__($$$): $$$");
  let _ = fs::remove_dir_all(&dir);

  assert_eq!(exit, 1, "envelope:\n{envelope:#}");
  assert_eq!(
    envelope.get("exit_status").and_then(|v| v.as_str()),
    Some("no_match"),
    "envelope:\n{envelope:#}",
  );
  assert!(
    envelope
      .get("warnings")
      .and_then(|v| v.as_array())
      .is_none_or(|warnings| warnings.iter().all(|warning| {
        warning.get("code").and_then(|v| v.as_str()) != Some("PYTHON_TYPED_DEF_PATTERN_FALLBACK")
      })),
    "no-hit fallback should not claim a successful retry:\n{envelope:#}",
  );
}
