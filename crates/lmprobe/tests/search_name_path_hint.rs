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
    "lmprobe-search-name-path-hint-{}-{suffix}",
    std::process::id()
  ));
  let _ = std::fs::remove_dir_all(&root);
  std::fs::create_dir_all(root.join("src")).expect("mkdir fixture");
  std::fs::write(root.join("src/lib.rs"), "pub fn css_plugin() {}\n").expect("write fixture");
  Fixture { root }
}

fn run_search_name(root: &Path, path: &str) -> (serde_json::Value, String, i32) {
  run_search_name_args(root, &["--format", "json", "search", "--name", "css", path])
}

fn run_search_name_args(root: &Path, args: &[&str]) -> (serde_json::Value, String, i32) {
  let out = Command::new(binary())
    .args(args)
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

fn search_name_positional_warning_hint(envelope: &serde_json::Value) -> &str {
  envelope
    .get("warnings")
    .and_then(|v| v.as_array())
    .and_then(|warnings| {
      warnings.iter().find_map(|warning| {
        (warning.get("code").and_then(|v| v.as_str()) == Some("SEARCH_NAME_POSITIONAL_AS_PATH"))
          .then(|| warning.get("hint").and_then(|v| v.as_str()))
          .flatten()
      })
    })
    .unwrap_or_else(|| panic!("missing SEARCH_NAME_POSITIONAL_AS_PATH warning:\n{envelope:#}"))
}

fn assert_matched(envelope: &serde_json::Value, stderr: &str, exit: i32) {
  assert_eq!(exit, 0, "search failed:\n{envelope:#}\nstderr:\n{stderr}");
  assert_eq!(
    envelope.get("exit_status").and_then(|v| v.as_str()),
    Some("matched"),
    "search should match fixture symbol:\n{envelope:#}"
  );
}

fn assert_no_search_name_path_warning(envelope: &serde_json::Value) {
  assert!(
    envelope
      .get("warnings")
      .and_then(|v| v.as_array())
      .map_or(true, |warnings| warnings.iter().all(|warning| {
        warning.get("code").and_then(|v| v.as_str()) != Some("SEARCH_NAME_POSITIONAL_AS_PATH")
      })),
    "should not emit SEARCH_NAME_POSITIONAL_AS_PATH warning:\n{envelope:#}"
  );
}

#[test]
fn search_name_current_dir_recipe_is_quiet() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_search_name(&fix.root, ".");

  assert_matched(&envelope, &stderr, exit);
  assert_no_search_name_path_warning(&envelope);
}

#[test]
fn search_name_directory_path_hint_uses_cd_directory() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_search_name(&fix.root, "src");

  assert_matched(&envelope, &stderr, exit);
  let hint = search_name_positional_warning_hint(&envelope);
  assert!(
    hint.contains("directory path was accepted")
      && hint.contains("--path <DIR>")
      && hint.contains("cd <DIR>"),
    "directory hint should be directory-safe:\n{hint}"
  );
}

#[test]
fn search_name_file_path_hint_does_not_suggest_cd_into_file() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_search_name(&fix.root, "src/lib.rs");

  assert_matched(&envelope, &stderr, exit);
  let hint = search_name_positional_warning_hint(&envelope);
  assert!(
    hint.contains("file path was accepted") && hint.contains("--path <FILE>"),
    "file hint should describe a file-safe form:\n{hint}"
  );
  assert!(
    !hint.contains("cd <DIR>") && !hint.contains("cd <PATH>"),
    "file hint should not suggest cd into the file path:\n{hint}"
  );
}

#[test]
fn search_name_explicit_path_flag_avoids_auto_swap_warning() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_search_name_args(
    &fix.root,
    &[
      "--format",
      "json",
      "search",
      "--name",
      "css",
      "--path",
      "src/lib.rs",
    ],
  );

  assert_matched(&envelope, &stderr, exit);
  assert_no_search_name_path_warning(&envelope);
}

#[test]
fn search_pattern_explicit_path_flag_matches_without_clap_conflict() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_search_name_args(
    &fix.root,
    &[
      "--format",
      "json",
      "search",
      "-p",
      "pub fn $NAME() {}",
      "-l",
      "rust",
      "--path",
      "src/lib.rs",
    ],
  );

  assert_matched(&envelope, &stderr, exit);
  assert!(
    envelope
      .get("warnings")
      .and_then(|v| v.as_array())
      .is_some_and(|warnings| warnings.is_empty()),
    "pattern-mode --path should not introduce warnings:\n{envelope:#}"
  );
}

#[test]
fn search_body_flag_is_accepted_for_symbol_search() {
  let fix = fixture();
  std::fs::write(
    fix.root.join("src/lib.rs"),
    "pub fn css_plugin() {\n    println!(\"css\");\n}\n",
  )
  .expect("rewrite fixture");

  let (envelope, stderr, exit) = run_search_name_args(
    &fix.root,
    &["--format", "json", "search", "--name", "css", "--body", "."],
  );

  assert_matched(&envelope, &stderr, exit);
  assert_no_search_name_path_warning(&envelope);
  let content = envelope["hits"][0]["content"]
    .as_str()
    .expect("hit content should be a string");
  assert!(
    content.contains("println!"),
    "`search --body` should return full declaration content:\n{envelope:#}"
  );
}
