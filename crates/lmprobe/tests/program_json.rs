use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(root: &Path, args: &[&str], input: Option<&[u8]>) -> Output {
  let mut child = Command::new(env!("CARGO_BIN_EXE_lmprobe"))
    .args(args)
    .current_dir(root)
    .env("XDG_CACHE_HOME", root.join("cache"))
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
  if let Some(input) = input {
    child.stdin.take().unwrap().write_all(input).unwrap();
  }
  child.wait_with_output().unwrap()
}
fn data(out: &Output) -> Value {
  serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
    panic!(
      "stdout {} stderr {}",
      String::from_utf8_lossy(&out.stdout),
      String::from_utf8_lossy(&out.stderr)
    )
  })
}
fn fixture() -> tempfile::TempDir {
  let root = tempfile::tempdir().unwrap();
  std::fs::write(
    root.path().join("one.py"),
    b"def hello():\n    return 1\n\nhello()\n",
  )
  .unwrap();
  std::fs::write(
    root.path().join("Cargo.toml"),
    b"[package]\nname=\"fixture\"\nversion=\"0.1.0\"\n",
  )
  .unwrap();
  root
}
fn check(value: &Value, status: &str, format: &str) {
  assert_eq!(value["schema"], "lmprobe.result.v1");
  assert!(value["source_schema"]
    .as_str()
    .unwrap()
    .starts_with("lmprobe."));
  assert_eq!(value["status"], status);
  assert_eq!(value["payload_format"], format);
  assert!(value["diagnostics"].is_array());
  assert!(value["metadata"].is_object());
}

#[test]
fn rows_lists_hierarchy_and_legacy_json_have_defined_shapes() {
  let root = fixture();
  let out = run(root.path(), &["--json", "find", "\\.py$", "."], None);
  assert!(out.status.success());
  let value = data(&out);
  check(&value, "ok", "rows");
  assert_eq!(value["payload"]["rows"][0]["path"], "./one.py");
  assert!(value["payload"]["columns"]
    .as_array()
    .unwrap()
    .iter()
    .any(|column| column == "path"));
  assert_eq!(value["metadata"]["paths"], json!(["./one.py"]));
  let out = run(root.path(), &["--json", "find", "absent", "."], None);
  assert_eq!(out.status.code(), Some(1));
  check(&data(&out), "no_match", "rows");
  assert_eq!(data(&out)["payload"]["rows"], json!([]));
  let out = run(
    root.path(),
    &["--json", "--exit-zero-on-no-match", "find", "absent", "."],
    None,
  );
  assert!(out.status.success());
  check(&data(&out), "no_match", "rows");
  let out = run(root.path(), &["discover", "--json", "."], None);
  check(&data(&out), "ok", "hierarchical");
  assert_eq!(data(&out)["payload"]["data"][0]["name"], "fixture");
  let out = run(
    root.path(),
    &["--json", "secrets-recipes", "show", "aws/python"],
    None,
  );
  check(&data(&out), "ok", "list");
  assert!(data(&out)["payload"]["items"][0]
    .as_str()
    .unwrap()
    .contains("callsites"));
  let out = run(
    root.path(),
    &["--format", "json", "find", "\\.py$", "."],
    None,
  );
  assert_eq!(data(&out)["schema"], "lmprobe.find.v1");
  assert!(data(&out).get("payload_format").is_none());
}

#[test]
fn json_help_version_parser_runtime_and_special_errors_are_one_document() {
  let root = fixture();
  for args in [
    vec!["--json", "--help"],
    vec!["find", "--json", "--help"],
    vec!["--json", "--version"],
  ] {
    let out = run(root.path(), &args, None);
    assert!(out.status.success());
    check(&data(&out), "ok", "list");
  }
  for args in [
    vec!["--json", "def"],
    vec!["--json", "unknown"],
    vec!["--json", "search", "-p", "foo($X)", "."],
    vec![
      "--json", "find", ".*", ".", "--action", "display", "--force",
    ],
    vec!["--json", "serve"],
    vec!["--json", "find", ".*", ".", "--paths-only"],
    vec!["--json", "--format", "ndjson", "find", ".*", "."],
    vec!["--json", "history", "churn", "."],
    vec!["--json", "secrets-recipes", "show", "nonexistent/python"],
    vec!["--json", "query", "@missing.graphql"],
    vec!["--json", "query", "{ invalidField }"],
  ] {
    let out = run(root.path(), &args, None);
    assert_eq!(out.status.code(), Some(2), "{args:?}");
    check(&data(&out), "error", "hierarchical");
  }
  // A following option must not be mistaken for the missing outfile value.
  let out = run(
    root.path(),
    &["find", ".*", ".", "--outfile", "--json"],
    None,
  );
  assert_eq!(out.status.code(), Some(2));
  check(&data(&out), "error", "hierarchical");
  assert!(!root.path().join("--json").exists());
}

