//! lmapi mode: lmprobe as a native lmapi tool (lmapi protocol section 6.1), served by lmrust.
//!
//! `cli::run()` calls [`maybe_run`] before clap parsing. Only lmapi-shaped argv (a published
//! operation name such as `git_status`, or a root lmapi global such as `--schema` or
//! `--input-json`) is handled here; every other invocation returns `None` and keeps the
//! legacy parser, output and exit codes unchanged. Published operation names never equal a
//! legacy single-word subcommand, so `lmprobe def X` stays legacy.
//!
//! In lmapi mode every result is one JSON document on stdout and exit codes are lmapi's: a
//! legacy `no_match` or `partial` is a successful result (exit 0) whose `status` says so.

mod child;
mod generate;
mod overlay;

use generate::{Binding, OpPlan, Slot, ValueKind};
use lmrust::server::{check_policy, CheckContext, CliError, CliSpec, ErrorCode, OpDef, Policy};
use lmrust::{Access, InputForm, Record};
use serde_json::Value;
use std::ffi::OsString;
use std::rc::Rc;

/// The tool name: also the env floor prefix (`LMPROBE_READONLY`, `LMPROBE_DRYRUN`).
const TOOL: &str = "lmprobe";

/// Run lmapi mode when `args` (the arguments after the binary name) are lmapi-shaped.
pub(crate) fn maybe_run(args: &[OsString]) -> Option<i32> {
  // Non-UTF-8 argv is never lmapi-shaped (lmapi values are JSON strings).
  let argv: Vec<String> = args
    .iter()
    .map(|a| a.to_str().map(str::to_string))
    .collect::<Option<_>>()?;
  if !could_be_lmapi(&argv) {
    return None; // legacy argv never pays for building the spec from clap
  }
  let spec = spec().unwrap_or_else(|error| panic!("lmapi spec: {error}"));
  lmrust::server::maybe_run_cli(&spec, argv)
}

/// Spellings of lmrust's root trigger globals (`--schema`, `--input-json`, policy flags ...).
const TRIGGER_FLAGS: [&str; 15] = [
  "--schema",
  "--list",
  "--explain",
  "--input-json",
  "--inputJson",
  "--input-file",
  "--inputFile",
  "--input-graphql",
  "--inputGraphql",
  "--dryrun",
  "--dry-run",
  "--readonly",
  "--read-only",
  "--confirm",
  "--i-understand",
];

/// A necessary condition for [`lmrust::server::is_lmapi_invocation`]: some token is a published
/// operation name (from the overlay, no clap walk) or spells a trigger flag.
fn could_be_lmapi(argv: &[String]) -> bool {
  let flag = |token: &str| {
    let name = token.split('=').next().unwrap_or(token);
    let name = name
      .strip_prefix("--no-")
      .map(|n| format!("--{n}"))
      .unwrap_or_else(|| name.to_string());
    TRIGGER_FLAGS.contains(&name.as_str())
  };
  if argv.iter().any(|a| flag(a)) {
    return true;
  }
  let Ok(overlay) = overlay::parse(overlay::OVERLAY_TOML) else {
    return true;
  };
  argv
    .iter()
    .any(|a| overlay.ops.iter().any(|op| &op.name == a))
}

/// Operation plans generated from clap and the overlay.
pub(crate) fn plans() -> Result<Vec<OpPlan>, String> {
  let overlay = overlay::parse(overlay::OVERLAY_TOML)?;
  generate::plans(&crate::cli::command(), &overlay)
}

/// The lmapi server spec: one op per plan, `format` global dropped (lmprobe owns `--format`),
/// flags and JSON inputs only, no `product` (lmprobe is its own product).
pub(crate) fn spec() -> Result<CliSpec<Policy>, String> {
  let mut spec = CliSpec::with_policy(TOOL)
    .version(env!("CARGO_PKG_VERSION"))
    .description("Structural code search, Git history, and runtime evidence.")
    .without_global("format")
    .inputs([InputForm::Flags, InputForm::Json]);
  for plan in plans()? {
    let plan = Rc::new(plan);
    let run = plan.clone();
    spec = spec.op(
      OpDef::new(
        plan.name.clone(),
        plan.access,
        plan.schema.description.clone().unwrap_or_default(),
        move |params, policy| handle(&run, params, policy),
      )
      .schema(plan.schema.clone()),
    );
  }
  Ok(spec)
}

