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
    .map(|d| {
      // Unique within the process: parallel tests can read the same clock value.
      static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
      d.as_nanos() * 1024 + N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as u128
    })
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

fn provision(tag: &str) -> Fixture {
  let repo = unique_dir(&format!("history-blame-absolute-{tag}"));
  let xdg = unique_dir(&format!("history-blame-absolute-xdg-{tag}"));
  let _ = std::fs::remove_dir_all(&repo);
  let _ = std::fs::remove_dir_all(&xdg);
  std::fs::create_dir_all(repo.join("src")).expect("mkdir repo");
  std::fs::create_dir_all(&xdg).expect("mkdir xdg");

  git(&repo, &["init", "-q"]);
  git(&repo, &["config", "user.email", "t@t"]);
  git(&repo, &["config", "user.name", "t"]);
  std::fs::write(
    repo.join("src/lib.rs"),
    "pub fn first() {}\npub fn second() {}\n",
  )
  .expect("write fixture");
  git(&repo, &["add", "src/lib.rs"]);
  git(&repo, &["commit", "-qm", "init"]);

  Fixture { repo, xdg }
}

fn run_blame(fix: &Fixture, target: &str) -> (serde_json::Value, String, i32) {
  let out = Command::new(binary())
    .args(["--format", "json", "history", "blame", target, "."])
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

fn normalized_entries(envelope: &serde_json::Value) -> Option<serde_json::Value> {
  let mut entries = envelope.get("entries")?.clone();
  if let Some(items) = entries.as_array_mut() {
    for item in items {
      if let Some(path) = item
        .get("file_path")
        .and_then(|v| v.as_str())
        .map(|path| path.replace('\\', "/"))
      {
        *item.get_mut("file_path").expect("file_path") = path.into();
      }
    }
  }
  Some(entries)
}

#[test]
fn history_blame_accepts_absolute_pathline_inside_repo() {
  let fix = provision("inside");
  let relative = "src/lib.rs:2";
  let absolute = format!("{}:2", fix.repo.join("src/lib.rs").display());

  let (relative_json, relative_stderr, relative_exit) = run_blame(&fix, relative);
  let (absolute_json, absolute_stderr, absolute_exit) = run_blame(&fix, &absolute);

  assert_eq!(
    relative_exit, 0,
    "relative blame failed:\n{relative_json:#}\nstderr:\n{relative_stderr}"
  );
  assert_eq!(
    absolute_exit, 0,
    "absolute blame failed:\n{absolute_json:#}\nstderr:\n{absolute_stderr}"
  );
  assert_eq!(
    normalized_entries(&absolute_json),
    normalized_entries(&relative_json),
    "absolute PATH:LINE should normalize to the same repo-relative blame target"
  );
  let absolute_path = absolute_json
    .pointer("/entries/0/file_path")
    .and_then(|v| v.as_str())
    .expect("absolute blame file path");
  assert_eq!(absolute_path.replace('\\', "/"), "src/lib.rs");
}

#[test]
fn history_blame_keeps_outside_absolute_pathline_failure() {
  let fix = provision("outside");
  let outside = unique_dir("history-blame-outside-file");
  std::fs::create_dir_all(&outside).expect("mkdir outside");
  let outside_file = outside.join("outside.rs");
  std::fs::write(&outside_file, "pub fn outside() {}\n").expect("write outside");
  let target = format!("{}:1", outside_file.display());

  let out = Command::new(binary())
    .args(["--format", "json", "history", "blame", &target, "."])
    .env("XDG_CACHE_HOME", &fix.xdg)
    .current_dir(&fix.repo)
    .output()
    .expect("spawn lmprobe");
  let _ = std::fs::remove_dir_all(&outside);
  let stderr = String::from_utf8_lossy(&out.stderr);

  assert!(
    !out.status.success(),
    "outside absolute PATH:LINE should still fail"
  );
  assert!(
    stderr.contains("repo path") && stderr.contains("should be relative"),
    "outside absolute PATH:LINE should keep the existing git blame failure:\n{stderr}"
  );
}
