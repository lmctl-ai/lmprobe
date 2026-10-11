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

/// The legacy child must not outlive lmapi mode: an agent that kills a hung `lmprobe grep_text`
/// would otherwise leave the worker running. Linux only (`PR_SET_PDEATHSIG`); elsewhere a killed
/// parent still orphans the child.
fn die_with_parent(command: &mut Command) {
  #[cfg(target_os = "linux")]
  {
    use std::os::unix::process::CommandExt;
    extern "C" {
      fn prctl(option: i32, ...) -> i32;
      fn getppid() -> i32;
    }
    const PR_SET_PDEATHSIG: i32 = 1;
    const SIGKILL: std::os::raw::c_ulong = 9;
    let parent = std::process::id() as i32;
    // SAFETY: prctl and getppid are async-signal-safe; nothing is allocated between fork and exec.
    unsafe {
      command.pre_exec(move || {
        if prctl(PR_SET_PDEATHSIG, SIGKILL) != 0 {
          return Err(std::io::Error::last_os_error());
        }
        // The parent died before prctl took effect: do not run at all.
        if getppid() != parent {
          return Err(std::io::Error::other(
            "lmprobe exited before its child started",
          ));
        }
        Ok(())
      });
    }
  }
  #[cfg(not(target_os = "linux"))]
  let _ = command;
}

/// Why a legacy run could not start.
#[derive(Debug, PartialEq)]
pub(crate) enum SpawnError {
  /// The params cannot be passed as process arguments (a NUL byte, or an argument or argument
  /// list longer than the OS allows): a problem with the request.
  Input(String),
  /// Anything else (the executable is gone, no processes left, ...).
  Internal(String),
}

fn spawn_error(error: std::io::Error) -> SpawnError {
  // E2BIG ("Argument list too long") is 7 on Linux and macOS.
  if error.kind() == std::io::ErrorKind::InvalidInput || error.raw_os_error() == Some(7) {
    SpawnError::Input(format!("params cannot be passed to lmprobe: {error}"))
  } else {
    SpawnError::Internal(format!("cannot start lmprobe: {error}"))
  }
}

/// Run `lmprobe --outfile <tmp> <legacy argv...>` (no shell) and collect the result document.
/// `--outfile` goes first so a `--` in the legacy argv cannot swallow it.
pub(crate) fn run_legacy(argv: &[String], stdin: Option<String>) -> Result<LegacyRun, SpawnError> {
  let exe = std::env::current_exe()
    .map_err(|e| SpawnError::Internal(format!("cannot locate the lmprobe executable: {e}")))?;
  run_legacy_with(&exe, argv, stdin)
}

/// [`run_legacy`] with an explicit executable (tests run stubs that misbehave).
pub(crate) fn run_legacy_with(
  exe: &std::path::Path,
  argv: &[String],
  stdin: Option<String>,
) -> Result<LegacyRun, SpawnError> {
  let outfile = outfile_path();
  let mut command = Command::new(exe);
  die_with_parent(&mut command);
  let mut child = command
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
    .map_err(spawn_error)?;
  if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
    // A closed pipe (the child stopped reading) is reported through its result instead.
    let _ = pipe.write_all(text.as_bytes());
  }
  let output = child.wait_with_output();
  let output = match output {
    Ok(output) => output,
    Err(e) => {
      let _ = std::fs::remove_file(&outfile);
      return Err(SpawnError::Internal(format!("lmprobe did not finish: {e}")));
    }
  };
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
