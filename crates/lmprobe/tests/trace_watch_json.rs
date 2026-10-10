use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

fn python3_available() -> bool {
  Command::new("python3")
    .arg("--version")
    .output()
    .map(|out| out.status.success())
    .unwrap_or(false)
}

fn bash_available() -> bool {
  Command::new("bash")
    .arg("--version")
    .output()
    .map(|out| out.status.success())
    .unwrap_or(false)
}

fn rscript_available() -> bool {
  Command::new("Rscript")
    .arg("--version")
    .output()
    .map(|out| out.status.success())
    .unwrap_or(false)
}

fn fixture_root(name: &str) -> tempfile::TempDir {
  tempfile::Builder::new()
    .prefix(&format!("lmprobe-trace-watch-json-{name}-"))
    .tempdir()
    .expect("create fixture root")
}

// Keep the TempDir binding alive for the whole test scope; dropping it removes
// the fixture directory.
fn fixture_path(fixture: &tempfile::TempDir) -> PathBuf {
  fixture.path().to_path_buf()
}

fn init_git(root: &Path) {
  let git = |args: &[&str]| {
    Command::new("git")
      .args(args)
      .current_dir(root)
      .output()
      .expect("spawn git")
  };
  git(&["init", "-q"]);
  git(&[
    "-c",
    "user.email=t@t",
    "-c",
    "user.name=t",
    "config",
    "commit.gpgsign",
    "false",
  ]);
  git(&["add", "."]);
  git(&[
    "-c",
    "user.email=t@t",
    "-c",
    "user.name=t",
    "commit",
    "-qm",
    "init",
  ]);
}

fn write_fixture(root: &Path) -> String {
  let source = r#"class Box:
    def __init__(self):
        self.shape = (2, 3)

    def __repr__(self):
        return "Box(" + "x" * 80 + ")"


def main():
    box = Box()
    values = [1, 2, 3, 4]
    total = sum(values)
    return total


if __name__ == "__main__":
    print(main())
"#;
  fs::write(root.join("sample.py"), source).expect("write sample.py");
  source.to_string()
}

fn write_second_matching_python_file(root: &Path) {
  let source = r#"def main():
    values = [10, 20]
    total = sum(values)
    return total


if __name__ == "__main__":
    print(main())
"#;
  fs::write(root.join("other.py"), source).expect("write other.py");
}

fn write_javascript_fixture(root: &Path) {
  fs::write(
    root.join("sample.js"),
    "const total = 1;\nconsole.log(total);\n",
  )
  .expect("write sample.js");
}

fn write_bash_fixture(root: &Path) -> String {
  let source = r##"#!/usr/bin/env bash
set -euo pipefail
count=4
message=$'hello "json"\nline'
items=("alpha" "two words" 'quote"here')
total=$((count + ${#items[@]}))
echo "total=$total"
"##;
  fs::write(root.join("sample.sh"), source).expect("write sample.sh");
  source.to_string()
}

fn write_r_fixture(root: &Path) -> String {
  let source = r#"value <- 4
items <- c(1, 2, 3)
matrix_value <- matrix(1:4, nrow = 2)
total <- value + length(items)
print(total)
"#;
  fs::write(root.join("sample.R"), source).expect("write sample.R");
  source.to_string()
}

fn write_r_dotted_fixture(root: &Path) -> String {
  let source = r#"my.value <- 4
sales.total <- my.value + 3
print(sales.total)
"#;
  fs::write(root.join("sample.R"), source).expect("write dotted sample.R");
  source.to_string()
}

fn write_r_continuation_fixture(root: &Path) -> String {
  let source = r#"value <- c(
  1
)
print(length(value))
"#;
  fs::write(root.join("sample.R"), source).expect("write continuation sample.R");
  source.to_string()
}

fn write_r_operator_continuation_fixture(root: &Path) -> String {
  let source = r#"value <- 1 +
  2
print(value)
"#;
  fs::write(root.join("sample.R"), source).expect("write operator continuation sample.R");
  source.to_string()
}

fn write_r_block_fixture(root: &Path) -> String {
  let source = r#"score <- 82
if (score >= 80) {
  grade <- "pass"
} else {
  grade <- "retry"
}
print(grade)
"#;
  fs::write(root.join("sample.R"), source).expect("write block sample.R");
  source.to_string()
}

fn run_trace(root: &Path, args: &[&str]) -> (serde_json::Value, String, i32) {
  let out = Command::new(binary())
    .args(args)
    .current_dir(root)
    .output()
    .expect("spawn lmprobe trace");
  let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
  let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    stderr,
    out.status.code().unwrap_or(-1),
  )
}

fn worktree_path(root: &Path, suffix: &str) -> String {
  root
    .with_file_name(format!(
      "{}-{suffix}",
      root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("worktree")
    ))
    .to_string_lossy()
    .into_owned()
}

fn event_for<'a>(events: &'a [serde_json::Value], var: &str) -> &'a serde_json::Value {
  events
    .iter()
    .find(|event| event["var"] == var)
    .unwrap_or_else(|| panic!("missing watch event for {var}; events={events:#?}"))
}

