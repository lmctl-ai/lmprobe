use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn run(root: &Path, args: &[&str]) -> Output {
  Command::new(env!("CARGO_BIN_EXE_lmprobe"))
    .args(args)
    .current_dir(root)
    .env("XDG_CACHE_HOME", root.join("cache"))
    .output()
    .unwrap()
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
fn repo(root: &Path, name: &str, url: &str) -> PathBuf {
  let path = root.join(name);
  std::fs::create_dir_all(&path).unwrap();
  let repository = git2::Repository::init(&path).unwrap();
  repository.remote("origin", url).unwrap();
  std::fs::create_dir(path.join("sibling-dir")).unwrap();
  std::fs::write(path.join("sibling-dir/widget.txt"), b"must not traverse").unwrap();
  std::fs::write(path.join("sibling-file.txt"), b"must not inspect").unwrap();
  path
}
fn fixture() -> tempfile::TempDir {
  let root = tempfile::tempdir().unwrap();
  repo(
    root.path(),
    "unexpected-local-name",
    "https://github.com/acme/widget.git",
  );
  let wrong = repo(root.path(), "widget", "https://github.com/acme/other.git");
  repo(
    &wrong,
    "nested-target",
    "https://github.com/acme/widget.git",
  );
  repo(
    root.path(),
    "group/different-local-name",
    "git@github.com:acme/widget.git",
  );
  repo(
    root.path(),
    "wrong-owner",
    "https://github.com/another/widget.git",
  );
  root
}
const FILTER: &str =
  r#"{"kind":"directory","git_remote":{"host":"github.com","owner":"acme","repo":"widget"}}"#;
const PRUNE: &str = r#"{"has_entry":".git"}"#;

#[test]
fn remote_repo_identity_is_independent_of_directory_name_and_pruning_is_independent_of_filter() {
  let root = fixture();
  let out = run(
    root.path(),
    &[
      "--format", "json", "find", ".*", ".", "--filter", FILTER, "--prune", PRUNE,
    ],
  );
  assert!(
    out.status.success(),
    "{}",
    String::from_utf8_lossy(&out.stderr)
  );
  let paths: Vec<String> = data(&out)["paths"]
    .as_array()
    .unwrap()
    .iter()
    .map(|p| p.as_str().unwrap().replace('\\', "/"))
    .collect();
  assert_eq!(paths.len(), 2, "{paths:?}");
  assert!(paths.contains(&"./unexpected-local-name".into()));
  assert!(paths.contains(&"./group/different-local-name".into()));
  assert!(!paths.iter().any(|p| p.contains("nested-target")));
  assert!(!paths.iter().any(|p| p == "./widget"));
  // Nothing inside .git or beside .git inside the checkout is inspected/listed.
  let out = run(
    root.path(),
    &[
      "--format",
      "json",
      "find",
      ".*",
      ".",
      "--prune",
      PRUNE,
      "--no-ignore",
      "--no-default-excludes",
    ],
  );
  assert!(out.status.success());
  let paths = data(&out)["paths"].as_array().unwrap().clone();
  assert!(!paths.iter().any(|p| p.as_str().unwrap().contains("/.git")
    || p.as_str().unwrap().contains("sibling-")
    || p.as_str().unwrap().contains("nested-target")));
}

#[test]
fn graphql_expresses_the_same_remote_selection_and_boundary() {
  let root = fixture();
  let query = r#"{ find(pattern: ".*", path: ".", filter: {kind: DIRECTORY, gitRemote: {host: "github.com", owner: "acme", repo: "widget"}}, prune: {hasEntry: ".git"}) {paths exitStatus warnings {code}} }"#;
  let out = run(root.path(), &["--format", "json", "query", query]);
  assert!(
    out.status.success(),
    "{}",
    String::from_utf8_lossy(&out.stdout)
  );
  assert_eq!(
    data(&out)["data"]["find"]["paths"]
      .as_array()
      .unwrap()
      .len(),
    2
  );
}

#[test]
fn worktree_git_file_and_arbitrary_marker_and_boolean_rules() {
  let root = tempfile::tempdir().unwrap();
  let original = repo(root.path(), "source", "https://github.com/acme/widget.git");
  let repository = git2::Repository::open(&original).unwrap();
  let mut index = repository.index().unwrap();
  index
    .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
    .unwrap();
  let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
  let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
  repository
    .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
    .unwrap();
  let worktree_path = root.path().join("unrelated-worktree-name");
  repository
    .worktree("fixture-worktree", &worktree_path, None)
    .unwrap();
  assert!(worktree_path.join(".git").is_file());
  let out = run(
    root.path(),
    &[
      "--format", "json", "find", ".*", ".", "--filter", FILTER, "--prune", PRUNE,
    ],
  );
  assert!(
    out.status.success(),
    "{}",
    String::from_utf8_lossy(&out.stderr)
  );
  assert_eq!(data(&out)["paths"].as_array().unwrap().len(), 2);
  let package = root.path().join("package");
  std::fs::create_dir(&package).unwrap();
  std::fs::write(package.join("package.json"), b"{}").unwrap();
  std::fs::write(package.join("child.txt"), b"not traversed").unwrap();
  let filter = r#"{"all":[{"kind":"directory"},{"any":[{"has_entry":".git"},{"has_entry":"package.json"}]},{"not":{"name_regex":"^source$"}}]}"#;
  let prune = r#"{"any":[{"has_entry":".git"},{"has_entry":"package.json"}]}"#;
  let out = run(
    root.path(),
    &[
      "--format", "json", "find", ".*", ".", "--filter", filter, "--prune", prune,
    ],
  );
  assert!(out.status.success());
  assert_eq!(data(&out)["paths"].as_array().unwrap().len(), 2);
}

#[test]
fn invalid_predicates_are_errors_and_no_rule_keeps_existing_recursion() {
  let root = fixture();
  for predicate in [
    r#"{"has_entry":"../.git"}"#,
    r#"{"has_entry":"a/.git"}"#,
    r#"{"typo":true}"#,
    r#"{"name_regex":"["}"#,
  ] {
    let out = run(root.path(), &["find", ".*", ".", "--prune", predicate]);
    assert_eq!(out.status.code(), Some(2));
  }
  let out = run(
    root.path(),
    &["find", ".*", ".", "--prune", PRUNE, "--quick-projects"],
  );
  assert_eq!(out.status.code(), Some(2));
  let out = run(
    root.path(),
    &["--format", "json", "find", "sibling-file\\.txt$", "."],
  );
  assert!(out.status.success());
  assert_eq!(data(&out)["paths"].as_array().unwrap().len(), 5);
}

#[cfg(unix)]
#[test]
fn ignored_counting_and_primary_walk_do_not_enter_unreadable_checkout_children() {
  use std::os::unix::fs::{MetadataExt, PermissionsExt};
  let root = fixture();
  if std::fs::metadata(root.path()).unwrap().uid() == 0 {
    return;
  }
  let checkout = root.path().join("unexpected-local-name");
  std::fs::write(checkout.join(".gitignore"), b"sibling-dir/\n").unwrap();
  let child = checkout.join("sibling-dir");
  std::fs::set_permissions(&child, std::fs::Permissions::from_mode(0o000)).unwrap();
  let out = run(
    root.path(),
    &[
      "--format", "json", "find", ".*", ".", "--filter", FILTER, "--prune", PRUNE,
    ],
  );
  std::fs::set_permissions(&child, std::fs::Permissions::from_mode(0o700)).unwrap();
  assert!(
    out.status.success(),
    "{}",
    String::from_utf8_lossy(&out.stderr)
  );
  assert_eq!(data(&out)["paths"].as_array().unwrap().len(), 2);
  assert_ne!(data(&out)["partial"], json!(true));
  assert!(!data(&out)["warnings"]
    .as_array()
    .unwrap()
    .iter()
    .any(|w| w["code"] == "WALK_ERROR" || w["code"] == "GITIGNORE_PATHS_EXCLUDED"));
}

#[test]
fn explicit_current_directory_root_uses_its_actual_basename_for_pruning() {
  let root = tempfile::tempdir().unwrap();
  let vendor = root.path().join("vendor");
  std::fs::create_dir(&vendor).unwrap();
  std::fs::write(vendor.join("inside.txt"), b"must not visit").unwrap();
  let out = run(
    &vendor,
    &[
      "--format",
      "json",
      "find",
      ".*",
      ".",
      "--prune",
      r#"{"name_regex":"^vendor$"}"#,
    ],
  );
  assert!(out.status.success());
  assert_eq!(data(&out)["paths"], json!(["."]));
}

#[test]
fn literal_local_remote_names_do_not_create_near_name_matches() {
  let root = tempfile::tempdir().unwrap();
  repo(root.path(), "first", "/srv/mirrors/widget#archive.git");
  repo(root.path(), "second", "./mirrors/team:archive/widget.git");
  let filter = r#"{"git_remote":{"repo":"widget"}}"#;
  let out = run(
    root.path(),
    &[
      "--format", "json", "find", ".*", ".", "--filter", filter, "--prune", PRUNE,
    ],
  );
  assert!(out.status.success());
  assert_eq!(data(&out)["paths"], json!(["./second"]));
  let filter = r#"{"git_remote":{"repo":"widget#archive"}}"#;
  let out = run(
    root.path(),
    &[
      "--format", "json", "find", ".*", ".", "--filter", filter, "--prune", PRUNE,
    ],
  );
  assert!(out.status.success());
  assert_eq!(data(&out)["paths"], json!(["./first"]));
}

#[test]
fn parent_root_spelling_uses_actual_name_for_pruning() {
  let root = tempfile::tempdir().unwrap();
  let parent = root.path().join("container");
  let cwd = parent.join("cwd");
  std::fs::create_dir_all(&cwd).unwrap();
  std::fs::write(parent.join("inside.txt"), b"must not visit").unwrap();
  let out = run(
    &cwd,
    &[
      "--format",
      "json",
      "find",
      ".*",
      "..",
      "--prune",
      r#"{"name_regex":"^container$"}"#,
    ],
  );
  assert!(out.status.success());
  assert_eq!(data(&out)["paths"], json!([".."]));
}
