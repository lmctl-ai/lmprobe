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
    "lmprobe-resource-sites-{}-{suffix}",
    std::process::id()
  ));
  let _ = fs::remove_dir_all(&root);
  fs::create_dir_all(&root).expect("create fixture root");
  fs::write(
    root.join("main.go"),
    r#"package main

import "os"

func withDefer(path string) error {
    f, err := os.Open(path)
    if err != nil {
        return err
    }
    defer f.Close()
    return nil
}

func withoutRelease(path string) error {
    f, err := os.Create(path)
    if err != nil {
        return err
    }
    _ = f
    return nil
}
"#,
  )
  .expect("write go fixture");
  Fixture { root }
}

fn run_raw(root: &Path, args: &[&str]) -> (String, String, i32) {
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
  let (stdout, stderr, exit) = run_raw(root, args);
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    stderr,
    exit,
  )
}

#[test]
fn resource_sites_reports_local_acquire_and_release_evidence() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "resource-sites",
      ".",
      "--acquire",
      r"os\.Open|os\.Create",
      "--release",
      "Close",
      "--around",
      "1",
      "--parents",
      "--body",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  let sites = envelope["sites"].as_array().expect("sites");
  assert_eq!(sites.len(), 2, "envelope:\n{envelope:#}");
  assert!(
    !envelope.to_string().contains("leak")
      && !envelope.to_string().contains("vulnerability")
      && !envelope.to_string().contains("bug"),
    "schema/output should stay evidence-only:\n{envelope:#}"
  );

  let with_defer = sites
    .iter()
    .find(|site| site["enclosing"]["name"] == "withDefer")
    .expect("withDefer site");
  assert_eq!(with_defer["confidence"], "local-evidence-only");
  assert!(
    with_defer.get("acquire_timing").is_none()
      && with_defer.get("release_lifecycle").is_none()
      && with_defer.get("optional_acquire").is_none(),
    "non-JS compact evidence should omit JS/TS classifications:\n{with_defer:#}"
  );
  assert_eq!(with_defer["acquire_calls"][0]["callee"], "os.Open");
  assert!(
    with_defer["release_calls"]
      .as_array()
      .is_some_and(|calls| !calls.is_empty()),
    "withDefer should report local release evidence:\n{with_defer:#}"
  );
  assert!(
    with_defer["defer_calls"]
      .as_array()
      .is_some_and(|calls| !calls.is_empty()),
    "withDefer should report defer evidence:\n{with_defer:#}"
  );
  assert!(
    with_defer["source_context"]
      .as_array()
      .is_some_and(|lines| !lines.is_empty())
      && with_defer["parents"]
        .as_array()
        .is_some_and(|parents| !parents.is_empty())
      && with_defer.get("body").is_some(),
    "requested context/parents/body should be populated:\n{with_defer:#}"
  );

  let without_release = sites
    .iter()
    .find(|site| site["enclosing"]["name"] == "withoutRelease")
    .expect("withoutRelease site");
  assert_eq!(without_release["acquire_calls"][0]["callee"], "os.Create");
  assert!(
    without_release["release_calls"]
      .as_array()
      .is_some_and(|calls| calls.is_empty()),
    "withoutRelease should simply have no local release evidence:\n{without_release:#}"
  );
}

#[test]
fn resource_sites_reports_go_returns_after_acquire_before_release() {
  let fix = fixture();
  fs::write(
    fix.root.join("go_error_branches.go"),
    r#"package main

import (
    "fmt"
    "os"
)

func chown(*os.File) error { return nil }

func returnBeforeClose(path string) error {
    f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY, 0600)
    if err != nil {
        return err
    }
    if err := chown(f); err != nil {
        return fmt.Errorf("chown %s: %w", f.Name(), err)
    }
    return f.Close()
}

func deferBeforeReturn(path string) error {
    f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY, 0600)
    if err != nil {
        return err
    }
    defer f.Close()
    if err := chown(f); err != nil {
        return err
    }
    return nil
}
"#,
  )
  .expect("write go error-branch fixture");
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "resource-sites",
      ".",
      "--acquire",
      "OpenFile",
      "--release",
      "Close",
      "--parents",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  let sites = envelope["sites"].as_array().expect("sites");
  assert_eq!(sites.len(), 2, "envelope:\n{envelope:#}");
  assert!(
    !envelope.to_string().contains("leak")
      && !envelope.to_string().contains("vulnerability")
      && !envelope.to_string().contains("bug"),
    "resource-sites should keep verdict language out of output:\n{envelope:#}"
  );

  let return_before_close = sites
    .iter()
    .find(|site| site["enclosing"]["name"] == "returnBeforeClose")
    .expect("returnBeforeClose site");
  let branches = return_before_close["error_branches"]
    .as_array()
    .expect("error branches");
  assert_eq!(
    branches.len(),
    1,
    "only the post-acquire/pre-release return should be surfaced:\n{return_before_close:#}"
  );
  assert_eq!(branches[0]["callee"], "return");
  assert!(
    branches[0]["line_content"]
      .as_str()
      .is_some_and(|line| line.contains("fmt.Errorf")),
    "expected chown return branch, not acquire failure or Close return:\n{return_before_close:#}"
  );

  let defer_before_return = sites
    .iter()
    .find(|site| site["enclosing"]["name"] == "deferBeforeReturn")
    .expect("deferBeforeReturn site");
  assert!(
    defer_before_return["error_branches"]
      .as_array()
      .is_some_and(|branches| branches.is_empty()),
    "returns after a local defer release should not be surfaced for this narrow evidence mode:\n{defer_before_return:#}"
  );
}

