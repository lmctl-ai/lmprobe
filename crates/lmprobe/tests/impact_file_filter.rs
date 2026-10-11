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
    "lmprobe-impact-file-filter-{}-{suffix}",
    std::process::id()
  ));
  let _ = std::fs::remove_dir_all(&root);
  std::fs::create_dir_all(root.join("src")).expect("mkdir src");
  std::fs::create_dir_all(root.join("tests")).expect("mkdir tests");
  std::fs::create_dir_all(root.join("pkg")).expect("mkdir pkg");
  std::fs::write(
    root.join("src/app.py"),
    "def target():\n    return 1\n\n\ndef source_caller():\n    return target()\n",
  )
  .expect("write source fixture");
  std::fs::write(
    root.join("tests/test_app.py"),
    "from src.app import target\n\n\ndef test_caller():\n    return target()\n",
  )
  .expect("write test fixture");
  std::fs::write(
    root.join("pkg/app.go"),
    "package pkg\n\nfunc targetGo() int { return 1 }\n\nfunc sourceGo() int { return targetGo() }\n",
  )
  .expect("write go source fixture");
  std::fs::write(
    root.join("pkg/app_test.go"),
    "package pkg\n\nfunc testGo() int { return targetGo() }\n",
  )
  .expect("write go test fixture");
  std::fs::write(
    root.join("script.sh"),
    "helper() { :; }\nmain() { helper; }\n",
  )
  .expect("write unsupported fixture");
  Fixture { root }
}

