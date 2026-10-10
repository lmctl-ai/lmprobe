use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

fn run(root: &Path, args: &[&str]) -> Output {
  Command::new(env!("CARGO_BIN_EXE_lmprobe"))
    .args(args)
    .arg("--json")
    .current_dir(root)
    .env("XDG_CACHE_HOME", root.join("cache"))
    .output()
    .unwrap()
}
fn data(output: &Output) -> Value {
  serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
    panic!(
      "{} {}",
      String::from_utf8_lossy(&output.stdout),
      String::from_utf8_lossy(&output.stderr)
    )
  })
}
fn write(root: &Path, name: &str, text: &str) {
  std::fs::write(root.join(name), text).unwrap();
}

#[test]
fn literal_whole_word_preview_and_force_apply_preserve_dollars_and_permissions() {
  let dir = tempfile::tempdir().unwrap();
  let root = dir.path();
  write(root, "one.txt", "old old_extra # old\n'old'\n");
  let args = ["edit", "one.txt", "-p", "old", "-r", "$1", "--word"];
  let output = run(root, &args);
  assert!(output.status.success());
  let value = data(&output);
  assert_eq!(value["payload_format"], "rows");
  assert_eq!(
    value["payload"]["rows"][0]["after"],
    "$1 old_extra # $1\n'$1'\n"
  );
  assert_eq!(
    std::fs::read_to_string(root.join("one.txt")).unwrap(),
    "old old_extra # old\n'old'\n"
  );
  let before = std::fs::metadata(root.join("one.txt"))
    .unwrap()
    .permissions();
  let mut apply = args.to_vec();
  apply.extend(["--apply", "--force"]);
  let output = run(root, &apply);
  assert!(output.status.success());
  let value = data(&output);
  assert_eq!(value["payload"]["rows"][0]["applied"], true);
  assert_eq!(
    std::fs::metadata(root.join("one.txt"))
      .unwrap()
      .permissions(),
    before
  );
  let audit = Path::new(value["metadata"]["audit_path"].as_str().unwrap());
  let recovery: Value = serde_json::from_slice(&std::fs::read(audit).unwrap()).unwrap();
  assert_eq!(
    recovery["files"][0]["before"],
    "old old_extra # old\n'old'\n"
  );
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
      std::fs::metadata(audit).unwrap().permissions().mode() & 0o777,
      0o600
    );
  }
}

#[test]
fn regex_capture_context_multiline_and_first() {
  let dir = tempfile::tempdir().unwrap();
  let root = dir.path();
  write(root, "one.txt", "foo1\nfoo2\nfoo3\n");
  let output = run(
    root,
    &[
      "edit",
      "one.txt",
      "--regex",
      "-p",
      r"(?m)^foo([0-9])$",
      "-r",
      "value=$1",
      "--first",
    ],
  );
  assert!(output.status.success());
  assert_eq!(
    data(&output)["payload"]["rows"][0]["after"],
    "value=1\nfoo2\nfoo3\n"
  );
  write(root, "one.txt", "xfoo yfoo");
  let output = run(
    root,
    &["edit", "one.txt", "--regex", "-p", r"\B(foo)", "-r", "$1!"],
  );
  assert!(output.status.success());
  assert_eq!(data(&output)["payload"]["rows"][0]["after"], "xfoo! yfoo!");
}

#[test]
fn scripts_sequence_line_operations_and_preserve_crlf() {
  let dir = tempfile::tempdir().unwrap();
  let root = dir.path();
  write(root, "one.txt", "one\r\ntwo\r\nthree");
  write(
    root,
    "edits.json",
    r#"[{"op":"insert_before","line":2,"text":"middle"},{"op":"delete_lines","start":3,"end":3},{"op":"replace_lines","start":3,"end":3,"text":"tail"},{"op":"prepend","text":"P"},{"op":"append","text":"S"}]"#,
  );
  let output = run(
    root,
    &[
      "edit",
      "one.txt",
      "--script",
      "edits.json",
      "--apply",
      "--force",
    ],
  );
  assert!(
    output.status.success(),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );
  assert_eq!(
    std::fs::read_to_string(root.join("one.txt")).unwrap(),
    "Pone\r\nmiddle\r\ntailS"
  );
}

