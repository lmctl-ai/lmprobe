use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

fn fixture_root(name: &str) -> PathBuf {
  let root = std::env::temp_dir().join(format!(
    "lmprobe-search-auto-lang-{name}-{}",
    std::process::id()
  ));
  let _ = fs::remove_dir_all(&root);
  fs::create_dir_all(&root).expect("create fixture root");
  root
}

fn run(root: &Path, args: &[&str]) -> (String, String, i32) {
  let out = Command::new(binary())
    .args(args)
    .current_dir(root)
    .output()
    .expect("spawn lmprobe");
  (
    String::from_utf8_lossy(&out.stdout).into_owned(),
    String::from_utf8_lossy(&out.stderr).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

#[test]
fn search_pattern_infers_language_for_single_file_extension() {
  let root = fixture_root("file");
  fs::write(
    root.join("app.py"),
    "def handle(user):\n    return user\n\ndef other():\n    return None\n",
  )
  .expect("write app.py");

  let (stdout, stderr, exit) = run(
    &root,
    &[
      "--format",
      "json",
      "search",
      "-p",
      "def $NAME($$$): $$$",
      "app.py",
    ],
  );
  let envelope: serde_json::Value =
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}"));

  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  let hits = envelope["hits"].as_array().expect("hits");
  assert_eq!(hits.len(), 2, "envelope:\n{envelope:#}");
  assert!(
    hits
      .iter()
      .any(|hit| hit["name"] == "handle" && hit["lang"] == "Python"),
    "expected inferred Python hit:\n{envelope:#}"
  );

  let _ = fs::remove_dir_all(&root);
}

#[test]
fn search_pattern_still_requires_language_for_directory_without_lang() {
  let root = fixture_root("directory");
  fs::write(root.join("app.py"), "def handle(user):\n    return user\n").expect("write app.py");

  let (stdout, stderr, exit) = run(
    &root,
    &[
      "--format",
      "json",
      "search",
      "-p",
      "def $NAME($$$): $$$",
      ".",
    ],
  );

  assert_eq!(exit, 2, "stdout:\n{stdout}\nstderr:\n{stderr}");
  assert!(
    stderr.contains("requires `-l / --lang`"),
    "directory pattern searches should still ask for an explicit language:\n{stderr}"
  );

  let _ = fs::remove_dir_all(&root);
}
