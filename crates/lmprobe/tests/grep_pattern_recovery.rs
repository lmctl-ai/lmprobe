use std::path::PathBuf;
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

fn run(args: &[&str]) -> (String, String, i32) {
  let out = Command::new(binary())
    .args(args)
    .output()
    .expect("spawn lmprobe");
  (
    String::from_utf8_lossy(&out.stdout).into_owned(),
    String::from_utf8_lossy(&out.stderr).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

fn assert_grep_pattern_hint(stdout: &str, stderr: &str, exit: i32) {
  assert!(stdout.is_empty(), "unexpected stdout:\n{stdout}");
  assert_eq!(exit, 2, "stderr:\n{stderr}");
  assert!(
    stderr
      .contains("`grep` keeps PATTERN positional in v1; rerun as `lmprobe grep <PATTERN> [PATH]`"),
    "stderr should explain the positional grep form\n{stderr}",
  );
}

#[test]
fn grep_line_number_flags_are_accepted_as_compatibility_noop() {
  let suffix = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_nanos())
    .unwrap_or(0);
  let fixture = std::env::temp_dir().join(format!("lmprobe-grep-n-compat-{suffix}"));
  let _ = std::fs::remove_dir_all(&fixture);
  std::fs::create_dir_all(&fixture).expect("create fixture dir");
  let target = fixture.join("case.txt");
  std::fs::write(&target, "needle\n").expect("write fixture file");

  let pattern = "needle";
  let target = target.to_str().expect("utf8");
  let (stdout, stderr, exit) = run(&["grep", "-n", pattern, target]);
  let (long_stdout, long_stderr, long_exit) = run(&["grep", "--line-number", pattern, target]);

  let _ = std::fs::remove_dir_all(&fixture);
  assert_eq!(
    exit, 0,
    "grep -n should match as a compatibility no-op\nstderr:\n{stderr}"
  );
  assert_eq!(
    long_exit, 0,
    "grep --line-number should match as a compatibility no-op\nstderr:\n{long_stderr}"
  );
  assert!(
    !stderr.contains("unexpected argument"),
    "line-number compat should not trigger an argument parse error: {stderr}",
  );
  assert!(
    !long_stderr.contains("unexpected argument"),
    "long line-number compat should not trigger an argument parse error: {long_stderr}",
  );
  assert!(
    stdout.contains("\"line_number\":1"),
    "grep -n should preserve normal grep output with line_number:\n{stdout}",
  );
  assert!(
    long_stdout.contains("\"line_number\":1"),
    "grep --line-number should preserve normal grep output with line_number:\n{long_stdout}",
  );
  assert!(
    !stdout.contains("`grep` keeps PATTERN positional"),
    "compatibility acceptance should not emit pattern hint:\n{stdout}",
  );
  assert!(
    !long_stdout.contains("`grep` keeps PATTERN positional"),
    "long compatibility acceptance should not emit pattern hint:\n{long_stdout}",
  );
}

#[test]
fn grep_short_pattern_flag_suggests_positional_form() {
  let (stdout, stderr, exit) = run(&["grep", "-p", "TODO", "."]);

  assert_grep_pattern_hint(&stdout, &stderr, exit);
  assert!(
    stderr.contains("unexpected argument '-p' found"),
    "stderr should keep clap's parse error\n{stderr}",
  );
}

#[test]
fn grep_long_pattern_flag_suggests_positional_form() {
  let (stdout, stderr, exit) = run(&["grep", "--pattern", "TODO", "."]);

  assert_grep_pattern_hint(&stdout, &stderr, exit);
  assert!(
    stderr.contains("unexpected argument '--pattern' found"),
    "stderr should keep clap's parse error\n{stderr}",
  );
}

#[test]
fn grep_pattern_flag_after_global_flags_gets_hint() {
  let (stdout, stderr, exit) = run(&["--format", "json", "grep", "-p", "TODO", "."]);

  assert_grep_pattern_hint(&stdout, &stderr, exit);
  assert!(
    stderr.contains("unexpected argument '-p' found"),
    "stderr should keep clap's parse error\n{stderr}",
  );
}

#[test]
fn grep_pattern_flag_after_grep_flags_gets_hint() {
  let (stdout, stderr, exit) = run(&["grep", "--path", ".", "-p", "TODO"]);

  assert_grep_pattern_hint(&stdout, &stderr, exit);
  assert!(
    stderr.contains("unexpected argument '-p' found"),
    "stderr should keep clap's parse error\n{stderr}",
  );
}

#[test]
fn grep_pattern_equals_form_gets_hint() {
  let (stdout, stderr, exit) = run(&["grep", "--pattern=TODO", "."]);

  assert_grep_pattern_hint(&stdout, &stderr, exit);
  assert!(
    stderr.contains("unexpected argument '--pattern' found")
      || stderr.contains("unexpected argument '--pattern=TODO' found"),
    "stderr should keep clap's parse error\n{stderr}",
  );
}

#[test]
fn grep_pattern_after_double_dash_does_not_get_hint() {
  let (stdout, stderr, exit) = run(&["grep", "TODO", "--", "--pattern=TODO", "extra"]);

  assert!(stdout.is_empty(), "unexpected stdout:\n{stdout}");
  assert_eq!(exit, 2, "stderr:\n{stderr}");
  assert!(
    !stderr
      .contains("`grep` keeps PATTERN positional in v1; rerun as `lmprobe grep <PATTERN> [PATH]`"),
    "stderr should not hint for args after --\n{stderr}",
  );
}

// `find` mirrors `grep`'s positional-pattern convention but historically
// lacked this same recovery hint (dogfood finding, 2026-09): typing
// `lmprobe find -p <PATTERN>` produced a bare clap parse error with no
// guidance, unlike the equivalent `grep` mistake.
fn assert_find_pattern_hint(stdout: &str, stderr: &str, exit: i32) {
  assert!(stdout.is_empty(), "unexpected stdout:\n{stdout}");
  assert_eq!(exit, 2, "stderr:\n{stderr}");
  assert!(
    stderr.contains(
      "`find` keeps PATTERN positional, like `grep`/`search`; rerun as `lmprobe find <PATTERN> [PATH]`"
    ),
    "stderr should explain the positional find form\n{stderr}",
  );
}

#[test]
fn find_short_pattern_flag_suggests_positional_form() {
  let (stdout, stderr, exit) = run(&["find", "-p", "\\.rs$", "."]);

  assert_find_pattern_hint(&stdout, &stderr, exit);
  assert!(
    stderr.contains("unexpected argument '-p' found"),
    "stderr should keep clap's parse error\n{stderr}",
  );
}

#[test]
fn find_long_pattern_flag_suggests_positional_form() {
  let (stdout, stderr, exit) = run(&["find", "--pattern", "\\.rs$", "."]);

  assert_find_pattern_hint(&stdout, &stderr, exit);
  assert!(
    stderr.contains("unexpected argument '--pattern' found"),
    "stderr should keep clap's parse error\n{stderr}",
  );
}

#[test]
fn find_pattern_after_double_dash_does_not_get_hint() {
  let (stdout, stderr, exit) = run(&["find", "\\.rs$", "--", "--pattern=x", "extra"]);

  assert!(stdout.is_empty(), "unexpected stdout:\n{stdout}");
  assert_eq!(exit, 2, "stderr:\n{stderr}");
  assert!(
    !stderr.contains("`find` keeps PATTERN positional"),
    "stderr should not hint for args after --\n{stderr}",
  );
}
