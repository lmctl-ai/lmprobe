use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

fn tmp_dir(name: &str) -> PathBuf {
  let dir = std::env::temp_dir().join(format!("lmprobe-{name}-{}", std::process::id()));
  let _ = fs::remove_dir_all(&dir);
  fs::create_dir_all(&dir).expect("create tmp dir");
  dir
}

fn hit_count(envelope: &serde_json::Value) -> Option<usize> {
  envelope
    .get("hits")
    .and_then(|v| v.as_array())
    .map(Vec::len)
}

#[test]
fn def_this_file_prefers_concrete_definition_over_interface() {
  let dir = tmp_dir("def-this-file");
  let file = dir.join("service.ts");
  fs::write(
    &file,
    "interface Service {\n  run(): void;\n}\n\nclass Service {\n  run(): void { }\n}\n",
  )
  .expect("write fixture");

  let baseline = Command::new(binary())
    .args(["--format", "json", "def", "Service"])
    .arg(&file)
    .output()
    .expect("spawn lmprobe");
  let baseline_stdout = String::from_utf8_lossy(&baseline.stdout);
  let baseline_envelope: serde_json::Value =
    serde_json::from_str(&baseline_stdout).expect("baseline stdout should be JSON");
  assert_eq!(
    hit_count(&baseline_envelope),
    Some(2),
    "baseline should show the ambiguous interface + class pair\nstdout:\n{baseline_stdout}",
  );

  let out = Command::new(binary())
    .args(["--format", "json", "def", "--this-file", "Service"])
    .arg(&file)
    .output()
    .expect("spawn lmprobe");
  let _ = fs::remove_dir_all(&dir);

  let stdout = String::from_utf8_lossy(&out.stdout);
  assert_eq!(out.status.code(), Some(0), "stdout:\n{stdout}");

  let envelope: serde_json::Value = serde_json::from_str(&stdout).expect("stdout should be JSON");
  let hits = envelope
    .get("hits")
    .and_then(|v| v.as_array())
    .expect("hits should be an array");

  assert_eq!(hits.len(), 1, "stdout:\n{stdout}");
  assert_eq!(
    hits[0].get("kind").and_then(|v| v.as_str()),
    Some("class_declaration"),
    "stdout:\n{stdout}",
  );
  assert_eq!(
    hits[0].get("line_start").and_then(|v| v.as_u64()),
    Some(5),
    "stdout:\n{stdout}",
  );
}

#[test]
fn def_this_file_keeps_signature_when_it_is_the_only_hit() {
  let dir = tmp_dir("def-this-file-interface-only");
  let file = dir.join("service.ts");
  fs::write(&file, "interface Service {\n  run(): void;\n}\n").expect("write fixture");

  let out = Command::new(binary())
    .args(["--format", "json", "def", "--this-file", "Service"])
    .arg(&file)
    .output()
    .expect("spawn lmprobe");
  let _ = fs::remove_dir_all(&dir);

  let stdout = String::from_utf8_lossy(&out.stdout);
  assert_eq!(out.status.code(), Some(0), "stdout:\n{stdout}");

  let envelope: serde_json::Value = serde_json::from_str(&stdout).expect("stdout should be JSON");
  let hits = envelope
    .get("hits")
    .and_then(|v| v.as_array())
    .expect("hits should be an array");

  assert_eq!(hits.len(), 1, "stdout:\n{stdout}");
  assert_eq!(
    hits[0].get("kind").and_then(|v| v.as_str()),
    Some("interface_declaration"),
    "stdout:\n{stdout}",
  );
}

#[test]
fn def_this_file_rejects_non_file_path() {
  let dir = tmp_dir("def-this-file-non-file");

  let out = Command::new(binary())
    .args(["--format", "json", "def", "--this-file", "Service"])
    .arg(&dir)
    .output()
    .expect("spawn lmprobe");
  let _ = fs::remove_dir_all(&dir);

  let stderr = String::from_utf8_lossy(&out.stderr);
  assert_eq!(out.status.code(), Some(1), "stderr:\n{stderr}");
  assert!(
    stderr.contains("--this-file requires PATH to be a file"),
    "stderr should explain the path requirement\n{stderr}",
  );
}
