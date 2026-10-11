//! Legacy error codes -> lmapi envelopes: every code lmprobe can end a run with has an explicit
//! standard code and exit code, warnings never become the reason, and a new SCREAMING_CASE code
//! anywhere in the source fails the build until it is mapped or declared a warning.

use super::child::LegacyRun;
use super::{mapped_code, outcome, standard_code, WARNING_CODES};
use lmrust::server::ErrorCode::{self, *};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

/// Every legacy error code with its standard code. Exit codes follow lmapi (`exit_of`).
const ERRORS: [(&str, ErrorCode); 69] = [
  ("ARGUMENT_ERROR", ValidationError),
  ("PATTERN_LANGUAGE_REQUIRED", ValidationError),
  ("PROGRAM_FORMAT_CONFLICT", ValidationError),
  ("INFILE_COMMAND_CONFLICT", ValidationError),
  ("STREAMING_COMMAND", ValidationError),
  ("MISSING_SCOPE", ValidationError),
  ("AMBIGUOUS_SCOPE", ValidationError),
  ("INVALID_SCOPE", ValidationError),
  ("INVALID_MARKER", ValidationError),
  ("INVALID_TRACE_MODE", ValidationError),
  ("INVALID_ACTION", ValidationError),
  ("FILE_PREDICATE_INVALID", ValidationError),
  ("PATTERN_PARSE_FAILED", ValidationError),
  ("REGEX_PARSE_FAILED", ValidationError),
  ("RULE_PARSE_FAILED", ValidationError),
  ("TRACE_JSON_REQUIRED", ValidationError),
  ("TRACE_WATCH_LANG_UNSUPPORTED", ValidationError),
  ("TRACE_WATCH_IDENTIFIER_UNSUPPORTED", ValidationError),
  ("NO_TRACE_TEMPLATE", ValidationError),
  ("CYTHON_NOT_SUPPORTED", ValidationError),
  ("NOT_IMPLEMENTED", ValidationError),
  ("GIT_WORKSPACE_UNSUPPORTED", ValidationError),
  ("AWS_WORKSPACE_UNSUPPORTED", ValidationError),
  ("AWS_PAGINATION_UNSUPPORTED", ValidationError),
  ("AWS_PAGINATION_INVALID", ValidationError),
  ("AWS_PARAMETER_INVALID", ValidationError),
  ("AWS_PARAMETERS_INVALID", ValidationError),
  ("AWS_REQUEST_INVALID", ValidationError),
  ("AWS_REQUEST_SCHEMA", ValidationError),
  ("AWS_SELECT_INVALID", ValidationError),
  ("AWS_TIMEOUT_INVALID", ValidationError),
  ("DIRTY_TREE", Conflict),
  ("EDIT_DIRTY_TREE", Conflict),
  ("EDIT_NON_GIT", Conflict),
  ("WORKSPACE_READ_ONLY", Conflict),
  ("NOT_GIT_REPO", NotFound),
  ("GIT_REPOSITORY_ERROR", NotFound),
  ("GIT_REVISION_ERROR", NotFound),
  ("PATH_NOT_FOUND", NotFound),
  ("SYMBOL_NOT_FOUND", NotFound),
  ("DELETE_REFUSED", Forbidden),
  ("FIX_REFUSED", Forbidden),
  ("TRACE_REFUSED", Forbidden),
  ("UNTRACE_REFUSED", Forbidden),
  ("AWS_OPERATION_REFUSED", Forbidden),
  ("AWS_QUERY_TIMEOUT", Timeout),
  ("AWS_CLI_UNAVAILABLE", CommandNotFound),
  ("OUTFILE_PREPARE_FAILED", InternalError),
  ("OUTFILE_SOURCE_CONFLICT", InternalError),
  ("OUTFILE_WRITE_FAILED", InternalError),
  ("COMMAND_FAILED", CommandFailed),
  ("GRAPHQL_ERROR", CommandFailed),
  ("AWS_CLI_ERROR", CommandFailed),
  ("AWS_CONFIG_RESOLUTION_FAILED", CommandFailed),
  ("AWS_ERROR_CAPTURE_FAILED", CommandFailed),
  ("AWS_JSON_INVALID", CommandFailed),
  ("AWS_OUTPUT_CAPTURE_FAILED", CommandFailed),
  ("AWS_OUTPUT_TOO_LARGE", CommandFailed),
  ("GITHUB_CLI_FAILED", CommandFailed),
  ("GITHUB_CLI_JSON", CommandFailed),
  ("GIT_BINARY_BLAME", CommandFailed),
  ("GIT_STATUS_FAILED", CommandFailed),
  ("AUDIT_LOG_WRITE_FAILED", CommandFailed),
  ("DELETE_AUDIT_FAILED", CommandFailed),
  ("EDIT_WRITE_FAILED", CommandFailed),
  ("FILE_SELECTION_FAILED", CommandFailed),
  ("FIX_FAILED", CommandFailed),
  ("PATCH_APPLY_FAILED", CommandFailed),
  ("PATCH_CAPTURE_FAILED", CommandFailed),
];

