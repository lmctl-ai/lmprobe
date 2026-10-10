use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

struct Fixture {
  _tmp: tempfile::TempDir,
  root: PathBuf,
}

fn fixture() -> Fixture {
  let tmp = tempfile::Builder::new()
    .prefix("lmprobe-async-leaks-")
    .tempdir()
    .expect("create fixture root");
  let root = tmp.path().to_path_buf();
  fs::create_dir_all(&root).expect("create fixture root");
  fs::write(
    root.join("cleanup.ts"),
    r#"export function teardown(worker?: Worker) {
  worker?.stop()
  return worker.stop()
}

export async function teardownAwait(worker: Worker) {
  await worker.stop()
  return await worker.stop()
}

export function unrelated(worker: Worker) {
  worker.stopSync()
  other.stopLater()
}
"#,
  )
  .expect("write typescript fixture");
  Fixture { _tmp: tmp, root }
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
fn async_leaks_reports_dropped_awaited_returned_and_optional_call_anchors() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "async-leaks",
      ".",
      "--call",
      "stop",
      "--around",
      "1",
      "--parents",
      "--body",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  assert_eq!(envelope["schema"], "lmprobe.async_leaks.v1");
  let sites = envelope["sites"].as_array().expect("sites");
  assert_eq!(sites.len(), 4, "envelope:\n{envelope:#}");
  assert!(
    sites
      .iter()
      .all(|site| site["confidence"] == "syntax-evidence-only"),
    "runtime output should stay evidence-only:\n{envelope:#}"
  );
  assert!(
    !envelope.to_string().contains("bug") && !envelope.to_string().contains("must fix"),
    "async-leaks should not emit verdict language:\n{envelope:#}"
  );

  let optional = sites
    .iter()
    .find(|site| site["callee"] == "worker?.stop")
    .expect("optional call site");
  assert_eq!(optional["awaited"], false);
  assert_eq!(optional["returned"], false);
  assert!(
    optional["parents"]
      .as_array()
      .is_some_and(|parents| !parents.is_empty())
      && optional.get("body").is_some()
      && optional["source_context"]
        .as_array()
        .is_some_and(|context| !context.is_empty()),
    "requested context/parents/body should be populated:\n{optional:#}"
  );

  let returned = sites
    .iter()
    .find(|site| site["line_content"] == "  return worker.stop()")
    .expect("returned call site");
  assert_eq!(returned["awaited"], false);
  assert_eq!(returned["returned"], true);

  let awaited = sites
    .iter()
    .find(|site| site["line_content"] == "  await worker.stop()")
    .expect("awaited call site");
  assert_eq!(awaited["awaited"], true);
  assert_eq!(awaited["returned"], false);

  let return_await = sites
    .iter()
    .find(|site| site["line_content"] == "  return await worker.stop()")
    .expect("return awaited call site");
  assert_eq!(return_await["awaited"], true);
  assert_eq!(return_await["returned"], true);
  assert!(
    sites
      .iter()
      .all(|site| site["line_content"] != "  worker.stopSync()"),
    "non-matching call names should not be reported:\n{envelope:#}"
  );
}

#[test]
fn async_leaks_ndjson_and_graphql_match_cli_surface() {
  let fix = fixture();
  let (stdout, stderr, exit) = run_raw(
    &fix.root,
    &["--format", "ndjson", "async-leaks", ".", "--call", "stop"],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nstdout:\n{stdout}");
  let lines = stdout.lines().collect::<Vec<_>>();
  assert_eq!(lines.len(), 5, "stdout:\n{stdout}");
  let item: serde_json::Value = serde_json::from_str(lines[0]).expect("site item");
  assert!(item.get("callee").is_some(), "item:\n{item:#}");
  let summary: serde_json::Value = serde_json::from_str(lines[4]).expect("summary");
  assert_eq!(summary["kind"], "summary");
  assert!(summary.get("sites").is_none());

  let query = r#"
    {
      asyncLeaks(path: ".", call: ["stop"], around: 1, parents: true, body: true) {
        sites {
          filePath
          lineNumber
          lineContent
          callee
          awaited
          returned
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
    .pointer("/data/asyncLeaks/sites")
    .and_then(|value| value.as_array())
    .expect("graphql asyncLeaks sites");
  assert_eq!(sites.len(), 4, "envelope:\n{envelope:#}");
  assert!(sites.iter().any(|site| site["callee"] == "worker?.stop"));
}