#[test]
fn resource_sites_reports_typescript_lazy_acquire_and_optional_release() {
  let fix = fixture();
  fs::write(
    fix.root.join("worker.ts"),
    r#"export function useWorker() {
  let worker: Worker | undefined
  worker ??= makeScssWorker({ maxWorkers: 1 })
  worker?.stop()
  onDispose(() => worker?.stop())
}
"#,
  )
  .expect("write typescript fixture");
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "resource-sites",
      ".",
      "--acquire",
      "makeScssWorker",
      "--release",
      "stop",
      "--around",
      "1",
      "--parents",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  let sites = envelope["sites"].as_array().expect("sites");
  assert_eq!(sites.len(), 1, "envelope:\n{envelope:#}");
  let site = &sites[0];
  assert_eq!(site["acquire_calls"][0]["callee"], "makeScssWorker");
  assert!(
    site["acquire_calls"][0]["line_content"]
      .as_str()
      .is_some_and(|line| line.contains("worker ??= makeScssWorker")),
    "lazy acquire line should be preserved:\n{site:#}"
  );
  assert!(
    site["release_calls"].as_array().is_some_and(|calls| calls
      .iter()
      .any(|call| call["line_content"]
        .as_str()
        .is_some_and(|line| line.contains("worker?.stop()")))),
    "optional-chained release evidence missing:\n{site:#}"
  );
  assert!(
    !envelope.to_string().contains("bug") && !envelope.to_string().contains("leak"),
    "resource-sites should remain syntactic evidence only:\n{envelope:#}"
  );
  assert_eq!(site["acquire_timing"], "lazy_callback");
  assert_eq!(site["release_lifecycle"], "dispose_callback");
  assert_eq!(site["optional_acquire"], true);
}

#[test]
fn resource_sites_classifies_js_lifecycle_and_dynamic_import_evidence() {
  let fix = fixture();
  fs::write(
    fix.root.join("client.ts"),
    r#"const socket = new WebSocket(url)

export function connectClient() {
  const transport = createWebSocketModuleRunnerTransport()
  window.addEventListener('pagehide', () => transport.disconnect())
  return transport
}

export async function loadModule(id: string) {
  return import(id).catch(() => jiti.import(id))
}
"#,
  )
  .expect("write client fixture");
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "resource-sites",
      "client.ts",
      "--acquire",
      r"new WebSocket|createWebSocketModuleRunnerTransport|import\(",
      "--release",
      r"disconnect|pagehide",
      "--parents",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  let sites = envelope["sites"].as_array().expect("sites");
  assert_eq!(sites.len(), 3, "envelope:\n{envelope:#}");

  let module_socket = sites
    .iter()
    .find(|site| site["acquire_timing"] == "module_load")
    .expect("module-load websocket site");
  assert!(
    module_socket["acquire_calls"][0]["line_content"]
      .as_str()
      .is_some_and(|line| line.contains("new WebSocket")),
    "module-load classification should stay anchored to the top-level acquire:\n{module_socket:#}"
  );

  let websocket = sites
    .iter()
    .find(|site| site["enclosing"]["name"] == "connectClient")
    .expect("websocket site");
  assert_eq!(websocket["release_lifecycle"], "pagehide");
  assert!(
    websocket.get("acquire_timing").is_none(),
    "release lifecycle context should not reclassify a preceding plain acquire:\n{websocket:#}"
  );
  assert!(
    websocket.get("optional_acquire").is_none(),
    "plain acquisition should not be marked optional:\n{websocket:#}"
  );

  let dynamic_import = sites
    .iter()
    .find(|site| site["acquire_timing"] == "dynamic_import")
    .expect("dynamic import site");
  assert_eq!(dynamic_import["acquire_timing"], "dynamic_import");
  assert_eq!(dynamic_import["optional_acquire"], true);
  assert!(
    dynamic_import.get("release_lifecycle").is_none(),
    "dynamic import classification should not invent release lifecycle:\n{dynamic_import:#}"
  );
}

