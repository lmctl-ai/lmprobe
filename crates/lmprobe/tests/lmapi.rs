//! lmapi mode (lmapi protocol 6.1): the published document, the channel files at the repo
//! root, and the built binary serving operations, policy, errors and domain statuses, while
//! legacy invocations keep their own exit codes.
//!
//! Regenerate the dev channel file after a CLI change:
//! `UPDATE_LMAPI_SCHEMA=1 cargo test -p lmprobe --test lmapi schema_dev_file`.

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn repo_root() -> PathBuf {
  Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn lmprobe(args: &[&str], cwd: &Path, env: &[(&str, &str)]) -> Output {
  let mut cmd = Command::new(env!("CARGO_BIN_EXE_lmprobe"));
  cmd
    .args(args)
    .current_dir(cwd)
    .env_remove("LMPROBE_READONLY")
    .env_remove("LMPROBE_DRYRUN");
  for (k, v) in env {
    cmd.env(k, v);
  }
  cmd.stdin(Stdio::null()).output().expect("run lmprobe")
}

fn json_out(out: &Output) -> Value {
  serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
    panic!(
      "stdout is not JSON ({e}): {}",
      String::from_utf8_lossy(&out.stdout)
    )
  })
}

fn code(out: &Output) -> i32 {
  out.status.code().expect("exit code")
}

/// A Git repository with one committed file `a.txt` ("hello world").
fn repo() -> tempfile::TempDir {
  let dir = tempfile::tempdir().unwrap();
  let git = |args: &[&str]| {
    let ok = Command::new("git")
      .args(args)
      .current_dir(dir.path())
      .env("GIT_CONFIG_GLOBAL", "/dev/null")
      .output()
      .unwrap()
      .status
      .success();
    assert!(ok, "git {args:?}");
  };
  git(&["init", "-q"]);
  fs::write(dir.path().join("a.txt"), "hello world\n").unwrap();
  git(&["add", "a.txt"]);
  git(&[
    "-c",
    "user.name=t",
    "-c",
    "user.email=t@example.com",
    "commit",
    "-qm",
    "init",
  ]);
  dir
}

fn published() -> Value {
  let out = lmprobe(&["--schema"], &repo_root(), &[]);
  assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
  json_out(&out)
}

// ---------------------------------------------------------------------------------------------
// The document and the channel files
// ---------------------------------------------------------------------------------------------

#[test]
fn schema_dev_file_matches_lmprobe_schema_byte_for_byte() {
  let out = lmprobe(&["--schema"], &repo_root(), &[]);
  assert_eq!(code(&out), 0);
  let file = repo_root().join("cli.schema.dev.json");
  if std::env::var_os("UPDATE_LMAPI_SCHEMA").is_some() {
    fs::write(&file, &out.stdout).unwrap();
  }
  let committed = fs::read(&file).unwrap();
  assert!(
    committed == out.stdout,
    "cli.schema.dev.json is stale: run UPDATE_LMAPI_SCHEMA=1 cargo test -p lmprobe --test lmapi schema_dev_file"
  );
}

fn channel(name: &str) -> Value {
  serde_json::from_str(&fs::read_to_string(repo_root().join(name)).unwrap()).unwrap()
}

#[test]
fn latest_is_an_additive_subset_of_dev() {
  let dev = channel("cli.schema.dev.json");
  let latest = channel("cli.schema.latest.json");
  assert_eq!(latest["tool"], dev["tool"]);
  let dev_ops: Vec<&Value> = dev["operations"].as_array().unwrap().iter().collect();
  for op in latest["operations"].as_array().unwrap() {
    let name = &op["name"];
    let dev_op = dev_ops
      .iter()
      .find(|d| &d["name"] == name)
      .unwrap_or_else(|| panic!("operation {name} of latest was removed from dev"));
    assert_eq!(dev_op["access"], op["access"], "access of {name} changed");
    for param in op["params"].as_array().unwrap() {
      let required = matches!(param["occurs"].as_str(), None | Some("1") | Some("+"));
      if required {
        let dev_param = dev_op["params"]
          .as_array()
          .unwrap()
          .iter()
          .find(|p| p["name"] == param["name"])
          .unwrap_or_else(|| panic!("required param {} of {name} was removed", param["name"]));
        assert!(matches!(
          dev_param["occurs"].as_str(),
          None | Some("1") | Some("+")
        ));
        assert_eq!(dev_param["type"], param["type"]);
      }
    }
  }
}