#[test]
fn invalid_batch_is_preflighted_before_any_writes() {
  let dir = tempfile::tempdir().unwrap();
  let root = dir.path();
  write(root, "one.txt", "one\ntwo\n");
  write(root, "two.txt", "one\n");
  let output = run(
    root,
    &[
      "edit",
      "one.txt",
      "two.txt",
      "--delete-lines",
      "2",
      "--apply",
      "--force",
    ],
  );
  assert_eq!(output.status.code(), Some(2));
  assert_eq!(data(&output)["status"], "error");
  assert_eq!(
    std::fs::read_to_string(root.join("one.txt")).unwrap(),
    "one\ntwo\n"
  );
  assert!(!root.join("cache").exists());
}

#[test]
fn non_git_apply_requires_force_and_no_match_has_exit_one() {
  let dir = tempfile::tempdir().unwrap();
  let root = dir.path();
  write(root, "one.txt", "old");
  let output = run(
    root,
    &["edit", "one.txt", "-p", "old", "-r", "new", "--apply"],
  );
  assert_eq!(output.status.code(), Some(2));
  assert!(String::from_utf8_lossy(&output.stdout).contains("EDIT_NON_GIT"));
  let output = run(root, &["edit", "one.txt", "-p", "absent", "-r", "new"]);
  assert_eq!(output.status.code(), Some(1));
  assert_eq!(data(&output)["status"], "no_match");
  let output = run(
    root,
    &[
      "edit",
      "one.txt",
      "-p",
      "absent",
      "-r",
      "new",
      "--exit-zero-on-no-match",
    ],
  );
  assert!(output.status.success());
}

#[test]
fn invalid_utf8_and_recovery_creation_failure_leave_sources_intact() {
  let dir = tempfile::tempdir().unwrap();
  let root = dir.path();
  write(root, "one.txt", "old");
  std::fs::write(root.join("binary.txt"), [0xff, 0xfe]).unwrap();
  let output = run(
    root,
    &[
      "edit",
      "one.txt",
      "binary.txt",
      "-p",
      "old",
      "-r",
      "new",
      "--apply",
      "--force",
    ],
  );
  assert_eq!(output.status.code(), Some(2));
  assert_eq!(
    std::fs::read_to_string(root.join("one.txt")).unwrap(),
    "old"
  );
  write(root, "cache", "not a directory");
  let output = run(
    root,
    &[
      "edit", "one.txt", "-p", "old", "-r", "new", "--apply", "--force",
    ],
  );
  assert_eq!(output.status.code(), Some(2));
  assert_eq!(
    std::fs::read_to_string(root.join("one.txt")).unwrap(),
    "old"
  );
}