#[test]
fn resource_sites_ndjson_and_markdown_render() {
  let fix = fixture();
  let (stdout, stderr, exit) = run_raw(
    &fix.root,
    &[
      "--format",
      "ndjson",
      "resource-sites",
      ".",
      "--acquire",
      r"os\.Open|os\.Create",
      "--release",
      "Close",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nstdout:\n{stdout}");
  let lines = stdout.lines().collect::<Vec<_>>();
  assert_eq!(lines.len(), 3, "stdout:\n{stdout}");
  let item: serde_json::Value = serde_json::from_str(lines[0]).expect("site item");
  assert!(item.get("acquire_calls").is_some());
  let summary: serde_json::Value = serde_json::from_str(lines[2]).expect("summary");
  assert_eq!(summary["kind"], "summary");
  assert!(summary.get("sites").is_none());

  let (markdown, stderr, exit) = run_raw(
    &fix.root,
    &[
      "--format",
      "markdown",
      "resource-sites",
      ".",
      "--acquire",
      r"os\.Open|os\.Create",
      "--release",
      "Close",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nmarkdown:\n{markdown}");
  assert!(markdown.contains("# resource-sites"));
  assert!(markdown.contains("acquire"));
  assert!(markdown.contains("release"));
}

#[test]
fn resource_sites_graphql_matches_cli_surface() {
  let fix = fixture();
  let query = r#"
    {
      resourceSites(path: ".", acquire: "os\\.Open|os\\.Create", release: "Close", around: 1, parents: true, body: true) {
        sites {
          filePath
          enclosing { kind name lineStart lineEnd }
          acquireCalls { callee lineNumber lineContent }
          releaseCalls { callee lineNumber lineContent }
          deferCalls { callee lineNumber lineContent }
          acquireTiming
          releaseLifecycle
          optionalAcquire
          sourceContext { lineNumber lineContent }
          parents { kind name lineStart lineEnd }
          body { kind name lineStart lineEnd content }
          confidence
        }
        exitStatus
      }
    }
  "#;
  let (envelope, stderr, exit) = run_json(&fix.root, &["--format", "json", "query", query]);
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  let sites = envelope
    .pointer("/data/resourceSites/sites")
    .and_then(|value| value.as_array())
    .expect("graphql sites");
  assert_eq!(sites.len(), 2, "envelope:\n{envelope:#}");
  assert!(sites
    .iter()
    .any(|site| site["enclosing"]["name"] == "withDefer"));
}

#[test]
fn resource_sites_reports_regex_parse_failures_in_envelope() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "resource-sites",
      ".",
      "--acquire",
      "[",
      "--release",
      "Close",
    ],
  );
  assert_eq!(exit, 2, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  assert_eq!(envelope["schema"], "lmprobe.resource_sites.v1");
  assert_eq!(envelope["exit_status"], "error");
  assert_eq!(envelope["warnings"][0]["code"], "REGEX_PARSE_FAILED");
}

#[test]
fn resource_sites_reports_rust_question_mark_evidence() {
  let suffix = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| {
      // Unique within the process: parallel tests can read the same clock value.
      static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
      d.as_nanos() * 1024 + N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as u128
    })
    .unwrap_or(0);
  let root = std::env::temp_dir().join(format!(
    "lmprobe-resource-sites-rust-{}-{suffix}",
    std::process::id()
  ));
  let _ = fs::remove_dir_all(&root);
  fs::create_dir_all(root.join("src")).expect("create rust fixture root");
  fs::write(
    root.join("src/lib.rs"),
    r#"use std::fs::File;

fn open_file(path: &str) -> std::io::Result<()> {
    let f = File::open(path)?;
    drop(f);
    Ok(())
}
"#,
  )
  .expect("write rust fixture");
  let fix = Fixture { root };
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "resource-sites",
      ".",
      "--acquire",
      "File::open",
      "--release",
      "drop",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  let site = envelope["sites"]
    .as_array()
    .and_then(|sites| sites.first())
    .expect("rust resource site");
  assert_eq!(site["acquire_calls"][0]["callee"], "File::open");
  assert!(
    site["error_branches"]
      .as_array()
      .is_some_and(|branches| branches
        .iter()
        .any(|branch| branch["callee"] == "question_mark")),
    "rust try operator should be reported as local error-branch evidence:\n{site:#}"
  );
}
