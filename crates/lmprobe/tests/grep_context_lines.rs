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
    "lmprobe-grep-context-lines-{}-{suffix}",
    std::process::id()
  ));
  let _ = std::fs::remove_dir_all(&root);
  std::fs::create_dir_all(&root).expect("mkdir fixture");
  std::fs::write(root.join("sample.txt"), "alpha\nneedle value\nomega\n").expect("write sample");
  std::fs::write(
    root.join("bounds.txt"),
    "needle start\nmiddle\nneedle end\n",
  )
  .expect("write bounds");
  std::fs::create_dir_all(root.join("src")).expect("mkdir src");
  std::fs::create_dir_all(root.join("node_modules/pkg")).expect("mkdir node_modules fixture");
  std::fs::write(root.join("src/app.js"), "needle source\n").expect("write source fixture");
  std::fs::write(
    root.join("node_modules/pkg/generated.js"),
    "needle dependency\n",
  )
  .expect("write dependency fixture");
  Fixture { root }
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

fn run_json(root: &Path, args: &[&str]) -> (serde_json::Value, String, i32) {
  let (stdout, stderr, exit) = run(root, args);
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    stderr,
    exit,
  )
}

fn first_hit(envelope: &serde_json::Value) -> &serde_json::Value {
  envelope
    .get("hits")
    .and_then(|v| v.as_array())
    .and_then(|hits| hits.first())
    .unwrap_or_else(|| panic!("expected at least one grep hit:\n{envelope:#}"))
}

fn hit_paths(envelope: &serde_json::Value) -> Vec<String> {
  envelope
    .get("hits")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected grep hits:\n{envelope:#}"))
    .iter()
    .filter_map(|hit| hit.get("file_path").and_then(|path| path.as_str()))
    .map(|path| path.replace('\\', "/"))
    .collect()
}

fn find_paths(envelope: &serde_json::Value) -> Vec<String> {
  envelope
    .get("paths")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected find paths:\n{envelope:#}"))
    .iter()
    .filter_map(|path| path.as_str())
    .map(|path| path.replace('\\', "/"))
    .collect()
}

fn hit_count(envelope: &serde_json::Value) -> usize {
  envelope
    .get("hits")
    .and_then(|v| v.as_array())
    .map(Vec::len)
    .unwrap_or_else(|| panic!("expected grep hits:\n{envelope:#}"))
}

fn quick_projects_fixture() -> Fixture {
  let fix = fixture();
  let js = fix.root.join("js-app/src");
  let perl = fix.root.join("perl-lib");
  let sparse_perl = fix.root.join("sparse-perl");
  let loose = fix.root.join("loose");
  std::fs::create_dir_all(&js).expect("mkdir js fixture");
  std::fs::create_dir_all(&perl).expect("mkdir perl fixture");
  std::fs::create_dir_all(&sparse_perl).expect("mkdir sparse perl fixture");
  std::fs::create_dir_all(&loose).expect("mkdir loose fixture");
  std::fs::write(fix.root.join("js-app/package.json"), r#"{"name":"js-app"}"#)
    .expect("write package marker");
  std::fs::write(
    js.join("copilot.ts"),
    "export const provider = 'copilot oauth';\n",
  )
  .expect("write js provider");
  for idx in 0..5 {
    let body = if idx == 3 {
      "package Provider::Auth; # copilot oauth\n"
    } else {
      "package Provider::Other;\n"
    };
    std::fs::write(perl.join(format!("Provider{idx}.pm")), body).expect("write perl provider");
  }
  for idx in 0..4 {
    let body = if idx == 2 {
      "package Sparse::Auth; # copilot oauth sparse\n"
    } else {
      "package Sparse::Other;\n"
    };
    std::fs::write(sparse_perl.join(format!("Sparse{idx}.pm")), body)
      .expect("write sparse perl provider");
  }
  std::fs::write(loose.join("copilot-notes.txt"), "copilot oauth loose\n")
    .expect("write loose match");
  fix
}

#[test]
fn find_and_grep_limit_quick_project_results() {
  let fix = quick_projects_fixture();

  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "--limit",
      "1",
      "find",
      "--quick-projects",
      "copilot",
      ".",
    ],
  );
  assert_eq!(exit, 0, "find failed:\n{envelope:#}\nstderr:\n{stderr}");
  assert_eq!(
    find_paths(&envelope).len(),
    1,
    "find --limit should cap quick results:\n{envelope:#}",
  );

  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "--limit",
      "1",
      "grep",
      "--quick-projects",
      "copilot oauth",
      ".",
    ],
  );
  assert_eq!(exit, 0, "grep failed:\n{envelope:#}\nstderr:\n{stderr}");
  assert_eq!(
    hit_count(&envelope),
    1,
    "grep --limit should cap quick results:\n{envelope:#}",
  );
}

