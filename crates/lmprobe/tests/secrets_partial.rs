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
    .map(|d| d.as_nanos())
    .unwrap_or(0);
  let root = std::env::temp_dir().join(format!(
    "lmprobe-secrets-partial-{}-{suffix}",
    std::process::id()
  ));
  let _ = std::fs::remove_dir_all(&root);
  std::fs::create_dir_all(&root).expect("mkdir fixture");
  std::fs::write(root.join("small.txt"), "ordinary text\n").expect("write small fixture");
  std::fs::write(root.join("large.txt"), vec![b'a'; 1_000_001]).expect("write large fixture");
  Fixture { root }
}

fn run_json(root: &Path, args: &[&str]) -> (serde_json::Value, String, i32) {
  let out = Command::new(binary())
    .args(args)
    .current_dir(root)
    .output()
    .expect("spawn lmprobe");
  let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    String::from_utf8_lossy(&out.stderr).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

#[test]
fn secrets_size_skip_marks_result_partial() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "secrets",
      "--max-target-megabytes",
      "1",
      ".",
    ],
  );

  assert_eq!(
    exit, 3,
    "secrets should return partial-coverage exit code when a selected file is skipped:\n{envelope:#}\nstderr:\n{stderr}"
  );
  assert_eq!(
    envelope.get("exit_status").and_then(|v| v.as_str()),
    Some("no_match"),
    "partial coverage should preserve the stable exit_status enum:\n{envelope:#}"
  );
  assert_eq!(
    envelope.get("partial").and_then(|v| v.as_bool()),
    Some(true),
    "partial coverage should be explicit in the secrets envelope:\n{envelope:#}"
  );
  assert!(
    envelope
      .get("warnings")
      .and_then(|v| v.as_array())
      .is_some_and(|warnings| warnings.iter().any(|warning| {
        warning.get("code").and_then(|v| v.as_str()) == Some("FILE_TOO_LARGE")
          && warning
            .get("message")
            .and_then(|v| v.as_str())
            .is_some_and(|message| message.contains("large.txt"))
      })),
    "secrets should disclose skipped selected files:\n{envelope:#}"
  );
  assert!(
    stderr.contains("FILE_TOO_LARGE"),
    "partial secrets diagnostics should mirror to stderr in JSON mode:\n{stderr}"
  );
}
