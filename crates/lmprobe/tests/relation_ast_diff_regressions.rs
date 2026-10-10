use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

struct Fixture {
  root: PathBuf,
}

impl Drop for Fixture {
  fn drop(&mut self) {
    let _ = fs::remove_dir_all(&self.root);
  }
}

fn fixture_root(prefix: &str) -> Fixture {
  let suffix = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_nanos())
    .unwrap_or(0);
  let root = std::env::temp_dir().join(format!("lmprobe-{prefix}-{}-{suffix}", std::process::id()));
  let _ = fs::remove_dir_all(&root);
  fs::create_dir_all(&root).expect("create fixture root");
  Fixture { root }
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

fn git(repo: &Path, args: &[&str]) {
  let status = Command::new("git")
    .args(["-C", repo.to_str().unwrap()])
    .args(args)
    .status()
    .expect("run git");
  assert!(status.success(), "git command failed: {args:?}");
}

fn git_commit(repo: &Path, message: &str) {
  let status = Command::new("git")
    .args(["-C", repo.to_str().unwrap()])
    .args([
      "-c",
      "user.name=lmprobe",
      "-c",
      "user.email=lmprobe@example.test",
    ])
    .args(["commit", "-am", message])
    .status()
    .expect("git commit");
  assert!(status.success(), "git commit failed");
}

#[test]
fn typescript_overrides_report_not_implemented_without_parse_failure() {
  let fix = fixture_root("ts-overrides");
  fs::write(
    fix.root.join("sample.ts"),
    r#"class Base { close() {} }
class Child extends Base {
  override close(): void { cleanup() }
}
"#,
  )
  .expect("write ts fixture");

  let (envelope, stderr, exit) =
    run_json(&fix.root, &["--format", "json", "overrides", "close", "."]);
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  let warnings = envelope["warnings"].as_array().expect("warnings");
  assert!(
    warnings
      .iter()
      .any(|warning| warning["code"] == "NOT_IMPLEMENTED" && warning["lang"] == "TypeScript"),
    "TypeScript overrides should be explicit unsupported evidence:\n{envelope:#}"
  );
  assert!(
    !warnings
      .iter()
      .any(|warning| warning["code"] == "PATTERN_PARSE_FAILED"),
    "overrides should not emit internal parse failures:\n{envelope:#}"
  );
}

#[test]
fn typescript_exported_class_relation_hits_are_deduped() {
  let fix = fixture_root("ts-relations");
  fs::write(
    fix.root.join("relation.ts"),
    r#"class Base {}
export class Child extends Base {}
class GrandChild extends Child {}
"#,
  )
  .expect("write ts relation fixture");

  let (supers, stderr, exit) =
    run_json(&fix.root, &["--format", "json", "supertypes", "Child", "."]);
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{supers:#}");
  let super_hits = supers["hits"].as_array().expect("super hits");
  assert_eq!(
    super_hits.len(),
    1,
    "exported class should dedupe:\n{supers:#}"
  );
  assert_eq!(super_hits[0]["name"], "Child");
  assert_eq!(super_hits[0]["line_start"], 2);

  let (subs, stderr, exit) = run_json(&fix.root, &["--format", "json", "subtypes", "Base", "."]);
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{subs:#}");
  let sub_hits = subs["hits"].as_array().expect("sub hits");
  assert_eq!(sub_hits.len(), 1, "exported class should dedupe:\n{subs:#}");
  assert_eq!(sub_hits[0]["name"], "Child");
  assert_eq!(sub_hits[0]["line_start"], 2);
}

#[test]
fn ast_diff_discovers_repo_from_file_path_outside_repo() {
  let fix = fixture_root("ast-diff-root");
  let repo = fix.root.join("repo");
  fs::create_dir_all(repo.join("src")).expect("create repo src");
  git(&fix.root, &["init", "repo"]);
  fs::write(
    repo.join("src/service.ts"),
    "export function handle() { return 1 }\n",
  )
  .expect("write initial file");
  git(&repo, &["add", "src/service.ts"]);
  git_commit(&repo, "initial");
  fs::write(
    repo.join("src/service.ts"),
    "export function handle(input: string) { return input.length }\n",
  )
  .expect("write changed file");
  git_commit(&repo, "change signature");

  let file_arg = "repo/src/service.ts";
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format", "json", "ast-diff", "--from", "HEAD~1", "--to", "HEAD", file_arg,
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  assert_eq!(envelope["schema"], "lmprobe.ast_diff.v1");
  assert!(
    envelope["nodes"]
      .as_array()
      .is_some_and(|nodes| nodes.iter().any(|node| node["name"] == "handle")),
    "ast-diff should resolve repo from the file path:\n{envelope:#}"
  );
}
