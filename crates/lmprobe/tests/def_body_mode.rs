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

fn run_json(args: &[&str]) -> (serde_json::Value, i32) {
  let out = Command::new(binary())
    .args(args)
    .current_dir(workspace_root())
    .output()
    .expect("spawn lmprobe");
  let stdout = String::from_utf8_lossy(&out.stdout);
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    out.status.code().unwrap_or(-1),
  )
}

fn first_content(envelope: &serde_json::Value) -> &str {
  envelope
    .pointer("/hits/0/content")
    .and_then(|v| v.as_str())
    .expect("first hit content")
}

#[test]
fn def_around_adds_bounded_source_context() {
  let path = "crates/lmprobe/tests/fixtures/rust/lib.rs";
  let (envelope, exit) = run_json(&[
    "--format",
    "json",
    "def",
    "--around",
    "1",
    "user_roundtrip",
    path,
  ]);

  assert_eq!(exit, 0, "envelope:\n{envelope:#}");
  assert_eq!(
    envelope
      .pointer("/hits/0/source_context/0/line_number")
      .and_then(|v| v.as_u64()),
    Some(13),
    "definition source context should start one line before the def:\n{envelope:#}",
  );
  assert_eq!(
    envelope
      .pointer("/hits/0/source_context/1/line_content")
      .and_then(|v| v.as_str()),
    Some("    fn user_roundtrip() {"),
    "definition source context should be centered on the definition line:\n{envelope:#}",
  );
  assert_eq!(
    envelope
      .pointer("/hits/0/source_context")
      .and_then(|v| v.as_array())
      .map(Vec::len),
    Some(3)
  );
}

#[test]
fn def_default_omits_source_context() {
  let path = "crates/lmprobe/tests/fixtures/rust/lib.rs";
  let (envelope, exit) = run_json(&["--format", "json", "def", "user_roundtrip", path]);

  assert_eq!(exit, 0, "envelope:\n{envelope:#}");
  assert!(
    envelope.pointer("/hits/0/source_context").is_none(),
    "default def output should remain compact:\n{envelope:#}",
  );
}

#[test]
fn def_body_mode_returns_full_declaration_body() {
  let path = "crates/lmprobe/tests/fixtures/rust";
  let (default_envelope, default_exit) =
    run_json(&["--format", "json", "def", "user_roundtrip", path]);
  let (body_envelope, body_exit) =
    run_json(&["--format", "json", "def", "--body", "user_roundtrip", path]);

  assert_eq!(default_exit, 0, "default envelope:\n{default_envelope:#}");
  assert_eq!(body_exit, 0, "body envelope:\n{body_envelope:#}");
  assert!(
    !first_content(&default_envelope).contains("let _ = User"),
    "default def output should stay skeletonized:\n{default_envelope:#}",
  );
  assert!(
    first_content(&body_envelope).contains("let _ = User"),
    "--body should include implementation statements:\n{body_envelope:#}",
  );
}

#[test]
fn query_def_body_mode_matches_cli_body_mode() {
  let query = r#"{ def(name: "user_roundtrip", path: "crates/lmprobe/tests/fixtures/rust", body: true) { hits { content } exitStatus } }"#;
  let (envelope, exit) = run_json(&["--format", "json", "query", query]);

  assert_eq!(exit, 0, "query envelope:\n{envelope:#}");
  assert_eq!(
    envelope
      .pointer("/data/def/exitStatus")
      .and_then(|v| v.as_str()),
    Some("MATCHED"),
    "GraphQL def should still classify matched:\n{envelope:#}",
  );
  assert!(
    envelope
      .pointer("/data/def/hits/0/content")
      .and_then(|v| v.as_str())
      .is_some_and(|content| content.contains("let _ = User")),
    "GraphQL def(body: true) should include full body:\n{envelope:#}",
  );
}
