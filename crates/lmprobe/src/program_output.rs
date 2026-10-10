//! Versioned program results and atomic JSON output destinations.

use ast_grep_language::Language;
use lmprobe_types::schema::{Diagnostic, PayloadFormat, ProgramResult, ProgramStatus};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Default)]
struct OutputState {
  program: bool,
  file: Option<AtomicOutput>,
  status: Option<ProgramStatus>,
  failed: bool,
}
thread_local! { static STATE: RefCell<OutputState> = RefCell::new(OutputState::default()); }

pub fn enable(program: bool) {
  STATE.with(|state| state.borrow_mut().program = program);
}
pub fn enabled() -> bool {
  STATE.with(|state| state.borrow().program)
}
pub fn partial() -> bool {
  STATE.with(|state| state.borrow().status == Some(ProgramStatus::Partial))
}
pub fn failed() -> bool {
  STATE.with(|state| state.borrow().failed)
}
pub fn target() -> Option<PathBuf> {
  STATE.with(|state| state.borrow().file.as_ref().map(|file| file.target.clone()))
}
pub fn cancel_file() {
  STATE.with(|state| {
    state.borrow_mut().file.take();
  });
}

pub fn protect(paths: impl IntoIterator<Item = PathBuf>) -> Result<(), String> {
  if let Some(target) = target() {
    for path in paths {
      if fs::canonicalize(&path)
        .or_else(|_| resolved_leaf(&path))
        .ok()
        .as_ref()
        == Some(&target)
        || same_file(&path, &target)
      {
        cancel_file();
        return Err(
          "OUTFILE_SOURCE_CONFLICT: outfile is also an operation target; no mutation performed"
            .into(),
        );
      }
    }
  }
  Ok(())
}

pub fn protect_trace_root(root: &Path) -> Result<(), String> {
  if let Some(target) = target() {
    if target.exists()
      && lmprobe_lang_core::SgLang::from_path(&target).is_some()
      && root
        .canonicalize()
        .ok()
        .is_some_and(|root| target.starts_with(root))
    {
      cancel_file();
      return Err(
        "OUTFILE_SOURCE_CONFLICT: trace outfile overlaps an existing recognized source file".into(),
      );
    }
  }
  Ok(())
}

pub fn prepare(path: Option<&Path>, input: Option<&Path>) -> std::io::Result<()> {
  if let Some(path) = path.filter(|path| *path != Path::new("-")) {
    let output = AtomicOutput::reserve(path)?;
    if let Some(input) = input.filter(|path| *path != Path::new("-")) {
      let source = fs::canonicalize(input)
        .or_else(|_| resolved_leaf(input))
        .ok();
      if source.as_ref() == Some(&output.target) || same_file(input, &output.target) {
        return Err(std::io::Error::new(
          std::io::ErrorKind::InvalidInput,
          "infile and outfile must be different files",
        ));
      }
    }
    STATE.with(|state| state.borrow_mut().file = Some(output));
  }
  Ok(())
}

fn same_file(left: &Path, right: &Path) -> bool {
  #[cfg(unix)]
  {
    use std::os::unix::fs::MetadataExt;
    if let (Ok(a), Ok(b)) = (fs::metadata(left), fs::metadata(right)) {
      return a.dev() == b.dev() && a.ino() == b.ino();
    }
  }
  false
}

fn resolved_leaf(path: &Path) -> std::io::Result<PathBuf> {
  let absolute = if path.is_absolute() {
    path.to_path_buf()
  } else {
    std::env::current_dir()?.join(path)
  };
  let name = absolute.file_name().ok_or_else(|| {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, "outfile must name a file")
  })?;
  Ok(absolute.parent().unwrap().canonicalize()?.join(name))
}

