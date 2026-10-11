//! Hostile and edge inputs in lmapi mode: no panic, no hang, exactly one JSON document on stdout,
//! and an exit code that says what happened.

mod lmapi_support;

use lmapi_support::*;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::time::Duration;

/// Run, then check the invariants every lmapi invocation keeps.
fn hostile(args: &[&str], cwd: &Path) -> (i32, Value) {
  let out = run(
    args,
    cwd,
    Opts {
      timeout: Some(Duration::from_secs(20)),
      ..Opts::default()
    },
  );
  let stderr = out.err_text();
  assert!(!stderr.contains("panicked"), "{args:?} panicked:\n{stderr}");
  let code = out
    .code
    .unwrap_or_else(|| panic!("{args:?} was killed by a signal"));
  // Exactly one JSON document and nothing else on stdout.
  let doc: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
    panic!(
      "{args:?}: stdout is not one JSON document ({e}):\n{}",
      out.text()
    )
  });
  (code, doc)
}

fn error_code(doc: &Value) -> &str {
  doc["error"]["code"].as_str().unwrap_or("<none>")
}

fn issue_code(doc: &Value) -> &str {
  doc["error"]["data"]["issues"][0]["code"]
    .as_str()
    .unwrap_or("<none>")
}

fn input_file(dir: &Path, name: &str, bytes: &[u8]) -> String {
  let path = dir.join(name);
  fs::write(&path, bytes).unwrap();
  path.display().to_string()
}

