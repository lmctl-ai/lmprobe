//! Every published lmapi operation runs end to end against a small offline fixture and returns a
//! `lmprobe.result.v1` document (or, for operations that need a network tool, a structured
//! envelope). AWS operations run against an `aws` stub on PATH.

mod lmapi_support;

use lmapi_support::*;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
  /// Read op on the shared fixture.
  Read,
  /// Write op on a fresh fixture, with `--confirm`.
  Write,
  /// Needs a network tool; offline it must fail with this standard code and tool_code.
  Offline(&'static str, &'static str),
  /// Covered by the aws stub tests below.
  Aws,
}
use Mode::*;

/// One invocation per published operation (JSON form; the flags form is unit-tested).
fn table() -> Vec<(&'static str, Value, Mode)> {
  vec![
    ("discover_projects", json!({"path": "."}), Read),
    (
      "search_symbols",
      json!({"name": "helper", "path": "src"}),
      Read,
    ),
    ("def_symbol", json!({"name": "helper"}), Read),
    ("ref_symbol", json!({"name": "helper"}), Read),
    ("list_kinds", json!({"lang": "rust"}), Read),
    ("callers_of", json!({"name": "helper"}), Read),
    ("imports_of", json!({"name": "os"}), Read),
    ("decorations_of", json!({"name": "require_auth"}), Read),
    ("evolution_of", json!({"name": "helper"}), Read),
    ("callees_of", json!({"name": "run"}), Read),
    ("supertypes_of", json!({"name": "Square"}), Read),
    ("subtypes_of", json!({"name": "Shape"}), Read),
    ("overrides_of", json!({"name": "area"}), Read),
    ("exports_of", json!({"name": "User"}), Read),
    ("impact_of", json!({"symbol": "helper"}), Read),
    (
      "pr_triage",
      json!({"url": "https://github.com/example/repo/pull/1"}),
      Offline("COMMAND_FAILED", "GITHUB_CLI_FAILED"),
    ),
    ("context_bundle", json!({"symbol": "User"}), Read),
    (
      "ast_diff",
      json!({"file": "src/lib.rs", "from": "HEAD~1", "to": "HEAD"}),
      Read,
    ),
    (
      "diff_context",
      json!({"from": "HEAD~1", "to": "HEAD"}),
      Read,
    ),
    (
      "resource_sites",
      json!({"acquire": "open", "release": "close", "path": "."}),
      Read,
    ),
    ("async_leaks", json!({"call": ["fetch"], "path": "."}), Read),
    (
      "crypto_sites",
      json!({"algorithm": ["md5"], "path": "."}),
      Read,
    ),
    (
      "config_keys",
      json!({"paths": ["config.yaml"], "key": "token_key"}),
      Read,
    ),
    (
      "secrets_scan",
      json!({"path": ".", "config": "rules.toml"}),
      Read,
    ),
    ("secrets_recipes_list", json!({"provider": "aws"}), Read),
    ("secrets_recipes_show", json!({"key": "aws/python"}), Read),
    ("secrets_sources_list", json!({"language": "python"}), Read),
    (
      "secrets_sources_show",
      json!({"key": "aws-secrets-manager/python"}),
      Read,
    ),
    (
      "guard_sites",
      json!({"sensitive": "delete_user", "guard": "authorize", "path": "."}),
      Read,
    ),
    ("openapi_facts", json!({"path": "openapi.yaml"}), Read),
    (
      "route_anchors",
      json!({"path": "app.py", "framework": "fastapi"}),
      Read,
    ),
    ("http_client_calls", json!({"path": "app.py"}), Read),
    ("py_slice_candidates", json!({"paths": ["app.py"]}), Read),
    (
      "callsite_args",
      json!({"callee": "authorize", "path": "."}),
      Read,
    ),
    (
      "grep_text",
      json!({"pattern": "hello", "path": ["."]}),
      Read,
    ),
    ("find_paths", json!({"pattern": "\\.py$"}), Read),
    ("git_status", json!({}), Read),
    ("git_diff", json!({"from": "HEAD~1", "to": "HEAD"}), Read),
    ("git_log", json!({"maxCount": 5}), Read),
    (
      "git_show",
      json!({"revision": "HEAD", "file": "src/lib.rs"}),
      Read,
    ),
    ("git_branch", json!({"all": true}), Read),
    ("git_remote", json!({}), Read),
    ("git_rev_parse", json!({"revision": "HEAD~1"}), Read),
    ("git_ls_files", json!({"path": ["src"]}), Read),
    ("git_blame", json!({"file": "src/lib.rs"}), Read),
    ("history_blame", json!({"target": "helper"}), Read),
    ("history_introduced", json!({"name": "helper"}), Read),
    ("history_commit", json!({"pattern": "helper"}), Read),
    ("history_diff", json!({"pattern": "helper"}), Read),
    ("history_churn", json!({"top": 3}), Read),
    ("history_co_change", json!({"file": "src/lib.rs"}), Read),
    ("history_todos", json!({}), Read),
    ("history_evolution", json!({"name": "helper"}), Read),
    ("aws_catalog", json!({}), Aws),
    (
      "aws_query",
      json!({"service": "sts", "operation": "get-caller-identity"}),
      Aws,
    ),
    (
      "edit_preview",
      json!({"pattern": "hello", "replacement": "bye", "files": ["notes.txt"]}),
      Read,
    ),
    (
      "edit_text",
      json!({"pattern": "hello", "replacement": "bye", "files": ["notes.txt"]}),
      Write,
    ),
    (
      "fix_preview",
      json!({"pattern": "x.saturating_add(1)", "replacement": "x + 1", "path": "src"}),
      Read,
    ),
    (
      "fix_ast",
      json!({"pattern": "x.saturating_add(1)", "replacement": "x + 1", "path": "src"}),
      Write,
    ),
    (
      "find_delete",
      json!({"pattern": "notes\\.txt$", "force": true}),
      Write,
    ),
    (
      "delete_paths",
      json!({"paths": ["notes.txt"], "force": true}),
      Write,
    ),
    (
      "trace_exec",
      json!({"query": "data = f.read()", "path": "app.py", "exec": "true"}),
      Write,
    ),
    (
      "query_graphql",
      json!({"query": "{ grep(pattern: \"hello\", path: \".\") { exitStatus } }"}),
      Read,
    ),
  ]
}

