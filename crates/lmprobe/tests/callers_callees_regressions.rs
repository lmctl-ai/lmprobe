use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

struct Fixture {
  root: PathBuf,
}

impl Drop for Fixture {
  fn drop(&mut self) {
    let _ = fs::remove_dir_all(&self.root);
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
    "lmprobe-callers-callees-{}-{suffix}",
    std::process::id()
  ));
  let _ = fs::remove_dir_all(&root);
  fs::create_dir_all(&root).expect("create fixture root");
  fs::write(
    root.join("selector.go"),
    r#"package main

type Context struct{}

func (c *Context) SaveUploadedFile(name string) error {
    return nil
}

func upload(c *Context) error {
    return c.SaveUploadedFile("avatar.png")
}
"#,
  )
  .expect("write go fixture");
  fs::write(
    root.join("routing.py"),
    r#"from typing import Callable

def solve_dependencies():
    return None

def get_route_handler(route: str) -> Callable:
    solve_dependencies()
    return lambda: route
"#,
  )
  .expect("write python fixture");
  fs::write(
    root.join("server.ts"),
    r#"async function loadConfig(): Promise<void> {
  return
}

export async function _createServer(root: string): Promise<void> {
  await loadConfig()
  plugin.configure(root)
}
"#,
  )
  .expect("write typescript fixture");
  Fixture { root }
}

fn run_json(root: &Path, args: &[&str]) -> (serde_json::Value, String, i32) {
  let out = Command::new(binary())
    .args(args)
    .current_dir(root)
    .output()
    .expect("spawn lmprobe");
  let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
  let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    stderr,
    out.status.code().unwrap_or(-1),
  )
}

fn warning_codes(envelope: &serde_json::Value) -> Vec<String> {
  envelope["warnings"]
    .as_array()
    .into_iter()
    .flatten()
    .filter_map(|warning| warning["code"].as_str().map(str::to_string))
    .collect()
}

#[test]
fn callers_matches_go_selector_method_calls() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &["--format", "json", "callers", "SaveUploadedFile", "."],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  let hits = envelope["hits"].as_array().expect("hits");
  assert!(
    hits.iter().any(|hit| hit["file_path"]
      .as_str()
      .is_some_and(|path| path.ends_with("selector.go"))
      && hit["line_start"] == 10
      && hit["content"]
        .as_str()
        .is_some_and(|content| content.contains("c.SaveUploadedFile"))
      && hit["enclosing_decl"] == "upload"),
    "Go selector callsite missing:\n{envelope:#}"
  );
}

#[test]
fn callees_python_typed_definition_has_no_pattern_parse_warning() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "callees",
      "get_route_handler",
      "routing.py",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  assert!(
    !warning_codes(&envelope)
      .iter()
      .any(|code| code == "PATTERN_PARSE_FAILED"),
    "unexpected parse warning:\n{envelope:#}"
  );
  assert!(
    envelope["hits"]
      .as_array()
      .expect("hits")
      .iter()
      .any(|hit| hit["name"] == "solve_dependencies"),
    "Python callees should include inner callsites:\n{envelope:#}"
  );
}

#[test]
fn callees_typescript_typed_async_definition_has_no_pattern_parse_warning() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &["--format", "json", "callees", "_createServer", "server.ts"],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  assert!(
    !warning_codes(&envelope)
      .iter()
      .any(|code| code == "PATTERN_PARSE_FAILED"),
    "unexpected parse warning:\n{envelope:#}"
  );
  let hits = envelope["hits"].as_array().expect("hits");
  assert!(
    hits.iter().any(|hit| hit["name"] == "loadConfig")
      && hits.iter().any(|hit| hit["name"] == "configure"),
    "TypeScript callees should include function and selector calls:\n{envelope:#}"
  );
}