/// The published document (`lmprobe --schema`).
#[cfg(test)]
pub(crate) fn document() -> Result<lmrust::PublishedCliSchema, String> {
  Ok(lmrust::server::schema_document(&spec()?))
}

fn value_text(value: &Value) -> String {
  lmrust::js::to_display(value)
}

/// Legacy argv for validated params: command words with their level's flags, fixed args, then
/// positionals (after `--` when one starts with `-` or a positional requires it).
pub(crate) fn legacy_argv(plan: &OpPlan, params: &Record) -> Vec<String> {
  let mut argv = Vec::new();
  let mut positionals: Vec<(&Binding, Vec<String>)> = Vec::new();
  for (level, word) in plan.words.iter().enumerate() {
    argv.push(word.clone());
    for b in plan.bindings.iter().filter(|b| b.level == level) {
      let Some(value) = params.get(&b.param) else {
        continue;
      };
      if value.is_null() || b.default.as_ref() == Some(value) {
        continue; // the legacy parser applies the same default itself
      }
      let values: Vec<&Value> = match value {
        Value::Array(items) if b.multi => items.iter().collect(),
        other => vec![other],
      };
      if let Slot::Positional { .. } = b.slot {
        positionals.push((b, values.iter().map(|v| value_text(v)).collect()));
        continue;
      }
      let flag = match &b.slot {
        Slot::Long(long) => format!("--{long}"),
        Slot::Short(short) => format!("-{short}"),
        Slot::Positional { .. } => unreachable!(),
      };
      match b.kind {
        ValueKind::Flag => {
          if value == &Value::Bool(true) {
            argv.push(flag);
          }
        }
        ValueKind::Count => {
          let times = value.as_u64().unwrap_or(0);
          argv.extend(std::iter::repeat_n(flag, times as usize));
        }
        ValueKind::Value => {
          for v in values {
            match &b.slot {
              Slot::Long(_) => argv.push(format!("{flag}={}", value_text(v))),
              _ => {
                argv.push(flag.clone());
                argv.push(value_text(v));
              }
            }
          }
        }
      }
    }
  }
  argv.extend(plan.fixed_args.iter().cloned());
  positionals.sort_by_key(|(b, _)| match b.slot {
    Slot::Positional { index, .. } => index,
    _ => 0,
  });
  let needs_separator = positionals.iter().any(|(b, vs)| {
    matches!(b.slot, Slot::Positional { last: true, .. }) || vs.iter().any(|v| v.starts_with('-'))
  });
  if needs_separator && !positionals.is_empty() {
    argv.push("--".into());
  }
  for (_, values) in positionals {
    argv.extend(values);
  }
  argv
}