#[test]
fn channel_files_pass_the_lmrust_publish_check_and_the_lmapi_json_schema() {
  let json_schema: Value = serde_json::from_str(
    &fs::read_to_string(repo_root().join("crates/lmprobe/lmapi/cli.schema.v1.schema.json"))
      .unwrap(),
  )
  .unwrap();
  let validator =
    jsonschema::validator_for(&json_schema).expect("compile cli.schema.v1 JSON Schema");
  // The validator is live: a document without operations or with a bad access is rejected.
  let mut broken = published();
  broken["operations"][0]["access"] = json!("sometimes");
  assert!(!validator.is_valid(&broken));
  assert!(!validator.is_valid(&json!({"schema": "cli.schema.v1", "tool": "lmprobe"})));
  for doc in [
    published(),
    channel("cli.schema.dev.json"),
    channel("cli.schema.latest.json"),
  ] {
    lmrust::parse_published_schema(&doc).unwrap_or_else(|issues| panic!("{issues:?}"));
    let errors: Vec<String> = validator
      .iter_errors(&doc)
      .map(|e| format!("{e} at {}", e.instance_path))
      .collect();
    assert!(errors.is_empty(), "{errors:#?}");
  }
}

#[test]
fn document_shape_follows_the_native_tool_rules() {
  let doc = published();
  assert_eq!(doc["tool"], "lmprobe");
  assert_eq!(doc["version"], env!("CARGO_PKG_VERSION"));
  assert!(doc.get("product").is_none(), "lmprobe is its own product");
  assert_eq!(doc["inputs"], json!(["flags", "json"]));
  let globals: Vec<&str> = doc["globals"]
    .as_array()
    .unwrap()
    .iter()
    .map(|g| g["name"].as_str().unwrap())
    .collect();
  assert!(!globals.contains(&"format"), "lmprobe owns --format");
  for op in doc["operations"].as_array().unwrap() {
    assert!(
      ["read", "write", "dynamic"].contains(&op["access"].as_str().unwrap()),
      "{}",
      op["name"]
    );
    assert!(
      !op["description"].as_str().unwrap().trim().is_empty(),
      "{}",
      op["name"]
    );
  }
  let raw = fs::read_to_string(repo_root().join("cli.schema.dev.json")).unwrap();
  for needle in ["/home/", "/Users/", "/root/", "/tmp/"] {
    assert!(!raw.contains(needle), "{needle} in the published document");
  }
}

#[test]
fn list_and_explain() {
  let out = lmprobe(&["--list"], &repo_root(), &[]);
  assert_eq!(code(&out), 0);
  let list = json_out(&out);
  let names: Vec<&str> = list
    .as_array()
    .unwrap()
    .iter()
    .map(|o| o["name"].as_str().unwrap())
    .collect();
  assert!(names.contains(&"git_status") && names.contains(&"query_graphql"));
  assert!(!names.contains(&"serve") && !names.contains(&"help"));
  let out = lmprobe(&["--explain", "find_delete"], &repo_root(), &[]);
  assert_eq!(
    (code(&out), json_out(&out)["access"].clone()),
    (0, json!("write"))
  );
  let out = lmprobe(&["git_status", "--help"], &repo_root(), &[]);
  assert_eq!(code(&out), 0);
  assert!(String::from_utf8_lossy(&out.stdout).starts_with("lmprobe git_status"));
}

// ---------------------------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------------------------

