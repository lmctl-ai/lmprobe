//! The self-exec path (`child::run_legacy_with`) against stub executables that misbehave: killed
//! by a signal, silent, huge output, unparsable output. Every path maps to a structured outcome
//! and removes the private outfile.

use super::child::{run_legacy_with, LegacyRun, SpawnError};
use super::outcome;
use lmrust::server::ErrorCode;
use std::fs;
use std::path::{Path, PathBuf};

/// A `/bin/sh` stub that records its outfile argument (`$2`, after `--outfile`) and runs `body`.
struct Stub {
  dir: tempfile::TempDir,
}

impl Stub {
  fn new(body: &str) -> Stub {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("stub.sh");
    let seen = dir.path().join("outfile-path");
    fs::write(
      &script,
      format!(
        "#!/bin/sh\nprintf '%s' \"$2\" > '{}'\n{body}\n",
        seen.display()
      ),
    )
    .unwrap();
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    }
    Stub { dir }
  }
  fn exe(&self) -> PathBuf {
    self.dir.path().join("stub.sh")
  }
  fn run(&self, argv: &[&str]) -> LegacyRun {
    let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    run_legacy_with(&self.exe(), &argv, None).expect("stub starts")
  }
  /// The outfile the parent passed; it must not exist after the run.
  fn outfile(&self) -> PathBuf {
    PathBuf::from(fs::read_to_string(self.dir.path().join("outfile-path")).unwrap())
  }
  fn assert_cleaned(&self) {
    let outfile = self.outfile();
    assert!(
      outfile.starts_with(std::env::temp_dir()),
      "{}",
      outfile.display()
    );
    assert!(!outfile.exists(), "{} was left behind", outfile.display());
  }
}

#[test]
fn the_child_gets_the_outfile_first_and_the_legacy_argv_verbatim() {
  let stub = Stub::new(
    "printf '%s\\n' \"$@\" > \"$(dirname \"$0\")/argv\"\n\
     printf '{\"schema\":\"lmprobe.result.v1\",\"status\":\"ok\"}' > \"$2\"",
  );
  let run = stub.run(&["grep", "--", "-x y", "a b"]);
  assert_eq!(run.exit_code, Some(0));
  assert_eq!(run.result.unwrap()["status"], "ok");
  let argv = fs::read_to_string(stub.dir.path().join("argv")).unwrap();
  let lines: Vec<&str> = argv.lines().collect();
  assert_eq!(lines[0], "--outfile");
  assert_eq!(lines[2..], ["grep", "--", "-x y", "a b"]);
  stub.assert_cleaned();
}

#[test]
fn a_child_killed_by_a_signal_is_an_internal_error() {
  let stub = Stub::new("echo 'about to die' >&2\nkill -9 $$");
  let run = stub.run(&["git", "status"]);
  assert_eq!(run.exit_code, None);
  let err = outcome(run).unwrap_err();
  assert_eq!((err.code, err.exit_code()), (ErrorCode::InternalError, 1));
  assert_eq!(err.message, "about to die");
  assert_eq!(err.data.unwrap()["tool_code"], "SIGNALED");
  stub.assert_cleaned();
}

#[test]
fn a_child_that_writes_nothing_is_an_internal_error() {
  for (body, tool) in [
    ("exit 0", "EXIT_0"),
    ("exit 3", "EXIT_3"),
    ("exit 101", "PANIC"),
  ] {
    let stub = Stub::new(body);
    let err = outcome(stub.run(&["x"])).unwrap_err();
    assert_eq!(err.code, ErrorCode::InternalError, "{body}");
    assert_eq!(err.message, "lmprobe produced no result", "{body}");
    assert_eq!(err.data.unwrap()["tool_code"], tool, "{body}");
    stub.assert_cleaned();
  }
}

#[test]
fn unparsable_or_truncated_output_is_an_internal_error() {
  for body in [
    "printf '{\"schema\": \"lmprobe.result.v1\", \"status\":' > \"$2\"; exit 0",
    "printf 'not json' > \"$2\"; printf 'neither' ; exit 0",
    "printf '\\377\\376' > \"$2\"; exit 0",
  ] {
    let stub = Stub::new(body);
    let err = outcome(stub.run(&["x"])).unwrap_err();
    assert_eq!(err.code, ErrorCode::InternalError, "{body}");
    stub.assert_cleaned();
  }
}