fn published_ops() -> BTreeSet<String> {
  let out = lm(&["--list"], Path::new(env!("CARGO_MANIFEST_DIR")));
  assert_eq!(out.code(), 0);
  out
    .json()
    .as_array()
    .unwrap()
    .iter()
    .map(|o| o["name"].as_str().unwrap().to_string())
    .collect()
}

#[test]
fn the_table_covers_every_published_operation_exactly_once() {
  let names: Vec<&str> = table().iter().map(|(n, _, _)| *n).collect();
  let unique: BTreeSet<String> = names.iter().map(|n| n.to_string()).collect();
  assert_eq!(unique.len(), names.len(), "duplicate rows");
  assert_eq!(unique, published_ops());
}

fn check_result(op: &str, out: &Run, mode: Mode) {
  let doc = out.json();
  match mode {
    Offline(code, tool) => {
      assert_eq!(doc["error"]["code"], code, "{op}: {doc}");
      assert_eq!(doc["error"]["data"]["tool_code"], tool, "{op}: {doc}");
    }
    _ => {
      assert_eq!(out.code(), 0, "{op}: {doc}\nstderr: {}", out.err_text());
      assert_eq!(doc["schema"], "lmprobe.result.v1", "{op}: {doc}");
      assert!(
        ["ok", "no_match", "partial"].contains(&doc["status"].as_str().unwrap_or_default()),
        "{op}: {doc}"
      );
      assert!(
        doc["source_schema"]
          .as_str()
          .is_some_and(|s| s.starts_with("lmprobe.")),
        "{op}: {doc}"
      );
    }
  }
}