#[test]
fn trace_watch_json_emits_bounded_structured_events_and_rolls_back() {
  if !python3_available() {
    return;
  }
  let fixture = fixture_root("structured");
  let root = fixture_path(&fixture);
  let original = write_fixture(&root);
  init_git(&root);
  let worktree = worktree_path(&root, "wt");

  let (envelope, stderr, exit) = run_trace(
    &root,
    &[
      "--format",
      "json",
      "trace",
      "total = sum(values)",
      ".",
      "--watch",
      "box,values,total",
      "--json-trace",
      "--shape",
      "--len",
      "--id",
      "--repr-limit",
      "12",
      "--exec",
      "python3 sample.py",
      "--worktree",
      &worktree,
    ],
  );

  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope}");
  assert_eq!(
    fs::read_to_string(root.join("sample.py")).expect("read rolled back file"),
    original,
    "trace should restore the original file byte-for-byte"
  );

  let events = envelope["events"]
    .as_array()
    .expect("trace events should be an array");
  let values_event = events
    .iter()
    .find(|event| event["var"] == "values")
    .expect("values watch event");
  assert_eq!(values_event["type"], "list");
  assert_eq!(values_event["len"], 4);
  assert!(values_event["id"].as_str().unwrap_or("").starts_with("0x"));

  let box_event = events
    .iter()
    .find(|event| event["var"] == "box")
    .expect("box watch event");
  assert_eq!(box_event["shape"], serde_json::json!([2, 3]));
  assert!(
    box_event["repr"].as_str().unwrap_or("").len() <= 12,
    "repr should be bounded: {box_event}"
  );

  let total_event = events
    .iter()
    .find(|event| event["var"] == "total")
    .expect("total watch event");
  assert_eq!(total_event["type"], "int");
  assert!(
    envelope["warnings"]
      .as_array()
      .expect("warnings")
      .iter()
      .any(|warning| warning["code"] == "WORKTREE_USED"),
    "watch trace should run in the isolated worktree"
  );

  let _ = fs::remove_dir_all(&root);
  let _ = fs::remove_dir_all(&worktree);
}

#[test]
fn trace_watch_json_emits_bash_scalar_array_lengths_and_rolls_back() {
  if !bash_available() {
    return;
  }
  let fixture = fixture_root("bash-structured");
  let root = fixture_path(&fixture);
  let original = write_bash_fixture(&root);
  init_git(&root);
  let worktree = worktree_path(&root, "bash-wt");

  let (envelope, stderr, exit) = run_trace(
    &root,
    &[
      "--format",
      "json",
      "trace",
      "total=$VALUE",
      "sample.sh",
      "--watch",
      "count,message,items,total,missing",
      "--json-trace",
      "--shape",
      "--len",
      "--id",
      "--repr-limit",
      "80",
      "--exec",
      "bash sample.sh",
      "--worktree",
      &worktree,
    ],
  );

  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope}");
  assert_eq!(
    fs::read_to_string(root.join("sample.sh")).expect("read rolled back bash file"),
    original,
    "trace should restore the original Bash file byte-for-byte"
  );

  let events = envelope["events"]
    .as_array()
    .expect("trace events should be an array");
  let count_event = event_for(events, "count");
  assert_eq!(count_event["type"], "str");
  assert_eq!(count_event["repr"], "4");
  assert_eq!(count_event["len"], 1);
  assert!(count_event["id"].is_null(), "Bash id metadata is omitted");
  assert!(
    count_event["shape"].is_null(),
    "Bash shape metadata is omitted"
  );

  let message_event = event_for(events, "message");
  assert_eq!(message_event["type"], "str");
  assert_eq!(
    message_event["repr"],
    serde_json::json!("hello \"json\"\nline")
  );

  let items_event = event_for(events, "items");
  assert_eq!(items_event["type"], "array");
  assert_eq!(items_event["len"], 3);
  assert!(
    items_event["repr"]
      .as_str()
      .unwrap_or("")
      .contains("declare -a items"),
    "array repr should come from declare -p: {items_event}"
  );

  let total_event = event_for(events, "total");
  assert_eq!(total_event["type"], "str");
  assert_eq!(total_event["repr"], "7");

  let missing_event = event_for(events, "missing");
  assert_eq!(missing_event["type"], "unset");
  assert!(missing_event["len"].is_null());

  assert!(
    envelope["warnings"]
      .as_array()
      .expect("warnings")
      .iter()
      .any(|warning| warning["code"] == "WORKTREE_USED"),
    "Bash watch trace should run in the isolated worktree"
  );

  let _ = fs::remove_dir_all(&root);
  let _ = fs::remove_dir_all(&worktree);
}

