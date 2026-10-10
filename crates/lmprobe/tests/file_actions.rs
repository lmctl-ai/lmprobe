use serde_json::Value;
use std::fs::{File, FileTimes};
use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, SystemTime};

fn run(root: &std::path::Path, args: &[&str], input: Option<&[u8]>) -> Output {
  let mut command = Command::new(env!("CARGO_BIN_EXE_lmprobe"));
  command
    .args(args)
    .current_dir(root)
    .env("XDG_CACHE_HOME", root.join("audit-cache"))
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .stdin(Stdio::piped());
  let mut child = command.spawn().unwrap();
  if let Some(input) = input {
    child.stdin.take().unwrap().write_all(input).unwrap();
  }
  child.wait_with_output().unwrap()
}
fn json(output: &Output) -> Value {
  serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
    panic!(
      "stdout: {} stderr: {}",
      String::from_utf8_lossy(&output.stdout),
      String::from_utf8_lossy(&output.stderr)
    )
  })
}
fn fixture() -> tempfile::TempDir {
  let root = tempfile::tempdir().unwrap();
  for (name, age) in [("z-old.lmctl", 8), ("a-new.lmctl", 0), ("other.txt", 9)] {
    let path = root.path().join(name);
    std::fs::write(&path, b"hello").unwrap();
    File::options()
      .write(true)
      .open(&path)
      .unwrap()
      .set_times(
        FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(age * 86400)),
      )
      .unwrap();
  }
  root
}
fn find(root: &std::path::Path, extras: &[&str]) -> Output {
  let mut args = vec!["--format", "json", "find", "\\.lmctl$", "."];
  args.extend(extras);
  run(root, &args, None)
}

#[test]
fn display_metadata_and_column_selection() {
  let root = fixture();
  let out = run(
    root.path(),
    &["--format", "markdown", "find", "\\.lmctl$", "."],
    None,
  );
  assert!(out.status.success());
  for line in String::from_utf8(out.stdout).unwrap().lines() {
    let columns: Vec<_> = line.split('\t').collect();
    assert_eq!(columns.len(), 3);
    assert!(chrono::DateTime::parse_from_rfc3339(columns[1]).is_ok());
    assert_eq!(columns[2], "5");
  }
  let out = run(
    root.path(),
    &[
      "--format",
      "markdown",
      "find",
      "\\.lmctl$",
      ".",
      "--display",
      "name",
    ],
    None,
  );
  assert!(!String::from_utf8(out.stdout).unwrap().contains('\t'));
  let data = json(&find(root.path(), &[]));
  assert_eq!(data["paths"].as_array().unwrap().len(), 2);
  assert_eq!(data["files"][0]["size_bytes"], 5);
  assert!(std::path::Path::new(data["files"][0]["absolute_path"].as_str().unwrap()).is_absolute());
}