#[test]
fn infile_advanced_documents_aliases_variables_and_literal_content() {
  let root = fixture();
  let source = r#"# Preserve comments, variables, aliases, directives, and fragments.
query Inspect($root: String!, $enabled: Boolean!) {
  first: find(pattern: "\\.py$", path: $root, filter: {kind: FILE, not: {nameRegex: "absent"}}) @include(if: $enabled) { ...Paths }
  second: discover(path: $root) { projects { name } }
}
fragment Paths on FindResult { paths }
"#;
  std::fs::write(root.path().join("expression.graphql"), source).unwrap();
  std::fs::write(
    root.path().join("vars.json"),
    r#"{"root":"missing","enabled":false}"#,
  )
  .unwrap();
  for alias in ["--infile", "--intfile", "-infile", "-intfile"] {
    let out = run(
      root.path(),
      &[
        alias,
        "expression.graphql",
        "--vars-file",
        "vars.json",
        "--var",
        "root=.",
        "--var",
        "enabled=true",
        "--operation-name",
        "Inspect",
      ],
      None,
    );
    assert!(
      out.status.success(),
      "{}",
      String::from_utf8_lossy(&out.stdout)
    );
    check(&data(&out), "ok", "hierarchical");
    assert_eq!(
      data(&out)["payload"]["data"]["first"]["paths"],
      json!(["./one.py"])
    );
  }
  std::fs::write(root.path().join("literal.graphql"), "@missing-indirection").unwrap();
  let out = run(root.path(), &["--infile", "literal.graphql"], None);
  assert_eq!(out.status.code(), Some(2));
  assert_eq!(data(&out)["source_schema"], "lmprobe.query.v1");
  assert!(data(&out)["diagnostics"]
    .as_array()
    .unwrap()
    .iter()
    .any(|d| d["code"] == "GRAPHQL_ERROR"));
  for args in [
    vec!["--infile", "expression.graphql", "query", "{ __typename }"],
    vec!["--infile", "expression.graphql", "find", ".*", "."],
    vec!["--infile", "missing.graphql"],
  ] {
    let out = run(root.path(), &args, None);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(data(&out)["status"], "error");
  }
}

#[test]
fn outfile_is_atomic_quiet_and_does_not_pollute_selection_or_input() {
  let root = fixture();
  let out = run(
    root.path(),
    &["find", ".*", ".", "--outfile", "result.json"],
    None,
  );
  assert!(out.status.success());
  assert!(out.stdout.is_empty());
  let value: Value =
    serde_json::from_slice(&std::fs::read(root.path().join("result.json")).unwrap()).unwrap();
  check(&value, "ok", "rows");
  assert!(!value["payload"]["rows"]
    .as_array()
    .unwrap()
    .iter()
    .any(
      |row| row["path"].as_str().unwrap().contains("lmprobe-output")
        || row["path"] == "./result.json"
    ));
  let out = run(
    root.path(),
    &["find", "absent", ".", "--outfile", "result.json"],
    None,
  );
  assert_eq!(out.status.code(), Some(1));
  assert!(out.stdout.is_empty());
  let value: Value =
    serde_json::from_slice(&std::fs::read(root.path().join("result.json")).unwrap()).unwrap();
  check(&value, "no_match", "rows");
  std::fs::write(root.path().join("query.graphql"), "{ __typename }").unwrap();
  let out = run(
    root.path(),
    &["--infile", "query.graphql", "--outfile", "query.graphql"],
    None,
  );
  assert_eq!(out.status.code(), Some(2));
  check(&data(&out), "error", "hierarchical");
  assert_eq!(
    std::fs::read_to_string(root.path().join("query.graphql")).unwrap(),
    "{ __typename }"
  );
  assert!(!std::fs::read_dir(root.path()).unwrap().any(|entry| entry
    .unwrap()
    .file_name()
    .to_string_lossy()
    .starts_with(".lmprobe-output")));
}