#[test]
fn every_operation_runs_end_to_end_offline() {
  let shared = fixture();
  let rows: Vec<_> = table()
    .into_iter()
    .filter(|(_, _, mode)| *mode != Aws)
    .collect();
  let failures = std::sync::Mutex::new(Vec::new());
  std::thread::scope(|scope| {
    for chunk in rows.chunks(rows.len().div_ceil(8)) {
      let shared = shared.path();
      let failures = &failures;
      scope.spawn(move || {
        for (op, params, mode) in chunk {
          let fresh;
          let cwd = if *mode == Write {
            fresh = fixture();
            fresh.path().to_path_buf()
          } else {
            shared.to_path_buf()
          };
          let body = params.to_string();
          let mut args = vec![*op, "--input-json", &body];
          if *mode == Write {
            args.push("--confirm");
          }
          let opts = Opts {
            env: vec![("LMPROBE_GH", "/nonexistent/gh".to_string())],
            ..Opts::default()
          };
          let out = run(&args, &cwd, opts);
          let result = std::panic::catch_unwind(|| check_result(op, &out, *mode));
          if result.is_err() {
            failures.lock().unwrap().push(op.to_string());
          }
        }
      });
    }
  });
  let failures = failures.into_inner().unwrap();
  assert!(failures.is_empty(), "failed operations: {failures:?}");
}

#[test]
fn write_operations_change_the_fixture() {
  let r = fixture();
  let out = run(
    &[
      "edit_text",
      "--input-json",
      r#"{"pattern": "hello", "replacement": "bye", "files": ["notes.txt"]}"#,
      "--confirm",
    ],
    r.path(),
    Opts::default(),
  );
  assert_eq!(out.code(), 0, "{}", out.text());
  assert_eq!(
    fs::read_to_string(r.path().join("notes.txt")).unwrap(),
    "bye world\n"
  );
  let r = fixture();
  let out = lm(
    &[
      "fix_ast",
      "--pattern",
      "x.saturating_add(1)",
      "--replacement",
      "x + 1",
      "src",
      "--confirm",
    ],
    r.path(),
  );
  assert_eq!(out.code(), 0, "{}", out.text());
  assert!(fs::read_to_string(r.path().join("src/lib.rs"))
    .unwrap()
    .contains("    x + 1\n"));
  let r = fixture();
  let out = lm(
    &["find_delete", "notes\\.txt$", "--force", "--confirm"],
    r.path(),
  );
  assert_eq!(out.code(), 0, "{}", out.text());
  assert!(!r.path().join("notes.txt").exists());
}

#[test]
fn conditionally_required_params_surface_as_validation_errors() {
  // The schema cannot express "service and operation, or requestFile": the legacy parser refuses.
  let r = fixture();
  let out = lm(&["aws_query"], r.path());
  let doc = out.json();
  assert_eq!(
    (out.code(), doc["error"]["code"].clone()),
    (2, json!("VALIDATION_ERROR")),
    "{doc}"
  );
  assert_eq!(doc["error"]["data"]["tool_code"], "ARGUMENT_ERROR");
  let out = lm(&["query_graphql"], r.path());
  assert_eq!(out.json()["error"]["code"], "VALIDATION_ERROR");
  let out = lm(&["edit_preview", "--files", "notes.txt"], r.path());
  assert_eq!(out.json()["error"]["code"], "VALIDATION_ERROR");
}

// ---------------------------------------------------------------------------------------------
// AWS: a stub `aws` on PATH (no network, no credentials)
// ---------------------------------------------------------------------------------------------