/// Legacy codes lmprobe only reports as warnings or notes: never the reason a run failed, so
/// they never become an error's `tool_code`. Every SCREAMING_CASE code in the source is either
/// listed here or mapped by [`mapped_code`] (`coverage_tests` scans the source).
pub(crate) const WARNING_CODES: [&str; 47] = [
  "ANCHORS_WITHOUT_GUARD",
  "ASYNC_LEAKS_NO_CALLS",
  "AUDIT_LOG_MISSING",
  "AWS_PAGE_INCOMPLETE",
  "AWS_PARTIAL_FAILURE",
  "AWS_SELECT_NOT_FOUND",
  "AWS_VALUES_REDACTED",
  "CONFIG_KEYS_GREP_ANCHOR_LIMIT",
  "CONFIG_KEYS_GREP_KEY_LIMIT",
  "CONFIG_PARSE_FAILED",
  "CONTEXT_LOCATION_RESOLVED",
  "CROSS_FILE_DEPTH_CAP",
  "CROSS_FILE_UNRESOLVED_IMPORT",
  "CRYPTO_SITES_NO_ALGORITHMS",
  "DELETE_FAILED",
  "DELETE_PREVIEW",
  "DEPRECATED_CACHE_PATH",
  "DEPRECATED_POSITIONAL_QUERY",
  "DIFF_CONTEXT_SOURCE_FALLBACK",
  "EVOLUTION_PARTIAL_CLONE",
  "FILE_METADATA_FAILED",
  "FILE_READ_FAILED",
  "FILE_TOO_LARGE",
  "GITHUB_CLI_REVIEW_THREADS_UNSUPPORTED",
  "GITIGNORE_PATHS_EXCLUDED",
  "GIT_GRAPH_INCOMPLETE",
  "GIT_REMOTE_NAME_ENCODING",
  "HISTORY_SHALLOW",
  "HISTORY_TRUNCATED",
  "IMPACT_DEPTH_CAPPED",
  "IMPORT_AWARE_CONTEXT",
  "LANGUAGE_UNSUPPORTED",
  "MULTI_PATH_DROPPED",
  "NO_ANCHORS_FOUND",
  "NO_FILES_SCANNED",
  "OPENAPI_PARSE_FAILED",
  "PATTERN_STRICTNESS_ELLIPSIS_UNSUPPORTED",
  "PYTHON_TYPED_DEF_PATTERN_FALLBACK",
  "QUOTED_INPUT",
  "REGEX_LITERAL_PIPE_NO_MATCH",
  "SEARCH_NAME_POSITIONAL_AS_PATH",
  "SVELTE_SYNTHETIC_CONTEXT",
  "TRACE_AUDIT_LOG",
  "WALK_ENTRY_ERROR",
  "WALK_ERROR",
  "WORKTREE_PRESERVED",
  "WORKTREE_USED",
];

/// Standard lmapi code for a legacy lmprobe error code: explicit codes first, then suffix
/// families for codes built at runtime (GraphQL extension codes). `None` for warning codes and
/// unknown codes.
pub(crate) fn mapped_code(legacy: &str) -> Option<ErrorCode> {
  if WARNING_CODES.contains(&legacy) {
    return None;
  }
  Some(match legacy {
    "ARGUMENT_ERROR"
    | "PATTERN_LANGUAGE_REQUIRED"
    | "PROGRAM_FORMAT_CONFLICT"
    | "INFILE_COMMAND_CONFLICT"
    | "STREAMING_COMMAND"
    | "MISSING_SCOPE"
    | "AMBIGUOUS_SCOPE"
    | "INVALID_SCOPE"
    | "INVALID_MARKER"
    | "INVALID_TRACE_MODE"
    | "INVALID_ACTION"
    | "FILE_PREDICATE_INVALID"
    | "PATTERN_PARSE_FAILED"
    | "REGEX_PARSE_FAILED"
    | "RULE_PARSE_FAILED"
    | "TRACE_JSON_REQUIRED"
    | "TRACE_WATCH_LANG_UNSUPPORTED"
    | "TRACE_WATCH_IDENTIFIER_UNSUPPORTED"
    | "NO_TRACE_TEMPLATE"
    | "CYTHON_NOT_SUPPORTED"
    | "NOT_IMPLEMENTED"
    | "GIT_WORKSPACE_UNSUPPORTED"
    | "AWS_WORKSPACE_UNSUPPORTED"
    | "AWS_PAGINATION_UNSUPPORTED"
    | "AWS_PAGINATION_INVALID"
    | "AWS_PARAMETER_INVALID"
    | "AWS_PARAMETERS_INVALID"
    | "AWS_REQUEST_INVALID"
    | "AWS_REQUEST_SCHEMA"
    | "AWS_SELECT_INVALID"
    | "AWS_TIMEOUT_INVALID" => ErrorCode::ValidationError,
    "WORKSPACE_READ_ONLY" | "EDIT_NON_GIT" => ErrorCode::Conflict,
    "NOT_GIT_REPO" | "GIT_REPOSITORY_ERROR" | "GIT_REVISION_ERROR" => ErrorCode::NotFound,
    "AWS_CLI_UNAVAILABLE" => ErrorCode::CommandNotFound,
    "PANIC" => ErrorCode::InternalError,
    "COMMAND_FAILED"
    | "GRAPHQL_ERROR"
    | "AWS_CLI_ERROR"
    | "AWS_CONFIG_RESOLUTION_FAILED"
    | "AWS_ERROR_CAPTURE_FAILED"
    | "AWS_JSON_INVALID"
    | "AWS_OUTPUT_CAPTURE_FAILED"
    | "AWS_OUTPUT_TOO_LARGE"
    | "GITHUB_CLI_FAILED"
    | "GITHUB_CLI_JSON"
    | "GIT_BINARY_BLAME"
    | "GIT_STATUS_FAILED"
    | "AUDIT_LOG_WRITE_FAILED"
    | "DELETE_AUDIT_FAILED"
    | "EDIT_WRITE_FAILED"
    | "FILE_SELECTION_FAILED"
    | "FIX_FAILED"
    | "PATCH_APPLY_FAILED"
    | "PATCH_CAPTURE_FAILED"
    | "TRACE_FAILED"
    | "UNTRACE_FAILED"
    | "WORKTREE_CLEANUP_FAILED"
    | "WORKTREE_PROVISION_FAILED" => ErrorCode::CommandFailed,
    code if code.starts_with("OUTFILE_") => ErrorCode::InternalError,
    code if code.ends_with("DIRTY_TREE") => ErrorCode::Conflict,
    code if code.ends_with("_NOT_FOUND") => ErrorCode::NotFound,
    code if code.ends_with("_REFUSED") => ErrorCode::Forbidden,
    code if code.ends_with("_TIMEOUT") => ErrorCode::Timeout,
    _ => return None,
  })
}

