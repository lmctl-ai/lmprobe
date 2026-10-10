use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

fn graphql_path_literal(path: &Path) -> String {
  serde_json::to_string(&path.to_string_lossy()).expect("GraphQL string literal")
}

fn fixture_dir(name: &str) -> PathBuf {
  let dir = std::env::temp_dir().join(format!("lmprobe-{name}-{}", std::process::id()));
  let _ = fs::remove_dir_all(&dir);
  fs::create_dir_all(&dir).expect("create tmp dir");
  dir
}

fn write_fixture(dir: &Path) {
  fs::write(
    dir.join("sample.txt"),
    "this line has TODO marker\nthis line has FIXME marker\nliteral a|b separator\n",
  )
  .expect("write fixture");
}

fn write_find_fixture(dir: &Path) {
  for name in ["foo.txt", "bar.txt"] {
    fs::write(dir.join(name), "").expect("write fixture path");
  }
  #[cfg(not(windows))]
  fs::write(dir.join("a|b.txt"), "").expect("write fixture path");
}

fn run_json(args: &[&str]) -> (serde_json::Value, String, i32) {
  let out = Command::new(binary())
    .args(args)
    .output()
    .expect("spawn lmprobe");
  let stdout = String::from_utf8_lossy(&out.stdout);
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    String::from_utf8_lossy(&out.stderr).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

fn warning_codes(envelope: &serde_json::Value) -> Vec<&str> {
  envelope
    .get("warnings")
    .and_then(|v| v.as_array())
    .into_iter()
    .flatten()
    .filter_map(|warning| warning.get("code").and_then(|v| v.as_str()))
    .collect()
}

fn has_literal_pipe_warning(envelope: &serde_json::Value) -> bool {
  warning_codes(envelope).contains(&"REGEX_LITERAL_PIPE_NO_MATCH")
}

#[test]
fn grep_warns_when_posix_bre_pipe_misuse_has_no_hits() {
  let dir = fixture_dir("regex-literal-pipe-grep-no-hit");
  write_fixture(&dir);
  let dir_arg = dir.to_string_lossy().into_owned();

  let (envelope, stderr, exit) = run_json(&["--format", "json", "grep", r"TODO\|FIXME", &dir_arg]);
  let _ = fs::remove_dir_all(&dir);

  assert_eq!(exit, 1, "envelope:\n{envelope:#}\nstderr:\n{stderr}");
  assert!(
    has_literal_pipe_warning(&envelope),
    "envelope:\n{envelope:#}"
  );
  assert!(
    envelope
      .get("warnings")
      .and_then(|v| v.as_array())
      .is_some_and(|warnings| warnings.iter().any(|warning| {
        warning
          .get("message")
          .and_then(|v| v.as_str())
          .is_some_and(|message| message.contains("Rust regex uses bare `|` for alternation"))
          && warning
            .get("hint")
            .and_then(|v| v.as_str())
            .is_some_and(|hint| hint.contains(r"replacing `\|` with `|`"))
      })),
    "warning should explain the regex dialect mismatch:\n{envelope:#}",
  );
  assert!(
    stderr.is_empty(),
    "explicit JSON non-error warnings should stay envelope-only by default:\n{stderr}",
  );
}

#[test]
fn grep_correct_alternation_matches_without_literal_pipe_warning() {
  let dir = fixture_dir("regex-literal-pipe-grep-alt");
  write_fixture(&dir);
  let dir_arg = dir.to_string_lossy().into_owned();

  let (envelope, stderr, exit) = run_json(&["--format", "json", "grep", "TODO|FIXME", &dir_arg]);
  let _ = fs::remove_dir_all(&dir);

  assert_eq!(exit, 0, "envelope:\n{envelope:#}\nstderr:\n{stderr}");
  assert_eq!(
    envelope
      .get("hits")
      .and_then(|v| v.as_array())
      .map(Vec::len),
    Some(2),
    "envelope:\n{envelope:#}",
  );
  assert!(
    !has_literal_pipe_warning(&envelope),
    "envelope:\n{envelope:#}"
  );
}

#[test]
fn grep_literal_pipe_match_stays_quiet() {
  let dir = fixture_dir("regex-literal-pipe-grep-literal");
  write_fixture(&dir);
  let dir_arg = dir.to_string_lossy().into_owned();

  let (envelope, stderr, exit) = run_json(&["--format", "json", "grep", r"a\|b", &dir_arg]);
  let _ = fs::remove_dir_all(&dir);

  assert_eq!(exit, 0, "envelope:\n{envelope:#}\nstderr:\n{stderr}");
  assert!(
    !has_literal_pipe_warning(&envelope),
    "envelope:\n{envelope:#}"
  );
}

#[test]
fn find_warns_when_posix_bre_pipe_misuse_has_no_hits() {
  let dir = fixture_dir("regex-literal-pipe-find-no-hit");
  write_fixture(&dir);
  let dir_arg = dir.to_string_lossy().into_owned();

  let (envelope, stderr, exit) = run_json(&["--format", "json", "find", r"TODO\|FIXME", &dir_arg]);
  let _ = fs::remove_dir_all(&dir);

  assert_eq!(exit, 1, "envelope:\n{envelope:#}\nstderr:\n{stderr}");
  assert!(
    has_literal_pipe_warning(&envelope),
    "envelope:\n{envelope:#}"
  );
}

#[test]
fn find_correct_alternation_matches_without_literal_pipe_warning() {
  let dir = fixture_dir("regex-literal-pipe-find-alt");
  write_find_fixture(&dir);
  let dir_arg = dir.to_string_lossy().into_owned();

  let (envelope, stderr, exit) =
    run_json(&["--format", "json", "find", r"foo\.txt|bar\.txt", &dir_arg]);
  let _ = fs::remove_dir_all(&dir);

  assert_eq!(exit, 0, "envelope:\n{envelope:#}\nstderr:\n{stderr}");
  assert_eq!(
    envelope
      .get("paths")
      .and_then(|v| v.as_array())
      .map(Vec::len),
    Some(2),
    "envelope:\n{envelope:#}",
  );
  assert!(
    !has_literal_pipe_warning(&envelope),
    "envelope:\n{envelope:#}"
  );
}

#[test]
fn find_literal_pipe_match_stays_quiet() {
  if cfg!(windows) {
    // Windows file names cannot contain `|`; grep covers literal-pipe regex
    // matching on every platform.
    return;
  }

  let dir = fixture_dir("regex-literal-pipe-find-literal");
  write_find_fixture(&dir);
  let dir_arg = dir.to_string_lossy().into_owned();

  let (envelope, stderr, exit) = run_json(&["--format", "json", "find", r"a\|b", &dir_arg]);
  let _ = fs::remove_dir_all(&dir);

  assert_eq!(exit, 0, "envelope:\n{envelope:#}\nstderr:\n{stderr}");
  assert!(
    !has_literal_pipe_warning(&envelope),
    "envelope:\n{envelope:#}"
  );
}

#[test]
fn query_grep_carries_literal_pipe_warning() {
  let dir = fixture_dir("regex-literal-pipe-query");
  write_fixture(&dir);
  let dir_arg = graphql_path_literal(&dir);
  let query = format!(
    r#"{{ grep(pattern: "TODO\\|FIXME", path: {dir_arg}) {{ warnings {{ code message hint }} exitStatus }} }}"#
  );

  let (envelope, stderr, exit) = run_json(&["--format", "json", "query", &query]);
  let _ = fs::remove_dir_all(&dir);

  assert_eq!(exit, 1, "envelope:\n{envelope:#}\nstderr:\n{stderr}");
  assert_eq!(
    envelope
      .pointer("/data/grep/exitStatus")
      .and_then(|v| v.as_str()),
    Some("NO_MATCH"),
    "envelope:\n{envelope:#}",
  );
  assert!(
    envelope
      .pointer("/data/grep/warnings")
      .and_then(|v| v.as_array())
      .is_some_and(|warnings| warnings.iter().any(|warning| {
        warning.get("code").and_then(|v| v.as_str()) == Some("REGEX_LITERAL_PIPE_NO_MATCH")
      })),
    "GraphQL grep warnings should include regex foot-gun diagnostic:\n{envelope:#}",
  );
}