/// The remaining error codes (kept short of the array above for readability).
const MORE_ERRORS: [(&str, ErrorCode); 4] = [
  ("TRACE_FAILED", CommandFailed),
  ("WORKTREE_CLEANUP_FAILED", CommandFailed),
  ("UNTRACE_FAILED", CommandFailed),
  ("WORKTREE_PROVISION_FAILED", CommandFailed),
];

/// Not diagnostics: environment variables, history change types, serde attributes and test
/// fixture strings that happen to be SCREAMING_CASE.
const NOT_DIAGNOSTICS: [&str; 30] = [
  "AWS_CA_BUNDLE",
  "AWS_CLI_AUTO_PROMPT",
  "AWS_CONFIG_FILE",
  "AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE",
  "AWS_CREDENTIAL_FILE",
  "AWS_MAX_ATTEMPTS",
  "AWS_PAGER",
  "AWS_SHARED_CREDENTIALS_FILE",
  "AWS_WEB_IDENTITY_TOKEN_FILE",
  "BOTO_CONFIG",
  "CARGO_MANIFEST_DIR",
  "CARGO_PKG_VERSION",
  "LMPROBE_AWS",
  "LMPROBE_GH",
  "LMPROBE_GIT_SHA",
  "XDG_CACHE_HOME",
  "BODY_REWRITE",
  "LOGIC_CHANGE",
  "PARAM_ADDED",
  "PARAM_REMOVED",
  "PARAM_RENAMED",
  "RETURN_TYPE_CHANGED",
  "SIGNATURE_CHANGE",
  "SCREAMING_SNAKE_CASE",
  "FUNCTION_B",
  "MODULE_A",
  "MODULE_SECRET",
  "OTHER_SECRET_AKIA222222222222",
  "OUTER_SECRET",
  "REAL_SECRET_AKIA111111111111",
];

/// Secrets provider/source database validation codes: reported by `lmprobe-secrets` while
/// loading its databases, as warnings of the secrets sensors.
const SECRETS_DB_CODES: [&str; 7] = [
  "PROVIDER_DB_DEPRECATED_FIELD",
  "PROVIDER_DB_SCHEMA_MISMATCH",
  "PROVIDER_DB_UNKNOWN_FIELD",
  "SOURCE_DB_DEPRECATED_FIELD",
  "SOURCE_DB_GENERIC_FAST_PATH_TOKEN",
  "SOURCE_DB_SCHEMA_MISMATCH",
  "SOURCE_DB_UNKNOWN_FIELD",
];

fn errors() -> impl Iterator<Item = (&'static str, ErrorCode)> {
  ERRORS.into_iter().chain(MORE_ERRORS)
}

fn exit_of(code: ErrorCode) -> i32 {
  match code {
    ValidationError => 2,
    NotFound => 4,
    Conflict => 5,
    Forbidden => 6,
    Timeout => 7,
    CommandNotFound => 127,
    InternalError | CommandFailed => 1,
    other => panic!("unexpected standard code {other:?}"),
  }
}

fn failed_run(diagnostics: Value) -> LegacyRun {
  LegacyRun {
    exit_code: Some(2),
    result: Some(json!({
      "schema": "lmprobe.result.v1",
      "status": "error",
      "diagnostics": diagnostics,
    })),
    stderr: String::new(),
  }
}

#[test]
fn every_legacy_error_code_maps_to_its_standard_code_exit_and_tool_code() {
  for (legacy, expected) in errors() {
    assert_eq!(mapped_code(legacy), Some(expected), "{legacy}");
    let err = outcome(failed_run(json!([{"code": legacy, "message": "why"}]))).unwrap_err();
    assert_eq!(err.code, expected, "{legacy}");
    assert_eq!(err.exit_code(), exit_of(expected), "{legacy}");
    assert_eq!(err.message, "why", "{legacy}");
    let data = err.data.unwrap();
    assert_eq!(data["tool_code"], legacy);
    assert_eq!(data["legacy_exit_code"], 2);
    assert_eq!(data["result"]["status"], "error");
  }
}

#[test]
fn warnings_never_become_the_reason_of_a_failure() {
  for warning in WARNING_CODES.iter().chain(&SECRETS_DB_CODES) {
    assert_eq!(mapped_code(warning), None, "{warning}");
    assert_eq!(standard_code(warning), CommandFailed, "{warning}");
  }
  // A partial delete selection: the read warning comes first, the refusal is the reason.
  let err = outcome(failed_run(json!([
    {"code": "FILE_READ_FAILED", "message": "unreadable"},
    {"code": "DELETE_REFUSED", "message": "selection is incomplete; no files deleted"},
  ])))
  .unwrap_err();
  assert_eq!((err.code, err.exit_code()), (Forbidden, 6));
  assert_eq!(err.data.as_ref().unwrap()["tool_code"], "DELETE_REFUSED");
  assert_eq!(err.message, "selection is incomplete; no files deleted");
  // Only warnings: the first one is reported as a command failure.
  let err = outcome(failed_run(
    json!([{"code": "NO_FILES_SCANNED", "message": "m"}]),
  ))
  .unwrap_err();
  assert_eq!(
    (err.code, err.data.unwrap()["tool_code"].clone()),
    (CommandFailed, json!("NO_FILES_SCANNED"))
  );
  // No diagnostics at all.
  let err = outcome(failed_run(json!([]))).unwrap_err();
  assert_eq!(
    (err.code, err.data.unwrap()["tool_code"].clone()),
    (CommandFailed, json!("COMMAND_FAILED"))
  );
}