#[test]
fn trace_watch_json_emits_r_line_based_values_and_rolls_back() {
  if !rscript_available() {
    return;
  }
  let fixture = fixture_root("r-structured");
  let root = fixture_path(&fixture);
  let original = write_r_fixture(&root);
  init_git(&root);
  let worktree = worktree_path(&root, "r-wt");

  let (envelope, stderr, exit) = run_trace(
    &root,
    &[
      "--format",
      "json",
      "trace",
      "total <- value + length(items)",
      "sample.R",
      "--watch",
      "value,items,matrix_value,total,missing",
      "--json-trace",
      "--shape",
      "--len",
      "--id",
      "--repr-limit",
      "80",
      "--exec",
      "Rscript sample.R",
      "--worktree",
      &worktree,
    ],
  );

  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope}");
  assert_eq!(
    fs::read_to_string(root.join("sample.R")).expect("read rolled back R file"),
    original,
    "trace should restore the original R file byte-for-byte"
  );

  let events = envelope["events"]
    .as_array()
    .expect("trace events should be an array");
  let value_event = event_for(events, "value");
  assert_eq!(value_event["type"], "numeric");
  assert_eq!(value_event["repr"], "[1] 4");
  assert_eq!(value_event["len"], 1);
  assert!(value_event["id"].is_null(), "R id metadata is null/omitted");

  let items_event = event_for(events, "items");
  assert_eq!(items_event["type"], "numeric");
  assert_eq!(items_event["len"], 3);
  assert_eq!(items_event["repr"], "[1] 1 2 3");

  let matrix_event = event_for(events, "matrix_value");
  assert_eq!(matrix_event["type"], "matrix/array");
  assert_eq!(matrix_event["len"], 4);
  assert_eq!(matrix_event["shape"], serde_json::json!([2, 2]));

  let total_event = event_for(events, "total");
  assert_eq!(total_event["type"], "numeric");
  assert_eq!(total_event["repr"], "[1] 7");

  let missing_event = event_for(events, "missing");
  assert_eq!(missing_event["type"], "unset");
  assert!(missing_event["len"].is_null());
  assert!(missing_event["shape"].is_null());
  assert!(missing_event["id"].is_null());

  assert!(
    envelope["warnings"]
      .as_array()
      .expect("warnings")
      .iter()
      .any(|warning| warning["code"] == "WORKTREE_USED"),
    "R watch trace should run in the isolated worktree"
  );

  let _ = fs::remove_dir_all(&root);
  let _ = fs::remove_dir_all(&worktree);
}

#[test]
fn trace_watch_json_emits_r_dotted_watch_names_and_rolls_back() {
  if !rscript_available() {
    return;
  }
  let fixture = fixture_root("r-dotted");
  let root = fixture_path(&fixture);
  let original = write_r_dotted_fixture(&root);
  init_git(&root);
  let worktree = worktree_path(&root, "r-dotted-wt");

  let (envelope, stderr, exit) = run_trace(
    &root,
    &[
      "--format",
      "json",
      "trace",
      "sales.total <- my.value + 3",
      "sample.R",
      "--watch",
      "my.value,sales.total",
      "--json-trace",
      "--len",
      "--repr-limit",
      "80",
      "--exec",
      "Rscript sample.R",
      "--worktree",
      &worktree,
    ],
  );

  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope}");
  assert_eq!(
    fs::read_to_string(root.join("sample.R")).expect("read rolled back R file"),
    original,
    "trace should restore the original dotted R file byte-for-byte"
  );

  let events = envelope["events"]
    .as_array()
    .expect("trace events should be an array");
  let my_value = event_for(events, "my.value");
  assert_eq!(my_value["type"], "numeric");
  assert_eq!(my_value["repr"], "[1] 4");
  assert_eq!(my_value["len"], 1);

  let sales_total = event_for(events, "sales.total");
  assert_eq!(sales_total["type"], "numeric");
  assert_eq!(sales_total["repr"], "[1] 7");
  assert_eq!(sales_total["len"], 1);

  let _ = fs::remove_dir_all(&root);
  let _ = fs::remove_dir_all(&worktree);
}

