use git2::{Repository, Signature};
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

fn run(root: &Path, args: &[&str]) -> Output {
  Command::new(env!("CARGO_BIN_EXE_lmprobe"))
    .arg("--json")
    .arg("git")
    .args(args)
    .current_dir(root)
    .output()
    .unwrap()
}
fn data(out: &Output) -> Value {
  serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
    panic!(
      "{} {}",
      String::from_utf8_lossy(&out.stdout),
      String::from_utf8_lossy(&out.stderr)
    )
  })
}
fn rows(out: &Output) -> Vec<Value> {
  data(out)["payload"]["rows"].as_array().unwrap().clone()
}
fn commit(repo: &Repository, message: &str) -> git2::Oid {
  let mut index = repo.index().unwrap();
  index
    .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
    .unwrap();
  index.write().unwrap();
  let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
  let author = Signature::new(
    "Author",
    "a@example.invalid",
    &git2::Time::new(1_700_000_000, 60),
  )
  .unwrap();
  let committer = Signature::new(
    "Committer",
    "c@example.invalid",
    &git2::Time::new(1_700_000_100, 120),
  )
  .unwrap();
  let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
  let parents: Vec<_> = parent.iter().collect();
  repo
    .commit(Some("HEAD"), &author, &committer, message, &tree, &parents)
    .unwrap()
}
fn fixture() -> tempfile::TempDir {
  let dir = tempfile::tempdir().unwrap();
  let root = dir.path();
  let repo = Repository::init(root).unwrap();
  repo.set_head("refs/heads/main").unwrap();
  std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
  std::fs::write(root.join(".gitignore"), "ignored.txt\n").unwrap();
  std::fs::create_dir(root.join("nested")).unwrap();
  std::fs::write(root.join("nested/other.txt"), "nested\n").unwrap();
  commit(&repo, "initial 'quoted'\n\nbody");
  dir
}

#[test]
fn status_distinguishes_staged_worktree_untracked_and_ignored() {
  let dir = fixture();
  let root = dir.path();
  let out = run(root, &["status"]);
  assert!(out.status.success());
  assert_eq!(data(&out)["status"], "ok");
  assert!(rows(&out).is_empty());
  assert_eq!(data(&out)["metadata"]["clean"], true);
  std::fs::write(root.join("a.txt"), "staged\ntwo\n").unwrap();
  let repo = Repository::open(root).unwrap();
  let mut index = repo.index().unwrap();
  index.add_path(Path::new("a.txt")).unwrap();
  index.write().unwrap();
  std::fs::write(root.join("a.txt"), "unstaged\ntwo\n").unwrap();
  std::fs::write(root.join("new\nfile.txt"), "new").unwrap();
  std::fs::write(root.join("ignored.txt"), "ignored").unwrap();
  let out = run(root, &["status", "--repo", "."]);
  assert!(out.status.success());
  let found = rows(&out);
  let a = found.iter().find(|r| r["path"] == "a.txt").unwrap();
  assert_eq!(a["index_status"], "modified");
  assert_eq!(a["worktree_status"], "modified");
  assert!(found
    .iter()
    .any(|r| r["path"] == "new\nfile.txt" && r["untracked"] == true));
  assert!(!found.iter().any(|r| r["path"] == "ignored.txt"));
  let out = run(root, &["status", "--ignored"]);
  assert!(rows(&out)
    .iter()
    .any(|r| r["path"] == "ignored.txt" && r["ignored"] == true));
}

#[test]
fn diff_has_real_hunks_line_content_and_distinct_comparison_modes() {
  let dir = fixture();
  let root = dir.path();
  let repo = Repository::open(root).unwrap();
  std::fs::write(root.join("a.txt"), "staged\ntwo\n").unwrap();
  let mut index = repo.index().unwrap();
  index.add_path(Path::new("a.txt")).unwrap();
  index.write().unwrap();
  std::fs::write(root.join("a.txt"), "unstaged\ntwo\n").unwrap();
  for (args, old, new) in [
    (vec!["diff"], "staged", "unstaged"),
    (vec!["diff", "--staged"], "one", "staged"),
    (vec!["diff", "HEAD"], "one", "unstaged"),
  ] {
    let out = run(root, &args);
    assert!(
      out.status.success(),
      "{}",
      String::from_utf8_lossy(&out.stdout)
    );
    let row = &rows(&out)[0];
    assert_eq!(row["additions"], 1);
    assert_eq!(row["deletions"], 1);
    assert!(!row["hunks"].as_array().unwrap().is_empty());
    let text = row.to_string();
    assert!(text.contains(old));
    assert!(text.contains(new));
  }
  let out = run(root, &["diff", "--path", "nested"]);
  assert_eq!(out.status.code(), Some(1));
  assert!(rows(&out).is_empty());
}

