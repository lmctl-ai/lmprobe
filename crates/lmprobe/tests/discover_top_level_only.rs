use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

fn fixture_dir(name: &str) -> PathBuf {
  let dir = std::env::temp_dir().join(format!("lmprobe-{name}-{}", std::process::id()));
  let _ = fs::remove_dir_all(&dir);
  fs::create_dir_all(&dir).expect("create tmp dir");
  dir
}

fn write_monorepo_fixture(dir: &Path) {
  fs::write(
    dir.join("Cargo.toml"),
    "[package]\nname = \"root\"\nversion = \"0.1.0\"\n",
  )
  .expect("write root Cargo.toml");
  fs::create_dir_all(dir.join("crates/inner")).expect("create nested crate");
  fs::write(
    dir.join("crates/inner/Cargo.toml"),
    "[package]\nname = \"inner\"\nversion = \"0.1.0\"\n",
  )
  .expect("write inner Cargo.toml");
  fs::create_dir_all(dir.join("packages/web")).expect("create nested npm package");
  fs::write(
    dir.join("packages/web/package.json"),
    r#"{"name":"web","version":"1.0.0","dependencies":{"vite":"latest"}}"#,
  )
  .expect("write package.json");
}

fn run_json(args: &[&str]) -> (serde_json::Value, i32) {
  let out = Command::new(binary())
    .args(args)
    .output()
    .expect("spawn lmprobe");
  let stdout = String::from_utf8_lossy(&out.stdout);
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    out.status.code().unwrap_or(-1),
  )
}

fn project_paths(envelope: &serde_json::Value) -> Vec<String> {
  let mut paths: Vec<String> = envelope
    .get("projects")
    .and_then(|v| v.as_array())
    .into_iter()
    .flatten()
    .filter_map(|project| project.get("path").and_then(|v| v.as_str()))
    .map(str::to_string)
    .collect();
  paths.sort();
  paths
}

#[test]
fn discover_top_level_only_suppresses_nested_projects() {
  let dir = fixture_dir("discover-top-level-only");
  write_monorepo_fixture(&dir);
  let dir_arg = dir.to_string_lossy().into_owned();

  let (full, full_exit) = run_json(&["--format", "json", "discover", &dir_arg]);
  let (top, top_exit) = run_json(&["--format", "json", "discover", "--top-level-only", &dir_arg]);
  let _ = fs::remove_dir_all(&dir);

  assert_eq!(full_exit, 0, "full discover:\n{full:#}");
  assert_eq!(top_exit, 0, "top-level discover:\n{top:#}");
  assert_eq!(
    project_paths(&full).len(),
    3,
    "full discovery should keep nested package markers:\n{full:#}",
  );
  assert_eq!(
    project_paths(&top),
    vec![dir_arg],
    "top-level-only should keep only projects with no discovered parent:\n{top:#}",
  );
  assert_eq!(
    top
      .pointer("/projects/0/children")
      .and_then(|v| v.as_array())
      .map(Vec::len)
      .unwrap_or(0),
    0,
    "top-level-only should not leak nested paths through children[]:\n{top:#}",
  );
}

#[test]
fn discover_top_level_only_keeps_sibling_projects_when_no_parent_marker_exists() {
  let dir = fixture_dir("discover-top-level-siblings");
  fs::create_dir_all(dir.join("crates/a")).expect("create crate a");
  fs::create_dir_all(dir.join("crates/b")).expect("create crate b");
  fs::write(
    dir.join("crates/a/Cargo.toml"),
    "[package]\nname = \"a\"\nversion = \"0.1.0\"\n",
  )
  .expect("write crate a");
  fs::write(
    dir.join("crates/b/Cargo.toml"),
    "[package]\nname = \"b\"\nversion = \"0.1.0\"\n",
  )
  .expect("write crate b");
  let dir_arg = dir.to_string_lossy().into_owned();

  let (top, exit) = run_json(&["--format", "json", "discover", "--top-level-only", &dir_arg]);
  let _ = fs::remove_dir_all(&dir);

  assert_eq!(exit, 0, "top-level discover:\n{top:#}");
  assert_eq!(
    project_paths(&top).len(),
    2,
    "siblings with no discovered parent should remain visible:\n{top:#}",
  );
}

#[test]
fn query_discover_exposes_top_level_only() {
  let dir = fixture_dir("discover-top-level-query");
  write_monorepo_fixture(&dir);
  let dir_arg = dir.to_string_lossy().into_owned();
  let path_literal = serde_json::to_string(&dir_arg).expect("GraphQL string literal");
  let query = format!(
    r#"{{ discover(path: {path_literal}, topLevelOnly: true) {{ projects {{ path children }} exitStatus }} }}"#
  );

  let (envelope, exit) = run_json(&["--format", "json", "query", &query]);
  let _ = fs::remove_dir_all(&dir);

  assert_eq!(exit, 0, "query discover:\n{envelope:#}");
  assert_eq!(
    envelope
      .pointer("/data/discover/projects")
      .and_then(|v| v.as_array())
      .map(Vec::len),
    Some(1),
    "GraphQL topLevelOnly should match CLI filtering:\n{envelope:#}",
  );
}

#[test]
fn discover_go_subpackage_uses_enclosing_module_context() {
  let dir = fixture_dir("discover-go-subpackage");
  fs::write(
    dir.join("go.mod"),
    "module example.com/root\n\ngo 1.22\n\nrequire golang.org/x/sync v0.1.0\n",
  )
  .expect("write go.mod");
  fs::create_dir_all(dir.join("server/api/membership")).expect("create nested go package");
  fs::write(
    dir.join("server/api/membership/member.go"),
    "package membership\n\nfunc MemberID() string { return \"id\" }\n",
  )
  .expect("write go file");
  let pkg_arg = dir
    .join("server/api/membership")
    .to_string_lossy()
    .into_owned();

  let (envelope, exit) = run_json(&["--format", "json", "discover", &pkg_arg]);
  let _ = fs::remove_dir_all(&dir);

  assert_eq!(exit, 0, "discover go subpackage:\n{envelope:#}");
  let projects = envelope["projects"].as_array().expect("projects");
  assert_eq!(projects.len(), 1, "discover go subpackage:\n{envelope:#}");
  assert_eq!(projects[0]["project_type"], "Go/Package(membership)");
  assert_eq!(
    projects[0]["name"],
    "example.com/root/server/api/membership"
  );
  assert_eq!(projects[0]["path"], pkg_arg);
  assert_eq!(projects[0]["dependencies_count"], 1);
}
