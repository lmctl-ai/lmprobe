use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

fn fixture_root(name: &str) -> PathBuf {
  let root = std::env::temp_dir().join(format!("lmprobe-callsites-{name}-{}", std::process::id()));
  let _ = fs::remove_dir_all(&root);
  fs::create_dir_all(&root).expect("create fixture root");
  root
}

fn write_python_fixture(root: &Path) {
  let source = r#"def foo(a, b, key=None):
    pass

# foo(commented)
text = "foo(in_string)"

def run(a, b, x, value):
    before = value
    foo(a, b, key=value)
    obj.foo(x, flag=True)
    foo(a == b, c=1)
    after = value
"#;
  fs::write(root.join("calls.py"), source).expect("write calls.py");
}

fn write_non_python_fixture(root: &Path) {
  fs::write(root.join("calls.js"), "foo(a, b, {key: value});\n").expect("write calls.js");
}

fn run_json(root: &Path, args: &[&str]) -> (serde_json::Value, String, i32) {
  let out = Command::new(binary())
    .args(args)
    .current_dir(root)
    .output()
    .expect("spawn lmprobe");
  let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
  let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    stderr,
    out.status.code().unwrap_or(-1),
  )
}

fn run_text(root: &Path, args: &[&str]) -> (String, String, i32) {
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

#[test]
fn callsites_args_extracts_python_single_line_arguments() {
  let root = fixture_root("python");
  write_python_fixture(&root);

  let (envelope, stderr, exit) = run_json(
    &root,
    &[
      "--format",
      "json",
      "callsites",
      "foo",
      "--args",
      "--around",
      "1",
      "calls.py",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope}");
  let callsites = envelope["callsites"].as_array().expect("callsites");
  assert_eq!(callsites.len(), 3, "envelope:\n{envelope:#}");

  for line_number in [1, 4, 5] {
    assert!(
      callsites
        .iter()
        .all(|callsite| callsite["line_number"] != line_number),
      "function definitions, comments, and strings must not be reported:\n{envelope:#}"
    );
  }

  let plain = callsites
    .iter()
    .find(|callsite| callsite["callee_text"] == "foo")
    .expect("plain foo call");
  assert_eq!(plain["line_number"], 9);
  assert_eq!(plain["positional_args"], serde_json::json!(["a", "b"]));
  assert_eq!(
    plain["keyword_args"],
    serde_json::json!([{ "name": "key", "value": "value" }])
  );
  assert_eq!(
    plain["source_context"].as_array().expect("context").len(),
    3
  );

  let method = callsites
    .iter()
    .find(|callsite| callsite["callee_text"] == "obj.foo")
    .expect("method foo call");
  assert_eq!(method["line_number"], 10);
  assert_eq!(method["positional_args"], serde_json::json!(["x"]));
  assert_eq!(
    method["keyword_args"],
    serde_json::json!([{ "name": "flag", "value": "True" }])
  );

  let comparison = callsites
    .iter()
    .find(|callsite| callsite["line_number"] == 11)
    .expect("comparison positional call");
  assert_eq!(comparison["positional_args"], serde_json::json!(["a == b"]));
  assert_eq!(
    comparison["keyword_args"],
    serde_json::json!([{ "name": "c", "value": "1" }])
  );

  let _ = fs::remove_dir_all(&root);
}

#[test]
fn callsites_args_extracts_python_multiline_arguments() {
  let root = fixture_root("python-multiline");
  let source = r#"def build_form(label, get_default):
    form = Form(
        get_default(),
        default=None,
        description=f"label: {label}",
        choices=[
            "one",
            "two",
        ],
    )
    return form
"#;
  fs::write(root.join("forms.py"), source).expect("write forms.py");

  let (envelope, stderr, exit) = run_json(
    &root,
    &[
      "--format",
      "json",
      "callsites",
      "Form",
      "--args",
      "--around",
      "1",
      "forms.py",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope}");
  let callsites = envelope["callsites"].as_array().expect("callsites");
  assert_eq!(callsites.len(), 1, "envelope:\n{envelope:#}");
  let hit = &callsites[0];
  assert_eq!(hit["line_number"], 2);
  assert_eq!(hit["callee_text"], "Form");
  assert_eq!(hit["positional_args"], serde_json::json!(["get_default()"]));
  assert_eq!(
    hit["keyword_args"],
    serde_json::json!([
      { "name": "default", "value": "None" },
      { "name": "description", "value": "f\"label: {label}\"" },
      { "name": "choices", "value": "[\n            \"one\",\n            \"two\",\n        ]" }
    ])
  );
  assert_eq!(hit["source_context"].as_array().expect("context").len(), 3);

  let _ = fs::remove_dir_all(&root);
}

#[test]
fn callsites_args_skips_non_python_without_claims() {
  let root = fixture_root("non-python");
  write_non_python_fixture(&root);

  let (envelope, stderr, exit) = run_json(
    &root,
    &["--format", "json", "callsites", "foo", "--args", "calls.js"],
  );
  assert_eq!(exit, 1, "stderr:\n{stderr}\nenvelope:\n{envelope}");
  assert_eq!(
    envelope["callsites"].as_array().expect("callsites").len(),
    0
  );
  assert_eq!(envelope["exit_status"], "no_match");

  let _ = fs::remove_dir_all(&root);
}

#[test]
fn callsites_markdown_renders_around_context() {
  let root = fixture_root("markdown");
  write_python_fixture(&root);

  let (stdout, stderr, exit) = run_text(
    &root,
    &[
      "--format",
      "markdown",
      "callsites",
      "foo",
      "--args",
      "--around",
      "1",
      "calls.py",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nstdout:\n{stdout}");
  assert!(
    stdout.contains("calls.py:9: foo(a, b, key=value)"),
    "stdout should include call summary:\n{stdout}"
  );
  assert!(
    stdout.contains("calls.py-8:     before = value")
      && stdout.contains("calls.py:9:     foo(a, b, key=value)")
      && stdout.contains("calls.py-10:     obj.foo(x, flag=True)"),
    "stdout should render line-numbered source context:\n{stdout}"
  );

  let _ = fs::remove_dir_all(&root);
}

#[test]
fn callsites_command_does_not_change_callers_shape() {
  let root = fixture_root("callers");
  write_python_fixture(&root);

  let (envelope, _stderr, _exit) =
    run_json(&root, &["--format", "json", "callers", "foo", "calls.py"]);
  assert!(
    envelope
      .get("hits")
      .and_then(|hits| hits.as_array())
      .is_some(),
    "callers should keep its existing hits envelope:\n{envelope:#}"
  );
  assert!(
    envelope.get("callsites").is_none(),
    "callers should not gain callsites payload:\n{envelope:#}"
  );

  let _ = fs::remove_dir_all(&root);
}