#[test]
fn grep_skips_dependency_dirs_by_default_with_explicit_opt_in() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(&fix.root, &["--format", "json", "grep", "needle", "."]);

  assert_eq!(exit, 0, "grep failed:\n{envelope:#}\nstderr:\n{stderr}");
  let paths = hit_paths(&envelope);
  assert!(
    paths.iter().any(|path| path.ends_with("src/app.js")),
    "source file should be scanned:\n{paths:#?}",
  );
  assert!(
    !paths.iter().any(|path| path.contains("node_modules")),
    "dependency tree should be skipped by default:\n{paths:#?}",
  );

  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "--no-default-excludes",
      "grep",
      "needle",
      ".",
    ],
  );

  assert_eq!(exit, 0, "grep failed:\n{envelope:#}\nstderr:\n{stderr}");
  let paths = hit_paths(&envelope);
  assert!(
    paths.iter().any(|path| path.contains("node_modules")),
    "explicit opt-in should scan dependency tree:\n{paths:#?}",
  );
}

#[test]
fn grep_quick_projects_scans_manifest_and_dense_perl_roots() {
  let fix = quick_projects_fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "grep",
      "--quick-projects",
      "copilot oauth",
      ".",
    ],
  );

  assert_eq!(exit, 0, "grep failed:\n{envelope:#}\nstderr:\n{stderr}");
  let paths = hit_paths(&envelope);
  assert!(
    paths
      .iter()
      .any(|path| path.ends_with("js-app/src/copilot.ts")),
    "quick scan should include package.json project roots:\n{paths:#?}",
  );
  assert!(
    paths
      .iter()
      .any(|path| path.ends_with("perl-lib/Provider3.pm")),
    "quick scan should include dense Perl directories:\n{paths:#?}",
  );
  assert!(
    !paths.iter().any(|path| path.contains("/loose/")),
    "quick scan should skip loose matches outside candidate roots:\n{paths:#?}",
  );
  assert!(
    !paths.iter().any(|path| path.contains("/sparse-perl/")),
    "quick scan should not promote Perl dirs below the density threshold:\n{paths:#?}",
  );
}

#[test]
fn find_quick_projects_narrows_path_search_to_project_roots() {
  let fix = quick_projects_fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "find",
      "--quick-projects",
      "copilot",
      ".",
    ],
  );

  assert_eq!(exit, 0, "find failed:\n{envelope:#}\nstderr:\n{stderr}");
  let paths = find_paths(&envelope);
  assert!(
    paths
      .iter()
      .any(|path| path.ends_with("js-app/src/copilot.ts")),
    "quick find should include paths under package.json project roots:\n{paths:#?}",
  );
  assert!(
    !paths.iter().any(|path| path.contains("/loose/")),
    "quick find should skip loose paths outside candidate roots:\n{paths:#?}",
  );
  assert!(
    !paths.iter().any(|path| path.contains("/sparse-perl/")),
    "quick find should not promote Perl dirs below the density threshold:\n{paths:#?}",
  );
}

#[test]
fn grep_quick_projects_falls_back_per_markerless_root() {
  let fix = quick_projects_fixture();
  let js_root = fix.root.join("js-app");
  let sparse_root = fix.root.join("sparse-perl");
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "grep",
      "--quick-projects",
      "copilot oauth",
      "--path",
      js_root.to_str().expect("utf8 path"),
      "--path",
      sparse_root.to_str().expect("utf8 path"),
    ],
  );

  assert_eq!(exit, 0, "grep failed:\n{envelope:#}\nstderr:\n{stderr}");
  let paths = hit_paths(&envelope);
  assert!(
    paths
      .iter()
      .any(|path| path.ends_with("js-app/src/copilot.ts")),
    "quick scan should include candidates from roots that have markers:\n{paths:#?}",
  );
  assert!(
    paths
      .iter()
      .any(|path| path.ends_with("sparse-perl/Sparse2.pm")),
    "markerless roots should fall back to full scan independently:\n{paths:#?}",
  );
}