#[test]
fn git_status_in_flags_and_json_forms() {
  let r = repo();
  let flags = lmprobe(&["git_status"], r.path(), &[]);
  assert_eq!(
    code(&flags),
    0,
    "{}",
    String::from_utf8_lossy(&flags.stderr)
  );
  let flags = json_out(&flags);
  assert_eq!(flags["schema"], "lmprobe.result.v1");
  assert_eq!(flags["source_schema"], "lmprobe.git_status.v1");
  assert_eq!(flags["status"], "ok");
  let doc = lmprobe(
    &["git_status", "--input-json", r#"{"repo": "."}"#],
    r.path(),
    &[],
  );
  assert_eq!(code(&doc), 0);
  assert_eq!(json_out(&doc)["source_schema"], "lmprobe.git_status.v1");
  let envelope = lmprobe(
    &["--input-json", r#"{"op": "git_status", "params": {}}"#],
    r.path(),
    &[],
  );
  assert_eq!(
    (code(&envelope), json_out(&envelope)["status"].clone()),
    (0, json!("ok"))
  );
}

#[test]
fn unknown_op_and_bad_params_are_validation_failures() {
  let r = repo();
  let out = lmprobe(
    &["--input-json", r#"{"op": "no_such_op", "params": {}}"#],
    r.path(),
    &[],
  );
  assert_eq!(
    (code(&out), json_out(&out)["error"]["code"].clone()),
    (2, json!("INVALID_REQUEST"))
  );
  let out = lmprobe(&["git_status", "--no-such-flag"], r.path(), &[]);
  let env = json_out(&out);
  assert_eq!(
    (code(&out), env["error"]["code"].clone()),
    (2, json!("VALIDATION_ERROR"))
  );
  assert_eq!(env["error"]["data"]["issues"][0]["code"], "UNKNOWN_PARAM");
  let out = lmprobe(&["list_kinds", "--threads", "many"], r.path(), &[]);
  assert_eq!(
    (
      code(&out),
      json_out(&out)["error"]["data"]["issues"][0]["code"].clone()
    ),
    (2, json!("TYPE_MISMATCH"))
  );
}

#[test]
fn unpublished_input_form_is_refused() {
  let r = repo();
  let out = lmprobe(
    &["git_status", "--input-graphql", "{ git_status }"],
    r.path(),
    &[],
  );
  let env = json_out(&out);
  assert_eq!(code(&out), 2);
  assert_eq!(
    env["error"]["data"]["issues"][0]["code"],
    "FORM_NOT_ALLOWED"
  );
}

#[test]
fn legacy_errors_map_to_standard_codes_with_tool_code() {
  let r = repo();
  let missing = r.path().join("missing");
  let out = lmprobe(
    &["git_status", "--repo", missing.to_str().unwrap()],
    r.path(),
    &[],
  );
  let env = json_out(&out);
  assert_eq!(code(&out), 4, "{env}");
  assert_eq!(env["error"]["code"], "NOT_FOUND");
  assert_eq!(env["error"]["data"]["tool_code"], "GIT_REPOSITORY_ERROR");
}

// ---------------------------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------------------------

const EDIT: [&str; 6] = [
  "--pattern",
  "hello",
  "--replacement",
  "bye",
  "a.txt",
  "--confirm",
];

fn edit_args<'a>(extra: &[&'a str]) -> Vec<&'a str> {
  let mut args = vec!["edit_text"];
  args.extend(EDIT);
  args.extend(extra);
  args
}

fn a_txt(r: &tempfile::TempDir) -> String {
  fs::read_to_string(r.path().join("a.txt")).unwrap()
}

#[test]
fn readonly_allows_reads_and_refuses_writes() {
  let r = repo();
  let out = lmprobe(&["git_status", "--readonly"], r.path(), &[]);
  assert_eq!(
    (code(&out), json_out(&out)["status"].clone()),
    (0, json!("ok"))
  );
  let out = lmprobe(&edit_args(&["--readonly"]), r.path(), &[]);
  assert_eq!(
    (code(&out), json_out(&out)["error"]["code"].clone()),
    (6, json!("READONLY_VIOLATION"))
  );
  assert_eq!(a_txt(&r), "hello world\n");
}

#[test]
fn the_env_floor_cannot_be_loosened_by_a_flag() {
  let r = repo();
  let out = lmprobe(
    &edit_args(&["--readonly=false"]),
    r.path(),
    &[("LMPROBE_READONLY", "1")],
  );
  assert_eq!(
    (code(&out), json_out(&out)["error"]["code"].clone()),
    (6, json!("READONLY_VIOLATION"))
  );
  assert_eq!(a_txt(&r), "hello world\n");
  let out = lmprobe(&edit_args(&[]), r.path(), &[("LMPROBE_DRYRUN", "yes")]);
  assert_eq!(
    (code(&out), json_out(&out)["dryRun"].clone()),
    (0, json!(true))
  );
  assert_eq!(a_txt(&r), "hello world\n");
}

#[test]
fn dangerous_ops_need_confirm() {
  let r = repo();
  let out = lmprobe(
    &[
      "edit_text",
      "--pattern",
      "hello",
      "--replacement",
      "bye",
      "a.txt",
    ],
    r.path(),
    &[],
  );
  assert_eq!(
    (code(&out), json_out(&out)["error"]["code"].clone()),
    (9, json!("MISSING_CONFIRMATION"))
  );
  let out = lmprobe(&["find_delete", "a\\.txt$", "--force"], r.path(), &[]);
  assert_eq!(code(&out), 9);
  assert!(r.path().join("a.txt").exists());
  let out = lmprobe(&edit_args(&[]), r.path(), &[]);
  assert_eq!(
    (code(&out), json_out(&out)["status"].clone()),
    (0, json!("ok")),
    "{}",
    String::from_utf8_lossy(&out.stdout)
  );
  assert_eq!(a_txt(&r), "bye world\n");
}