#[test]
fn log_show_revision_and_blame_are_structured_and_bounded() {
  let dir = fixture();
  let root = dir.path();
  let repo = Repository::open(root).unwrap();
  std::fs::write(root.join("a.txt"), "changed\ntwo\n").unwrap();
  let head = commit(&repo, "second\n\nmessage");
  let out = run(root, &["log", "--max-count", "1"]);
  assert!(out.status.success());
  let found = rows(&out);
  assert_eq!(found.len(), 1);
  assert_eq!(found[0]["id"], head.to_string());
  assert_eq!(found[0]["parents"].as_array().unwrap().len(), 1);
  assert_eq!(found[0]["author"]["time"], 1_700_000_000i64);
  assert_eq!(found[0]["committer"]["time"], 1_700_000_100i64);
  let out = run(root, &["show"]);
  assert!(out.status.success());
  assert_eq!(data(&out)["payload_format"], "hierarchical");
  assert_eq!(
    data(&out)["payload"]["data"]["commit"]["id"],
    head.to_string()
  );
  let out = run(root, &["show", "HEAD", "--file", "a.txt"]);
  assert!(out.status.success());
  assert_eq!(data(&out)["payload"]["data"]["content"], "changed\ntwo\n");
  let out = run(root, &["rev-parse", "HEAD~1"]);
  assert!(out.status.success());
  assert_eq!(
    data(&out)["payload"]["data"]["oid"],
    repo
      .head()
      .unwrap()
      .peel_to_commit()
      .unwrap()
      .parent_id(0)
      .unwrap()
      .to_string()
  );
  let out = run(root, &["blame", "a.txt"]);
  assert!(out.status.success());
  assert_eq!(rows(&out).len(), 2);
}

#[test]
fn branches_remotes_and_index_pathspec_are_parsed() {
  let dir = fixture();
  let root = dir.path();
  let repo = Repository::open(root).unwrap();
  let head = repo.head().unwrap().peel_to_commit().unwrap();
  repo.branch("feature/test", &head, false).unwrap();
  repo
    .remote(
      "origin",
      "https://user:secret@example.invalid/owner/repo.git?token=SECRET",
    )
    .unwrap();
  let out = run(root, &["branch", "--all"]);
  assert!(out.status.success());
  assert!(rows(&out).iter().any(|r| r["name"] == "feature/test"));
  let out = run(root, &["remote"]);
  assert!(out.status.success());
  let text = String::from_utf8(out.stdout.clone()).unwrap();
  assert!(!text.contains("secret"));
  assert!(!text.contains("SECRET"));
  assert_eq!(rows(&out)[0]["name"], "origin");
  let out = run(root, &["ls-files", "--path", "*.txt"]);
  assert!(out.status.success());
  assert_eq!(rows(&out).len(), 2);
  let out = run(root, &["ls-files", "--path", "nested"]);
  assert!(out.status.success());
  assert_eq!(rows(&out).len(), 1);
  assert_eq!(rows(&out)[0]["stage"], 0);
}

#[test]
fn multiple_remote_urls_are_preserved_and_redacted() {
  let dir = fixture();
  let root = dir.path();
  let repo = Repository::open(root).unwrap();
  repo
    .remote("origin", "https://u:firstsecret@example.invalid/first.git")
    .unwrap();
  let mut config = repo.config().unwrap();
  config
    .set_multivar(
      "remote.origin.url",
      "^nevermatches$",
      "https://u:secondsecret@example.invalid/second.git",
    )
    .unwrap();
  config
    .set_multivar(
      "remote.origin.pushurl",
      "^nevermatches$",
      "https://u:pushsecret@example.invalid/push1.git",
    )
    .unwrap();
  config
    .set_multivar(
      "remote.origin.pushurl",
      "^nevermatches$",
      "https://example.invalid/push2.git?token=SECRET",
    )
    .unwrap();
  let out = run(root, &["remote"]);
  assert!(out.status.success());
  let found = rows(&out);
  assert_eq!(
    found[0]["configured_fetch_urls"].as_array().unwrap().len(),
    2
  );
  assert_eq!(
    found[0]["configured_push_urls"].as_array().unwrap().len(),
    2
  );
  assert!(!String::from_utf8(out.stdout).unwrap().contains("secret"));
}