#[test]
fn stdin_script_and_outfile_keep_stdout_quiet() {
  use std::io::Write;
  use std::process::Stdio;
  let dir = tempfile::tempdir().unwrap();
  let root = dir.path();
  write(root, "one.txt", "");
  let mut child = Command::new(env!("CARGO_BIN_EXE_lmprobe"))
    .args([
      "edit",
      "one.txt",
      "--script",
      "-",
      "--outfile",
      "result.json",
      "--apply",
      "--force",
    ])
    .current_dir(root)
    .env("XDG_CACHE_HOME", root.join("cache"))
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
  child
    .stdin
    .take()
    .unwrap()
    .write_all(br#"[{"op":"append","text":"new"}]"#)
    .unwrap();
  let output = child.wait_with_output().unwrap();
  assert!(output.status.success());
  assert!(output.stdout.is_empty());
  let result: Value =
    serde_json::from_slice(&std::fs::read(root.join("result.json")).unwrap()).unwrap();
  assert_eq!(result["status"], "ok");
  assert_eq!(
    std::fs::read_to_string(root.join("one.txt")).unwrap(),
    "new"
  );
}

#[test]
fn outfile_conflicts_sources_scripts_and_unknown_scripts_are_refused() {
  let dir = tempfile::tempdir().unwrap();
  let root = dir.path();
  write(root, "one.txt", "old");
  let output = run(
    root,
    &[
      "edit",
      "one.txt",
      "-p",
      "old",
      "-r",
      "new",
      "--apply",
      "--force",
      "--outfile",
      "one.txt",
    ],
  );
  assert_eq!(output.status.code(), Some(2));
  assert_eq!(
    std::fs::read_to_string(root.join("one.txt")).unwrap(),
    "old"
  );
  write(root, "edits.json", r#"[{"op":"append","text":"x"}]"#);
  let output = run(
    root,
    &[
      "edit",
      "one.txt",
      "--script",
      "edits.json",
      "--apply",
      "--force",
      "--outfile",
      "edits.json",
    ],
  );
  assert_eq!(output.status.code(), Some(2));
  assert_eq!(
    std::fs::read_to_string(root.join("one.txt")).unwrap(),
    "old"
  );
  write(
    root,
    "edits.json",
    r#"[{"op":"append","text":"x","typo":true}]"#,
  );
  let output = run(root, &["edit", "one.txt", "--script", "edits.json"]);
  assert_eq!(output.status.code(), Some(2));
  let bad = std::fs::read(root.join("edits.json")).unwrap();
  let output = run(
    root,
    &[
      "edit",
      "one.txt",
      "--script",
      "edits.json",
      "--outfile",
      "edits.json",
    ],
  );
  assert_eq!(output.status.code(), Some(2));
  assert_eq!(std::fs::read(root.join("edits.json")).unwrap(), bad);
}

#[test]
fn clean_git_apply_uses_live_source_and_dirty_git_refuses() {
  let dir = tempfile::tempdir().unwrap();
  let root = dir.path();
  write(root, "one.txt", "old");
  let repo = git2::Repository::init(root).unwrap();
  let mut index = repo.index().unwrap();
  index.add_path(Path::new("one.txt")).unwrap();
  index.write().unwrap();
  let oid = index.write_tree().unwrap();
  let tree = repo.find_tree(oid).unwrap();
  let sig = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
  repo
    .commit(Some("HEAD"), &sig, &sig, "fixture", &tree, &[])
    .unwrap();
  let output = run(
    root,
    &["edit", "one.txt", "-p", "old", "-r", "new", "--apply"],
  );
  assert!(output.status.success());
  let output = run(
    root,
    &["edit", "one.txt", "-p", "new", "-r", "next", "--apply"],
  );
  assert_eq!(output.status.code(), Some(2));
  assert_eq!(
    std::fs::read_to_string(root.join("one.txt")).unwrap(),
    "new"
  );
  let output = run(
    root,
    &[
      "edit", "one.txt", "-p", "new", "-r", "next", "--apply", "--force",
    ],
  );
  assert!(output.status.success());
  assert_eq!(
    std::fs::read_to_string(root.join("one.txt")).unwrap(),
    "next"
  );
}

#[cfg(unix)]
#[test]
fn symlink_and_hardlink_aliases_are_refused_before_edits() {
  let dir = tempfile::tempdir().unwrap();
  let root = dir.path();
  write(root, "one.txt", "old");
  std::os::unix::fs::symlink(root.join("one.txt"), root.join("link.txt")).unwrap();
  let output = run(
    root,
    &[
      "edit", "one.txt", "link.txt", "-p", "old", "-r", "new", "--apply", "--force",
    ],
  );
  assert_eq!(output.status.code(), Some(2));
  std::fs::hard_link(root.join("one.txt"), root.join("hard.txt")).unwrap();
  let output = run(
    root,
    &[
      "edit", "one.txt", "-p", "old", "-r", "new", "--apply", "--force",
    ],
  );
  assert_eq!(output.status.code(), Some(2));
  assert_eq!(
    std::fs::read_to_string(root.join("one.txt")).unwrap(),
    "old"
  );
}
