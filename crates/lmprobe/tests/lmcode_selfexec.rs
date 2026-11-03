//! The lmcode operation adapter still uses the legacy CLI as its execution engine.
//! These tests pin private outfile cleanup, large results, stdin forwarding and isolation.

mod lmprobe_support;

use lmprobe_support::{fixture, lmcode, Opts};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

fn in_tmp<'a>(tmp: &Path) -> Opts<'a> {
  Opts {
    env: vec![("TMPDIR", tmp.display().to_string())],
    ..Opts::default()
  }
}

fn leftovers(tmp: &Path) -> Vec<String> {
  fs::read_dir(tmp)
    .unwrap()
    .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
    .collect()
}

fn envelope(out: &lmprobe_support::Run) -> Value {
  assert_eq!(out.code(), 0, "{}", out.err_text());
  out.json()
}

#[test]
fn private_child_output_files_are_removed_after_success_and_failure() {
  let root = fixture();
  let tmp = tempfile::tempdir().unwrap();
  let cases = [
    ("git_status", json!({}), true),
    (
      "grep_text",
      json!({"pattern":"zzz_absent_zzz","path":["."]}),
      true,
    ),
    ("git_status", json!({"repo":"/nonexistent"}), false),
    ("def_symbol", json!({}), false),
    (
      "edit_text",
      json!({"pattern":"a","replacement":"b","files":["notes.txt"]}),
      false,
    ),
    ("grep_text", json!({"pattern":"a\u{0}b"}), false),
    ("query_graphql", json!({"query":"{ nope }"}), false),
    (
      "fix_ast",
      json!({"pattern":"x.saturating_add(1)","replacement":"x + 1","path":"src","confirm":true}),
      true,
    ),
    (
      "edit_text",
      json!({"pattern":"hello","replacement":"bye","files":["notes.txt"],"confirm":true}),
      true,
    ),
  ];
  for (op, params, expected_ok) in cases {
    let fresh = (op == "fix_ast"
      || params.get("confirm").and_then(serde_json::Value::as_bool) == Some(true))
    .then(fixture);
    let cwd = fresh.as_ref().map_or(root.path(), |fixture| fixture.path());
    let result = envelope(&lmcode(op, &params, cwd, in_tmp(tmp.path())));
    assert_eq!(result["ok"], expected_ok, "{op}: {result}");
    assert_eq!(leftovers(tmp.path()), Vec::<String>::new(), "after {op}");
  }
}

#[test]
fn parallel_lmcode_invocations_keep_their_operation_results_separate() {
  let root = fixture();
  let needles = [
    "hello",
    "helper",
    "Square",
    "authorize",
    "token_key",
    "openapi",
    "fixture",
    "zzz_absent",
  ];
  let results: Vec<_> = std::thread::scope(|scope| {
    let handles: Vec<_> = needles
      .iter()
      .map(|needle| {
        let root = root.path();
        scope.spawn(move || {
          let out = lmcode(
            "grep_text",
            &json!({"pattern":needle,"path":["."]}),
            root,
            Opts::default(),
          );
          let envelope = envelope(&out);
          assert_eq!(envelope["ok"], true, "{needle}: {envelope}");
          (needle.to_string(), envelope["result"].clone())
        })
      })
      .collect();
    handles
      .into_iter()
      .map(|handle| handle.join().unwrap())
      .collect()
  });
  for (needle, result) in results {
    if needle == "zzz_absent" {
      assert_eq!(result["status"], "no_match");
    } else {
      assert_eq!(result["status"], "ok", "{needle}: {result}");
      assert!(
        result.to_string().contains(&needle),
        "{needle} not in its own result"
      );
    }
  }
}

#[test]
fn large_lmcode_operation_results_are_not_truncated() {
  let root = fixture();
  let tmp = tempfile::tempdir().unwrap();
  let lines: String = (0..20_000).map(|i| format!("needle line {i}\n")).collect();
  fs::write(root.path().join("big.txt"), lines).unwrap();
  let out = lmcode(
    "grep_text",
    &json!({"pattern":"needle line","path":["big.txt"],"limit":100_000}),
    root.path(),
    in_tmp(tmp.path()),
  );
  let envelope = envelope(&out);
  assert_eq!(envelope["ok"], true, "{envelope}");
  assert!(envelope["result"].to_string().contains("needle line 19999"));
  assert!(out.stdout.len() > 1 << 20, "{} bytes", out.stdout.len());
  assert_eq!(leftovers(tmp.path()), Vec::<String>::new());
}