#[test]
fn unsafe_output_destinations_and_selected_delete_targets_are_rejected_before_effects() {
  let root = fixture();
  for destination in ["missing/result.json", "."] {
    let out = run(
      root.path(),
      &[
        "find",
        "\\.py$",
        ".",
        "--action",
        "delete",
        "--force",
        "--outfile",
        destination,
      ],
      None,
    );
    assert_eq!(out.status.code(), Some(2));
    check(&data(&out), "error", "hierarchical");
    assert!(root.path().join("one.py").exists());
  }
  let out = run(
    root.path(),
    &[
      "find",
      "\\.py$",
      ".",
      "--action",
      "delete",
      "--force",
      "--outfile",
      "one.py",
    ],
    None,
  );
  assert_eq!(out.status.code(), Some(2));
  assert_eq!(data(&out)["status"], "error");
  assert!(std::fs::read_to_string(root.path().join("one.py"))
    .unwrap()
    .starts_with("def hello"));
}

#[test]
fn program_find_can_pipe_typed_rows_to_delete_without_losing_identity() {
  let root = fixture();
  let selection = run(root.path(), &["--json", "find", "\\.py$", "."], None);
  let out = run(root.path(), &["--json", "delete"], Some(&selection.stdout));
  assert!(out.status.success());
  check(&data(&out), "ok", "rows");
  assert_eq!(data(&out)["metadata"]["deleted_count"], 0);
  let out = run(
    root.path(),
    &["--json", "delete", "--force"],
    Some(&selection.stdout),
  );
  assert!(out.status.success());
  assert_eq!(data(&out)["metadata"]["deleted_count"], 1);
  assert!(!root.path().join("one.py").exists());
}

#[test]
fn graphql_status_and_diagnostics_are_independent_of_projection_and_quiet() {
  let root = fixture();
  std::fs::write(root.path().join("binary.dat"), [0xff, 0xff]).unwrap();
  for fields in [
    "hits { lineNumber }",
    "hits { lineNumber } partial warnings { code }",
  ] {
    let query = format!("{{ grep(pattern: \"hello\", path: \".\") {{ {fields} }} }}");
    let out = run(root.path(), &["--json", "--quiet", "query", &query], None);
    assert_eq!(out.status.code(), Some(3));
    check(&data(&out), "partial", "hierarchical");
    assert!(data(&out)["diagnostics"]
      .as_array()
      .unwrap()
      .iter()
      .any(|d| d["code"] == "FILE_READ_FAILED"));
  }
}

#[test]
fn outfile_error_help_and_input_document_results_are_written_once() {
  let root = fixture();
  for args in [
    vec!["--json", "def", "--outfile", "error.json"],
    vec!["--json", "--help", "--outfile", "help.json"],
  ] {
    let out = run(root.path(), &args, None);
    assert!(out.stdout.is_empty());
    let target = if args.contains(&"def") {
      "error.json"
    } else {
      "help.json"
    };
    let value: Value =
      serde_json::from_slice(&std::fs::read(root.path().join(target)).unwrap()).unwrap();
    assert_eq!(value["schema"], "lmprobe.result.v1");
    assert_eq!(
      value["status"],
      if target == "error.json" {
        "error"
      } else {
        "ok"
      }
    );
  }
  std::fs::write(
    root.path().join("document.graphql"),
    "{ find(pattern: \"\\\\.py$\", path: \".\") { paths } }",
  )
  .unwrap();
  let out = run(
    root.path(),
    &["--infile", "document.graphql", "--outfile", "result.json"],
    None,
  );
  assert!(out.status.success());
  assert!(out.stdout.is_empty());
  let value: Value =
    serde_json::from_slice(&std::fs::read(root.path().join("result.json")).unwrap()).unwrap();
  assert_eq!(
    value["payload"]["data"]["find"]["paths"],
    json!(["./one.py"])
  );
}