#[test]
fn runs_without_a_result_are_internal_errors() {
  let run = |exit_code, stderr: &str| LegacyRun {
    exit_code,
    result: None,
    stderr: stderr.to_string(),
  };
  for (exit, tool) in [
    (None, "SIGNALED"),
    (Some(101), "PANIC"),
    (Some(0), "EXIT_0"),
    (Some(3), "EXIT_3"),
  ] {
    let err = outcome(run(exit, "first\nlast\n")).unwrap_err();
    assert_eq!((err.code, err.exit_code()), (InternalError, 1), "{exit:?}");
    assert_eq!(err.message, "last");
    let data = err.data.unwrap();
    assert_eq!(data["tool_code"], tool);
    assert_eq!(data["legacy_exit_code"], json!(exit));
  }
  assert_eq!(
    outcome(run(Some(2), "")).unwrap_err().message,
    "lmprobe produced no result"
  );
}

#[test]
fn unknown_codes_fall_back_to_command_failed_and_families_still_apply() {
  assert_eq!(mapped_code("SOMETHING_NEW"), None);
  assert_eq!(standard_code("SOMETHING_NEW"), CommandFailed);
  assert_eq!(mapped_code("NEW_DIRTY_TREE"), Some(Conflict));
  assert_eq!(mapped_code("WIDGET_NOT_FOUND"), Some(NotFound));
  assert_eq!(mapped_code("WIDGET_REFUSED"), Some(Forbidden));
  assert_eq!(mapped_code("WIDGET_TIMEOUT"), Some(Timeout));
  assert_eq!(mapped_code("OUTFILE_ANYTHING"), Some(InternalError));
}

/// SCREAMING_CASE codes in the product sources: quoted literals (`"DIRTY_TREE"`) and the
/// `"CODE: message"` prefixes that `cli.rs` turns into program-JSON error codes.
fn source_codes() -> BTreeSet<String> {
  let root = Path::new(env!("CARGO_MANIFEST_DIR"));
  let dirs = [
    root.join("src"),
    root.join("../lmprobe-engine/src"),
    root.join("../lmprobe-types/src"),
    root.join("../lmprobe-secrets/src"),
  ];
  let literal = regex::Regex::new(r#""([A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+)(?:"|: )"#).unwrap();
  let mut codes = BTreeSet::new();
  let mut stack: Vec<std::path::PathBuf> = dirs.to_vec();
  while let Some(path) = stack.pop() {
    if path.ends_with("src/lmapi") {
      continue; // the mapping tables themselves
    }
    if path.is_dir() {
      stack.extend(std::fs::read_dir(&path).unwrap().map(|e| e.unwrap().path()));
    } else if path.extension().is_some_and(|e| e == "rs") {
      let text = std::fs::read_to_string(&path).unwrap();
      codes.extend(literal.captures_iter(&text).map(|c| c[1].to_string()));
    }
  }
  codes
}

#[test]
fn every_code_in_the_source_is_mapped_or_declared() {
  let codes = source_codes();
  assert!(
    codes.len() > 100,
    "the scan found only {} codes",
    codes.len()
  );
  let declared: BTreeSet<&str> = errors()
    .map(|(c, _)| c)
    .chain(WARNING_CODES)
    .chain(SECRETS_DB_CODES)
    .chain(NOT_DIAGNOSTICS)
    .collect();
  let undeclared: Vec<&String> = codes
    .iter()
    .filter(|c| !declared.contains(c.as_str()))
    .collect();
  assert!(
    undeclared.is_empty(),
    "new legacy codes: map them in lmapi::mapped_code (and ERRORS here) or declare them in \
     WARNING_CODES / NOT_DIAGNOSTICS: {undeclared:?}"
  );
  // No stale entries: every declared code still exists in the source.
  let stale: Vec<&&str> = declared.iter().filter(|c| !codes.contains(**c)).collect();
  assert!(stale.is_empty(), "codes no longer in the source: {stale:?}");
}

#[test]
fn the_tables_are_disjoint() {
  let mut seen = BTreeSet::new();
  for code in errors()
    .map(|(c, _)| c)
    .chain(WARNING_CODES)
    .chain(SECRETS_DB_CODES)
    .chain(NOT_DIAGNOSTICS)
  {
    assert!(seen.insert(code), "{code} is listed twice");
  }
}