#[test]
fn a_result_on_stdout_is_used_when_the_outfile_is_missing() {
  // Late failures (outfile write errors) print the program document on stdout instead.
  let stub = Stub::new(
    "printf '{\"schema\":\"lmprobe.result.v1\",\"status\":\"error\",\"diagnostics\":[{\"code\":\"OUTFILE_WRITE_FAILED\",\"message\":\"disk full\"}]}'; exit 2",
  );
  let err = outcome(stub.run(&["x"])).unwrap_err();
  assert_eq!(err.code, ErrorCode::InternalError);
  assert_eq!(err.data.unwrap()["tool_code"], "OUTFILE_WRITE_FAILED");
  stub.assert_cleaned();
}

#[test]
fn huge_stdout_stderr_and_outfile_do_not_deadlock() {
  // 8 MiB on each pipe (far beyond the pipe buffer) plus a 16 MiB result document.
  let stub = Stub::new(
    "head -c 8388608 /dev/zero | tr '\\0' 'o'\nhead -c 8388608 /dev/zero | tr '\\0' 'e' >&2\n\
     { printf '{\"schema\":\"lmprobe.result.v1\",\"status\":\"ok\",\"payload\":\"'; head -c 16777216 /dev/zero | tr '\\0' 'p'; printf '\"}'; } > \"$2\"",
  );
  let started = std::time::Instant::now();
  let run = stub.run(&["x"]);
  assert!(started.elapsed().as_secs() < 30);
  let result = outcome(run).unwrap();
  assert_eq!(result["payload"].as_str().unwrap().len(), 16 << 20);
  stub.assert_cleaned();
}

#[test]
fn a_stdin_list_reaches_the_child_and_a_closed_pipe_is_harmless() {
  let stub = Stub::new(
    "cat > \"$(dirname \"$0\")/stdin\"\nprintf '{\"schema\":\"lmprobe.result.v1\",\"status\":\"ok\"}' > \"$2\"",
  );
  let run = run_legacy_with(&stub.exe(), &["delete".into()], Some("a\nb c\n".into())).unwrap();
  assert_eq!(run.result.unwrap()["status"], "ok");
  assert_eq!(
    fs::read_to_string(stub.dir.path().join("stdin")).unwrap(),
    "a\nb c\n"
  );
  // The child exits without reading 4 MiB of stdin.
  let stub = Stub::new("exit 0");
  let big = "x\n".repeat(2 << 20);
  let run = run_legacy_with(&stub.exe(), &["delete".into()], Some(big)).unwrap();
  assert_eq!(run.exit_code, Some(0));
  stub.assert_cleaned();
}

#[test]
fn arguments_the_os_refuses_are_input_errors() {
  let stub = Stub::new("exit 0");
  let nul = run_legacy_with(&stub.exe(), &["a\0b".into()], None).unwrap_err();
  assert!(matches!(nul, SpawnError::Input(_)), "{nul:?}");
  let long = run_legacy_with(&stub.exe(), &["x".repeat(1 << 20)], None).unwrap_err();
  assert!(matches!(long, SpawnError::Input(_)), "{long:?}");
  let missing = run_legacy_with(Path::new("/nonexistent/lmprobe"), &[], None).unwrap_err();
  assert!(matches!(missing, SpawnError::Internal(_)), "{missing:?}");
}

#[test]
fn concurrent_runs_get_distinct_outfiles() {
  let stub = Stub::new(
    "sleep 0.05\nprintf '{\"schema\":\"lmprobe.result.v1\",\"status\":\"ok\",\"outfile\":\"%s\"}' \"$2\" > \"$2\"",
  );
  let outfiles: Vec<String> = std::thread::scope(|scope| {
    let handles: Vec<_> = (0..16)
      .map(|_| scope.spawn(|| outcome(stub.run(&["x"])).unwrap()))
      .collect();
    handles
      .into_iter()
      .map(|h| h.join().unwrap()["outfile"].as_str().unwrap().to_string())
      .collect()
  });
  let mut unique = outfiles.clone();
  unique.sort();
  unique.dedup();
  assert_eq!(unique.len(), 16, "{outfiles:?}");
  for f in &outfiles {
    assert!(!Path::new(f).exists(), "{f} was left behind");
  }
}
