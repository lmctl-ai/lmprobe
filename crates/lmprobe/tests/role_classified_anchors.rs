use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

fn fixture_root(name: &str) -> PathBuf {
  let root = std::env::temp_dir().join(format!(
    "lmprobe-role-classified-{name}-{}",
    std::process::id()
  ));
  let _ = fs::remove_dir_all(&root);
  fs::create_dir_all(&root).expect("create fixture root");
  root
}

fn write_fixture(root: &Path) {
  let source = r#"import os
from pkg import thing

base_config_key = "seed"
read_value = base_config_key
consume(base_config_key)
config = {"text_config": base_config_key}


def produce(flag, obj, data, items):
    if flag:
        return base_config_key
    attr_value = obj.name
    dict_read = data["text_config"]
    index_value = items[0]
    call_value = consume(base_config_key)
    return call_value


foo = foo
foo(foo)
"#;
  fs::write(root.join("roles.py"), source).expect("write roles.py");
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

fn hits(envelope: &serde_json::Value) -> &[serde_json::Value] {
  envelope["hits"].as_array().expect("hits")
}

fn role_for_line<'a>(envelope: &'a serde_json::Value, needle: &str) -> &'a str {
  hits(envelope)
    .iter()
    .find(|hit| {
      hit["line_content"]
        .as_str()
        .is_some_and(|line| line.contains(needle))
    })
    .unwrap_or_else(|| panic!("missing hit line containing {needle:?}:\n{envelope:#}"))["role"]
    .as_str()
    .expect("role")
}

#[test]
fn grep_role_classifies_python_anchor_lines() {
  let root = fixture_root("grep");
  write_fixture(&root);

  let (base, stderr, exit) = run_json(
    &root,
    &[
      "--format",
      "json",
      "grep",
      "--role",
      "base_config_key",
      "roles.py",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{base}");
  assert_eq!(role_for_line(&base, "base_config_key ="), "assignment");
  assert_eq!(role_for_line(&base, "read_value ="), "read");
  assert_eq!(role_for_line(&base, "consume(base_config_key)"), "argument");
  assert_eq!(role_for_line(&base, "return base_config_key"), "return");

  let (call, _stderr, exit) = run_json(
    &root,
    &["--format", "json", "grep", "--role", "consume", "roles.py"],
  );
  assert_eq!(exit, 0);
  assert_eq!(role_for_line(&call, "consume(base_config_key)"), "call");

  let (misc, _stderr, exit) = run_json(
    &root,
    &["--format", "json", "grep", "--role", "flag", "roles.py"],
  );
  assert_eq!(exit, 0);
  assert_eq!(role_for_line(&misc, "if flag"), "condition");

  let (attr, _stderr, exit) = run_json(
    &root,
    &["--format", "json", "grep", "--role", "name", "roles.py"],
  );
  assert_eq!(exit, 0);
  assert_eq!(role_for_line(&attr, "obj.name"), "attribute_access");

  let (dict_key, _stderr, exit) = run_json(
    &root,
    &[
      "--format",
      "json",
      "grep",
      "--role",
      "\"text_config\"",
      "roles.py",
    ],
  );
  assert_eq!(exit, 0);
  assert_eq!(role_for_line(&dict_key, "config = {"), "dict_key");

  let (index, _stderr, exit) = run_json(
    &root,
    &["--format", "json", "grep", "--role", "items", "roles.py"],
  );
  assert_eq!(exit, 0);
  assert_eq!(role_for_line(&index, "items[0]"), "index_access");

  let (import, _stderr, exit) = run_json(
    &root,
    &["--format", "json", "grep", "--role", "os", "roles.py"],
  );
  assert_eq!(exit, 0);
  assert_eq!(role_for_line(&import, "import os"), "import");

  let _ = fs::remove_dir_all(&root);
}

#[test]
fn grep_role_omits_role_by_default_and_composes_with_expansions() {
  let root = fixture_root("compose");
  write_fixture(&root);

  let (default_env, _stderr, exit) = run_json(
    &root,
    &["--format", "json", "grep", "base_config_key", "roles.py"],
  );
  assert_eq!(exit, 0);
  assert!(
    hits(&default_env)
      .iter()
      .all(|hit| hit.get("role").is_none()),
    "default grep output should stay compact:\n{default_env:#}"
  );

  let (expanded, _stderr, exit) = run_json(
    &root,
    &[
      "--format",
      "json",
      "-C",
      "1",
      "grep",
      "--role",
      "--parents",
      "--body",
      "base_config_key",
      "roles.py",
    ],
  );
  assert_eq!(exit, 0);
  let hit = hits(&expanded)
    .iter()
    .find(|hit| hit["line_content"] == "return base_config_key")
    .expect("return hit");
  assert_eq!(hit["role"], "return");
  assert!(
    hit.get("parents").is_some(),
    "parents should compose:\n{hit:#}"
  );
  assert!(hit.get("body").is_some(), "body should compose:\n{hit:#}");
  assert!(
    hit.get("before_context").is_some() || hit.get("after_context").is_some(),
    "context lines should compose:\n{hit:#}"
  );

  let _ = fs::remove_dir_all(&root);
}

#[test]
fn ref_role_uses_same_additive_role_field() {
  let root = fixture_root("ref");
  write_fixture(&root);

  let (envelope, _stderr, exit) = run_json(
    &root,
    &[
      "--format",
      "json",
      "ref",
      "--role",
      "base_config_key",
      "roles.py",
    ],
  );
  assert_eq!(exit, 0);
  assert_eq!(role_for_line(&envelope, "base_config_key ="), "assignment");
  assert_eq!(role_for_line(&envelope, "return base_config_key"), "return");

  let _ = fs::remove_dir_all(&root);
}

#[test]
fn grep_role_uses_unknown_for_conflicting_multiple_matches_on_one_line() {
  let root = fixture_root("ambiguous");
  write_fixture(&root);

  let (envelope, _stderr, exit) = run_json(
    &root,
    &["--format", "json", "grep", "--role", "foo", "roles.py"],
  );
  assert_eq!(exit, 0);
  assert_eq!(role_for_line(&envelope, "foo = foo"), "unknown");
  assert_eq!(role_for_line(&envelope, "foo(foo)"), "unknown");

  let _ = fs::remove_dir_all(&root);
}