#[cfg(unix)]
#[test]
fn outfile_symlink_hardlink_input_and_readonly_parent_are_rejected() {
  use std::os::unix::fs::PermissionsExt;
  let root = fixture();
  std::fs::write(root.path().join("expression.graphql"), "{ __typename }").unwrap();
  std::os::unix::fs::symlink(
    root.path().join("expression.graphql"),
    root.path().join("alias.json"),
  )
  .unwrap();
  let out = run(
    root.path(),
    &["find", ".*", ".", "--outfile", "alias.json"],
    None,
  );
  assert_eq!(out.status.code(), Some(2));
  assert_eq!(data(&out)["status"], "error");
  assert_eq!(
    std::fs::read_to_string(root.path().join("expression.graphql")).unwrap(),
    "{ __typename }"
  );
  std::fs::hard_link(
    root.path().join("expression.graphql"),
    root.path().join("hardlink.json"),
  )
  .unwrap();
  let out = run(
    root.path(),
    &[
      "--infile",
      "expression.graphql",
      "--outfile",
      "hardlink.json",
    ],
    None,
  );
  assert_eq!(out.status.code(), Some(2));
  assert_eq!(data(&out)["status"], "error");
  let readonly = root.path().join("readonly");
  std::fs::create_dir(&readonly).unwrap();
  std::fs::set_permissions(&readonly, std::fs::Permissions::from_mode(0o555)).unwrap();
  let out = run(
    root.path(),
    &[
      "find",
      "\\.py$",
      ".",
      "--action",
      "delete",
      "--force",
      "--outfile",
      "readonly/result.json",
    ],
    None,
  );
  std::fs::set_permissions(&readonly, std::fs::Permissions::from_mode(0o755)).unwrap();
  assert_eq!(out.status.code(), Some(2));
  assert!(root.path().join("one.py").exists());
}

#[test]
fn outfile_does_not_dirty_git_before_fix_and_overlap_is_refused() {
  let root = fixture();
  let repository = git2::Repository::init(root.path()).unwrap();
  let mut index = repository.index().unwrap();
  index
    .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
    .unwrap();
  let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
  index.write().unwrap();
  let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
  repository
    .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
    .unwrap();
  let out = run(
    root.path(),
    &[
      "fix",
      "-p",
      "hello()",
      "-r",
      "goodbye()",
      "-l",
      "python",
      ".",
      "--apply",
      "--outfile",
      "result.json",
    ],
    None,
  );
  assert!(
    out.status.success(),
    "stdout={} stderr={} outfile={:?}",
    String::from_utf8_lossy(&out.stdout),
    String::from_utf8_lossy(&out.stderr),
    std::fs::read_to_string(root.path().join("result.json"))
  );
  assert!(out.stdout.is_empty());
  assert!(std::fs::read_to_string(root.path().join("one.py"))
    .unwrap()
    .contains("goodbye()"));
  let original = std::fs::read(root.path().join("one.py")).unwrap();
  let out = run(
    root.path(),
    &[
      "fix",
      "-p",
      "goodbye()",
      "-r",
      "hello()",
      "-l",
      "python",
      ".",
      "--apply",
      "--force",
      "--outfile",
      "one.py",
    ],
    None,
  );
  assert_eq!(out.status.code(), Some(2));
  assert_eq!(data(&out)["status"], "error");
  assert_eq!(std::fs::read(root.path().join("one.py")).unwrap(), original);
}

#[test]
fn literal_flag_like_values_do_not_enable_program_mode_or_change_patterns() {
  let root = fixture();
  std::fs::write(root.path().join("flags.txt"), "--json\n-infile\n").unwrap();
  let out = run(
    root.path(),
    &["--format", "json", "grep", "--", "--json", "flags.txt"],
    None,
  );
  assert_eq!(data(&out)["schema"], "lmprobe.grep.v1");
  let out = run(
    root.path(),
    &["--format", "json", "grep", "--", "-infile", "flags.txt"],
    None,
  );
  assert!(out.status.success());
  assert_eq!(data(&out)["hits"][0]["line_content"], "-infile");
}

