use std::path::PathBuf;
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

fn workspace_root() -> PathBuf {
  let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  p.pop();
  p.pop();
  p
}

fn run_query(query: &str) -> (serde_json::Value, i32) {
  let out = Command::new(binary())
    .args(["--format", "json", "query", query])
    .current_dir(workspace_root())
    .output()
    .expect("spawn lmprobe query");
  let stdout = String::from_utf8_lossy(&out.stdout);
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    out.status.code().unwrap_or(-1),
  )
}

#[test]
fn no_match_query_exit_is_selection_set_independent() {
  let with_hits = r#"{ def(name: "DefinitelyNotInFixture", path: "crates/lmprobe/tests/fixtures/rust") { hits { name } } }"#;
  let schema_only = r#"{ def(name: "DefinitelyNotInFixture", path: "crates/lmprobe/tests/fixtures/rust") { schema } }"#;

  for query in [with_hits, schema_only] {
    let (envelope, exit) = run_query(query);
    assert_eq!(exit, 1, "query should exit no_match:\n{envelope:#}");
    assert_eq!(
      envelope.get("exit_status").and_then(|v| v.as_str()),
      Some("no_match"),
      "top-level status should come from resolver state:\n{envelope:#}",
    );
  }
}

#[test]
fn matched_query_exit_is_selection_set_independent() {
  let with_hits =
    r#"{ def(name: "User", path: "crates/lmprobe/tests/fixtures/rust") { hits { name } } }"#;
  let schema_only =
    r#"{ def(name: "User", path: "crates/lmprobe/tests/fixtures/rust") { schema } }"#;

  for query in [with_hits, schema_only] {
    let (envelope, exit) = run_query(query);
    assert_eq!(exit, 0, "query should exit matched:\n{envelope:#}");
    assert_eq!(
      envelope.get("exit_status").and_then(|v| v.as_str()),
      Some("matched"),
      "top-level status should come from resolver state:\n{envelope:#}",
    );
  }
}
