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
    "lmprobe-find-path-ranking-{}-{suffix}",
    std::process::id()
  ));
  let _ = std::fs::remove_dir_all(&root);

  for dir in [
    "src/main/java/com/example",
    "src/test/java/com/example",
    "docs",
    "fixtures",
    "samples",
  ] {
    std::fs::create_dir_all(root.join(dir)).expect("mkdir fixture dir");
  }
  for file in [
    "src/main/java/com/example/ConfigurationProcessor.java",
    "src/test/java/com/example/ConfigurationProcessorTests.java",
    "docs/configuration-processor.md",
    "fixtures/configuration-processor-sample.java",
    "samples/configuration-processor.kt",
  ] {
    std::fs::write(root.join(file), "").expect("write fixture file");
  }

  Fixture { root }
}

fn run_find(root: &Path, pattern: &str) -> (serde_json::Value, String, i32) {
  let out = Command::new(binary())
    .args(["--format", "json", "-i", "find", pattern, "."])
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
  path.replace('\\', "/").trim_start_matches("./").to_string()
}

#[test]
fn find_ranks_source_path_before_parallel_tests_docs_and_samples() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_find(&fix.root, "configuration.*processor");

  assert_eq!(exit, 0, "find failed:\n{envelope:#}\nstderr:\n{stderr}");
  let paths = envelope
    .get("paths")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected find paths:\n{envelope:#}"));
  let paths: Vec<String> = paths
    .iter()
    .map(|value| normalize_path(value.as_str().expect("path should be a string")))
    .collect();

  assert_eq!(
    paths.first().map(String::as_str),
    Some("src/main/java/com/example/ConfigurationProcessor.java"),
    "source path should rank first:\n{paths:#?}"
  );
  for expected in [
    "src/test/java/com/example/ConfigurationProcessorTests.java",
    "docs/configuration-processor.md",
    "fixtures/configuration-processor-sample.java",
    "samples/configuration-processor.kt",
  ] {
    assert!(
      paths.iter().any(|path| path == expected),
      "ranking should not drop matched path {expected}:\n{paths:#?}"
    );
  }
}

#[test]
fn find_does_not_demote_low_signal_paths_when_pattern_names_them() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_find(&fix.root, "test.*configuration.*processor");

  assert_eq!(exit, 0, "find failed:\n{envelope:#}\nstderr:\n{stderr}");
  let paths = envelope
    .get("paths")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected find paths:\n{envelope:#}"));
  let paths: Vec<String> = paths
    .iter()
    .filter_map(|value| value.as_str())
    .map(normalize_path)
    .collect();

  assert_eq!(
    paths.first().map(String::as_str),
    Some("src/test/java/com/example/ConfigurationProcessorTests.java"),
    "explicit test query should not demote test paths:\n{paths:#?}"
  );
}