#[test]
fn age_filter_is_applied_before_limit_and_invalid_ages_are_errors() {
  let root = fixture();
  let out = find(root.path(), &["--older-than", "1w", "--limit", "1"]);
  assert!(out.status.success());
  assert_eq!(json(&out)["paths"], serde_json::json!(["./z-old.lmctl"]));
  for age in ["0w", "-1w", "week", "é", "99999999999999999999999w"] {
    let out = find(
      root.path(),
      &["--older-than", age, "--action", "delete", "--force"],
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(root.path().join("z-old.lmctl").exists());
  }
}

#[test]
fn preview_then_force_deletes_only_old_matches_and_audits() {
  let root = fixture();
  let preview = find(root.path(), &["--older-than", "1w", "--action", "delete"]);
  assert!(preview.status.success());
  assert_eq!(json(&preview)["action"]["mode"], "preview");
  assert_eq!(json(&preview)["action"]["deleted_count"], 0);
  assert!(!root.path().join("audit-cache").exists());
  assert!(root.path().join("z-old.lmctl").exists());
  let executed = find(
    root.path(),
    &["--older-than", "1w", "--action", "delete", "--force"],
  );
  assert!(
    executed.status.success(),
    "{}",
    String::from_utf8_lossy(&executed.stderr)
  );
  let data = json(&executed);
  assert_eq!(data["action"]["deleted_count"], 1);
  assert!(std::path::Path::new(data["action"]["audit_log_path"].as_str().unwrap()).is_file());
  assert!(!root.path().join("z-old.lmctl").exists());
  assert!(root.path().join("a-new.lmctl").exists());
  assert!(root.path().join("other.txt").exists());
}

#[test]
fn force_without_delete_action_never_mutates() {
  let root = fixture();
  for args in [vec!["--force"], vec!["--action", "display", "--force"]] {
    assert!(!find(root.path(), &args).status.success());
    assert!(root.path().join("z-old.lmctl").exists());
  }
}

#[test]
fn piped_json_and_ndjson_preview_and_execution() {
  for format in ["json", "ndjson"] {
    let root = fixture();
    let selection = run(
      root.path(),
      &[
        "--format",
        format,
        "find",
        "\\.lmctl$",
        ".",
        "--older-than",
        "1w",
      ],
      None,
    );
    assert!(selection.status.success());
    let preview = run(
      root.path(),
      &["--format", "json", "delete"],
      Some(&selection.stdout),
    );
    assert!(preview.status.success());
    assert_eq!(json(&preview)["mode"], "preview");
    assert!(root.path().join("z-old.lmctl").exists());
    let action = run(
      root.path(),
      &["--format", "json", "delete", "--force"],
      Some(&selection.stdout),
    );
    assert!(
      action.status.success(),
      "{}",
      String::from_utf8_lossy(&action.stderr)
    );
    assert_eq!(json(&action)["deleted_count"], 1);
    assert!(root.path().join("a-new.lmctl").exists());
  }
}

#[test]
fn stale_or_malformed_or_partial_selection_deletes_nothing() {
  let root = fixture();
  let selection = find(root.path(), &[]);
  std::fs::write(root.path().join("z-old.lmctl"), b"changed").unwrap();
  let action = run(
    root.path(),
    &["--format", "json", "delete", "--force"],
    Some(&selection.stdout),
  );
  assert_eq!(action.status.code(), Some(2));
  assert_eq!(json(&action)["deleted_count"], 0);
  assert!(root.path().join("a-new.lmctl").exists());
  let mut malformed = find(root.path(), &[]).stdout;
  malformed.extend_from_slice(b"\n{ invalid");
  let action = run(
    root.path(),
    &["--format", "json", "delete", "--force"],
    Some(&malformed),
  );
  assert_eq!(action.status.code(), Some(2));
  assert!(root.path().join("a-new.lmctl").exists());
  let mut partial = json(&find(root.path(), &[]));
  partial["partial"] = Value::Bool(true);
  let action = run(
    root.path(),
    &["--format", "json", "delete", "--force"],
    Some(&serde_json::to_vec(&partial).unwrap()),
  );
  assert_eq!(action.status.code(), Some(2));
  assert!(root.path().join("z-old.lmctl").exists());
}

#[test]
fn nul_pipeline_handles_spaces_and_newlines_and_duplicates() {
  let root = tempfile::tempdir().unwrap();
  for name in ["space name.log", "line\nbreak.log"] {
    std::fs::write(root.path().join(name), b"x").unwrap();
  }
  let selection = run(
    root.path(),
    &[
      "find",
      "\\.log$",
      "--path",
      ".",
      "--path",
      ".",
      "--paths-only",
      "--null",
    ],
    None,
  );
  assert!(selection.status.success());
  let deleted = run(
    root.path(),
    &["--format", "json", "delete", "--null", "--force"],
    Some(&selection.stdout),
  );
  assert!(deleted.status.success());
  assert_eq!(json(&deleted)["deleted_count"], 2);
}

#[test]
fn directories_are_never_deleted_and_whole_batch_is_validated() {
  let root = fixture();
  std::fs::create_dir(root.path().join("dir.lmctl")).unwrap();
  let out = find(root.path(), &["--action", "delete", "--force"]);
  assert_eq!(out.status.code(), Some(2));
  assert_eq!(json(&out)["action"]["deleted_count"], 0);
  assert!(root.path().join("z-old.lmctl").exists());
  assert!(root.path().join("dir.lmctl").is_dir());
}

#[cfg(unix)]
#[test]
fn symlinks_and_replaced_files_are_refused() {
  let root = fixture();
  std::os::unix::fs::symlink(
    root.path().join("other.txt"),
    root.path().join("link.lmctl"),
  )
  .unwrap();
  let selection = find(root.path(), &[]);
  let out = run(
    root.path(),
    &["--format", "json", "delete", "--force"],
    Some(&selection.stdout),
  );
  assert_eq!(out.status.code(), Some(2));
  assert!(root.path().join("other.txt").exists());
  std::fs::remove_file(root.path().join("link.lmctl")).unwrap();
  let selection = find(root.path(), &[]);
  let old = root.path().join("z-old.lmctl");
  let timestamp = std::fs::metadata(&old).unwrap().modified().unwrap();
  std::fs::remove_file(&old).unwrap();
  std::fs::write(&old, b"hello").unwrap();
  File::options()
    .write(true)
    .open(&old)
    .unwrap()
    .set_times(FileTimes::new().set_modified(timestamp))
    .unwrap();
  let out = run(
    root.path(),
    &["--format", "json", "delete", "--force"],
    Some(&selection.stdout),
  );
  assert_eq!(out.status.code(), Some(2));
  assert!(old.exists());
}

#[test]
fn stale_ndjson_uses_original_summary_snapshot_and_audit_failure_prevents_deletion() {
  let root = fixture();
  let selection = run(
    root.path(),
    &["--format", "ndjson", "find", "\\.lmctl$", "."],
    None,
  );
  std::fs::write(root.path().join("z-old.lmctl"), b"updated").unwrap();
  let action = run(
    root.path(),
    &["--format", "json", "delete", "--force"],
    Some(&selection.stdout),
  );
  assert_eq!(action.status.code(), Some(2));
  assert!(root.path().join("a-new.lmctl").exists());
  // Prevent XDG directory creation without touching selected files.
  std::fs::write(root.path().join("audit-cache"), b"not a directory").unwrap();
  let action = find(root.path(), &["--action", "delete", "--force"]);
  assert_eq!(action.status.code(), Some(2));
  assert_eq!(json(&action)["action"]["deleted_count"], 0);
  assert!(root.path().join("z-old.lmctl").exists());
}

#[test]
fn forced_actions_refuse_workspace_mode() {
  let root = fixture();
  let config = root.path().join("workspace.yaml");
  std::fs::write(
    &config,
    format!("roots:\n  - path: {}\n", root.path().display()),
  )
  .unwrap();
  let selection = find(root.path(), &[]);
  let action = run(
    root.path(),
    &["--workspace", config.to_str().unwrap(), "delete", "--force"],
    Some(&selection.stdout),
  );
  assert!(!action.status.success());
  assert!(root.path().join("a-new.lmctl").exists());
  let action = find(
    root.path(),
    &[
      "--workspace",
      config.to_str().unwrap(),
      "--action",
      "delete",
      "--force",
    ],
  );
  assert!(!action.status.success());
  assert!(root.path().join("z-old.lmctl").exists());
}

#[test]
fn truncated_ndjson_and_mismatched_footer_delete_nothing() {
  let root = fixture();
  let selection = run(
    root.path(),
    &["--format", "ndjson", "find", "\\.lmctl$", "."],
    None,
  );
  let text = String::from_utf8(selection.stdout).unwrap();
  let rows: Vec<_> = text.lines().collect();
  let truncated = rows[..rows.len() - 1].join("\n");
  let action = run(
    root.path(),
    &["--format", "json", "delete", "--force"],
    Some(truncated.as_bytes()),
  );
  assert_eq!(action.status.code(), Some(2));
  assert!(root.path().join("a-new.lmctl").exists());
  let mismatched = rows[1..].join("\n");
  let action = run(
    root.path(),
    &["--format", "json", "delete", "--force"],
    Some(mismatched.as_bytes()),
  );
  assert_eq!(action.status.code(), Some(2));
  assert!(root.path().join("z-old.lmctl").exists());
}

#[cfg(unix)]
#[test]
fn symlink_parent_is_preserved_in_snapshot_and_refused() {
  let root = fixture();
  std::fs::create_dir(root.path().join("real")).unwrap();
  std::fs::write(root.path().join("real/target.log"), b"x").unwrap();
  std::os::unix::fs::symlink(root.path().join("real"), root.path().join("alias")).unwrap();
  let selection = run(
    root.path(),
    &["--format", "json", "find", "\\.log$", "alias/target.log"],
    None,
  );
  assert!(selection.status.success());
  assert_eq!(json(&selection)["files"][0]["has_symlink_parent"], true);
  let action = run(
    root.path(),
    &["--format", "json", "delete", "--force"],
    Some(&selection.stdout),
  );
  assert_eq!(action.status.code(), Some(2));
  assert!(root.path().join("real/target.log").exists());
  let direct = run(
    root.path(),
    &[
      "--format",
      "json",
      "find",
      "\\.log$",
      "alias/target.log",
      "--action",
      "delete",
      "--force",
    ],
    None,
  );
  assert_eq!(direct.status.code(), Some(2));
  assert!(root.path().join("real/target.log").exists());
  let raw = run(
    root.path(),
    &["--format", "json", "delete", "--force"],
    Some(b"alias/target.log\n"),
  );
  assert_eq!(raw.status.code(), Some(2));
  assert!(root.path().join("real/target.log").exists());
}

#[cfg(unix)]
#[test]
fn quick_project_prescan_errors_cannot_trigger_deletion() {
  use std::os::unix::fs::{MetadataExt, PermissionsExt};
  let root = fixture();
  // Root bypasses directory permissions; this gate needs an unprivileged user.
  if std::fs::metadata(root.path()).unwrap().uid() == 0 {
    return;
  }
  let project = root.path().join("project");
  std::fs::create_dir(&project).unwrap();
  std::fs::write(project.join("package.json"), b"{\"name\":\"test-project\"}").unwrap();
  std::fs::write(project.join("deletable.lmctl"), b"x").unwrap();
  let locked = root.path().join("locked");
  std::fs::create_dir(&locked).unwrap();
  std::fs::write(locked.join("hidden.lmctl"), b"x").unwrap();
  std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
  let action = find(
    root.path(),
    &["--quick-projects", "--action", "delete", "--force"],
  );
  std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
  assert!(!action.status.success());
  assert!(root.path().join("z-old.lmctl").exists());
  assert!(locked.join("hidden.lmctl").exists());
  assert!(project.join("deletable.lmctl").exists());
}

#[test]
fn graphql_age_filter_and_invalid_duration_match_the_cli() {
  let root = fixture();
  let query = r#"{ find(pattern: "\\.lmctl$", path: ".", olderThan: "1w") { paths files { path sizeBytes modifiedAt } exitStatus } }"#;
  let out = run(root.path(), &["--format", "json", "query", query], None);
  assert!(
    out.status.success(),
    "{}",
    String::from_utf8_lossy(&out.stdout)
  );
  assert_eq!(
    json(&out)["data"]["find"]["paths"],
    serde_json::json!(["./z-old.lmctl"])
  );
  let invalid = query.replace("1w", "0w");
  let out = run(root.path(), &["--format", "json", "query", &invalid], None);
  assert_eq!(out.status.code(), Some(2));
}

#[test]
fn invalid_completion_fields_are_not_treated_as_success() {
  let root = fixture();
  for (field, value) in [
    ("partial", serde_json::json!("false")),
    ("exit_status", serde_json::json!("unknown")),
  ] {
    let mut selection = json(&find(root.path(), &[]));
    selection[field] = value;
    let out = run(
      root.path(),
      &["--format", "json", "delete", "--force"],
      Some(&serde_json::to_vec(&selection).unwrap()),
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(root.path().join("a-new.lmctl").exists());
  }
}

#[cfg(unix)]
#[test]
fn eligible_duplicate_cannot_hide_a_symlink_alias() {
  let root = fixture();
  std::fs::create_dir(root.path().join("real")).unwrap();
  std::fs::write(root.path().join("real/target.log"), b"x").unwrap();
  std::os::unix::fs::symlink(root.path().join("real"), root.path().join("alias")).unwrap();
  let out = run(
    root.path(),
    &["--format", "json", "delete", "--force"],
    Some(b"real/target.log\nalias/target.log\n"),
  );
  assert_eq!(out.status.code(), Some(2));
  assert!(root.path().join("real/target.log").exists());
  let regular = json(&run(
    root.path(),
    &["--format", "json", "find", "\\.log$", "real/target.log"],
    None,
  ));
  let alias = json(&run(
    root.path(),
    &["--format", "json", "find", "\\.log$", "alias/target.log"],
    None,
  ));
  let selection = serde_json::json!({"schema":"lmprobe.find.v1","exit_status":"matched",
    "paths":["real/target.log","alias/target.log"],
    "files":[regular["files"][0],alias["files"][0]]});
  let out = run(
    root.path(),
    &["--format", "json", "delete", "--force"],
    Some(&serde_json::to_vec(&selection).unwrap()),
  );
  assert_eq!(out.status.code(), Some(2));
  assert!(root.path().join("real/target.log").exists());
}

#[cfg(unix)]
#[test]
fn mixed_delete_outcomes_report_partial_without_hiding_success() {
  use std::os::unix::fs::{MetadataExt, PermissionsExt};
  let root = tempfile::tempdir().unwrap();
  if std::fs::metadata(root.path()).unwrap().uid() == 0 {
    return;
  }
  let locked = root.path().join("locked");
  std::fs::create_dir(&locked).unwrap();
  std::fs::write(locked.join("keep.log"), b"x").unwrap();
  std::fs::write(root.path().join("remove.log"), b"x").unwrap();
  let selection = run(
    root.path(),
    &["--format", "json", "find", "\\.log$", "."],
    None,
  );
  std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
  let action = run(
    root.path(),
    &["--format", "json", "delete", "--force"],
    Some(&selection.stdout),
  );
  std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
  assert_eq!(action.status.code(), Some(3));
  assert_eq!(json(&action)["deleted_count"], 1);
  assert_eq!(json(&action)["partial"], true);
  assert!(locked.join("keep.log").exists());
  assert!(!root.path().join("remove.log").exists());
}