/// [`mapped_code`], with `COMMAND_FAILED` for unknown and warning codes.
pub(crate) fn standard_code(legacy: &str) -> ErrorCode {
  mapped_code(legacy).unwrap_or(ErrorCode::CommandFailed)
}

/// `mutation` as a GraphQL keyword (outside comments and strings is good enough here: a
/// false positive only makes the op subject to read-only/dry-run).
fn mentions_mutation(document: &str) -> bool {
  document
    .lines()
    .map(|l| l.split('#').next().unwrap_or_default())
    .any(|line| {
      line
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .any(|word| word == "mutation")
    })
}

/// The effective access of one invocation: `query_graphql` is a write when its document
/// contains a mutation (inline or `@file`); recipes are read-only queries.
fn effective_access(plan: &OpPlan, params: &Record) -> Access {
  if plan.access != Access::Dynamic {
    return plan.access;
  }
  let document = match params.get("query").and_then(Value::as_str) {
    Some(text) => match text.strip_prefix('@') {
      Some(path) => std::fs::read_to_string(path).unwrap_or_default(),
      None => text.to_string(),
    },
    None => String::new(),
  };
  if mentions_mutation(&document) {
    Access::Write
  } else {
    Access::Read
  }
}

fn handle(plan: &OpPlan, params: &Record, policy: &Policy) -> Result<Value, CliError> {
  let argv = legacy_argv(plan, params);
  let access = effective_access(plan, params);
  if access == Access::Write {
    let command = std::iter::once(TOOL.to_string()).chain(argv.iter().cloned());
    let ctx = CheckContext::new(plan.name.clone())
      .command(command)
      .dangerous(plan.dangerous);
    if let Some(simulated) = check_policy(Access::Write, policy, &ctx)? {
      return Ok(Value::Object(simulated));
    }
  }
  let stdin = plan
    .stdin_param
    .as_ref()
    .and_then(|name| params.get(name))
    .map(|v| {
      let lines: Vec<String> = match v {
        Value::Array(items) => items.iter().map(value_text).collect(),
        other => vec![value_text(other)],
      };
      lines.iter().map(|l| format!("{l}\n")).collect::<String>()
    });
  let run = child::run_legacy(&argv, stdin).map_err(|e| match e {
    child::SpawnError::Input(message) => CliError::new(ErrorCode::ValidationError, message),
    child::SpawnError::Internal(message) => CliError::new(ErrorCode::InternalError, message),
  })?;
  outcome(run)
}

