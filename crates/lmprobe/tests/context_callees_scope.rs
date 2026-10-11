use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
  if let Ok(p) = std::env::var("LMPROBE_BIN") {
    return PathBuf::from(p);
  }
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

fn workspace_root() -> PathBuf {
  let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  p.pop();
  p.pop();
  p
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

fn git(repo: &Path, args: &[&str]) {
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
  let suffix = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| {
      // Unique within the process: parallel tests can read the same clock value.
      static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
      d.as_nanos() * 1024 + N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as u128
    })
    .unwrap_or(0);
  let repo = std::env::temp_dir().join(format!(
    "lmprobe-context-callees-scope-{}-{suffix}",
    std::process::id()
  ));
  let xdg = std::env::temp_dir().join(format!(
    "lmprobe-context-callees-scope-xdg-{}-{suffix}",
    std::process::id()
  ));
  let _ = std::fs::remove_dir_all(&repo);
  let _ = std::fs::remove_dir_all(&xdg);
  std::fs::create_dir_all(&repo).expect("mkdir repo");
  std::fs::create_dir_all(&xdg).expect("mkdir xdg");

  let fixture_src = workspace_root().join("crates/lmprobe/tests/fixtures/go/context_scope.go");
  std::fs::copy(&fixture_src, repo.join("context_scope.go")).expect("copy fixture");

  git(&repo, &["init", "-q"]);
  git(&repo, &["config", "user.email", "t@t"]);
  git(&repo, &["config", "user.name", "t"]);
  git(&repo, &["add", "context_scope.go"]);
  git(&repo, &["commit", "-qm", "fixture"]);

  Fixture { repo, xdg }
}

#[test]
fn context_callees_are_scoped_to_resolved_definition_body() {
  let fix = fixture();
  let target = fix.repo.join("context_scope.go");
  let target_arg = target.to_string_lossy().into_owned();
  let out = Command::new(binary())
    .args(["--format", "json", "context", "second", &target_arg])
    .env("XDG_CACHE_HOME", &fix.xdg)
    .output()
    .expect("spawn lmprobe context");
  let stdout = String::from_utf8_lossy(&out.stdout);
  let envelope: serde_json::Value =
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}"));

  assert_eq!(
    out.status.code(),
    Some(0),
    "context should resolve fixture:\n{envelope:#}\nstderr:\n{}",
    String::from_utf8_lossy(&out.stderr)
  );
  assert_eq!(
    envelope
      .pointer("/definition/name")
      .and_then(|v| v.as_str()),
    Some("second"),
    "definition should resolve the second function:\n{envelope:#}"
  );
  let callee_names: Vec<&str> = envelope
    .get("callees")
    .and_then(|v| v.as_array())
    .into_iter()
    .flatten()
    .filter_map(|hit| hit.get("name").and_then(|v| v.as_str()))
    .collect();
  assert!(
    callee_names.contains(&"rightCall"),
    "callee from target function should be present:\n{envelope:#}"
  );
  assert!(
    !callee_names.contains(&"wrongCall"),
    "callee from earlier function should be excluded:\n{envelope:#}"
  );
}
