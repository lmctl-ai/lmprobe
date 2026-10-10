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

fn run_context(symbol: &str) -> (serde_json::Value, i32) {
  run_context_json(&[
    "--format",
    "json",
    "context",
    "--recent-history-limit",
    "1",
    symbol,
  ])
}

fn run_context_json(args: &[&str]) -> (serde_json::Value, i32) {
  let out = Command::new(binary())
    .args(args)
    .current_dir(workspace_root())
    .output()
    .expect("spawn lmprobe context");
  let stdout = String::from_utf8_lossy(&out.stdout);
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    out.status.code().unwrap_or(-1),
  )
}

fn run_context_stdout(args: &[&str]) -> (String, i32) {
  let out = Command::new(binary())
    .args(args)
    .current_dir(workspace_root())
    .output()
    .expect("spawn lmprobe context");
  (
    String::from_utf8_lossy(&out.stdout).into_owned(),
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
fn context_path_line_resolves_enclosing_declaration() {
  let (envelope, exit) = run_context("crates/lmprobe/tests/fixtures/rust/lib.rs:15");

  assert_eq!(exit, 0, "envelope:\n{envelope:#}");
  assert_eq!(
    envelope.get("symbol").and_then(|v| v.as_str()),
    Some("user_roundtrip"),
    "location should resolve to the enclosing test function:\n{envelope:#}",
  );
  assert_eq!(
    envelope
      .pointer("/definition/name")
      .and_then(|v| v.as_str()),
    Some("user_roundtrip"),
    "resolved context should use the normal definition bundle:\n{envelope:#}",
  );
  assert!(
    envelope
      .get("warnings")
      .and_then(|v| v.as_array())
      .is_some_and(|warnings| warnings.iter().any(|warning| {
        warning.get("code").and_then(|v| v.as_str()) == Some("CONTEXT_LOCATION_RESOLVED")
          && warning
            .get("message")
            .and_then(|v| v.as_str())
            .is_some_and(|message| message.contains("user_roundtrip"))
      })),
    "location resolution should be explicit in warnings:\n{envelope:#}",
  );
}

#[test]
fn context_path_line_outside_declaration_does_not_resolve() {
  let (envelope, exit) = run_context("crates/lmprobe/tests/fixtures/rust/lib.rs:8");

  assert_eq!(exit, 1, "envelope:\n{envelope:#}");
  assert_eq!(
    envelope.get("exit_status").and_then(|v| v.as_str()),
    Some("no_match"),
    "outside-declaration line should remain unresolved:\n{envelope:#}",
  );
  assert!(
    !has_warning(&envelope, "CONTEXT_LOCATION_RESOLVED"),
    "unresolved location should not emit a resolution warning:\n{envelope:#}",
  );
}

#[test]
fn context_path_line_import_line_does_not_resolve_to_import_keyword() {
  let (envelope, exit) = run_context("crates/lmprobe/tests/fixtures/rust/lib.rs:1");

  assert_eq!(exit, 1, "envelope:\n{envelope:#}");
  assert!(
    !has_warning(&envelope, "CONTEXT_LOCATION_RESOLVED"),
    "import lines are not enclosing declarations for context:\n{envelope:#}",
  );
  assert_ne!(
    envelope
      .pointer("/definition/name")
      .and_then(|v| v.as_str()),
    Some("use"),
    "context should not resolve location requests to import keywords:\n{envelope:#}",
  );
}

#[test]
fn context_plain_symbol_does_not_emit_location_resolution_warning() {
  let (envelope, exit) = run_context("User");

  assert_eq!(exit, 0, "envelope:\n{envelope:#}");
  assert_eq!(
    envelope.get("symbol").and_then(|v| v.as_str()),
    Some("User"),
    "plain symbol context should remain unchanged:\n{envelope:#}",
  );
  assert!(
    !has_warning(&envelope, "CONTEXT_LOCATION_RESOLVED"),
    "plain symbol context should not look like a location rewrite:\n{envelope:#}",
  );
  assert!(
    envelope.get("source_context").is_none(),
    "default context output should remain compact:\n{envelope:#}",
  );
}

#[test]
fn context_symbol_around_adds_definition_source_context() {
  let (envelope, exit) = run_context_json(&[
    "--format",
    "json",
    "context",
    "--recent-history-limit",
    "1",
    "--around",
    "1",
    "user_roundtrip",
    "crates/lmprobe/tests/fixtures/rust/lib.rs",
  ]);

  assert_eq!(exit, 0, "envelope:\n{envelope:#}");
  assert_eq!(
    envelope
      .pointer("/source_context/0/line_number")
      .and_then(|v| v.as_u64()),
    Some(13)
  );
  assert_eq!(
    envelope
      .pointer("/source_context/1/line_content")
      .and_then(|v| v.as_str()),
    Some("    fn user_roundtrip() {")
  );
  assert_eq!(
    envelope
      .get("source_context")
      .and_then(|v| v.as_array())
      .map(Vec::len),
    Some(3)
  );
}

#[test]
fn context_path_line_around_uses_requested_anchor_line() {
  let (envelope, exit) = run_context_json(&[
    "--format",
    "json",
    "context",
    "--recent-history-limit",
    "1",
    "--around",
    "1",
    "crates/lmprobe/tests/fixtures/rust/lib.rs:15",
  ]);

  assert_eq!(exit, 0, "envelope:\n{envelope:#}");
  assert_eq!(
    envelope.get("symbol").and_then(|v| v.as_str()),
    Some("user_roundtrip"),
  );
  assert_eq!(
    envelope
      .pointer("/source_context/1/line_number")
      .and_then(|v| v.as_u64()),
    Some(15),
    "PATH:LINE context should be centered on the requested line, not the enclosing def start:\n{envelope:#}",
  );
  assert_eq!(
    envelope
      .pointer("/source_context/1/line_content")
      .and_then(|v| v.as_str()),
    Some("        let _ = User { id: 1, name: \"x\".into() };")
  );
}

#[test]
fn context_around_is_present_in_ndjson_summary() {
  let (stdout, exit) = run_context_stdout(&[
    "--format",
    "ndjson",
    "context",
    "--recent-history-limit",
    "1",
    "--around",
    "1",
    "user_roundtrip",
    "crates/lmprobe/tests/fixtures/rust/lib.rs",
  ]);
  let summary: serde_json::Value = stdout
    .lines()
    .last()
    .map(|line| serde_json::from_str(line).expect("ndjson summary should parse"))
    .unwrap_or_else(|| panic!("expected ndjson output:\n{stdout}"));

  assert_eq!(exit, 0, "stdout:\n{stdout}");
  assert_eq!(
    summary.get("kind").and_then(|v| v.as_str()),
    Some("summary")
  );
  assert_eq!(
    summary
      .pointer("/source_context/1/line_content")
      .and_then(|v| v.as_str()),
    Some("    fn user_roundtrip() {")
  );
}
