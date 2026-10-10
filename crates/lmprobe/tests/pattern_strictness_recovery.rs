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

fn write_python_fixture(dir: &Path) {
  fs::write(
    dir.join("model.py"),
    "def f(x: int) -> str:\n    return str(x)\n",
  )
  .expect("write fixture");
}

fn run_json(args: &[&str]) -> (serde_json::Value, String, i32) {
  let out = Command::new(binary())
    .args(args)
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
fn signature_strictness_with_ellipsis_metavar_returns_diagnostic_not_panic() {
  let dir = fixture_dir("signature-strictness-ellipsis");
  write_python_fixture(&dir);
  let dir_arg = dir.to_string_lossy().into_owned();

  let (envelope, stderr, exit) = run_json(&[
    "--format",
    "json",
    "search",
    "-p",
    "def f($$$): $$$",
    "-l",
    "python",
    "--strictness",
    "signature",
    &dir_arg,
  ]);
  let _ = fs::remove_dir_all(&dir);

  assert_eq!(exit, 1, "envelope:\n{envelope:#}\nstderr:\n{stderr}");
  assert!(
    !stderr.contains("panicked at") && !stderr.contains("Ellipsis should be matched"),
    "stderr should not contain a worker panic:\n{stderr}",
  );
  assert!(
    envelope
      .get("warnings")
      .and_then(|v| v.as_array())
      .is_some_and(|warnings| warnings.iter().any(|warning| {
        warning.get("code").and_then(|v| v.as_str())
          == Some("PATTERN_STRICTNESS_ELLIPSIS_UNSUPPORTED")
          && warning
            .get("message")
            .and_then(|v| v.as_str())
            .is_some_and(|message| message.contains("--strictness signature"))
          && warning
            .get("hint")
            .and_then(|v| v.as_str())
            .is_some_and(|hint| hint.contains("drop `--strictness signature`"))
      })),
    "envelope should carry a recovery diagnostic:\n{envelope:#}",
  );
}
