//! Phase 1 execution: the ONLY place lmapi mode spawns a process. An operation runs the
//! current executable with the legacy argv in program-JSON mode and reads the
//! `lmprobe.result.v1` document from a private outfile. Phase 2 (in-process) replaces this
//! function; it is blocked by the `process::exit` sites in `cli.rs` (`run`, the parse-error
//! path and the sensor dispatch exits), which terminate instead of returning a result.

use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

/// What one legacy run produced.
#[derive(Debug)]
pub(crate) struct LegacyRun {
  /// `None` when the child was killed by a signal.
  pub exit_code: Option<i32>,
  /// The `lmprobe.result.v1` document, from the outfile or (late failures) stdout.
  pub result: Option<Value>,
  pub stderr: String,
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn outfile_path() -> PathBuf {
  let n = COUNTER.fetch_add(1, Ordering::SeqCst);
  let nanos = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_nanos())
    .unwrap_or(0);
  std::env::temp_dir().join(format!(
    "lmprobe-lmapi-{}-{nanos}-{n}.json",
    std::process::id()
  ))
}

/// Run `lmprobe --outfile <tmp> <legacy argv...>` (no shell) and collect the result document.
/// `--outfile` goes first so a `--` in the legacy argv cannot swallow it.
pub(crate) fn run_legacy(argv: &[String], stdin: Option<String>) -> Result<LegacyRun, String> {
  let exe =
    std::env::current_exe().map_err(|e| format!("cannot locate the lmprobe executable: {e}"))?;
  let outfile = outfile_path();
  let mut child = Command::new(exe)
    .arg("--outfile")
    .arg(&outfile)
    .args(argv)
    .stdin(if stdin.is_some() {
      Stdio::piped()
    } else {
      Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .map_err(|e| format!("cannot start lmprobe: {e}"))?;
  if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
    // A closed pipe (the child stopped reading) is reported through its result instead.
    let _ = pipe.write_all(text.as_bytes());
  }
  let output = child
    .wait_with_output()
    .map_err(|e| format!("lmprobe did not finish: {e}"))?;
  let from_file = std::fs::read_to_string(&outfile).ok();
  let _ = std::fs::remove_file(&outfile);
  let stdout = String::from_utf8_lossy(&output.stdout);
  let result = from_file
    .as_deref()
    .and_then(|text| serde_json::from_str::<Value>(text).ok())
    .or_else(|| serde_json::from_str::<Value>(stdout.trim()).ok());
  Ok(LegacyRun {
    exit_code: output.status.code(),
    result,
    stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
  })
}