struct AtomicOutput {
  target: PathBuf,
  temporary: Option<PathBuf>,
  file: Option<File>,
}
impl AtomicOutput {
  fn reserve(path: &Path) -> std::io::Result<Self> {
    let target = resolved_leaf(path)?;
    check_target(&target)?;
    if fs::metadata(target.parent().unwrap())?
      .permissions()
      .readonly()
    {
      return Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "outfile parent is read-only",
      ));
    }
    if target.exists() {
      OpenOptions::new().write(true).open(&target)?;
    }
    // Creating an output temp before selection would alter directory metadata
    // and dirty Git worktrees. Preflight is read-only; reserve at completion.
    Ok(Self {
      target,
      temporary: None,
      file: None,
    })
  }
  fn reserve_temporary(&mut self) -> std::io::Result<()> {
    let timestamp = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
    for attempt in 0..32 {
      let temporary = self.target.parent().unwrap().join(format!(
        ".lmprobe-output-{}-{timestamp}-{attempt}",
        std::process::id()
      ));
      let mut options = OpenOptions::new();
      options.write(true).create_new(true);
      #[cfg(unix)]
      {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
      }
      match options.open(&temporary) {
        Ok(file) => {
          self.file = Some(file);
          self.temporary = Some(temporary);
          return Ok(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
        Err(error) => return Err(error),
      }
    }
    Err(std::io::Error::new(
      std::io::ErrorKind::AlreadyExists,
      "cannot reserve outfile temporary file",
    ))
  }
  fn commit(&mut self, text: &str) -> std::io::Result<()> {
    self.reserve_temporary()?;
    let file = self.file.as_mut().unwrap();
    file.write_all(text.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    self.file.take();
    check_target(&self.target)?;
    fs::rename(self.temporary.as_ref().unwrap(), &self.target)
  }
}
impl Drop for AtomicOutput {
  fn drop(&mut self) {
    self.file.take();
    if let Some(path) = &self.temporary {
      let _ = fs::remove_file(path);
    }
  }
}
fn check_target(path: &Path) -> std::io::Result<()> {
  match fs::symlink_metadata(path) {
    Ok(metadata)
      if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.permissions().readonly() =>
    {
      Err(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "outfile must be a writable regular file, not a directory or symlink",
      ))
    }
    Ok(_) => Ok(()),
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
    Err(error) => Err(error),
  }
}

pub fn wrap(mut source: Value) -> ProgramResult {
  let schema = source
    .get("schema")
    .and_then(Value::as_str)
    .unwrap_or("lmprobe.unknown.v1")
    .to_string();
  let status = if source.get("exit_status").and_then(Value::as_str) == Some("error") {
    ProgramStatus::Error
  } else if source.get("partial").and_then(Value::as_bool) == Some(true)
    || source
      .get("warnings")
      .and_then(Value::as_array)
      .is_some_and(|warnings| {
        warnings.iter().any(|warning| {
          matches!(
            warning.get("code").and_then(Value::as_str),
            Some(
              "FILE_METADATA_FAILED"
                | "FILE_READ_FAILED"
                | "FILE_TOO_LARGE"
                | "WALK_ERROR"
                | "WALK_ENTRY_ERROR"
                | "DELETE_FAILED"
            )
          )
        })
      })
  {
    ProgramStatus::Partial
  } else if source.get("exit_status").and_then(Value::as_str) == Some("no_match") {
    ProgramStatus::NoMatch
  } else {
    ProgramStatus::Ok
  };
  let diagnostics = source
    .get("warnings")
    .cloned()
    .and_then(|value| serde_json::from_value(value).ok())
    .unwrap_or_default();
  let object = source.as_object_mut().expect("sensor result object");
  object.remove("schema");
  object.remove("exit_status");
  object.remove("warnings");
  let name = schema
    .strip_prefix("lmprobe.")
    .unwrap_or(&schema)
    .split(".v")
    .next()
    .unwrap();
  let primary = match name {
    "find" | "delete" | "edit" => Some("files"),
    "list_kinds" | "history_churn" | "history_co_change" | "aws_catalog" => Some("rows"),
    "git_status" | "git_diff" | "git_log" | "git_branch" | "git_remote" | "git_ls_files"
    | "git_blame" => Some("rows"),
    "history_blame" | "history_introduced" => Some("entries"),
    "discover" => Some("projects"),
    "evolution" => Some("milestones"),
    "trace" => Some("events"),
    "untrace" => Some("reverted_sites"),
    "fix" => Some("diffs"),
    "ast_diff" => Some("nodes"),
    "py_slice_candidates" => Some("candidates"),
    "callsites" => Some("callsites"),
    "source_reads" => Some("source_reads"),
    "diff_context" => Some("anchors"),
    "resource_sites" | "async_leaks" | "crypto_sites" => Some("sites"),
    "guard_sites" => Some("candidates"),
    "config_keys" => Some("keys"),
    "openapi_facts" => Some("operations"),
    "route_anchors" => Some("routes"),
    "http_client_calls" => Some("calls"),
    "secrets_recipes" | "secrets_sources" => Some("recipes"),
    "search" | "def" | "ref" | "grep" | "callers" | "callees" | "imports" | "exports"
    | "decorations" | "supertypes" | "subtypes" | "overrides" | "history_commit"
    | "history_diff" | "history_todos" => Some("hits"),
    _ => None,
  };
  let (payload_format, payload) = if name.ends_with(".show") || name == "help" || name == "version"
  {
    let key = if name.ends_with(".show") {
      "graphql"
    } else if name == "help" {
      "text"
    } else {
      "version"
    };
    (
      PayloadFormat::List,
      json!({"items":[object.remove(key).unwrap_or(Value::Null)]}),
    )
  } else if name == "discover" {
    (
      PayloadFormat::Hierarchical,
      json!({"data":object.remove("projects").unwrap_or(json!([]))}),
    )
  } else if matches!(name, "query" | "git_show" | "git_rev_parse" | "aws_query") {
    (
      PayloadFormat::Hierarchical,
      json!({"data":object.remove("data").unwrap_or(Value::Null)}),
    )
  } else if let Some(key) = primary {
    let rows = object.remove(key).unwrap_or(json!([]));
    let columns: BTreeSet<_> = rows
      .as_array()
      .into_iter()
      .flatten()
      .filter_map(Value::as_object)
      .flat_map(|row| row.keys().cloned())
      .collect();
    (PayloadFormat::Rows, json!({"columns":columns,"rows":rows}))
  } else {
    (
      PayloadFormat::Hierarchical,
      json!({"data":std::mem::take(object)}),
    )
  };
  ProgramResult {
    schema: ProgramResult::SCHEMA.into(),
    source_schema: schema,
    status,
    payload_format,
    payload,
    metadata: source,
    diagnostics,
  }
}

pub fn error(code: &str, message: String) {
  crate::output::render_envelope_json(
    &json!({"schema":"lmprobe.cli_error.v1", "exit_status":"error",
    "warnings":[Diagnostic::new(code,message.clone())], "message":message}),
  );
}

pub fn emit<E: serde::Serialize>(source: &E) {
  if !enabled() {
    // Legacy sensor JSON preserves its original field order and byte snapshots.
    crate::output::stdout_line(
      &serde_json::to_string_pretty(source).expect("serialize sensor result"),
    );
    return;
  }
  let source = serde_json::to_value(source).expect("serialize sensor result");
  let program = enabled();
  let mut value = if program {
    let result = wrap(source);
    STATE.with(|state| state.borrow_mut().status = Some(result.status.clone()));
    serde_json::to_value(result).unwrap()
  } else {
    source
  };
  let text = serde_json::to_string_pretty(&value).unwrap();
  let destination = STATE.with(|state| state.borrow_mut().file.take());
  if let Some(mut file) = destination {
    match file.commit(&text) {
      Ok(()) => return,
      Err(error) => {
        STATE.with(|state| state.borrow_mut().failed = true);
        value["metadata"]["operation_status"] = value["status"].clone();
        value["status"] = json!("error");
        value["diagnostics"]
          .as_array_mut()
          .unwrap()
          .push(json!(Diagnostic::new(
            "OUTFILE_WRITE_FAILED",
            error.to_string()
          )));
        eprintln!("lmprobe: outfile write failed: {error}");
        drop(file);
        let _ = writeln!(
          std::io::stdout().lock(),
          "{}",
          serde_json::to_string_pretty(&value).unwrap()
        );
        return;
      }
    }
  }
  crate::output::stdout_line(&text);
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn heterogeneous_rows_preserve_types_and_sorted_union_columns() {
    let rows = json!([{"z":false,"nested":{"count":0}}, {"a":null,"z":true}]);
    let result = wrap(
      json!({"schema":"lmprobe.git_status.v1","exit_status":"matched","warnings":[],"rows":rows,"clean":false,"head":null}),
    );
    assert_eq!(
      result.payload,
      json!({"columns":["a","nested","z"],"rows":rows})
    );
    assert_eq!(result.metadata, json!({"clean":false,"head":null}));
    assert_eq!(result.source_schema, "lmprobe.git_status.v1");
  }

  #[test]
  fn empty_edit_rows_keep_rows_contract_and_recovery_metadata() {
    let result = wrap(
      json!({"schema":"lmprobe.edit.v1","exit_status":"no_match","files":[],"mode":"dry_run","audit_path":null,"warnings":[]}),
    );
    assert_eq!(result.status, ProgramStatus::NoMatch);
    assert_eq!(result.payload, json!({"columns":[],"rows":[]}));
    assert_eq!(result.metadata, json!({"mode":"dry_run","audit_path":null}));
  }

  #[test]
  fn query_partial_results_preserve_data_errors_and_diagnostics() {
    let data = json!({"node":null,"other":{"status":"error","partial":false}});
    let errors = json!([{"message":"unavailable","path":["node"],"extensions":{"code":"MISSING"}}]);
    let result = wrap(
      json!({"schema":"lmprobe.query.v1","data":data,"errors":errors,"partial":true,"warnings":[{"code":"GIT_GRAPH_INCOMPLETE","message":"missing parent","hint":"fetch history","lang":null}],"exit_status":"matched"}),
    );
    assert_eq!(result.status, ProgramStatus::Partial);
    assert_eq!(result.payload, json!({"data":data}));
    assert_eq!(result.metadata["errors"], errors);
    assert_eq!(result.diagnostics[0].code, "GIT_GRAPH_INCOMPLETE");
    assert_eq!(result.diagnostics[0].hint.as_deref(), Some("fetch history"));
  }

  #[test]
  fn preview_diagnostics_do_not_turn_no_match_into_partial() {
    let result = wrap(
      json!({"schema":"lmprobe.delete.v1","exit_status":"no_match","files":[],"warnings":[{"code":"DELETE_PREVIEW","message":"preview only","hint":null,"lang":null}]}),
    );
    assert_eq!(result.status, ProgramStatus::NoMatch);
    assert_eq!(result.diagnostics.len(), 1);
  }

  #[test]
  fn recipe_documents_and_future_sensor_data_are_not_discarded() {
    let document = "query { find(pattern: \"\\\\.rs$\") { paths } }";
    let result = wrap(
      json!({"schema":"lmprobe.secrets_recipes.show.v1","graphql":document,"provider":"example","warnings":[],"exit_status":"matched"}),
    );
    assert_eq!(result.payload, json!({"items":[document]}));
    assert_eq!(result.metadata, json!({"provider":"example"}));
    let unknown = wrap(
      json!({"schema":"lmprobe.future.v1","nested":[{"bytes_hex":"00ff"}],"count":0,"warnings":[],"exit_status":"matched"}),
    );
    assert_eq!(
      unknown.payload,
      json!({"data":{"nested":[{"bytes_hex":"00ff"}],"count":0}})
    );
    assert_eq!(unknown.metadata, json!({}));
  }

  #[test]
  fn output_reservation_is_read_only_until_commit() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("result.json");
    fs::write(&target, "old result").unwrap();
    let mut output = AtomicOutput::reserve(&target).unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "old result");
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    output.commit("{\"ok\":true}").unwrap();
    drop(output);
    assert_eq!(fs::read_to_string(target).unwrap(), "{\"ok\":true}\n");
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
  }

  #[test]
  fn failed_output_commit_preserves_replaced_directory_and_cleans_temp() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("result.json");
    let mut output = AtomicOutput::reserve(&target).unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(target.join("sentinel"), "preserve").unwrap();
    assert!(output.commit("{}").is_err());
    drop(output);
    assert_eq!(
      fs::read_to_string(target.join("sentinel")).unwrap(),
      "preserve"
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
  }

  #[cfg(unix)]
  #[test]
  fn symlink_swap_at_output_commit_never_overwrites_its_target() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("result.json");
    let victim = root.path().join("source.txt");
    fs::write(&victim, "preserve").unwrap();
    let mut output = AtomicOutput::reserve(&target).unwrap();
    std::os::unix::fs::symlink(&victim, &target).unwrap();
    assert!(output.commit("{}").is_err());
    drop(output);
    assert_eq!(fs::read_to_string(victim).unwrap(), "preserve");
    assert!(fs::symlink_metadata(target)
      .unwrap()
      .file_type()
      .is_symlink());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
  }

  #[test]
  fn invalid_output_parent_is_refused_without_creating_directories() {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join("missing");
    assert!(AtomicOutput::reserve(&parent.join("result.json")).is_err());
    assert!(!parent.exists());
    assert!(AtomicOutput::reserve(root.path()).is_err());
  }
  #[test]
  fn classification_is_schema_based_and_preserves_composite_status_data() {
    for (name, format) in [
      ("find", PayloadFormat::Rows),
      ("history_blame", PayloadFormat::Rows),
      ("history_churn", PayloadFormat::Rows),
      ("ast_diff", PayloadFormat::Rows),
      ("fix", PayloadFormat::Rows),
      ("guard_sites", PayloadFormat::Rows),
      ("secrets_recipes", PayloadFormat::Rows),
      ("secrets_sources", PayloadFormat::Rows),
      ("discover", PayloadFormat::Hierarchical),
      ("impact", PayloadFormat::Hierarchical),
      ("context", PayloadFormat::Hierarchical),
      ("secrets", PayloadFormat::Hierarchical),
      ("pr_triage", PayloadFormat::Hierarchical),
      ("query", PayloadFormat::Hierarchical),
    ] {
      let result =
        wrap(json!({"schema":format!("lmprobe.{name}.v1"),"warnings":[],"exit_status":"no_match"}));
      assert_eq!(result.payload_format, format, "{name}");
      assert_eq!(result.status, ProgramStatus::NoMatch);
    }
    let result = wrap(
      json!({"schema":"lmprobe.pr_triage.v1","exit_status":"matched","warnings":[],"status":{"state":"OPEN"},"checks":[{"status":"SUCCESS"}],"evidence":[]}),
    );
    assert_eq!(result.status, ProgramStatus::Ok);
    assert_eq!(result.payload["data"]["status"]["state"], "OPEN");
    assert_eq!(result.payload["data"]["checks"][0]["status"], "SUCCESS");
  }
  #[test]
  fn status_precedence_uses_explicit_coverage_and_never_arbitrary_nested_status() {
    let input = json!({"schema":"lmprobe.context.v1","exit_status":"matched","warnings":[],"definition":{"status":"error","partial":true}});
    assert_eq!(wrap(input).status, ProgramStatus::Ok);
    let input = json!({"schema":"lmprobe.find.v1","exit_status":"no_match","warnings":[{"code":"FILE_METADATA_FAILED","message":"failed","hint":null,"lang":null}],"files":[],"paths":[]});
    assert_eq!(wrap(input).status, ProgramStatus::Partial);
    assert_eq!(
      wrap(json!({"schema":"lmprobe.find.v1","exit_status":"error","partial":true,"warnings":[]}))
        .status,
      ProgramStatus::Error
    );
  }
}