#[test]
fn reserved_names_are_ordinary_unknown_names() {
  let r = fixture();
  for key in ["__proto__", "constructor", "prototype", "toString"] {
    let body = format!(r#"{{"{key}": 1}}"#);
    let (code, doc) = hostile(&["git_status", "--input-json", &body], r.path());
    assert_eq!(
      (code, issue_code(&doc)),
      (2, "UNKNOWN_PARAM"),
      "{key}: {doc}"
    );
    let body = format!(r#"{{"op": "{key}", "params": {{}}}}"#);
    let (code, doc) = hostile(&["--input-json", &body], r.path());
    assert_eq!(
      (code, error_code(&doc)),
      (2, "INVALID_REQUEST"),
      "{key}: {doc}"
    );
    let flag = format!("--{key}=1");
    let (code, doc) = hostile(&["git_status", &flag], r.path());
    assert_eq!(code, 2, "{key}: {doc}");
    assert_eq!(error_code(&doc), "VALIDATION_ERROR");
  }
}

#[test]
fn values_starting_with_a_dash_stay_values() {
  let r = fixture();
  fs::write(
    r.path().join("dash.txt"),
    "-x marks the spot\n--schema too\n",
  )
  .unwrap();
  for args in [
    vec!["grep_text", "--path", "dash.txt", "--", "-x"],
    vec!["grep_text", "--pattern=-x", "--path=dash.txt"],
    vec![
      "grep_text",
      "--input-json",
      r#"{"pattern": "-x", "path": ["dash.txt"]}"#,
    ],
    vec![
      "grep_text",
      "--input-json",
      r#"{"pattern": "--schema", "path": ["dash.txt"]}"#,
    ],
  ] {
    let (code, doc) = hostile(&args, r.path());
    assert_eq!(
      (code, doc["status"].as_str()),
      (0, Some("ok")),
      "{args:?}: {doc}"
    );
  }
  // A dash-leading path is a path, not an option, for the legacy command.
  let (code, doc) = hostile(&["git_status", "--repo=-not-a-flag"], r.path());
  assert_eq!((code, error_code(&doc)), (4, "NOT_FOUND"), "{doc}");
}

#[test]
fn unicode_and_spaces_in_paths() {
  let r = fixture();
  let dir = r.path().join("dir with space");
  fs::create_dir_all(&dir).unwrap();
  fs::write(dir.join("ünïcødé file ☃.txt"), "needle\n").unwrap();
  let (code, doc) = hostile(
    &["grep_text", "needle", "--path", "dir with space"],
    r.path(),
  );
  assert_eq!((code, doc["status"].as_str()), (0, Some("ok")), "{doc}");
  assert!(doc.to_string().contains("ünïcødé file ☃.txt"), "{doc}");
  let (code, doc) = hostile(&["find_paths", "☃", "--path", "dir with space"], r.path());
  assert_eq!((code, doc["status"].as_str()), (0, Some("ok")), "{doc}");
  let (code, doc) = hostile(
    &[
      "git_status",
      "--input-json",
      r#"{"repo": "dir with space"}"#,
    ],
    r.path(),
  );
  assert_eq!(code, 0, "{doc}");
}

#[test]
fn empty_strings() {
  let r = fixture();
  let (code, doc) = hostile(
    &["grep_text", "--input-json", r#"{"pattern": ""}"#],
    r.path(),
  );
  assert!(code == 0 || code == 2, "empty pattern: exit {code}: {doc}");
  let (code, doc) = hostile(&["git_status", "--repo="], r.path());
  assert!(code == 0 || error_code(&doc) != "<none>", "{doc}");
  let (code, doc) = hostile(&["def_symbol", "--input-json", r#"{"name": ""}"#], r.path());
  assert!(code == 0 || code == 2, "{doc}");
  let (code, doc) = hostile(&["git_status", "--input-json", ""], r.path());
  assert_eq!(code, 2, "{doc}");
}

#[test]
fn a_one_megabyte_param_fails_cleanly() {
  // Too long for one argv entry of the legacy child (Linux MAX_ARG_STRLEN is 128 KiB): the
  // operation refuses the request as a validation error instead of crashing.
  let r = fixture();
  let big = "a".repeat(1 << 20);
  let file = input_file(
    r.path(),
    "big.json",
    json!({"repo": big}).to_string().as_bytes(),
  );
  let (code, doc) = hostile(&["git_status", "--input-file", &file], r.path());
  assert_eq!((code, error_code(&doc)), (2, "VALIDATION_ERROR"));
  assert!(doc["error"]["message"]
    .as_str()
    .unwrap()
    .starts_with("params cannot be passed to lmprobe"));
  // 100 KiB still fits and reaches the legacy command (which reports the missing repo).
  let mid = "b".repeat(100 * 1024);
  let file = input_file(
    r.path(),
    "mid.json",
    json!({"repo": mid}).to_string().as_bytes(),
  );
  let (code, doc) = hostile(&["git_status", "--input-file", &file], r.path());
  assert_eq!((code, error_code(&doc)), (4, "NOT_FOUND"));
  // Lenient coercion: a numeric string is accepted for a number param (lmapi conformance
  // `validate/types.json`); a non-numeric one is a TYPE_MISMATCH (below).
  let (code, doc) = hostile(
    &["git_log", "--input-json", r#"{"maxCount": "1"}"#],
    r.path(),
  );
  assert_eq!(
    (code, doc["metadata"]["max_count"].clone()),
    (0, json!(1)),
    "{doc}"
  );
}

#[test]
fn nul_bytes_in_values_are_validation_errors() {
  let r = fixture();
  let (code, doc) = hostile(
    &["grep_text", "--input-json", r#"{"pattern": "a\u0000b"}"#],
    r.path(),
  );
  assert_eq!((code, error_code(&doc)), (2, "VALIDATION_ERROR"), "{doc}");
  let (code, doc) = hostile(
    &["git_status", "--input-json", r#"{"repo": "\u0000"}"#],
    r.path(),
  );
  assert_eq!(code, 2, "{doc}");
}

#[test]
fn duplicate_flags_and_ops() {
  let r = fixture();
  let (code, doc) = hostile(
    &["git_log", "--max-count", "1", "--max-count", "2"],
    r.path(),
  );
  assert!(code == 0 || code == 2, "{doc}");
  if code == 0 {
    assert_eq!(doc["schema"], "lmprobe.result.v1");
  }
  // The op twice: the second is a positional git_status does not have.
  let (code, doc) = hostile(&["git_status", "git_status"], r.path());
  assert_eq!((code, error_code(&doc)), (2, "VALIDATION_ERROR"), "{doc}");
  // Two different ops.
  let (code, doc) = hostile(&["git_status", "git_log"], r.path());
  assert_eq!(code, 2, "{doc}");
}

#[test]
fn huge_arrays() {
  let r = fixture();
  let paths: Vec<String> = (0..5_000).map(|i| format!("missing-{i}")).collect();
  let file = input_file(
    r.path(),
    "paths.json",
    json!({"pattern": "x", "path": paths})
      .to_string()
      .as_bytes(),
  );
  let (code, doc) = hostile(&["grep_text", "--input-file", &file], r.path());
  assert_eq!((code, error_code(&doc)), (4, "NOT_FOUND"), "exit {code}");
  assert_eq!(doc["error"]["data"]["tool_code"], "PATH_NOT_FOUND");
}

#[test]
fn deeply_nested_json() {
  let r = fixture();
  let depth = 100_000;
  let body = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
  let file = input_file(
    r.path(),
    "deep.json",
    format!(r#"{{"pattern": {body}}}"#).as_bytes(),
  );
  let (code, doc) = hostile(&["grep_text", "--input-file", &file], r.path());
  assert_eq!(code, 2, "{doc}");
  let file = input_file(r.path(), "deep2.json", body.as_bytes());
  let (code, _) = hostile(&["--input-file", &file], r.path());
  assert_eq!(code, 2);
}

#[test]
fn input_file_problems() {
  let r = fixture();
  let file = input_file(r.path(), "bad.json", b"{\"pattern\": \"\xff\xfe\"}");
  let (code, doc) = hostile(&["grep_text", "--input-file", &file], r.path());
  assert_eq!((code, error_code(&doc)), (2, "INVALID_REQUEST"), "{doc}");
  let (code, doc) = hostile(
    &["grep_text", "--input-file", "/nonexistent/request.json"],
    r.path(),
  );
  assert_eq!((code, error_code(&doc)), (2, "INVALID_REQUEST"), "{doc}");
  let file = input_file(r.path(), "notjson.json", b"pattern: x");
  let (code, _) = hostile(&["grep_text", "--input-file", &file], r.path());
  assert_eq!(code, 2);
  for body in ["[1, 2]", "42", "null", "\"x\"", "{"] {
    let (code, doc) = hostile(&["grep_text", "--input-json", body], r.path());
    assert_eq!(code, 2, "{body}: {doc}");
  }
}

#[test]
fn mixed_and_unpublished_input_forms() {
  let r = fixture();
  let (code, doc) = hostile(
    &[
      "grep_text",
      "--input-json",
      r#"{"pattern": "x"}"#,
      "--path",
      ".",
    ],
    r.path(),
  );
  assert_eq!((code, issue_code(&doc)), (2, "MIXED_INPUT"), "{doc}");
  let (code, doc) = hostile(
    &[
      "grep_text",
      "--input-graphql",
      "{ grep_text(pattern: \"x\") }",
    ],
    r.path(),
  );
  assert_eq!((code, issue_code(&doc)), (2, "FORM_NOT_ALLOWED"), "{doc}");
  let (code, doc) = hostile(&["--input-graphql", "{ git_status }"], r.path());
  assert_eq!(code, 2, "{doc}");
}

#[test]
fn unknown_globals_and_flags() {
  let r = fixture();
  for args in [
    vec!["git_status", "--frobnicate"],
    vec!["--frobnicate", "git_status"],
    vec!["git_status", "--frobnicate=1", "--readonly"],
    vec!["git_status", "-z"],
  ] {
    let (code, doc) = hostile(&args, r.path());
    assert_eq!(
      (code, error_code(&doc)),
      (2, "VALIDATION_ERROR"),
      "{args:?}: {doc}"
    );
  }
  // lmprobe owns --format: it is not an lmapi global and not a git_status param.
  let (code, doc) = hostile(&["git_status", "--format", "json"], r.path());
  assert_eq!((code, issue_code(&doc)), (2, "UNKNOWN_PARAM"), "{doc}");
  let (code, doc) = hostile(&["--explain"], r.path());
  assert_eq!((code, error_code(&doc)), (2, "INVALID_REQUEST"), "{doc}");
  let (code, doc) = hostile(&["--explain", "nope"], r.path());
  assert_eq!((code, error_code(&doc)), (2, "INVALID_REQUEST"), "{doc}");
}

#[test]
fn type_errors_never_reach_the_legacy_command() {
  let r = fixture();
  for (args, issue) in [
    (vec!["git_log", "--max-count", "many"], "TYPE_MISMATCH"),
    (
      vec!["git_log", "--input-json", r#"{"maxCount": "five"}"#],
      "TYPE_MISMATCH",
    ),
    (
      vec!["grep_text", "--input-json", r#"{"pattern": ["a"]}"#],
      "TYPE_MISMATCH",
    ),
    (
      vec!["find_paths", "x", "--display", "colour"],
      "ENUM_VIOLATION",
    ),
    (vec!["def_symbol"], "MISSING_REQUIRED"),
  ] {
    let (code, doc) = hostile(&args, r.path());
    assert_eq!((code, issue_code(&doc)), (2, issue), "{args:?}: {doc}");
  }
}
