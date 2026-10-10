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
    "lmprobe-multiple-positional-paths-{}-{suffix}",
    std::process::id()
  ));
  let _ = std::fs::remove_dir_all(&root);
  std::fs::create_dir_all(root.join("dir1")).expect("mkdir dir1");
  std::fs::create_dir_all(root.join("dir2")).expect("mkdir dir2");
  std::fs::write(root.join("dir1/file1.txt"), "needle\n").expect("write file1");
  std::fs::write(root.join("dir2/file2.txt"), "needle\n").expect("write file2");
  Fixture { root }
}

fn run(root: &Path, args: &[&str]) -> (String, i32) {
  let out = Command::new(binary())
    .args(args)
    .current_dir(root)
    .output()
    .expect("spawn lmprobe");
  (
    String::from_utf8_lossy(&out.stderr).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

#[test]
fn grep_extra_positional_path_suggests_repeatable_path_flag() {
  let fix = fixture();
  let (stderr, exit) = run(
    &fix.root,
    &["--format", "json", "grep", "needle", "dir1", "dir2"],
  );

  assert_eq!(exit, 2, "stderr:\n{stderr}");
  assert!(
    stderr.contains("unexpected argument") && stderr.contains("dir2"),
    "should keep clap's unexpected argument context:\n{stderr}"
  );
  assert!(
    stderr.contains("--path <PATH>") && stderr.contains("lmprobe grep <PATTERN>"),
    "should explain the repeatable path form for grep:\n{stderr}"
  );
}

#[test]
fn find_extra_positional_path_suggests_repeatable_path_flag() {
  let fix = fixture();
  let (stderr, exit) = run(
    &fix.root,
    &["--format", "json", "find", "file", "dir1", "dir2"],
  );

  assert_eq!(exit, 2, "stderr:\n{stderr}");
  assert!(
    stderr.contains("unexpected argument") && stderr.contains("dir2"),
    "should keep clap's unexpected argument context:\n{stderr}"
  );
  assert!(
    stderr.contains("--path <PATH>") && stderr.contains("lmprobe find <REGEX>"),
    "should explain the repeatable path form for find:\n{stderr}"
  );
}

#[test]
fn search_arguments_named_grep_do_not_get_grep_path_hint() {
  let fix = fixture();
  let (stderr, exit) = run(
    &fix.root,
    &["--format", "json", "search", "grep", "foo", "bar", "baz"],
  );

  assert_eq!(exit, 2, "stderr:\n{stderr}");
  assert!(
    stderr.contains("unexpected argument"),
    "should still be a clap parse error:\n{stderr}"
  );
  assert!(
    !stderr.contains("`grep` accepts at most one positional PATH")
      && !stderr.contains("lmprobe grep <PATTERN>"),
    "search arguments should not be diagnosed as the grep subcommand:\n{stderr}"
  );
}

#[test]
fn invalid_option_value_does_not_get_multiple_path_hint() {
  let fix = fixture();
  let (stderr, exit) = run(
    &fix.root,
    &[
      "--format",
      "json",
      "grep",
      "needle",
      "dir1",
      "--threads",
      "nope",
    ],
  );

  assert_eq!(exit, 2, "stderr:\n{stderr}");
  assert!(
    stderr.contains("invalid value") || stderr.contains("invalid digit"),
    "should keep the invalid option value diagnostic:\n{stderr}"
  );
  assert!(
    !stderr.contains("for multiple roots use repeatable")
      && !stderr.contains("lmprobe grep <PATTERN> --path"),
    "invalid option values should not get the multiple path hint:\n{stderr}"
  );
}
