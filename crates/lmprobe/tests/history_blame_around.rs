use std::path::PathBuf;
use std::process::Command;

fn binary() -> PathBuf {
  if let Ok(p) = std::env::var("LMPROBE_BIN") {
    return PathBuf::from(p);
  }
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

struct Fixture {
  repo: PathBuf,
  xdg: PathBuf,
}

impl Drop for Fixture {
  fn drop(&mut self) {
    let _ = std::fs::remove_dir_all(&self.repo);
    let _ = std::fs::remove_dir_all(&self.xdg);
  }
}

fn unique_dir(prefix: &str) -> PathBuf {
  let suffix = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_nanos())
    .unwrap_or(0);
  std::env::temp_dir().join(format!("lmprobe-{prefix}-{}-{suffix}", std::process::id()))
}

fn git(repo: &std::path::Path, args: &[&str]) {
  let out = Command::new("git")
    .args(args)
    .current_dir(repo)
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

fn fixture() -> Fixture {
  let repo = unique_dir("history-blame-around");
  let xdg = unique_dir("history-blame-around-xdg");
  let _ = std::fs::remove_dir_all(&repo);
  let _ = std::fs::remove_dir_all(&xdg);
  std::fs::create_dir_all(repo.join("src")).expect("mkdir repo");
  std::fs::create_dir_all(&xdg).expect("mkdir xdg");

  git(&repo, &["init", "-q"]);
  git(&repo, &["config", "user.email", "t@t"]);
  git(&repo, &["config", "user.name", "t"]);
  std::fs::write(
    repo.join("src/lib.rs"),
    "line one\nline two\nline three\nline four\nline five\n",
  )
  .expect("write fixture");
  git(&repo, &["add", "src/lib.rs"]);
  git(&repo, &["commit", "-qm", "init"]);

  Fixture { repo, xdg }
}

fn run_json(fix: &Fixture, args: &[&str]) -> (serde_json::Value, String, i32) {
  let out = Command::new(binary())
    .args(args)
    .env("XDG_CACHE_HOME", &fix.xdg)
    .current_dir(&fix.repo)
    .output()
    .expect("spawn lmprobe");
  let stdout = String::from_utf8_lossy(&out.stdout);
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    String::from_utf8_lossy(&out.stderr).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

fn run_stdout(fix: &Fixture, args: &[&str]) -> (String, String, i32) {
  let out = Command::new(binary())
    .args(args)
    .env("XDG_CACHE_HOME", &fix.xdg)
    .current_dir(&fix.repo)
    .output()
    .expect("spawn lmprobe");
  (
    String::from_utf8_lossy(&out.stdout).into_owned(),
    String::from_utf8_lossy(&out.stderr).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

#[test]
fn history_blame_line_default_omits_source_context() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix,
    &["--format", "json", "history", "blame", "src/lib.rs:3", "."],
  );

  assert_eq!(exit, 0, "blame failed:\n{envelope:#}\nstderr:\n{stderr}");
  assert!(
    envelope.get("source_context").is_none(),
    "default line blame should keep compact envelope:\n{envelope:#}"
  );
}

#[test]
fn history_blame_line_around_adds_bounded_source_context() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix,
    &[
      "--format",
      "json",
      "history",
      "blame",
      "--around",
      "1",
      "src/lib.rs:3",
      ".",
    ],
  );

  assert_eq!(exit, 0, "blame failed:\n{envelope:#}\nstderr:\n{stderr}");
  assert_eq!(
    envelope
      .pointer("/source_context/0/line_number")
      .and_then(|v| v.as_u64()),
    Some(2)
  );
  assert_eq!(
    envelope
      .pointer("/source_context/0/line_content")
      .and_then(|v| v.as_str()),
    Some("line two")
  );
  assert_eq!(
    envelope
      .pointer("/source_context/1/line_number")
      .and_then(|v| v.as_u64()),
    Some(3)
  );
  assert_eq!(
    envelope
      .pointer("/source_context/2/line_content")
      .and_then(|v| v.as_str()),
    Some("line four")
  );
  assert_eq!(
    envelope
      .get("source_context")
      .and_then(|v| v.as_array())
      .map(Vec::len),
    Some(3)
  );
}

#[test]
fn history_blame_line_around_respects_file_start_bound() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix,
    &[
      "--format",
      "json",
      "history",
      "blame",
      "--around",
      "2",
      "src/lib.rs:1",
      ".",
    ],
  );

  assert_eq!(exit, 0, "blame failed:\n{envelope:#}\nstderr:\n{stderr}");
  let context = envelope
    .get("source_context")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected source_context:\n{envelope:#}"));
  assert_eq!(context.len(), 3, "context should stop at file start");
  assert_eq!(
    context[0].get("line_number").and_then(|v| v.as_u64()),
    Some(1)
  );
}

#[test]
fn history_blame_line_around_is_present_in_ndjson_summary() {
  let fix = fixture();
  let (stdout, stderr, exit) = run_stdout(
    &fix,
    &[
      "--format",
      "ndjson",
      "history",
      "blame",
      "--around",
      "1",
      "src/lib.rs:3",
      ".",
    ],
  );
  let summary: serde_json::Value = stdout
    .lines()
    .last()
    .map(|line| serde_json::from_str(line).expect("ndjson summary should parse"))
    .unwrap_or_else(|| panic!("expected ndjson output:\n{stdout}"));

  assert_eq!(exit, 0, "blame failed:\n{stdout}\nstderr:\n{stderr}");
  assert_eq!(
    summary.get("kind").and_then(|v| v.as_str()),
    Some("summary")
  );
  assert_eq!(
    summary
      .pointer("/source_context/1/line_content")
      .and_then(|v| v.as_str()),
    Some("line three")
  );
}