#[test]
fn dryrun_on_a_write_op_returns_the_synthetic_result_and_changes_nothing() {
  let r = repo();
  let out = lmprobe(
    &["find_delete", "a\\.txt$", "--force", "--dryrun"],
    r.path(),
    &[],
  );
  let env = json_out(&out);
  assert_eq!(code(&out), 0);
  assert_eq!(env["dryRun"], true);
  assert_eq!(env["simulated"], "find_delete");
  assert_eq!(
    env["command"],
    json!(["lmprobe", "find", "--force", "--action", "delete", "a\\.txt$"])
  );
  assert!(r.path().join("a.txt").exists());
}

#[test]
fn delete_paths_feeds_the_selection_on_stdin() {
  let r = repo();
  fs::write(r.path().join("b.log"), "x").unwrap();
  let out = lmprobe(
    &["delete_paths", "--paths", "b.log", "--confirm"],
    r.path(),
    &[],
  );
  assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stdout));
  assert!(
    r.path().join("b.log").exists(),
    "without force delete only previews"
  );
  let out = lmprobe(
    &["delete_paths", "--paths", "b.log", "--force", "--confirm"],
    r.path(),
    &[],
  );
  assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stdout));
  assert!(!r.path().join("b.log").exists());
}

// ---------------------------------------------------------------------------------------------
// Domain statuses and legacy exit codes
// ---------------------------------------------------------------------------------------------

#[test]
fn no_match_exits_zero_in_lmapi_mode_and_one_in_legacy_mode() {
  let r = repo();
  let out = lmprobe(
    &["grep_text", "zzz_no_such_text_zzz", "--path", "."],
    r.path(),
    &[],
  );
  assert_eq!(
    (code(&out), json_out(&out)["status"].clone()),
    (0, json!("no_match"))
  );
  let legacy = lmprobe(&["grep", "zzz_no_such_text_zzz", "."], r.path(), &[]);
  assert_eq!(code(&legacy), 1);
  let legacy_json = lmprobe(
    &["grep", "zzz_no_such_text_zzz", ".", "--json"],
    r.path(),
    &[],
  );
  assert_eq!(
    (code(&legacy_json), json_out(&legacy_json)["status"].clone()),
    (1, json!("no_match"))
  );
}

#[test]
fn legacy_help_and_no_args_are_untouched() {
  let help = lmprobe(&["--help"], &repo_root(), &[]);
  assert_eq!(code(&help), 0);
  assert!(String::from_utf8_lossy(&help.stdout).starts_with("Structural code search"));
  let none = lmprobe(&[], &repo_root(), &[]);
  assert_eq!(code(&none), 0);
  assert!(String::from_utf8_lossy(&none.stdout).starts_with("Structural code search"));
}

// ---------------------------------------------------------------------------------------------
// Clients: the published document drives lmrust's interpreter (lmapi protocol section 8)
// ---------------------------------------------------------------------------------------------

#[test]
fn lmrust_interprets_command_lines_against_the_published_document() {
  let doc: lmrust::PublishedCliSchema = serde_json::from_value(published()).unwrap();
  let docs = [doc];
  let read = lmrust::interpret_command(&docs, "lmprobe git_status --repo .", &[]).unwrap();
  assert_eq!(
    (read.op.as_str(), read.effect),
    ("git_status", lmrust::Effect::Read)
  );
  let write =
    lmrust::interpret_command(&docs, "lmprobe find_delete '\\.log$' --force", &[]).unwrap();
  assert_eq!(write.effect, lmrust::Effect::Effectful);
  let dynamic = lmrust::interpret_command(&docs, "lmprobe query_graphql '{ a }'", &[]).unwrap();
  assert_eq!(
    dynamic.effect,
    lmrust::Effect::Effectful,
    "dynamic is never read-only"
  );
  let legacy = lmrust::interpret_command(&docs, "lmprobe grep x .", &[]).unwrap_err();
  assert_eq!(legacy[0].code, lmrust::IssueCode::UnknownOperation);
  let refused = lmrust::interpret_command(&docs, "lmprobe git_status; rm -rf /", &[]).unwrap_err();
  assert_eq!(refused[0].code, lmrust::IssueCode::ShellSyntax);
  let prompt = lmrust::describe_for_prompt(&docs);
  assert!(prompt.starts_with("# lmprobe\n"));
  assert!(prompt.contains("lmprobe git_status"));
}