#[test]
fn late_outfile_failure_keeps_old_file_and_never_reexecutes_operation() {
  let root = tempfile::tempdir().unwrap();
  std::fs::write(root.path().join("app.py"), "print(1)\n").unwrap();
  std::fs::create_dir(root.path().join("out")).unwrap();
  std::fs::write(root.path().join("out/result.report"), "previous result").unwrap();
  let exec="python3 -c 'from pathlib import Path; p=Path(\"counter\"); p.write_text(str(int(p.read_text())+1) if p.exists() else \"1\"); Path(\"out/result.report\").chmod(0o444)'";
  let out = run(
    root.path(),
    &[
      "trace",
      "print($X)",
      ".",
      "--exec",
      exec,
      "--force",
      "--template",
      "print($X)",
      "--outfile",
      "out/result.report",
    ],
    None,
  );
  // Restore permissions for fixture cleanup on platforms that enforce readonly.
  let path = root.path().join("out/result.report");
  #[cfg(unix)]
  let permissions = {
    use std::os::unix::fs::PermissionsExt;
    std::fs::Permissions::from_mode(0o600)
  };
  #[cfg(not(unix))]
  let permissions = {
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(false);
    permissions
  };
  std::fs::set_permissions(&path, permissions).unwrap();
  assert_eq!(out.status.code(), Some(2));
  assert_eq!(data(&out)["status"], "error");
  assert!(data(&out)["diagnostics"]
    .as_array()
    .unwrap()
    .iter()
    .any(|d| d["code"] == "OUTFILE_WRITE_FAILED"));
  assert_eq!(std::fs::read_to_string(&path).unwrap(), "previous result");
  assert_eq!(
    std::fs::read_to_string(root.path().join("counter")).unwrap(),
    "1"
  );
  assert_eq!(
    std::fs::read_to_string(root.path().join("app.py")).unwrap(),
    "print(1)\n"
  );
  assert!(!std::fs::read_dir(root.path().join("out"))
    .unwrap()
    .any(|entry| entry
      .unwrap()
      .file_name()
      .to_string_lossy()
      .starts_with(".lmprobe-output")));
}

#[test]
fn every_command_and_nested_group_accepts_program_json_help() {
  let root = fixture();
  let out = run(root.path(), &["--json", "--help"], None);
  let value = data(&out);
  let help = value["payload"]["items"][0].as_str().unwrap();
  let command_section = help
    .split("Commands:\n")
    .nth(1)
    .unwrap()
    .split("\nOptions:")
    .next()
    .unwrap()
    .split("\nCommon options:")
    .next()
    .unwrap();
  let commands: Vec<_> = command_section
    .lines()
    .filter_map(|line| line.split_whitespace().next())
    .collect();
  for command in commands {
    let args = if command == "help" {
      vec!["--json", "help"]
    } else {
      vec!["--json", command, "--help"]
    };
    let out = run(root.path(), &args, None);
    assert!(
      out.status.success(),
      "{command}: {}",
      String::from_utf8_lossy(&out.stderr)
    );
    check(&data(&out), "ok", "list");
  }
  for group in ["history", "secrets-recipes", "secrets-sources"] {
    let out = run(root.path(), &["--json", group, "--help"], None);
    let value = data(&out);
    let text = value["payload"]["items"][0].as_str().unwrap();
    let section = text
      .split("Commands:\n")
      .nth(1)
      .unwrap()
      .split("\nOptions:")
      .next()
      .unwrap()
      .split("\nCommon options:")
      .next()
      .unwrap();
    for child in section
      .lines()
      .filter_map(|line| line.split_whitespace().next())
      .filter(|name| *name != "help")
    {
      let out = run(root.path(), &["--json", group, child, "--help"], None);
      assert!(out.status.success(), "{group} {child}");
      check(&data(&out), "ok", "list");
    }
  }
}

