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
    .map(|d| {
      // Unique within the process: parallel tests can read the same clock value.
      static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
      d.as_nanos() * 1024 + N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as u128
    })
    .unwrap_or(0);
  let root = std::env::temp_dir().join(format!(
    "lmprobe-diff-context-{}-{suffix}",
    std::process::id()
  ));
  let _ = std::fs::remove_dir_all(&root);
  std::fs::create_dir_all(root.join("src")).expect("mkdir src");
  run_git(&root, &["init"]);
  std::fs::write(
    root.join("src/app.py"),
    "class Box:\n    def compute(self):\n        value = 1\n        return value\n",
  )
  .expect("write initial file");
  std::fs::write(
    root.join("src/app file.py"),
    "def spaced():\n    value = 1\n    return value\n",
  )
  .expect("write initial spaced file");
  run_git(&root, &["add", "."]);
  run_git(&root, &["commit", "-m", "initial"]);
  std::fs::write(
    root.join("src/app.py"),
    "class Box:\n    def compute(self):\n        value = 2\n        extra = value + 1\n        return extra\n",
  )
  .expect("write changed file");
  std::fs::write(
    root.join("src/app file.py"),
    "def spaced():\n    value = 2\n    return value\n",
  )
  .expect("write changed spaced file");
  run_git(&root, &["add", "."]);
  run_git(&root, &["commit", "-m", "change compute"]);
  Fixture { root }
}

fn run_git(root: &Path, args: &[&str]) {
  let out = Command::new("git")
    .args([
      "-c",
      "user.name=lmprobe",
      "-c",
      "user.email=lmprobe@example.test",
    ])
    .args(args)
    .current_dir(root)
    .output()
    .expect("spawn git");
  assert!(
    out.status.success(),
    "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
    args,
    String::from_utf8_lossy(&out.stdout),
    String::from_utf8_lossy(&out.stderr)
  );
}