#[cfg(unix)]
#[test]
fn non_utf8_remote_urls_are_preserved_without_wrong_push_fallback() {
  use std::io::Write;
  let dir = fixture();
  let root = dir.path();
  let mut config = std::fs::OpenOptions::new()
    .append(true)
    .open(root.join(".git/config"))
    .unwrap();
  config
    .write_all(b"\n[remote \"raw\"]\n\turl = /tmp/fetch-\xff\n\tpushurl = /tmp/push-\xfe\n")
    .unwrap();
  drop(config);
  let out = run(root, &["remote"]);
  assert!(
    out.status.success(),
    "{}",
    String::from_utf8_lossy(&out.stdout)
  );
  let found = rows(&out);
  let raw = found.iter().find(|row| row["name"] == "raw").unwrap();
  assert_eq!(raw["fetch_url"]["bytes_hex"], "2f746d702f66657463682dff");
  assert_eq!(raw["push_url"]["bytes_hex"], "2f746d702f707573682dfe");
  assert_eq!(
    raw["configured_push_urls"][0]["bytes_hex"],
    "2f746d702f707573682dfe"
  );
}

#[test]
fn incomplete_upstream_graph_exits_three_in_all_output_modes() {
  let dir = fixture();
  let root = dir.path();
  let repo = Repository::open(root).unwrap();
  let head = repo.head().unwrap().peel_to_commit().unwrap();
  let raw=format!("tree {}\nparent ffffffffffffffffffffffffffffffffffffffff\nauthor Fixture <fixture@example.invalid> 1700000000 +0000\ncommitter Fixture <fixture@example.invalid> 1700000000 +0000\n\nbroken parent\n",head.tree_id());
  let oid = repo
    .odb()
    .unwrap()
    .write(git2::ObjectType::Commit, raw.as_bytes())
    .unwrap();
  repo
    .reference("refs/heads/broken", oid, true, "fixture")
    .unwrap();
  repo
    .remote("origin", "https://example.invalid/fixture.git")
    .unwrap();
  repo
    .reference("refs/remotes/origin/broken", head.id(), true, "fixture")
    .unwrap();
  repo
    .find_branch("broken", git2::BranchType::Local)
    .unwrap()
    .set_upstream(Some("origin/broken"))
    .unwrap();
  for mode in [
    vec!["--json"],
    vec!["--format", "json"],
    vec!["--format", "ndjson"],
    vec!["--format", "markdown"],
  ] {
    let out = Command::new(env!("CARGO_BIN_EXE_lmprobe"))
      .args(["git", "branch"])
      .args(&mode)
      .current_dir(root)
      .output()
      .unwrap();
    assert_eq!(
      out.status.code(),
      Some(3),
      "mode {mode:?}: {}",
      String::from_utf8_lossy(&out.stdout)
    );
    assert!(String::from_utf8(out.stdout)
      .unwrap()
      .contains("GIT_GRAPH_INCOMPLETE"));
  }
}

#[test]
fn shallow_history_is_explicit_and_log_limits_are_reported() {
  let dir = fixture();
  let root = dir.path();
  let repo = Repository::open(root).unwrap();
  std::fs::write(root.join("a.txt"), "second\n").unwrap();
  let head = commit(&repo, "second");
  let out = run(root, &["log", "--max-count", "1"]);
  assert!(out.status.success());
  assert_eq!(data(&out)["metadata"]["truncated"], true);
  std::fs::write(root.join(".git/shallow"), format!("{head}\n")).unwrap();
  let out = run(root, &["log"]);
  assert!(out.status.success());
  assert_eq!(data(&out)["metadata"]["shallow"], true);
  assert_eq!(rows(&out).len(), 1);
  assert_eq!(data(&out)["metadata"]["truncated"], false);
}