#[test]
fn parser_and_help_outcomes_cannot_overwrite_explicit_inputs_or_workspace_config() {
  let root = fixture();
  for (name, content) in [
    ("query.graphql", "{ __typename }"),
    ("vars.json", "{}"),
    ("rule.yaml", "kind: call"),
    ("config.toml", "title=\"fixture\""),
  ] {
    std::fs::write(root.path().join(name), content).unwrap();
  }
  for args in [
    vec![
      "--json",
      "query",
      "@query.graphql",
      "--outfile",
      "query.graphql",
      "--unknown",
    ],
    vec![
      "--json",
      "query",
      "@query.graphql",
      "--outfile",
      "query.graphql",
      "--help",
    ],
    vec![
      "--json",
      "query",
      "{ __typename }",
      "--vars-file",
      "vars.json",
      "--outfile",
      "vars.json",
      "--unknown",
    ],
    vec![
      "--json",
      "search",
      "--rule",
      "rule.yaml",
      "--outfile",
      "rule.yaml",
      "--help",
    ],
    vec![
      "--json",
      "secrets",
      "--config",
      "config.toml",
      "--outfile",
      "config.toml",
      "--unknown",
    ],
  ] {
    let out = run(root.path(), &args, None);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(data(&out)["status"], "error");
    assert!(data(&out)["diagnostics"]
      .as_array()
      .unwrap()
      .iter()
      .any(|d| d["code"] == "OUTFILE_SOURCE_CONFLICT"));
  }
  assert_eq!(
    std::fs::read_to_string(root.path().join("query.graphql")).unwrap(),
    "{ __typename }"
  );
  assert_eq!(
    std::fs::read_to_string(root.path().join("vars.json")).unwrap(),
    "{}"
  );
  assert_eq!(
    std::fs::read_to_string(root.path().join("rule.yaml")).unwrap(),
    "kind: call"
  );
  assert_eq!(
    std::fs::read_to_string(root.path().join("config.toml")).unwrap(),
    "title=\"fixture\""
  );
  for text in [
    format!("roots:\n  - path: {}\n", root.path().display()),
    "invalid: [".into(),
  ] {
    std::fs::write(root.path().join("workspace.yaml"), &text).unwrap();
    let out = run(
      root.path(),
      &[
        "--workspace",
        "workspace.yaml",
        "find",
        ".*",
        ".",
        "--outfile",
        "workspace.yaml",
      ],
      None,
    );
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(data(&out)["status"], "error");
    assert_eq!(
      std::fs::read_to_string(root.path().join("workspace.yaml")).unwrap(),
      text
    );
  }
}

