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
    "lmprobe-grep-json-schema-{}-{suffix}",
    std::process::id()
  ));
  let _ = std::fs::remove_dir_all(&root);
  std::fs::create_dir_all(&root).expect("mkdir fixture");
  std::fs::write(root.join("sample.txt"), "alpha\nneedle value\n").expect("write fixture");
  Fixture { root }
}

fn run_grep(root: &Path) -> (serde_json::Value, String, i32) {
  let out = Command::new(binary())
    .args(["--format", "json", "grep", "needle", "sample.txt"])
    .current_dir(root)
    .output()
    .expect("spawn lmprobe");
  let stdout = String::from_utf8_lossy(&out.stdout);
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    String::from_utf8_lossy(&out.stderr).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

#[test]
fn grep_json_hits_expose_documented_line_fields() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_grep(&fix.root);

  assert_eq!(exit, 0, "grep failed:\n{envelope:#}\nstderr:\n{stderr}");
  assert_eq!(
    envelope.get("schema").and_then(|v| v.as_str()),
    Some("lmprobe.grep.v1")
  );
  let hit = envelope
    .get("hits")
    .and_then(|v| v.as_array())
    .and_then(|hits| hits.first())
    .unwrap_or_else(|| panic!("grep should return one hit:\n{envelope:#}"));

  assert_eq!(
    hit.get("file_path").and_then(|v| v.as_str()),
    Some("sample.txt")
  );
  assert_eq!(hit.get("line_number").and_then(|v| v.as_u64()), Some(2));
  assert_eq!(
    hit.get("line_content").and_then(|v| v.as_str()),
    Some("needle value")
  );
  assert!(
    hit.get("line").is_none(),
    "GrepHit v1 should use line_content, not a line alias:\n{hit:#}"
  );
}