#[test]
fn trace_watch_r_continuation_line_is_not_instrumented() {
  if !rscript_available() {
    return;
  }
  let fixture = fixture_root("r-continuation-no-match");
  let root = fixture_path(&fixture);
  let original = write_r_continuation_fixture(&root);
  init_git(&root);

  let (envelope, stderr, exit) = run_trace(
    &root,
    &[
      "--format",
      "json",
      "trace",
      "1",
      "sample.R",
      "--watch",
      "value",
      "--json-trace",
      "--exec",
      "Rscript sample.R",
    ],
  );

  assert_eq!(exit, 1, "stderr:\n{stderr}\nenvelope:\n{envelope}");
  assert_eq!(envelope["exit_status"], "no_match");
  assert!(
    envelope["events"].as_array().expect("events").is_empty(),
    "unsafe continuation lines should behave like no match"
  );
  assert_eq!(
    fs::read_to_string(root.join("sample.R")).expect("read R file after no-match"),
    original,
    "no-match trace should leave the original R file untouched"
  );

  let _ = fs::remove_dir_all(&root);
}

#[test]
fn trace_watch_r_operator_continuation_line_is_not_instrumented() {
  if !rscript_available() {
    return;
  }
  let fixture = fixture_root("r-operator-continuation-no-match");
  let root = fixture_path(&fixture);
  let original = write_r_operator_continuation_fixture(&root);
  init_git(&root);

  let (envelope, stderr, exit) = run_trace(
    &root,
    &[
      "--format",
      "json",
      "trace",
      "value <- 1 +",
      "sample.R",
      "--watch",
      "value",
      "--json-trace",
      "--exec",
      "Rscript sample.R",
    ],
  );

  assert_eq!(exit, 1, "stderr:\n{stderr}\nenvelope:\n{envelope}");
  assert_eq!(envelope["exit_status"], "no_match");
  assert!(
    envelope["events"].as_array().expect("events").is_empty(),
    "operator continuation lines should behave like no match"
  );
  assert_eq!(
    fs::read_to_string(root.join("sample.R")).expect("read R file after no-match"),
    original,
    "no-match trace should leave the original R file untouched"
  );

  let _ = fs::remove_dir_all(&root);
}

#[test]
fn trace_watch_r_block_internal_line_is_instrumented() {
  if !rscript_available() {
    return;
  }
  let fixture = fixture_root("r-block-internal");
  let root = fixture_path(&fixture);
  let original = write_r_block_fixture(&root);
  init_git(&root);
  let worktree = worktree_path(&root, "r-block-wt");

  let (envelope, stderr, exit) = run_trace(
    &root,
    &[
      "--format",
      "json",
      "trace",
      "grade <- \"pass\"",
      "sample.R",
      "--watch",
      "score,grade",
      "--json-trace",
      "--len",
      "--exec",
      "Rscript sample.R",
      "--worktree",
      &worktree,
    ],
  );

  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope}");
  assert_eq!(
    fs::read_to_string(root.join("sample.R")).expect("read rolled back R file"),
    original,
    "trace should restore the original block R file byte-for-byte"
  );

  let events = envelope["events"]
    .as_array()
    .expect("trace events should be an array");
  let score_event = event_for(events, "score");
  assert_eq!(score_event["repr"], "[1] 82");

  let grade_event = event_for(events, "grade");
  assert_eq!(grade_event["type"], "character");
  assert_eq!(grade_event["repr"], "[1] \"pass\"");
  assert_eq!(grade_event["len"], 1);

  let _ = fs::remove_dir_all(&root);
  let _ = fs::remove_dir_all(&worktree);
}