/// A directory holding an `aws` stub that records its argv and prints a fixed identity.
fn aws_stub() -> tempfile::TempDir {
  let dir = tempfile::tempdir().unwrap();
  let script = dir.path().join("aws");
  fs::write(
    &script,
    format!(
      "#!/bin/sh\necho \"$@\" >> '{}/calls.log'\necho '{{\"UserId\": \"AIDEXAMPLE\", \"Account\": \"000000000000\", \"Arn\": \"arn:aws:iam::000000000000:user/fixture\"}}'\n",
      dir.path().display()
    ),
  )
  .unwrap();
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
  }
  dir
}

fn path_with(dir: &Path) -> String {
  format!(
    "{}:{}",
    dir.display(),
    std::env::var("PATH").unwrap_or_default()
  )
}

fn aws_opts(stub: &Path, home: &Path) -> Opts<'static> {
  Opts {
    env: vec![
      ("PATH", path_with(stub)),
      ("HOME", home.display().to_string()),
    ],
    remove_env: vec![
      "LMPROBE_AWS",
      "AWS_PROFILE",
      "AWS_CONFIG_FILE",
      "AWS_SHARED_CREDENTIALS_FILE",
    ],
    ..Opts::default()
  }
}

#[test]
fn aws_catalog_lists_the_read_only_allowlist() {
  let stub = aws_stub();
  let home = tempfile::tempdir().unwrap();
  let out = run(
    &["aws_catalog"],
    home.path(),
    aws_opts(stub.path(), home.path()),
  );
  check_result("aws_catalog", &out, Read);
  let text = out.text();
  assert!(text.contains("get-caller-identity"), "{text}");
  assert!(
    !stub.path().join("calls.log").exists(),
    "the catalog never runs aws"
  );
}

#[test]
fn aws_query_runs_an_allowlisted_operation_through_the_stub() {
  let stub = aws_stub();
  let home = tempfile::tempdir().unwrap();
  let out = run(
    &["aws_query", "sts", "get-caller-identity"],
    home.path(),
    aws_opts(stub.path(), home.path()),
  );
  check_result("aws_query", &out, Read);
  assert!(out.text().contains("000000000000"), "{}", out.text());
  let calls = fs::read_to_string(stub.path().join("calls.log")).unwrap();
  assert!(calls.starts_with("sts get-caller-identity"), "{calls}");
}

#[test]
fn aws_query_refuses_operations_outside_the_allowlist_without_running_aws() {
  let stub = aws_stub();
  let home = tempfile::tempdir().unwrap();
  for (service, operation) in [
    ("s3api", "delete-bucket"),
    ("iam", "create-user"),
    ("sts", "assume-role"),
  ] {
    let out = run(
      &["aws_query", service, operation],
      home.path(),
      aws_opts(stub.path(), home.path()),
    );
    let doc = out.json();
    assert_eq!(
      doc["error"]["code"], "FORBIDDEN",
      "{service} {operation}: {doc}"
    );
    assert_eq!(doc["error"]["data"]["tool_code"], "AWS_OPERATION_REFUSED");
    assert_eq!(out.code(), 6);
  }
  assert!(
    !stub.path().join("calls.log").exists(),
    "aws must not run for refused operations"
  );
}

#[test]
fn a_missing_aws_cli_is_command_not_found() {
  let empty = tempfile::tempdir().unwrap();
  let home = tempfile::tempdir().unwrap();
  let out = run(
    &["aws_query", "sts", "get-caller-identity"],
    home.path(),
    Opts {
      env: vec![
        ("PATH", empty.path().display().to_string()),
        ("HOME", home.path().display().to_string()),
      ],
      remove_env: vec!["LMPROBE_AWS"],
      ..Opts::default()
    },
  );
  let doc = out.json();
  assert_eq!(out.code(), 127, "{doc}");
  assert_eq!(doc["error"]["code"], "COMMAND_NOT_FOUND");
  assert_eq!(doc["error"]["data"]["tool_code"], "AWS_CLI_UNAVAILABLE");
}
