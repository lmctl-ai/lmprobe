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

fn fixture_root(name: &str) -> PathBuf {
  let root = std::env::temp_dir().join(format!(
    "lmprobe-py-slice-candidates-{name}-{}",
    std::process::id()
  ));
  let _ = fs::remove_dir_all(&root);
  fs::create_dir_all(&root).expect("create fixture root");
  root
}

fn write_positive_fixture(root: &Path) {
  let source = r#"class Trainer:
    def fit(self, Y, sl, Y_norm_squared):
        Y_chunk = Y[sl]
        scratch = len(Y_chunk)
        consume(Y_chunk, Y_norm_squared, scratch)

    def safe_related_sliced(self, Z, sl, Z_norm_squared):
        Z_chunk = Z[sl]
        consume(Z_chunk, Z_norm_squared[sl])
"#;
  fs::write(root.join("positive.py"), source).expect("write positive.py");
}

fn write_overmatch_fixture(root: &Path) {
  let source = r#"class Trainer:
    def fit(self, Y, sl, Y_norm_squared):
        Y_chunk = Y[sl]
        consume(Y_chunk, Y_norm_squared)
        consume( Y_chunk, Y_norm_squared )
"#;
  fs::write(root.join("overmatch.py"), source).expect("write overmatch.py");
}

fn write_traceable_comment_fixture(root: &Path) {
  let source = r#"def consume(chunk, norm):
    print(len(chunk), len(norm))


def main():
    Y = [1, 2, 3]
    sl = slice(0, 2)
    Y_norm_squared = [1, 4, 9]
    Y_chunk = Y[sl]
    consume(Y_chunk, Y_norm_squared)  # trace anchor should omit this comment


if __name__ == "__main__":
    main()
"#;
  fs::write(root.join("traceable.py"), source).expect("write traceable.py");
}

fn write_negative_fixture(root: &Path) {
  let source = r#"def safe(Y, sl, Y_norm_squared):
    Y_chunk = Y[sl]
    consume(Y_chunk, Y_norm_squared[sl])


def unrelated(Y, sl, X_norm_squared):
    Y_chunk = Y[sl]
    consume(Y_chunk, X_norm_squared)


def false_positive(Y, sl, Y_norm_squared):
    Y_chunk = Y[sl]
    Y_chunk = normalize(Y_norm_squared)
    consume(Y_chunk)
"#;
  fs::write(root.join("negative.py"), source).expect("write negative.py");
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

fn run_raw(root: &Path, args: &[&str]) -> (String, String, i32) {
  let out = Command::new(binary())
    .args(args)
    .current_dir(root)
    .output()
    .expect("spawn lmprobe");
  let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
  let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
  (stdout, stderr, out.status.code().unwrap_or(-1))
}

fn run_json(root: &Path, args: &[&str]) -> (serde_json::Value, String, i32) {
  let (stdout, stderr, exit) = run_raw(root, args);
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    stderr,
    exit,
  )
}

#[test]
fn py_slice_candidates_reports_conservative_evidence() {
  let root = fixture_root("positive");
  write_positive_fixture(&root);

  let (envelope, stderr, exit) = run_json(
    &root,
    &["--format", "json", "py-slice-candidates", "positive.py"],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope}");
  let candidates = envelope["candidates"].as_array().expect("candidates");
  assert_eq!(candidates.len(), 1, "envelope:\n{envelope:#}");
  let candidate = &candidates[0];
  assert_eq!(candidate["enclosing_class"], "Trainer");
  assert_eq!(candidate["enclosing_function"], "fit");
  assert_eq!(candidate["sliced_base_variable"], "Y");
  assert_eq!(candidate["derived_sliced_variable"], "Y_chunk");
  assert_eq!(candidate["slice_expression"], "Y[sl]");
  assert_eq!(candidate["related_unsliced_variable"], "Y_norm_squared");
  assert_eq!(
    candidate["call_expression_line"],
    "consume(Y_chunk, Y_norm_squared, scratch)"
  );
  assert_eq!(candidate["confidence"], "low");
  assert_eq!(candidate["slice_line_number"], 3);
  assert_eq!(candidate["call_line_number"], 5);
  assert!(
    candidate.get("candidate_id").is_none(),
    "default output should not include trace-plan-only id:\n{envelope:#}"
  );
  assert!(
    candidate.get("trace_plan").is_none(),
    "default output should not include trace_plan:\n{envelope:#}"
  );

  let _ = fs::remove_dir_all(&root);
}

