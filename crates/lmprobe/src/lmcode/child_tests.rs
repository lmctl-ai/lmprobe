use super::{run_legacy_with, LegacyRun, SpawnError};
use std::fs;
use std::path::{Path, PathBuf};

/// A `/bin/sh` stub that records its outfile argument (`$2`, after `--outfile`) and runs `body`.
struct Stub {
  dir: tempfile::TempDir,
}

impl Stub {
  fn new(body: &str) -> Self {
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
    Self { dir }
  }
  fn exe(&self) -> PathBuf {
    self.dir.path().join("stub.sh")
  }
  fn run(&self, argv: &[&str]) -> LegacyRun {
    run_legacy_with(
      &self.exe(),
      &argv.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
      None,
    )
    .unwrap()
  }
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
  let stub = Stub::new("printf '%s\\n' \"$@\" > \"$(dirname \"$0\")/argv\"\nprintf '{\"schema\":\"lmprobe.result.v1\",\"status\":\"ok\"}' > \"$2\"");
  let run = stub.run(&["grep", "--", "-x y", "a b"]);
  assert_eq!(run.exit_code, Some(0));
  assert_eq!(run.result.unwrap()["status"], "ok");
  let argv = fs::read_to_string(stub.dir.path().join("argv")).unwrap();
  let lines: Vec<&str> = argv.lines().collect();
  assert_eq!(lines[0], "--outfile");
  assert_eq!(&lines[2..], &["grep", "--", "-x y", "a b"]);
  stub.assert_cleaned();
}

#[test]
fn child_signal_empty_output_and_malformed_output_are_distinct() {
  let signaled = Stub::new("echo 'child signal' >&2\nkill -9 $$");
  let run = signaled.run(&["git", "status"]);
  assert_eq!(run.exit_code, None);
  assert_eq!(run.result, None);
  assert_eq!(run.stderr.trim(), "child signal");
  signaled.assert_cleaned();

  for body in [
    "exit 0",
    "exit 3",
    "exit 101",
    "printf 'not json' > \"$2\"; exit 0",
  ] {
    let stub = Stub::new(body);
    let run = stub.run(&["x"]);
    assert!(run.result.is_none(), "{body}: {:?}", run.result);
    stub.assert_cleaned();
  }
}

#[test]
fn stdout_fallback_huge_streams_and_stdin_are_collected_without_truncation() {
  let stub = Stub::new("printf '{\"schema\":\"lmprobe.result.v1\",\"status\":\"error\"}'");
  let run = stub.run(&["x"]);
  assert_eq!(run.result.unwrap()["status"], "error");
  stub.assert_cleaned();

  let stub = Stub::new("head -c 8388608 /dev/zero | tr '\\000' 'o'\nhead -c 8388608 /dev/zero | tr '\\000' 'e' >&2\n{ printf '{\"schema\":\"lmprobe.result.v1\",\"status\":\"ok\",\"payload\":\"'; head -c 16777216 /dev/zero | tr '\\000' 'p'; printf '\"}'; } > \"$2\"");
  let run = stub.run(&["x"]);
  assert!(
    run.stderr.len() >= 8 << 20,
    "stderr length: {}",
    run.stderr.len()
  );
  assert_eq!(
    run.result.unwrap()["payload"].as_str().unwrap().len(),
    16 << 20
  );
  stub.assert_cleaned();

  let stub = Stub::new("cat > \"$(dirname \"$0\")/stdin\"\nprintf '{\"schema\":\"lmprobe.result.v1\",\"status\":\"ok\"}' > \"$2\"");
  let run = run_legacy_with(&stub.exe(), &["delete".into()], Some("a\nb c\n".into())).unwrap();
  assert_eq!(run.result.unwrap()["status"], "ok");
  assert_eq!(
    fs::read_to_string(stub.dir.path().join("stdin")).unwrap(),
    "a\nb c\n"
  );
  stub.assert_cleaned();
}

#[test]
fn invalid_process_arguments_are_input_errors_and_parallel_runs_get_unique_outfiles() {
  let stub = Stub::new("exit 0");
  let nul = run_legacy_with(&stub.exe(), &["a\0b".into()], None).unwrap_err();
  assert!(matches!(nul, SpawnError::Input(_)), "{nul:?}");
  let long = run_legacy_with(&stub.exe(), &["x".repeat(1 << 20)], None).unwrap_err();
  assert!(matches!(long, SpawnError::Input(_)), "{long:?}");
  let missing = run_legacy_with(Path::new("/nonexistent/lmprobe"), &[], None).unwrap_err();
  assert!(matches!(missing, SpawnError::Internal(_)), "{missing:?}");

  let stub = Stub::new("sleep 0.05\nprintf '{\"schema\":\"lmprobe.result.v1\",\"status\":\"ok\",\"outfile\":\"%s\"}' \"$2\" > \"$2\"");
  let files: Vec<String> = std::thread::scope(|scope| {
    let handles: Vec<_> = (0..16)
      .map(|_| {
        scope.spawn(|| {
          stub.run(&["x"]).result.unwrap()["outfile"]
            .as_str()
            .unwrap()
            .to_string()
        })
      })
      .collect();
    handles
      .into_iter()
      .map(|handle| handle.join().unwrap())
      .collect()
  });
  let mut unique = files.clone();
  unique.sort();
  unique.dedup();
  assert_eq!(unique.len(), 16, "{files:?}");
  for file in files {
    assert!(!Path::new(&file).exists(), "{file} was left behind");
  }
}