fn run_impact(root: &Path, symbol: &str, extra: &[&str]) -> (serde_json::Value, String, i32) {
  let mut args = vec!["--format", "json", "impact", symbol, ".", "--depth", "1"];
  args.extend_from_slice(extra);
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

fn run_callers(root: &Path, symbol: &str, extra: &[&str]) -> (serde_json::Value, String, i32) {
  run_callers_path(root, symbol, extra, ".")
}

fn run_callers_path(
  root: &Path,
  symbol: &str,
  extra: &[&str],
  path: &str,
) -> (serde_json::Value, String, i32) {
  let mut args = vec!["--format", "json", "callers", symbol];
  args.extend_from_slice(extra);
  args.push(path);
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

fn run_query(root: &Path, query: &str) -> (serde_json::Value, String, i32) {
  let out = Command::new(binary())
    .args(["--format", "json", "query", query])
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

fn normalize_path(path: &str) -> String {
  path.replace('\\', "/")
}

fn hit_paths(envelope: &serde_json::Value) -> Vec<String> {
  envelope
    .get("hits")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected callers hits:\n{envelope:#}"))
    .iter()
    .map(|hit| {
      hit
        .get("file_path")
        .and_then(|v| v.as_str())
        .expect("hit file_path")
    })
    .map(normalize_path)
    .collect()
}

fn node_names(envelope: &serde_json::Value) -> Vec<&str> {
  envelope
    .get("nodes")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected impact nodes:\n{envelope:#}"))
    .iter()
    .map(|node| {
      node
        .get("name")
        .and_then(|v| v.as_str())
        .expect("node name")
    })
    .collect()
}

fn query_node_names(envelope: &serde_json::Value) -> Vec<&str> {
  envelope
    .pointer("/data/impact/nodes")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected query impact nodes:\n{envelope:#}"))
    .iter()
    .map(|node| {
      node
        .get("name")
        .and_then(|v| v.as_str())
        .expect("node name")
    })
    .collect()
}

fn assert_callers_matched(envelope: &serde_json::Value, stderr: &str, exit: i32) {
  assert_eq!(exit, 0, "callers failed:\n{envelope:#}\nstderr:\n{stderr}");
  assert_eq!(
    envelope.get("exit_status").and_then(|v| v.as_str()),
    Some("matched")
  );
}

fn warning_codes(envelope: &serde_json::Value) -> Vec<&str> {
  envelope
    .get("warnings")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected warnings:\n{envelope:#}"))
    .iter()
    .map(|warning| {
      warning
        .get("code")
        .and_then(|v| v.as_str())
        .expect("warning code")
    })
    .collect()
}

fn assert_matched(envelope: &serde_json::Value, stderr: &str, exit: i32) {
  assert_eq!(exit, 0, "impact failed:\n{envelope:#}\nstderr:\n{stderr}");
  assert_eq!(
    envelope.get("exit_status").and_then(|v| v.as_str()),
    Some("matched")
  );
}

#[test]
fn callers_tests_only_keeps_likely_test_paths() {
  let fix = fixture();

  let (default_env, default_stderr, default_exit) = run_callers(&fix.root, "target", &[]);
  assert_callers_matched(&default_env, &default_stderr, default_exit);
  let default_paths = hit_paths(&default_env);
  assert!(
    default_paths
      .iter()
      .any(|path| path.ends_with("src/app.py"))
      && default_paths
        .iter()
        .any(|path| path.ends_with("tests/test_app.py")),
    "default callers should include source and test paths:\n{default_env:#}"
  );

  let (tests_env, tests_stderr, tests_exit) = run_callers(&fix.root, "target", &["--tests-only"]);
  assert_callers_matched(&tests_env, &tests_stderr, tests_exit);
  let tests_paths = hit_paths(&tests_env);
  assert!(
    tests_paths
      .iter()
      .any(|path| path.ends_with("tests/test_app.py"))
      && !tests_paths.iter().any(|path| path.ends_with("src/app.py")),
    "callers --tests-only should keep likely test paths only:\n{tests_env:#}"
  );
}

#[test]
fn callers_tests_only_preserves_unsupported_language_diagnostic_status() {
  let fix = fixture();

  let (default_env, default_stderr, default_exit) =
    run_callers_path(&fix.root, "helper", &[], "script.sh");
  assert_callers_matched(&default_env, &default_stderr, default_exit);
  assert!(
    warning_codes(&default_env).contains(&"NOT_IMPLEMENTED"),
    "unsupported caller scan should carry diagnostic warning:\n{default_env:#}"
  );

  let (tests_env, tests_stderr, tests_exit) =
    run_callers_path(&fix.root, "helper", &["--tests-only"], "script.sh");
  assert_callers_matched(&tests_env, &tests_stderr, tests_exit);
  assert!(
    warning_codes(&tests_env).contains(&"NOT_IMPLEMENTED"),
    "callers --tests-only should preserve diagnostic-only status:\n{tests_env:#}"
  );
}

#[test]
fn impact_file_filters_split_source_and_test_callers() {
  let fix = fixture();

  let (default_env, default_stderr, default_exit) = run_impact(&fix.root, "target", &[]);
  assert_matched(&default_env, &default_stderr, default_exit);
  let default_names = node_names(&default_env);
  assert!(
    default_names.contains(&"source_caller") && default_names.contains(&"test_caller"),
    "default impact should include source and test callers:\n{default_env:#}"
  );

  let (exclude_env, exclude_stderr, exclude_exit) =
    run_impact(&fix.root, "target", &["--exclude-tests"]);
  assert_matched(&exclude_env, &exclude_stderr, exclude_exit);
  let exclude_names = node_names(&exclude_env);
  assert!(
    exclude_names.contains(&"source_caller") && !exclude_names.contains(&"test_caller"),
    "--exclude-tests should keep source/non-test callers only:\n{exclude_env:#}"
  );

  let (tests_env, tests_stderr, tests_exit) = run_impact(&fix.root, "target", &["--tests-only"]);
  assert_matched(&tests_env, &tests_stderr, tests_exit);
  let tests_names = node_names(&tests_env);
  assert!(
    tests_names.contains(&"test_caller") && !tests_names.contains(&"source_caller"),
    "--tests-only should keep test callers only:\n{tests_env:#}"
  );
}

#[test]
fn impact_file_filters_recognize_go_test_suffix() {
  let fix = fixture();

  let (default_env, default_stderr, default_exit) = run_impact(&fix.root, "targetGo", &[]);
  assert_matched(&default_env, &default_stderr, default_exit);
  let default_names = node_names(&default_env);
  assert!(
    default_names.contains(&"sourceGo") && default_names.contains(&"testGo"),
    "default impact should include Go source and *_test.go callers:\n{default_env:#}"
  );

  let (exclude_env, exclude_stderr, exclude_exit) =
    run_impact(&fix.root, "targetGo", &["--exclude-tests"]);
  assert_matched(&exclude_env, &exclude_stderr, exclude_exit);
  let exclude_names = node_names(&exclude_env);
  assert!(
    exclude_names.contains(&"sourceGo") && !exclude_names.contains(&"testGo"),
    "--exclude-tests should drop Go *_test.go callers:\n{exclude_env:#}"
  );

  let (tests_env, tests_stderr, tests_exit) = run_impact(&fix.root, "targetGo", &["--tests-only"]);
  assert_matched(&tests_env, &tests_stderr, tests_exit);
  let tests_names = node_names(&tests_env);
  assert!(
    tests_names.contains(&"testGo") && !tests_names.contains(&"sourceGo"),
    "--tests-only should keep Go *_test.go callers:\n{tests_env:#}"
  );
}

#[test]
fn query_impact_exclude_tests_matches_cli_filter() {
  let fix = fixture();
  let query = r#"{ impact(symbol: "target", path: ".", depth: 1, excludeTests: true) { nodes { name filePath } exitStatus } }"#;
  let (envelope, stderr, exit) = run_query(&fix.root, query);

  assert_eq!(
    exit, 0,
    "GraphQL impact excludeTests should match:\n{envelope:#}\nstderr:\n{stderr}"
  );
  assert_eq!(
    envelope
      .pointer("/data/impact/exitStatus")
      .and_then(|v| v.as_str()),
    Some("MATCHED")
  );
  let names = query_node_names(&envelope);
  assert!(
    names.contains(&"source_caller") && !names.contains(&"test_caller"),
    "GraphQL excludeTests should keep source/non-test callers only:\n{envelope:#}"
  );
}
