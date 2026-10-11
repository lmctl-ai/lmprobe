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
    .map(|d| {
      // Unique within the process: parallel tests can read the same clock value.
      static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
      d.as_nanos() * 1024 + N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as u128
    })
    .unwrap_or(0);
  let root = std::env::temp_dir().join(format!(
    "lmprobe-def-class-filter-{}-{suffix}",
    std::process::id()
  ));
  let _ = std::fs::remove_dir_all(&root);
  std::fs::create_dir_all(&root).expect("mkdir fixture");
  std::fs::write(
    root.join("strategy.py"),
    r#"class OpStrategy:
    def __str__(self):
        return "op"

class OtherStrategy:
    def __str__(self):
        return "other"
"#,
  )
  .expect("write fixture");
  std::fs::write(
    root.join("widget.rs"),
    r#"struct Widget;
struct Other;

impl Widget {
    fn render(&self) -> &'static str {
        "widget"
    }
}

impl Other {
    fn render(&self) -> &'static str {
        "other"
    }
}
"#,
  )
  .expect("write rust fixture");
  Fixture { root }
}

fn run_json(root: &Path, args: &[&str]) -> (serde_json::Value, String, i32) {
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

#[test]
fn def_class_filter_matches_rust_impl_container() {
  let fix = fixture();
  let (filtered, filtered_stderr, filtered_exit) = run_json(
    &fix.root,
    &[
      "--format", "json", "-G", "**/*.rs", "def", "--class", "Widget", "render", ".",
    ],
  );

  assert_eq!(
    filtered_exit, 0,
    "Rust impl class filter should match one method:\n{filtered:#}\nstderr:\n{filtered_stderr}"
  );
  let filtered_hits = filtered
    .get("hits")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected filtered hits:\n{filtered:#}"));
  assert_eq!(
    filtered_hits.len(),
    1,
    "Rust impl class filter should narrow to Widget::render:\n{filtered:#}"
  );
  let hit = &filtered_hits[0];
  assert_eq!(hit.get("name").and_then(|v| v.as_str()), Some("render"));
  assert_eq!(
    hit.get("enclosing_decl").and_then(|v| v.as_str()),
    Some("Widget"),
    "filtered Rust hit should expose impl target:\n{hit:#}"
  );
  assert_eq!(
    hit.get("enclosing_decl_kind").and_then(|v| v.as_str()),
    Some("impl_item"),
    "filtered Rust hit should expose impl container kind:\n{hit:#}"
  );
}

#[test]
fn def_class_filter_narrows_common_method_names() {
  let fix = fixture();
  let (unfiltered, unfiltered_stderr, unfiltered_exit) =
    run_json(&fix.root, &["--format", "json", "def", "__str__", "."]);

  assert_eq!(
    unfiltered_exit, 0,
    "unfiltered def should match both methods:\n{unfiltered:#}\nstderr:\n{unfiltered_stderr}"
  );
  let unfiltered_hits = unfiltered
    .get("hits")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected unfiltered hits:\n{unfiltered:#}"));
  assert_eq!(
    unfiltered_hits.len(),
    2,
    "unfiltered def should keep default behavior:\n{unfiltered:#}"
  );

  let (filtered, filtered_stderr, filtered_exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "def",
      "--class",
      "OpStrategy",
      "__str__",
      ".",
    ],
  );

  assert_eq!(
    filtered_exit, 0,
    "class-filtered def should match one method:\n{filtered:#}\nstderr:\n{filtered_stderr}"
  );
  let filtered_hits = filtered
    .get("hits")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected filtered hits:\n{filtered:#}"));
  assert_eq!(
    filtered_hits.len(),
    1,
    "class filter should narrow to one hit:\n{filtered:#}"
  );
  let hit = &filtered_hits[0];
  assert_eq!(hit.get("name").and_then(|v| v.as_str()), Some("__str__"));
  assert_eq!(
    hit.get("enclosing_decl").and_then(|v| v.as_str()),
    Some("OpStrategy"),
    "filtered hit should expose enclosing class:\n{hit:#}"
  );
  assert_eq!(
    hit.get("enclosing_decl_kind").and_then(|v| v.as_str()),
    Some("class_definition"),
    "filtered hit should expose enclosing class kind:\n{hit:#}"
  );
}

#[test]
fn query_def_class_name_matches_cli_filter() {
  let fix = fixture();
  let query = r#"{ def(name: "__str__", path: ".", className: "OpStrategy") { hits { name enclosing { name kind } } exitStatus } }"#;
  let (envelope, stderr, exit) = run_json(&fix.root, &["--format", "json", "query", query]);

  assert_eq!(
    exit, 0,
    "GraphQL def className should match:\n{envelope:#}\nstderr:\n{stderr}"
  );
  assert_eq!(
    envelope
      .pointer("/data/def/exitStatus")
      .and_then(|v| v.as_str()),
    Some("MATCHED")
  );
  let hits = envelope
    .pointer("/data/def/hits")
    .and_then(|v| v.as_array())
    .unwrap_or_else(|| panic!("expected GraphQL def hits:\n{envelope:#}"));
  assert_eq!(hits.len(), 1, "className should narrow hits:\n{envelope:#}");
  assert_eq!(
    hits[0].pointer("/enclosing/name").and_then(|v| v.as_str()),
    Some("OpStrategy")
  );
}
