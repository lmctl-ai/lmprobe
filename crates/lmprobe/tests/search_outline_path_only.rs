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
    "lmprobe-search-outline-path-only-{}-{suffix}",
    std::process::id()
  ));
  let _ = std::fs::remove_dir_all(&root);
  std::fs::create_dir_all(root.join("src")).expect("mkdir fixture");
  std::fs::write(
    root.join("src/lib.rs"),
    "pub fn target() {}\npub struct Thing;\n",
  )
  .expect("write rust fixture");
  Fixture { root }
}

fn run_outline(root: &Path, path: &str) -> (serde_json::Value, String, i32) {
  let out = Command::new(binary())
    .args(["--format", "json", "search", "--outline", path])
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

fn assert_path_only_outline(envelope: &serde_json::Value, stderr: &str, exit: i32) {
  assert_eq!(
    exit, 0,
    "outline search failed:\n{envelope:#}\nstderr:\n{stderr}"
  );
  assert_eq!(
    envelope.get("exit_status").and_then(|v| v.as_str()),
    Some("matched"),
    "path-only outline should match declarations:\n{envelope:#}"
  );
  assert!(
    envelope
      .get("hits")
      .and_then(|v| v.as_array())
      .is_some_and(|hits| {
        !hits.is_empty()
          && hits.iter().all(|hit| {
            hit
              .get("file_path")
              .and_then(|v| v.as_str())
              .is_some_and(|path| path.replace('\\', "/") == "src/lib.rs")
          })
      }),
    "outline hits should come only from the requested fixture file:\n{envelope:#}"
  );
  assert!(
    !has_deprecated_positional_query_warning(envelope),
    "existing path positional should not be treated as deprecated query:\n{envelope:#}"
  );
}

fn has_deprecated_positional_query_warning(envelope: &serde_json::Value) -> bool {
  envelope
    .get("warnings")
    .and_then(|v| v.as_array())
    .is_some_and(|warnings| {
      warnings.iter().any(|warning| {
        warning.get("code").and_then(|v| v.as_str()) == Some("DEPRECATED_POSITIONAL_QUERY")
      })
    })
}

#[test]
fn search_outline_lone_file_path_is_path_not_deprecated_query() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_outline(&fix.root, "src/lib.rs");

  assert_path_only_outline(&envelope, &stderr, exit);
}

#[test]
fn search_outline_lone_directory_path_is_path_not_deprecated_query() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_outline(&fix.root, "src");

  assert_path_only_outline(&envelope, &stderr, exit);
}

#[test]
fn search_outline_lone_non_path_stays_deprecated_query() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_outline(&fix.root, "DefinitelyNotAPathOrKnownSymbol");

  assert_eq!(
    exit, 1,
    "non-path query fallback should keep no-match exit semantics:\n{envelope:#}\nstderr:\n{stderr}"
  );
  assert!(
    has_deprecated_positional_query_warning(&envelope),
    "non-path positional should remain deprecated query input:\n{envelope:#}"
  );
}