fn run_lmprobe(root: &Path, args: &[&str]) -> (String, String, i32) {
  let out = Command::new(binary())
    .args(args)
    .current_dir(root)
    .output()
    .expect("spawn lmprobe");
  (
    String::from_utf8_lossy(&out.stdout).into_owned(),
    String::from_utf8_lossy(&out.stderr).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

#[test]
fn diff_context_json_returns_changed_line_anchors_with_expansions() {
  let fix = fixture();
  let (stdout, stderr, exit) = run_lmprobe(
    &fix.root,
    &[
      "--format",
      "json",
      "diff-context",
      "--from",
      "HEAD~1",
      "--to",
      "HEAD",
      "--around",
      "1",
      "--parents",
      "--body",
      ".",
    ],
  );
  assert_eq!(exit, 0, "lmprobe failed\n{stdout}\n{stderr}");
  let env: serde_json::Value =
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}"));

  assert_eq!(
    env.get("schema").and_then(|v| v.as_str()),
    Some("lmprobe.diff_context.v1")
  );
  assert_eq!(
    env.get("exit_status").and_then(|v| v.as_str()),
    Some("matched")
  );
  let anchors = env
    .get("anchors")
    .and_then(|v| v.as_array())
    .expect("anchors array");
  assert!(
    anchors.iter().any(|anchor| {
      anchor
        .get("line_content")
        .and_then(|v| v.as_str())
        .is_some_and(|line| line.contains("extra = value + 1"))
    }),
    "expected added line anchor:\n{env:#}"
  );
  let first = anchors.first().expect("at least one changed anchor");
  assert_eq!(
    first.get("anchor_kind").and_then(|v| v.as_str()),
    Some("changed_line")
  );
  assert!(
    first
      .get("source_context")
      .and_then(|v| v.as_array())
      .is_some_and(|lines| !lines.is_empty()),
    "around context should be attached:\n{first:#}"
  );
  assert!(
    first
      .get("parents")
      .and_then(|v| v.as_array())
      .is_some_and(|parents| !parents.is_empty()),
    "parents should be attached:\n{first:#}"
  );
  assert!(
    first.get("body").is_some(),
    "body should be attached:\n{first:#}"
  );
}

#[test]
fn diff_context_ndjson_itemizes_anchors() {
  let fix = fixture();
  let (stdout, stderr, exit) = run_lmprobe(
    &fix.root,
    &[
      "--format",
      "ndjson",
      "diff-context",
      "--from",
      "HEAD~1",
      "--to",
      "HEAD",
      ".",
    ],
  );
  assert_eq!(exit, 0, "lmprobe failed\n{stdout}\n{stderr}");
  let rows = stdout.lines().collect::<Vec<_>>();
  assert!(
    rows.len() >= 2,
    "expected item rows plus summary:\n{stdout}"
  );
  let first: serde_json::Value = serde_json::from_str(rows[0]).expect("first ndjson item");
  assert_eq!(
    first.get("anchor_kind").and_then(|v| v.as_str()),
    Some("changed_line")
  );
  let summary: serde_json::Value =
    serde_json::from_str(rows.last().expect("summary row")).expect("summary JSON");
  assert_eq!(
    summary.get("schema").and_then(|v| v.as_str()),
    Some("lmprobe.diff_context.v1")
  );
  assert_eq!(
    summary.get("kind").and_then(|v| v.as_str()),
    Some("summary")
  );
  assert!(
    summary.get("anchors").is_none(),
    "summary should omit item array:\n{summary:#}"
  );
}

#[test]
fn diff_context_uses_target_revision_source_when_worktree_is_dirty() {
  let fix = fixture();
  std::fs::write(
    fix.root.join("src/app.py"),
    "class Box:\n    def compute(self):\n        value = 999\n        extra = value + 1\n        return extra\n",
  )
  .expect("dirty worktree");

  let (stdout, stderr, exit) = run_lmprobe(
    &fix.root,
    &[
      "--format",
      "json",
      "diff-context",
      "--from",
      "HEAD~1",
      "--to",
      "HEAD",
      "--around",
      "1",
      "src/app.py",
    ],
  );
  assert_eq!(exit, 0, "lmprobe failed\n{stdout}\n{stderr}");
  let env: serde_json::Value =
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}"));
  let rendered = serde_json::to_string(&env).expect("render env");
  assert!(
    rendered.contains("value = 2"),
    "diff anchor/context should come from HEAD:\n{env:#}"
  );
  assert!(
    !rendered.contains("value = 999"),
    "dirty worktree content should not overwrite HEAD diff evidence:\n{env:#}"
  );
  assert!(
    env
      .get("warnings")
      .and_then(|v| v.as_array())
      .is_some_and(|warnings| warnings.is_empty()),
    "HEAD source expansion should not warn:\n{env:#}"
  );
}

#[test]
fn diff_context_handles_paths_with_spaces() {
  let fix = fixture();
  let (stdout, stderr, exit) = run_lmprobe(
    &fix.root,
    &[
      "--format",
      "json",
      "diff-context",
      "--from",
      "HEAD~1",
      "--to",
      "HEAD",
      "--around",
      "1",
      "--parents",
      "--body",
      "src/app file.py",
    ],
  );
  assert_eq!(exit, 0, "lmprobe failed\n{stdout}\n{stderr}");
  let env: serde_json::Value =
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}"));
  let anchors = env
    .get("anchors")
    .and_then(|v| v.as_array())
    .expect("anchors array");
  assert!(
    anchors.iter().any(|anchor| {
      anchor.get("file_path").and_then(|v| v.as_str()) == Some("src/app file.py")
        && anchor
          .get("source_context")
          .and_then(|v| v.as_array())
          .is_some_and(|lines| !lines.is_empty())
        && anchor
          .get("parents")
          .and_then(|v| v.as_array())
          .is_some_and(|parents| !parents.is_empty())
    }),
    "path with spaces should keep exact path and expansions:\n{env:#}"
  );
}