#[test]
fn unborn_and_detached_repositories_and_invalid_revisions() {
  let dir = tempfile::tempdir().unwrap();
  let root = dir.path();
  Repository::init(root).unwrap();
  let out = run(root, &["status"]);
  assert!(out.status.success());
  let out = run(root, &["log"]);
  assert_eq!(out.status.code(), Some(1));
  let out = run(root, &["diff", "--staged"]);
  assert_eq!(out.status.code(), Some(1));
  let out = run(root, &["rev-parse", "absent"]);
  assert_eq!(out.status.code(), Some(2));
  assert_eq!(data(&out)["status"], "error");
  let dir = fixture();
  let root = dir.path();
  let repo = Repository::open(root).unwrap();
  repo
    .set_head_detached(repo.head().unwrap().target().unwrap())
    .unwrap();
  let out = run(root, &["status"]);
  assert!(out.status.success());
  let out = run(root, &["branch"]);
  assert!(out.status.success());
}

#[test]
fn outfile_never_overwrites_git_metadata_or_existing_worktree_sources() {
  let dir = fixture();
  let root = dir.path();
  for path in [".git/config", ".git/index", "a.txt"] {
    let before = std::fs::read(root.join(path)).unwrap();
    let out = run(root, &["status", "--outfile", path]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(std::fs::read(root.join(path)).unwrap(), before);
  }
  let before = std::fs::read(root.join(".git/index")).unwrap();
  for args in [
    vec!["status", "--unknown", "--outfile", ".git/index"],
    vec!["status", "--help", "--outfile", ".git/index"],
  ] {
    let out = run(root, &args);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), before);
  }
  let out = run(root, &["status", "--outfile", ".git/new-metadata.json"]);
  assert_eq!(out.status.code(), Some(2));
  assert!(!root.join(".git/new-metadata.json").exists());
  let out = run(root, &["status", "--outfile", "result.json"]);
  assert!(out.status.success());
  assert!(out.stdout.is_empty());
  let value: Value =
    serde_json::from_slice(&std::fs::read(root.join("result.json")).unwrap()).unwrap();
  assert!(value["payload"]["rows"].as_array().unwrap().is_empty());
}

#[test]
fn corrupt_repository_cannot_overwrite_metadata_with_error_results() {
  let dir = fixture();
  let root = dir.path();
  let bad = b"[unterminated config\n";
  std::fs::write(root.join(".git/config"), bad).unwrap();
  let index = std::fs::read(root.join(".git/index")).unwrap();
  for args in [
    vec!["status", "--outfile", ".git/config"],
    vec!["status", "--unknown", "--outfile", ".git/config"],
    vec!["status", "--help", "--outfile", ".git/config"],
  ] {
    let out = run(root, &args);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(std::fs::read(root.join(".git/config")).unwrap(), bad);
    assert_eq!(data(&out)["status"], "error");
  }
  let out = run(root, &["status", "--outfile", ".git/index"]);
  assert_eq!(out.status.code(), Some(2));
  assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
  let out = run(root, &["status", "--outfile", ".git/new-output.json"]);
  assert_eq!(out.status.code(), Some(2));
  assert!(!root.join(".git/new-output.json").exists());
}

#[test]
fn rename_records_preserve_old_and_new_paths_and_tag_revisions() {
  let dir = fixture();
  let root = dir.path();
  let repo = Repository::open(root).unwrap();
  let old = repo.head().unwrap().peel_to_commit().unwrap();
  let sig = Signature::now("Fixture", "fixture@example.invalid").unwrap();
  repo
    .tag("v1", old.as_object(), &sig, "annotated", false)
    .unwrap();
  let out = run(root, &["log", "v1"]);
  assert!(out.status.success());
  assert_eq!(rows(&out)[0]["id"], old.id().to_string());
  std::fs::rename(root.join("a.txt"), root.join("renamed.txt")).unwrap();
  let mut index = repo.index().unwrap();
  index.remove_path(Path::new("a.txt")).unwrap();
  index.add_path(Path::new("renamed.txt")).unwrap();
  index.write().unwrap();
  let out = run(root, &["status"]);
  assert!(out.status.success());
  let found = rows(&out);
  let row = found
    .iter()
    .find(|r| r["index_status"] == "renamed")
    .unwrap();
  assert_eq!(row["index_old_path"], "a.txt");
  assert_eq!(row["index_new_path"], "renamed.txt");
  let out = run(root, &["diff", "--staged"]);
  assert!(out.status.success());
  let found = rows(&out);
  let row = found.iter().find(|r| r["status"] == "renamed").unwrap();
  assert_eq!(row["old_path"], "a.txt");
  assert_eq!(row["new_path"], "renamed.txt");
}