#[test]
fn grep_json_context_flags_add_bounded_line_context() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "-C",
      "1",
      "grep",
      "needle",
      "sample.txt",
    ],
  );

  assert_eq!(exit, 0, "grep failed:\n{envelope:#}\nstderr:\n{stderr}");
  let hit = first_hit(&envelope);
  assert_eq!(hit.get("line_number").and_then(|v| v.as_u64()), Some(2));
  assert_eq!(
    hit
      .pointer("/before_context/0/line_number")
      .and_then(|v| v.as_u64()),
    Some(1)
  );
  assert_eq!(
    hit
      .pointer("/before_context/0/line_content")
      .and_then(|v| v.as_str()),
    Some("alpha")
  );
  assert_eq!(
    hit
      .pointer("/after_context/0/line_number")
      .and_then(|v| v.as_u64()),
    Some(3)
  );
  assert_eq!(
    hit
      .pointer("/after_context/0/line_content")
      .and_then(|v| v.as_str()),
    Some("omega")
  );
}

#[test]
fn grep_json_omits_context_fields_without_context_flags() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &["--format", "json", "grep", "needle", "sample.txt"],
  );

  assert_eq!(exit, 0, "grep failed:\n{envelope:#}\nstderr:\n{stderr}");
  let hit = first_hit(&envelope);
  assert!(
    hit.get("before_context").is_none(),
    "compact grep hit should omit before_context:\n{hit:#}"
  );
  assert!(
    hit.get("after_context").is_none(),
    "compact grep hit should omit after_context:\n{hit:#}"
  );
}

#[test]
fn grep_json_context_respects_file_start_and_end_bounds() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "-C",
      "2",
      "grep",
      "needle",
      "bounds.txt",
    ],
  );

  assert_eq!(exit, 0, "grep failed:\n{envelope:#}\nstderr:\n{stderr}");
  let hits = envelope
    .get("hits")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected grep hits:\n{envelope:#}"));
  let first = hits
    .iter()
    .find(|hit| hit.get("line_number").and_then(|v| v.as_u64()) == Some(1))
    .expect("line 1 hit");
  let last = hits
    .iter()
    .find(|hit| hit.get("line_number").and_then(|v| v.as_u64()) == Some(3))
    .expect("line 3 hit");

  assert!(first.get("before_context").is_none());
  assert_eq!(
    first
      .pointer("/after_context/0/line_number")
      .and_then(|v| v.as_u64()),
    Some(2)
  );
  assert_eq!(
    first
      .pointer("/after_context/1/line_number")
      .and_then(|v| v.as_u64()),
    Some(3)
  );
  assert_eq!(
    last
      .pointer("/before_context/0/line_number")
      .and_then(|v| v.as_u64()),
    Some(1)
  );
  assert_eq!(
    last
      .pointer("/before_context/1/line_number")
      .and_then(|v| v.as_u64()),
    Some(2)
  );
  assert!(last.get("after_context").is_none());
}

#[test]
fn grep_ndjson_context_flags_add_line_context_to_items() {
  let fix = fixture();
  let (stdout, stderr, exit) = run(
    &fix.root,
    &[
      "--format",
      "ndjson",
      "-C",
      "1",
      "grep",
      "needle",
      "sample.txt",
    ],
  );

  assert_eq!(exit, 0, "grep failed:\n{stdout}\nstderr:\n{stderr}");
  let item: serde_json::Value = stdout
    .lines()
    .map(|line| serde_json::from_str(line).expect("ndjson line should parse"))
    .find(|value: &serde_json::Value| value.get("kind").is_none())
    .unwrap_or_else(|| panic!("expected an item line:\n{stdout}"));

  assert_eq!(
    item
      .pointer("/before_context/0/line_content")
      .and_then(|v| v.as_str()),
    Some("alpha")
  );
  assert_eq!(
    item
      .pointer("/after_context/0/line_content")
      .and_then(|v| v.as_str()),
    Some("omega")
  );
}