#[test]
fn trace_watch_requires_json_trace() {
  if !python3_available() {
    return;
  }
  let fixture = fixture_root("requires-json");
  let root = fixture_path(&fixture);
  write_fixture(&root);
  init_git(&root);

  let (envelope, _stderr, exit) = run_trace(
    &root,
    &[
      "--format",
      "json",
      "trace",
      "total = sum(values)",
      "sample.py",
      "--watch",
      "total",
      "--exec",
      "python3 sample.py",
    ],
  );

  assert_eq!(exit, 2);
  assert_eq!(envelope["exit_status"], "error");
  assert!(envelope["warnings"]
    .as_array()
    .expect("warnings")
    .iter()
    .any(|warning| warning["code"] == "TRACE_JSON_REQUIRED"));

  let _ = fs::remove_dir_all(&root);
}

#[test]
fn trace_watch_rejects_non_identifier_watch_expressions() {
  if !python3_available() {
    return;
  }
  let fixture = fixture_root("identifier-only");
  let root = fixture_path(&fixture);
  write_fixture(&root);
  init_git(&root);

  let (envelope, _stderr, exit) = run_trace(
    &root,
    &[
      "--format",
      "json",
      "trace",
      "total = sum(values)",
      "sample.py",
      "--watch",
      "box.shape",
      "--json-trace",
      "--exec",
      "python3 sample.py",
    ],
  );

  assert_eq!(exit, 2);
  assert!(envelope["warnings"]
    .as_array()
    .expect("warnings")
    .iter()
    .any(|warning| warning["code"] == "TRACE_WATCH_IDENTIFIER_UNSUPPORTED"));

  let _ = fs::remove_dir_all(&root);
}

#[test]
fn trace_watch_rejects_invalid_watch_names_before_match_discovery() {
  let fixture = fixture_root("invalid-identifier-no-match");
  let root = fixture_path(&fixture);
  write_fixture(&root);
  init_git(&root);

  let (envelope, _stderr, exit) = run_trace(
    &root,
    &[
      "--format",
      "json",
      "trace",
      "no_matching_statement = 1",
      "sample.py",
      "--watch",
      "bad name",
      "--json-trace",
      "--exec",
      "true",
    ],
  );

  assert_eq!(exit, 2);
  assert_eq!(envelope["exit_status"], "error");
  assert!(envelope["warnings"]
    .as_array()
    .expect("warnings")
    .iter()
    .any(|warning| warning["code"] == "TRACE_WATCH_IDENTIFIER_UNSUPPORTED"));

  let _ = fs::remove_dir_all(&root);
}

#[test]
fn trace_watch_worktree_preserves_requested_file_scope() {
  if !python3_available() {
    return;
  }
  let fixture = fixture_root("worktree-file-scope");
  let root = fixture_path(&fixture);
  write_fixture(&root);
  write_second_matching_python_file(&root);
  init_git(&root);
  let worktree = worktree_path(&root, "scope-wt");

  let (envelope, stderr, exit) = run_trace(
    &root,
    &[
      "--format",
      "json",
      "trace",
      "total = sum(values)",
      "sample.py",
      "--watch",
      "values",
      "--json-trace",
      "--len",
      "--exec",
      "python3 sample.py && python3 other.py",
      "--worktree",
      &worktree,
    ],
  );

  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope}");
  let value_lengths: Vec<u64> = envelope["events"]
    .as_array()
    .expect("events")
    .iter()
    .filter(|event| event["var"] == "values")
    .filter_map(|event| event["len"].as_u64())
    .collect();
  assert_eq!(
    value_lengths,
    vec![4],
    "only the requested sample.py file should be instrumented"
  );

  let _ = fs::remove_dir_all(&root);
  let _ = fs::remove_dir_all(&worktree);
}

#[test]
fn trace_watch_non_python_non_bash_match_reports_unsupported_language_diagnostic() {
  let fixture = fixture_root("unsupported-language-diagnostic");
  let root = fixture_path(&fixture);
  write_javascript_fixture(&root);
  init_git(&root);

  let (envelope, _stderr, exit) = run_trace(
    &root,
    &[
      "--format",
      "json",
      "trace",
      "const total = 1",
      "sample.js",
      "--watch",
      "total",
      "--json-trace",
      "--exec",
      "true",
    ],
  );

  assert_eq!(exit, 2);
  let diagnostic = envelope["warnings"]
    .as_array()
    .expect("warnings")
    .iter()
    .find(|warning| warning["code"] == "TRACE_WATCH_LANG_UNSUPPORTED")
    .expect("unsupported language diagnostic");
  assert!(
    diagnostic["message"]
      .as_str()
      .unwrap_or("")
      .contains("Python, Bash, and line-based R"),
    "diagnostic should mention Python/Bash/R support: {diagnostic}"
  );

  let _ = fs::remove_dir_all(&root);
}