#[test]
fn bare_and_linked_worktree_reads_protect_common_metadata() {
  let dir = fixture();
  let root = dir.path();
  let bare = root.join("bare.git");
  let mut builder = git2::build::RepoBuilder::new();
  builder.bare(true);
  builder.clone(root.to_str().unwrap(), &bare).unwrap();
  let out = run(root, &["log", "--repo", "bare.git"]);
  assert!(out.status.success());
  assert_eq!(data(&out)["metadata"]["bare"], true);
  let out = run(root, &["status", "--repo", "bare.git"]);
  assert_eq!(out.status.code(), Some(2));
  let repo = Repository::open(root).unwrap();
  let worktree = root.join("linked");
  repo.worktree("fixture-linked", &worktree, None).unwrap();
  let out = run(&worktree, &["status"]);
  assert!(out.status.success());
  assert!(rows(&out).is_empty());
  let config = root.join(".git/config");
  let before = std::fs::read(&config).unwrap();
  let out = run(&worktree, &["log", "--outfile", config.to_str().unwrap()]);
  assert_eq!(out.status.code(), Some(2));
  assert_eq!(std::fs::read(config).unwrap(), before);
  let admin = Repository::open(&worktree).unwrap().path().to_path_buf();
  std::fs::write(admin.join("commondir"), "missing-common-directory\n").unwrap();
  let config = root.join(".git/config");
  let before = std::fs::read(&config).unwrap();
  let out = run(
    &worktree,
    &["status", "--outfile", config.to_str().unwrap()],
  );
  assert_eq!(out.status.code(), Some(2));
  assert_eq!(std::fs::read(config).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn unusual_filenames_and_binary_blobs_are_lossless() {
  use std::os::unix::ffi::OsStringExt;
  let dir = fixture();
  let root = dir.path();
  let bytes = b"odd-\xff.txt".to_vec();
  let name = std::ffi::OsString::from_vec(bytes.clone());
  std::fs::write(root.join(&name), "text").unwrap();
  let out = run(root, &["status"]);
  assert!(out.status.success());
  assert!(String::from_utf8(out.stdout).unwrap().contains("ff"));
  std::fs::write(root.join("binary.dat"), [0, 255, 1]).unwrap();
  let repo = Repository::open(root).unwrap();
  commit(&repo, "binary and unusual paths");
  let out = run(root, &["ls-files"]);
  assert!(out.status.success());
  assert!(rows(&out)
    .iter()
    .any(|r| r["path"].is_object() || r["path_bytes_hex"].is_string()));
  let out = run(root, &["show", "--file", "binary.dat"]);
  assert!(out.status.success());
  assert_eq!(data(&out)["payload"]["data"]["binary"], true);
  assert!(data(&out)["payload"]["data"].to_string().contains("00ff01"));
}

#[test]
fn readonly_inspection_does_not_change_index_or_worktree() {
  let dir = fixture();
  let root = dir.path();
  let before = std::fs::read(root.join(".git/index")).unwrap();
  let source = std::fs::read(root.join("a.txt")).unwrap();
  for args in [
    vec!["status"],
    vec!["diff"],
    vec!["log"],
    vec!["show"],
    vec!["branch"],
    vec!["remote"],
    vec!["rev-parse"],
    vec!["ls-files"],
    vec!["blame", "a.txt"],
  ] {
    let out = run(root, &args);
    assert!(matches!(out.status.code(), Some(0 | 1)));
  }
  assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), before);
  assert_eq!(std::fs::read(root.join("a.txt")).unwrap(), source);
}