/// Map a legacy run to an lmapi result: ok / no_match / partial are results (exit 0, status in
/// the document); errors become standard codes with the legacy code in `data.tool_code`.
fn outcome(run: child::LegacyRun) -> Result<Value, CliError> {
  let Some(result) = run.result else {
    let (code, legacy) = match run.exit_code {
      None => (ErrorCode::InternalError, "SIGNALED".to_string()),
      Some(101) => (ErrorCode::InternalError, "PANIC".to_string()),
      Some(code) => (ErrorCode::InternalError, format!("EXIT_{code}")),
    };
    let message = run
      .stderr
      .trim()
      .lines()
      .last()
      .unwrap_or("lmprobe produced no result")
      .to_string();
    return Err(
      CliError::new(code, message)
        .tool_code(legacy)
        .with("legacy_exit_code", run.exit_code.into()),
    );
  };
  match result.get("status").and_then(Value::as_str) {
    Some("ok" | "no_match" | "partial") => Ok(result),
    _ => {
      // The reason is the first diagnostic with an error code: warnings collected before the
      // failure (FILE_READ_FAILED before DELETE_REFUSED) must not become the tool_code.
      let diagnostics = result
        .get("diagnostics")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
      let code_of = |d: &Value| d.get("code").and_then(Value::as_str).map(str::to_string);
      let diagnostic = diagnostics
        .iter()
        .find(|d| code_of(d).as_deref().and_then(mapped_code).is_some())
        .or(diagnostics.first());
      let legacy = diagnostic
        .and_then(|d| d.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("COMMAND_FAILED")
        .to_string();
      let message = diagnostic
        .and_then(|d| d.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("lmprobe reported an error")
        .to_string();
      Err(
        CliError::new(standard_code(&legacy), message)
          .tool_code(legacy)
          .with("legacy_exit_code", run.exit_code.into())
          .with("result", result),
      )
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  fn plan(name: &str) -> OpPlan {
    plans()
      .unwrap()
      .into_iter()
      .find(|p| p.name == name)
      .unwrap_or_else(|| panic!("no op {name}"))
  }

  fn rec(v: Value) -> Record {
    v.as_object().cloned().unwrap()
  }

  #[test]
  fn every_clap_command_is_published_or_excluded() {
    let overlay = overlay::parse(overlay::OVERLAY_TOML).unwrap();
    assert_eq!(
      generate::unmapped_commands(&crate::cli::command(), &overlay),
      Vec::<String>::new()
    );
  }

  #[test]
  fn published_names_never_shadow_legacy_commands() {
    let legacy: Vec<String> = crate::cli::command()
      .get_subcommands()
      .map(|c| c.get_name().to_string())
      .collect();
    for p in plans().unwrap() {
      assert!(
        !legacy.contains(&p.name),
        "{} equals a legacy subcommand",
        p.name
      );
      assert!(
        p.name
          .chars()
          .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
        "{}",
        p.name
      );
    }
  }

  #[test]
  fn access_table_matches_the_design() {
    let writes = [
      "edit_text",
      "fix_ast",
      "delete_paths",
      "find_delete",
      "trace_exec",
    ];
    for p in plans().unwrap() {
      let expected = match p.name.as_str() {
        n if writes.contains(&n) => Access::Write,
        "query_graphql" => Access::Dynamic,
        _ => Access::Read,
      };
      assert_eq!(p.access, expected, "{}", p.name);
      assert_eq!(p.dangerous, writes.contains(&p.name.as_str()), "{}", p.name);
    }
  }

  #[test]
  fn document_passes_the_lmrust_publish_check() {
    let doc = serde_json::to_value(document().unwrap()).unwrap();
    lmrust::parse_published_schema(&doc).unwrap_or_else(|issues| panic!("{issues:?}"));
    assert_eq!(doc["inputs"], json!(["flags", "json"]));
    assert!(doc.get("product").is_none());
    assert_eq!(doc["version"], env!("CARGO_PKG_VERSION"));
    let globals: Vec<&str> = doc["globals"]
      .as_array()
      .unwrap()
      .iter()
      .map(|g| g["name"].as_str().unwrap())
      .collect();
    assert!(!globals.contains(&"format"));
    let names: Vec<&str> = doc["operations"]
      .as_array()
      .unwrap()
      .iter()
      .map(|o| o["name"].as_str().unwrap())
      .collect();
    for n in [
      "git_status",
      "history_blame",
      "list_kinds",
      "secrets_recipes_show",
      "aws_query",
      "search_symbols",
    ] {
      assert!(names.contains(&n), "{n}");
    }
  }

  #[test]
  fn common_options_are_published_only_where_consumed() {
    let doc = serde_json::to_value(document().unwrap()).unwrap();
    let params = |op: &str| -> Vec<String> {
      let op = doc["operations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["name"] == op)
        .unwrap();
      op["params"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap().to_string())
        .collect()
    };
    for op in ["git_status", "git_log", "aws_query", "secrets_recipes_show"] {
      assert!(
        !params(op)
          .iter()
          .any(|p| p == "lang" || p == "limit" || p == "workspace"),
        "{op}"
      );
    }
    assert!(params("git_status").contains(&"repo".to_string()));
    for p in ["after", "before", "context"] {
      assert!(params("grep_text").contains(&p.to_string()));
      assert!(params("ref_symbol").contains(&p.to_string()));
      assert!(!params("def_symbol").contains(&p.to_string()));
    }
    for op in doc["operations"].as_array().unwrap() {
      let names: Vec<&str> = op["params"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
      assert!(
        !names.contains(&"quiet") && !names.contains(&"verbose"),
        "{}",
        op["name"]
      );
    }
    assert!(params("def_symbol").contains(&"lang".to_string()));
  }

  #[test]
  fn legacy_argv_for_nested_and_positional_ops() {
    let p = plan("git_status");
    assert_eq!(
      legacy_argv(&p, &rec(json!({"repo": ".", "ignored": true}))),
      ["git", "status", "--ignored"]
    );
    assert_eq!(
      legacy_argv(&p, &rec(json!({"repo": "/r"}))),
      ["git", "status", "--repo=/r"]
    );
    let p = plan("grep_text");
    assert_eq!(
      legacy_argv(
        &p,
        &rec(json!({"pattern": "-x", "path": ["src", "tests"], "lang": "rust"}))
      ),
      [
        "grep",
        "--path=src",
        "--path=tests",
        "--lang=rust",
        "--",
        "-x"
      ]
    );
    let p = plan("find_delete");
    assert_eq!(
      legacy_argv(&p, &rec(json!({"pattern": "\\.log$", "force": true}))),
      ["find", "--force", "--action", "delete", "\\.log$"]
    );
    let p = plan("edit_text");
    assert!(legacy_argv(&p, &rec(json!({}))).contains(&"--apply".to_string()));
  }

  #[test]
  fn error_codes_map_to_standard_codes() {
    assert_eq!(standard_code("ARGUMENT_ERROR"), ErrorCode::ValidationError);
    assert_eq!(standard_code("DIRTY_TREE"), ErrorCode::Conflict);
    assert_eq!(standard_code("WORKSPACE_READ_ONLY"), ErrorCode::Conflict);
    assert_eq!(standard_code("NOT_GIT_REPO"), ErrorCode::NotFound);
    assert_eq!(standard_code("GIT_REPOSITORY_ERROR"), ErrorCode::NotFound);
    assert_eq!(standard_code("EDIT_DIRTY_TREE"), ErrorCode::Conflict);
    assert_eq!(standard_code("SYMBOL_NOT_FOUND"), ErrorCode::NotFound);
    assert_eq!(standard_code("PATH_NOT_FOUND"), ErrorCode::NotFound);
    assert_eq!(standard_code("FIX_REFUSED"), ErrorCode::Forbidden);
    assert_eq!(
      standard_code("OUTFILE_WRITE_FAILED"),
      ErrorCode::InternalError
    );
    assert_eq!(standard_code("PANIC"), ErrorCode::InternalError);
    assert_eq!(standard_code("AWS_CLI_ERROR"), ErrorCode::CommandFailed);
    assert_eq!(
      standard_code("AWS_CLI_UNAVAILABLE"),
      ErrorCode::CommandNotFound
    );
    assert_eq!(standard_code("AWS_QUERY_TIMEOUT"), ErrorCode::Timeout);
    assert_eq!(standard_code("DELETE_REFUSED"), ErrorCode::Forbidden);
    assert_eq!(standard_code("SOMETHING_ELSE"), ErrorCode::CommandFailed);
  }

  #[test]
  fn outcome_maps_statuses_and_errors() {
    let run = |exit, result: Option<Value>| child::LegacyRun {
      exit_code: exit,
      result,
      stderr: "boom\nlast line".into(),
    };
    for status in ["ok", "no_match", "partial"] {
      let doc = json!({"schema": "lmprobe.result.v1", "status": status});
      assert_eq!(outcome(run(Some(1), Some(doc.clone()))).unwrap(), doc);
    }
    let err = outcome(run(Some(2), Some(json!({"status": "error", "diagnostics": [{"code": "NOT_GIT_REPO", "message": "not a repo"}]})))).unwrap_err();
    assert_eq!(
      (err.code, err.message.as_str()),
      (ErrorCode::NotFound, "not a repo")
    );
    let data = err.data.unwrap();
    assert_eq!(data["tool_code"], "NOT_GIT_REPO");
    assert_eq!(data["legacy_exit_code"], 2);
    let err = outcome(run(Some(101), None)).unwrap_err();
    assert_eq!(
      (err.code, err.data.unwrap()["tool_code"].clone()),
      (ErrorCode::InternalError, json!("PANIC"))
    );
    assert_eq!(outcome(run(None, None)).unwrap_err().message, "last line");
  }

  #[test]
  fn pre_filter_never_rejects_lmapi_shaped_argv() {
    let spec = spec().unwrap();
    let cases: &[&[&str]] = &[
      &["--schema"],
      &["--list"],
      &["--explain", "git_status"],
      &["--input-json={}"],
      &["--inputFile", "x"],
      &["--input-graphql", "{ a }"],
      &["--dry-run"],
      &["--no-readonly"],
      &["--i-understand"],
      &["git_status"],
      &["--lang", "rust", "grep_text", "x"],
      &["--", "list_kinds", "rust"],
      &["def", "x"],
      &["--help"],
      &[],
      &["grep", "x", "."],
      &["git", "status", "--json"],
      &["--format", "json", "search", "--name", "x"],
      &["--outfile", "/tmp/x", "find", "a"],
    ];
    for case in cases {
      let argv: Vec<String> = case.iter().map(|s| s.to_string()).collect();
      if lmrust::server::is_lmapi_invocation(&spec, &argv) {
        assert!(could_be_lmapi(&argv), "{argv:?}");
      }
    }
    assert!(!could_be_lmapi(&["grep".to_string(), "x".to_string()]));
    assert!(!could_be_lmapi(&["--help".to_string()]));
    assert!(could_be_lmapi(&["git_status".to_string()]));
  }

  #[test]
  fn legacy_argv_stays_legacy() {
    let spec = spec().unwrap();
    let cases: &[&[&str]] = &[
      &[],
      &["--help"],
      &["-h"],
      &["--version"],
      &["def", "x"],
      &["search", "--name", "Parser", "."],
      &["grep", "TODO", "src"],
      &["git", "status", "--json"],
      &["git", "--repo", ".", "status"],
      &["history", "blame", "src/lib.rs:1"],
      &["find", "x", ".", "--action", "delete", "--force"],
      &["trace", "q", ".", "--exec", "true"],
      &["--format", "json", "list-kinds", "rust"],
      &["--infile", "q.graphql"],
      &["--outfile", "out.json", "grep", "x"],
      &["secrets-recipes", "list"],
    ];
    for case in cases {
      let argv: Vec<String> = case.iter().map(|s| s.to_string()).collect();
      assert!(
        !lmrust::server::is_lmapi_invocation(&spec, &argv),
        "{argv:?} must stay legacy"
      );
    }
  }

  #[test]
  fn graphql_mutations_are_writes() {
    let p = plan("query_graphql");
    assert_eq!(
      effective_access(
        &p,
        &rec(json!({"query": "{ search(query: \"x\") { hits { filePath } } }"}))
      ),
      Access::Read
    );
    assert_eq!(
      effective_access(
        &p,
        &rec(json!({"query": "mutation { untrace(scope: \".\") { ok } }"}))
      ),
      Access::Write
    );
    assert_eq!(
      effective_access(&p, &rec(json!({"query": "# mutation\n{ a }"}))),
      Access::Read
    );
    assert_eq!(
      effective_access(&p, &rec(json!({"recipe": "secrets/aws/python"}))),
      Access::Read
    );
    assert_eq!(
      effective_access(&plan("git_status"), &Record::new()),
      Access::Read
    );
  }
}
