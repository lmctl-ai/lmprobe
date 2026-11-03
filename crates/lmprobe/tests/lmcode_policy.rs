mod lmprobe_support;

use lmprobe_support::{fixture, lmcode, Opts};
use serde_json::json;
use std::fs;
use std::path::Path;
use std::process::Command;

fn compile_write_program(dir: &Path) -> std::path::PathBuf {
  let source = dir.join("write.lms");
  let bytecode = dir.join("write.bytecode");
  let schema = Path::new(env!("CARGO_MANIFEST_DIR")).join("lmcode/host.schema.json");
  let schema = serde_json::to_string(&schema.to_string_lossy()).unwrap();
  fs::write(
    &source,
    format!(
      "program = _lmcode_ schema={schema};\n$return = json_parse(probe_edit_text(\"{{\\\"pattern\\\":\\\"hello\\\",\\\"replacement\\\":\\\"bye\\\",\\\"files\\\":[\\\"notes.txt\\\"],\\\"confirm\\\":true}}\"));\nend _lmcode_\n"
    ),
  )
  .unwrap();
  let helper = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/compile_lmcode_fixture.mjs");
  let compiler =
    std::env::var_os("LMSCRIPT_BIN").expect("LMSCRIPT_BIN is required for lmcode host tests");
  let output = Command::new("node")
    .arg(helper)
    .arg(&source)
    .arg(&bytecode)
    .env("LMSCRIPT_BIN", compiler)
    .output()
    .unwrap();
  assert!(
    output.status.success(),
    "{}",
    String::from_utf8_lossy(&output.stderr)
  );
  bytecode
}

fn invoke_manifest(
  host: &Path,
  bytecode: &Path,
  manifest: &Path,
  cwd: &Path,
) -> (i32, serde_json::Value) {
  let output = Command::new(host)
    .args(["--lmcode", bytecode.to_str().unwrap()])
    .current_dir(cwd)
    .env("LMCODE_INVOCATION_FILE", manifest)
    .output()
    .unwrap();
  let stdout = String::from_utf8(output.stdout).unwrap();
  assert!(stdout.starts_with("LMCODE_RETURN_BEGIN\n"), "{stdout}");
  assert!(stdout.ends_with("\nLMCODE_RETURN_END\n"), "{stdout}");
  let envelope = serde_json::from_str(
    &stdout["LMCODE_RETURN_BEGIN\n".len()..stdout.len() - "\nLMCODE_RETURN_END\n".len()],
  )
  .unwrap();
  (output.status.code().unwrap_or(4), envelope)
}

fn edit_params(confirm: Option<bool>, readonly: Option<bool>) -> serde_json::Value {
  let mut params = json!({"pattern":"hello","replacement":"bye","files":["notes.txt"]});
  if let Some(value) = confirm {
    params["confirm"] = json!(value);
  }
  if let Some(value) = readonly {
    params["readonly"] = json!(value);
  }
  params
}

#[test]
fn dangerous_write_requires_confirm_and_leaves_the_file_unchanged() {
  let root = fixture();
  let out = lmcode(
    "edit_text",
    &edit_params(None, None),
    root.path(),
    Opts::default(),
  );
  let frame = out.json();
  assert_eq!(frame["ok"], false, "{frame}");
  assert_eq!(frame["code"], "CONFIRM_REQUIRED", "{frame}");
  assert_eq!(
    std::fs::read_to_string(root.path().join("notes.txt")).unwrap(),
    "hello world\n"
  );
}

#[test]
fn explicit_readonly_and_environment_floor_refuse_writes() {
  let root = fixture();
  let out = lmcode(
    "edit_text",
    &edit_params(Some(true), Some(true)),
    root.path(),
    Opts::default(),
  );
  assert_eq!(out.json()["code"], "FORBIDDEN", "{}", out.text());
  assert_eq!(
    std::fs::read_to_string(root.path().join("notes.txt")).unwrap(),
    "hello world\n"
  );

  let opts = Opts {
    env: vec![("LMPROBE_READONLY", "yes".into())],
    ..Opts::default()
  };
  let out = lmcode(
    "edit_text",
    &edit_params(Some(true), Some(false)),
    root.path(),
    opts,
  );
  assert_eq!(out.json()["code"], "FORBIDDEN", "{}", out.text());
  assert_eq!(
    std::fs::read_to_string(root.path().join("notes.txt")).unwrap(),
    "hello world\n"
  );
}

#[test]
fn confirmed_real_write_preserves_the_existing_cli_behavior() {
  let root = fixture();
  let out = lmcode(
    "edit_text",
    &edit_params(Some(true), None),
    root.path(),
    Opts::default(),
  );
  assert_eq!(out.json()["ok"], true, "{}", out.text());
  assert_eq!(
    std::fs::read_to_string(root.path().join("notes.txt")).unwrap(),
    "bye world\n"
  );
}

#[test]
fn invocation_dry_skips_the_effectful_callback_and_real_mode_still_executes_it() {
  let root = fixture();
  let temp = tempfile::tempdir().unwrap();
  let bytecode = compile_write_program(temp.path());
  let manifest = temp.path().join("invocation.json");
  let host = Path::new(env!("CARGO_BIN_EXE_lmprobe"));

  fs::write(
    &manifest,
    serde_json::json!({
      "version":1,"mode":"single","dry":true,
      "parts":[{"index":0,"file":bytecode.to_string_lossy()}]
    })
    .to_string(),
  )
  .unwrap();
  let (code, dry) = invoke_manifest(host, &bytecode, &manifest, root.path());
  assert_eq!(code, 0, "{dry}");
  assert_eq!(dry["ok"], true, "{dry}");
  assert_eq!(dry["dry"], true, "{dry}");
  assert_eq!(dry["result"], serde_json::Value::Null, "{dry}");
  assert_eq!(
    fs::read_to_string(root.path().join("notes.txt")).unwrap(),
    "hello world\n"
  );

  fs::write(
    &manifest,
    serde_json::json!({
      "version":1,"mode":"single","dry":false,
      "parts":[{"index":0,"file":bytecode.to_string_lossy()}]
    })
    .to_string(),
  )
  .unwrap();
  let (code, real) = invoke_manifest(host, &bytecode, &manifest, root.path());
  assert_eq!(code, 0, "{real}");
  assert_eq!(real["ok"], true, "{real}");
  assert!(real.get("dry").is_none(), "{real}");
  assert_eq!(
    fs::read_to_string(root.path().join("notes.txt")).unwrap(),
    "bye world\n"
  );
}