#[cfg(unix)]
#[test]
fn untrace_outfile_target_and_audit_aliases_are_refused_before_restoration() {
  let root = fixture();
  let repository = git2::Repository::init(root.path()).unwrap();
  std::fs::write(root.path().join(".gitignore"), "cache/\n").unwrap();
  std::fs::write(
    root.path().join("one.py"),
    "print('hello') # [TRACE-fixture]\n",
  )
  .unwrap();
  let mut index = repository.index().unwrap();
  index
    .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
    .unwrap();
  let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
  index.write().unwrap();
  let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
  repository
    .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
    .unwrap();
  let original = std::fs::read(root.path().join("one.py")).unwrap();
  let query = "mutation { untrace(marker: \"fixture\", path: \".\") { revertedCount } }";
  let out = run(root.path(), &["query", query, "--outfile", "one.py"], None);
  assert_eq!(out.status.code(), Some(2));
  assert_eq!(data(&out)["status"], "error");
  assert_eq!(std::fs::read(root.path().join("one.py")).unwrap(), original);
  // Audit may target a file outside the requested repo root; aliases need the same guard.
  let outside = tempfile::tempdir().unwrap();
  let file = outside.path().join("outside.py");
  std::fs::write(&file, &original).unwrap();
  let alias = outside.path().join("alias.report");
  std::fs::hard_link(&file, &alias).unwrap();
  let audit_dir = root.path().join("cache/lmprobe");
  std::fs::create_dir_all(&audit_dir).unwrap();
  let audit = lmprobe_engine::sensors::persistent_audit_path("fixture");
  // Build the same path under this invocation's isolated XDG cache.
  let name = audit.file_name().unwrap();
  let value = json!({"schema":"lmprobe.trace_audit.v1","marker":"fixture","created_at":"fixture","scope_summary":"fixture","insertions":[{"file_path":file.to_string_lossy(),"line_number":1,"inserted_text":"# [TRACE-fixture]"}]});
  std::fs::write(audit_dir.join(name), serde_json::to_vec(&value).unwrap()).unwrap();
  let out = run(
    root.path(),
    &["query", query, "--outfile", alias.to_str().unwrap()],
    None,
  );
  assert_eq!(out.status.code(), Some(2));
  assert_eq!(data(&out)["status"], "error");
  assert_eq!(std::fs::read(&file).unwrap(), original);
  assert_eq!(std::fs::read(&alias).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn attached_short_input_values_and_untrace_arbitrary_extensions_are_protected() {
  let root = fixture();
  std::fs::write(root.path().join("rule.yaml"), "kind: call").unwrap();
  std::fs::hard_link(
    root.path().join("rule.yaml"),
    root.path().join("alias.json"),
  )
  .unwrap();
  for (option, outfile, end) in [
    ("-rrule.yaml", "rule.yaml", "--help"),
    ("-rrule.yaml", "alias.json", "--unknown"),
    ("-vrrule.yaml", "rule.yaml", "--help"),
  ] {
    let out = run(
      root.path(),
      &["search", option, "--outfile", outfile, end],
      None,
    );
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(data(&out)["status"], "error");
    assert_eq!(
      std::fs::read_to_string(root.path().join("rule.yaml")).unwrap(),
      "kind: call"
    );
    assert_eq!(
      std::fs::read_to_string(root.path().join("alias.json")).unwrap(),
      "kind: call"
    );
  }
  let repository = git2::Repository::init(root.path()).unwrap();
  std::fs::write(root.path().join(".gitignore"), "cache/\n").unwrap();
  let original = "line [TRACE-fixture]\nkeep\n";
  std::fs::write(root.path().join("notes.report"), original).unwrap();
  std::fs::write(root.path().join("extensionless"), original).unwrap();
  let mut index = repository.index().unwrap();
  index
    .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
    .unwrap();
  let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
  index.write().unwrap();
  let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
  repository
    .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
    .unwrap();
  let outside = tempfile::tempdir().unwrap();
  let alias = outside.path().join("alias.report");
  std::fs::hard_link(root.path().join("notes.report"), &alias).unwrap();
  let query = "mutation { untrace(marker: \"fixture\", path: \".\") { revertedCount } }";
  for target in ["notes.report", "extensionless", alias.to_str().unwrap()] {
    let out = run(root.path(), &["query", query, "--outfile", target], None);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(data(&out)["status"], "error");
    assert!(data(&out)["diagnostics"]
      .as_array()
      .unwrap()
      .iter()
      .any(|d| d["code"] == "OUTFILE_SOURCE_CONFLICT"));
    assert_eq!(
      std::fs::read_to_string(root.path().join("notes.report")).unwrap(),
      original
    );
    assert_eq!(
      std::fs::read_to_string(root.path().join("extensionless")).unwrap(),
      original
    );
  }
}

#[test]
fn parser_guards_preserve_equals_and_at_signs_in_literal_input_filenames() {
  let root = fixture();
  for name in ["rule=one.yaml", "@rule.yaml"] {
    std::fs::write(root.path().join(name), "kind: call").unwrap();
  }
  for args in [
    vec![
      "search",
      "-rrule=one.yaml",
      "--outfile",
      "rule=one.yaml",
      "--help",
    ],
    vec![
      "search",
      "-vr=rule=one.yaml",
      "--outfile",
      "rule=one.yaml",
      "--unknown",
    ],
    vec![
      "search",
      "--rule",
      "@rule.yaml",
      "--outfile",
      "@rule.yaml",
      "--help",
    ],
    vec![
      "search",
      "-r@rule.yaml",
      "--outfile",
      "@rule.yaml",
      "--unknown",
    ],
  ] {
    let out = run(root.path(), &args, None);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(data(&out)["status"], "error");
    assert_eq!(
      std::fs::read_to_string(root.path().join("rule=one.yaml")).unwrap(),
      "kind: call"
    );
    assert_eq!(
      std::fs::read_to_string(root.path().join("@rule.yaml")).unwrap(),
      "kind: call"
    );
  }
}