#[test]
fn py_slice_candidates_trace_plan_adds_id_and_watch_plan() {
  let root = fixture_root("trace-plan-json");
  write_overmatch_fixture(&root);

  let (envelope, stderr, exit) = run_json(
    &root,
    &[
      "--format",
      "json",
      "py-slice-candidates",
      "overmatch.py",
      "--trace-plan",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope}");
  let candidates = envelope["candidates"].as_array().expect("candidates");
  assert_eq!(candidates.len(), 1, "envelope:\n{envelope:#}");
  let candidate = &candidates[0];
  let first_id = candidate["candidate_id"]
    .as_str()
    .expect("candidate_id should be present with --trace-plan");
  assert!(
    first_id.starts_with("pyslice-"),
    "candidate_id should be deterministic-prefixed: {first_id}"
  );
  let plan = &candidate["trace_plan"];
  assert_eq!(plan["file_path"], "overmatch.py");
  assert_eq!(
    plan["statement_pattern"],
    "consume(Y_chunk, Y_norm_squared)"
  );
  assert_eq!(
    plan["watch_variables"],
    serde_json::json!(["Y", "Y_chunk", "Y_norm_squared"])
  );
  assert_eq!(plan["suggested_line_number"], 4);
  assert_eq!(plan["enclosing_function"], "fit");
  assert!(plan["warnings"]
    .as_array()
    .expect("warnings")
    .iter()
    .any(|warning| warning
      .as_str()
      .is_some_and(|text| text.contains("appears 2 times"))));

  let (second, _stderr, second_exit) = run_json(
    &root,
    &[
      "--format",
      "json",
      "py-slice-candidates",
      "--trace-plan",
      "overmatch.py",
    ],
  );
  assert_eq!(second_exit, 0, "second envelope:\n{second}");
  assert_eq!(second["candidates"][0]["candidate_id"], first_id);

  let _ = fs::remove_dir_all(&root);
}

#[test]
fn py_slice_candidates_trace_plan_strips_comment_and_pattern_traces() {
  if !python3_available() {
    return;
  }
  let root = fixture_root("trace-plan-comment");
  write_traceable_comment_fixture(&root);

  let (envelope, stderr, exit) = run_json(
    &root,
    &[
      "--format",
      "json",
      "py-slice-candidates",
      "traceable.py",
      "--trace-plan",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope}");
  let pattern = envelope["candidates"][0]["trace_plan"]["statement_pattern"]
    .as_str()
    .expect("statement pattern");
  assert_eq!(pattern, "consume(Y_chunk, Y_norm_squared)");
  assert!(
    !pattern.contains('#'),
    "trace statement pattern must not include trailing comments"
  );
  let watch_variables = envelope["candidates"][0]["trace_plan"]["watch_variables"]
    .as_array()
    .expect("watch vars")
    .iter()
    .map(|value| value.as_str().expect("watch var"))
    .collect::<Vec<_>>()
    .join(",");

  init_git(&root);
  let worktree = worktree_path(&root, "wt");
  let (trace_envelope, trace_stderr, trace_exit) = run_json(
    &root,
    &[
      "--format",
      "json",
      "trace",
      pattern,
      "traceable.py",
      "--watch",
      &watch_variables,
      "--json-trace",
      "--len",
      "--exec",
      "python3 traceable.py",
      "--worktree",
      &worktree,
    ],
  );
  assert_eq!(
    trace_exit, 0,
    "stderr:\n{trace_stderr}\nenvelope:\n{trace_envelope}"
  );
  let events = trace_envelope["events"].as_array().expect("trace events");
  assert!(
    events.iter().any(|event| event["var"] == "Y_chunk"),
    "trace should emit watch evidence for the candidate plan:\n{trace_envelope:#}"
  );

  let _ = fs::remove_dir_all(&root);
  let _ = fs::remove_dir_all(worktree);
}

#[test]
fn py_slice_candidates_ignores_sliced_or_unrelated_variables() {
  let root = fixture_root("negative");
  write_negative_fixture(&root);

  let (envelope, stderr, exit) = run_json(
    &root,
    &["--format", "json", "py-slice-candidates", "negative.py"],
  );
  assert_eq!(exit, 1, "stderr:\n{stderr}\nenvelope:\n{envelope}");
  assert_eq!(
    envelope["candidates"].as_array().expect("candidates").len(),
    0
  );
  assert_eq!(envelope["exit_status"], "no_match");

  let _ = fs::remove_dir_all(&root);
}

#[test]
fn py_slice_candidates_ndjson_itemizes_candidates() {
  let root = fixture_root("ndjson");
  write_positive_fixture(&root);

  let (stdout, stderr, exit) = run_raw(
    &root,
    &["--format", "ndjson", "py-slice-candidates", "positive.py"],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nstdout:\n{stdout}");
  let lines = stdout.lines().collect::<Vec<_>>();
  assert_eq!(lines.len(), 2, "stdout:\n{stdout}");
  let item: serde_json::Value = serde_json::from_str(lines[0]).expect("candidate item");
  assert_eq!(item["derived_sliced_variable"], "Y_chunk");
  assert!(item.get("kind").is_none(), "first line should be an item");
  let summary: serde_json::Value = serde_json::from_str(lines[1]).expect("summary item");
  assert_eq!(summary["kind"], "summary");
  assert!(summary.get("candidates").is_none());

  let _ = fs::remove_dir_all(&root);
}

#[test]
fn py_slice_candidates_ndjson_itemizes_trace_plan_candidates() {
  let root = fixture_root("ndjson-trace-plan");
  write_positive_fixture(&root);

  let (stdout, stderr, exit) = run_raw(
    &root,
    &[
      "--format",
      "ndjson",
      "py-slice-candidates",
      "--trace-plan",
      "positive.py",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nstdout:\n{stdout}");
  let lines = stdout.lines().collect::<Vec<_>>();
  assert_eq!(lines.len(), 2, "stdout:\n{stdout}");
  let item: serde_json::Value = serde_json::from_str(lines[0]).expect("candidate item");
  assert!(item["candidate_id"]
    .as_str()
    .is_some_and(|id| id.starts_with("pyslice-")));
  assert_eq!(
    item["trace_plan"]["watch_variables"],
    serde_json::json!(["Y", "Y_chunk", "Y_norm_squared"])
  );
  let summary: serde_json::Value = serde_json::from_str(lines[1]).expect("summary item");
  assert_eq!(summary["kind"], "summary");
  assert!(summary.get("candidates").is_none());

  let _ = fs::remove_dir_all(&root);
}

#[test]
fn py_slice_candidates_missing_path_reports_structured_diagnostic() {
  let root = fixture_root("missing-path");
  let (envelope, _stderr, exit) = run_json(
    &root,
    &["--format", "json", "py-slice-candidates", "missing.py"],
  );
  assert_eq!(exit, 2);
  assert_eq!(envelope["exit_status"], "error");
  assert!(envelope["warnings"]
    .as_array()
    .expect("warnings")
    .iter()
    .any(|warning| warning["code"] == "PATH_NOT_FOUND"));

  let _ = fs::remove_dir_all(&root);
}
