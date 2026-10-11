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
    "lmprobe-regex-parse-diagnostics-{}-{suffix}",
    std::process::id()
  ));
  let _ = std::fs::remove_dir_all(&root);
  std::fs::create_dir_all(&root).expect("mkdir fixture");
  std::fs::write(root.join("sample.txt"), "replace(value)\n").expect("write fixture");
  Fixture { root }
}

fn run(root: &Path, args: &[&str]) -> (String, String, i32) {
  let out = Command::new(binary())
    .args(args)
    .current_dir(root)
    .output()
    .expect("spawn lmprobe");
  (
    String::from_utf8_lossy(&out.stdout).into_owned(),
    String::from_utf8_lossy(&out.stderr).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

fn warning<'a>(envelope: &'a serde_json::Value, code: &str) -> Option<&'a serde_json::Value> {
  envelope
    .get("warnings")
    .and_then(|v| v.as_array())
    .and_then(|warnings| {
      warnings
        .iter()
        .find(|warning| warning.get("code").and_then(|v| v.as_str()) == Some(code))
    })
}

fn assert_regex_parse_envelope(
  envelope: &serde_json::Value,
  item_field: &str,
  expected_schema: &str,
) {
  assert_eq!(
    envelope.get("schema").and_then(|v| v.as_str()),
    Some(expected_schema)
  );
  assert_eq!(
    envelope.get("exit_status").and_then(|v| v.as_str()),
    Some("error")
  );
  if let Some(items) = envelope.get(item_field) {
    assert!(
      items.as_array().is_some_and(|items| items.is_empty()),
      "parse-error envelope should not contain matches:\n{envelope:#}"
    );
  }
  let diagnostic = warning(envelope, "REGEX_PARSE_FAILED")
    .unwrap_or_else(|| panic!("missing REGEX_PARSE_FAILED:\n{envelope:#}"));
  assert!(
    diagnostic
      .get("message")
      .and_then(|v| v.as_str())
      .is_some_and(|message| message.contains("regex pattern failed to parse")),
    "diagnostic should explain regex parse failure:\n{diagnostic:#}"
  );
  assert!(
    diagnostic
      .get("message")
      .and_then(|v| v.as_str())
      .is_some_and(|message| message.contains("regex parse error")),
    "diagnostic should include parser context:\n{diagnostic:#}"
  );
  assert!(
    diagnostic
      .get("hint")
      .and_then(|v| v.as_str())
      .is_some_and(|hint| hint.contains("Rust regex")),
    "diagnostic should include recovery hint:\n{diagnostic:#}"
  );
  assert!(
    warning(envelope, "REGEX_LITERAL_PIPE_NO_MATCH").is_none(),
    "parse errors should not also emit literal-pipe no-match warning:\n{envelope:#}"
  );
}

#[test]
fn grep_malformed_regex_json_returns_diagnostic_envelope() {
  let fix = fixture();
  let (stdout, stderr, exit) = run(&fix.root, &["--format", "json", "grep", r"replace\\(", "."]);
  let envelope: serde_json::Value =
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}"));

  assert_eq!(exit, 2, "grep should fail:\n{stdout}\nstderr:\n{stderr}");
  assert_regex_parse_envelope(&envelope, "hits", "lmprobe.grep.v1");
  let diagnostic = warning(&envelope, "REGEX_PARSE_FAILED").expect("diagnostic");
  assert!(
    diagnostic
      .get("hint")
      .and_then(|v| v.as_str())
      .is_some_and(|hint| hint.contains("remove one backslash")),
    "double-escaped parenthesis should get a targeted hint:\n{diagnostic:#}"
  );
  assert!(
    stderr.contains("lmprobe: grep [REGEX_PARSE_FAILED]"),
    "error diagnostics should still mirror to stderr:\n{stderr}"
  );
}

#[test]
fn grep_malformed_regex_ndjson_returns_diagnostic_summary() {
  let fix = fixture();
  let (stdout, stderr, exit) = run(&fix.root, &["--format", "ndjson", "grep", "foo(", "."]);
  let summary: serde_json::Value = stdout
    .lines()
    .last()
    .map(|line| serde_json::from_str(line).expect("summary should parse"))
    .unwrap_or_else(|| panic!("expected ndjson summary:\n{stdout}"));

  assert_eq!(exit, 2, "grep should fail:\n{stdout}\nstderr:\n{stderr}");
  assert_eq!(
    summary.get("kind").and_then(|v| v.as_str()),
    Some("summary")
  );
  assert_regex_parse_envelope(&summary, "hits", "lmprobe.grep.v1");
}

#[test]
fn grep_malformed_regex_markdown_renders_stable_diagnostic() {
  let fix = fixture();
  let (stdout, stderr, exit) = run(&fix.root, &["--format", "markdown", "grep", "foo(", "."]);

  assert_eq!(exit, 2, "grep should fail:\n{stdout}\nstderr:\n{stderr}");
  assert!(
    stdout.trim().is_empty(),
    "markdown parse error should not render fake hits:\n{stdout}"
  );
  assert!(
    stderr.contains("lmprobe: grep [REGEX_PARSE_FAILED]"),
    "markdown should render stable lmprobe diagnostic:\n{stderr}"
  );
  assert!(
    stderr.contains("hint: use Rust regex syntax"),
    "markdown diagnostic should include hint:\n{stderr}"
  );
}

#[test]
fn find_malformed_regex_json_returns_diagnostic_envelope() {
  let fix = fixture();
  let (stdout, stderr, exit) = run(&fix.root, &["--format", "json", "find", "foo(", "."]);
  let envelope: serde_json::Value =
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}"));

  assert_eq!(exit, 2, "find should fail:\n{stdout}\nstderr:\n{stderr}");
  assert_regex_parse_envelope(&envelope, "paths", "lmprobe.find.v1");
  assert!(
    stderr.contains("lmprobe: find [REGEX_PARSE_FAILED]"),
    "error diagnostics should still mirror to stderr:\n{stderr}"
  );
}
