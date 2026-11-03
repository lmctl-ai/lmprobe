use std::process::Command;

mod lmprobe_support;
use lmprobe_support::{lmcode, Opts};

fn lmprobe(args: &[&str]) -> std::process::Output {
  Command::new(env!("CARGO_BIN_EXE_lmprobe"))
    .args(args)
    .output()
    .expect("spawn lmprobe")
}

#[test]
fn lmcode_schema_and_signatures_publish_the_native_host_contract() {
  let schema = lmprobe(&["--lmcode-schema", "--json"]);
  assert!(
    schema.status.success(),
    "{}",
    String::from_utf8_lossy(&schema.stderr)
  );
  assert!(schema.stderr.is_empty());
  let document: serde_json::Value = serde_json::from_slice(&schema.stdout).expect("schema JSON");
  assert_eq!(document["dsl.schema"], 2);
  let functions = document["functions"].as_object().unwrap();
  assert_eq!(functions.len(), 63);
  assert!(functions.get("probe_git_status").is_some());
  assert!(functions.get("probe_search_symbols").is_some());
  assert!(
    document.get("schema").is_none(),
    "legacy cli.schema.v1 is not the lmcode host contract"
  );
  let catalog: serde_json::Value =
    serde_json::from_slice(include_bytes!("../lmcode/operations.json")).unwrap();
  let operations = catalog["operations"].as_array().unwrap();
  assert_eq!(operations.len(), functions.len());
  for operation in operations {
    let name = operation["host_function"].as_str().unwrap();
    assert!(
      functions.contains_key(name),
      "missing host signature for {name}"
    );
    assert_eq!(operation["return_type"], "Json", "{name}");
  }

  let signatures = lmprobe(&["--lmcode-signatures"]);
  assert!(
    signatures.status.success(),
    "{}",
    String::from_utf8_lossy(&signatures.stderr)
  );
  let text = String::from_utf8(signatures.stdout).expect("UTF-8 signatures");
  assert!(text
    .lines()
    .any(|line| line.starts_with("fn probe_git_status(")));
  assert!(text
    .lines()
    .any(|line| line.starts_with("fn probe_search_symbols(")));
  assert_eq!(
    text
      .lines()
      .filter(|line| line.starts_with("fn probe_"))
      .count(),
    63
  );
  assert!(text.lines().all(|line| !line.contains("lmapi")));
}

#[test]
fn legacy_schema_flag_no_longer_exposes_the_removed_lmapi_surface() {
  let output = lmprobe(&["--schema"]);
  assert!(
    !output.status.success(),
    "retired lmapi --schema unexpectedly succeeded"
  );
  assert!(!String::from_utf8_lossy(&output.stdout).contains("cli.schema.v1"));
}

#[test]
fn lmcode_missing_program_uses_the_terminal_return_frame() {
  let output = lmprobe(&["--lmcode", "/definitely/missing/lmprobe.bytecode"]);
  let stdout = String::from_utf8_lossy(&output.stdout);
  assert!(stdout.starts_with("LMCODE_RETURN_BEGIN\n"), "{stdout}");
  assert!(stdout.ends_with("\nLMCODE_RETURN_END\n"), "{stdout}");
  assert!(stdout.contains("\"phase\":\"app\""), "{stdout}");
  assert!(!stdout.contains("cli.schema.v1"), "{stdout}");
}

#[test]
fn lmscript_calls_a_native_operation_and_receives_its_structured_json_result() {
  let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
  let output = lmcode("git_status", &serde_json::json!({}), &repo, Opts::default());
  assert_eq!(output.code(), 0, "{}", output.err_text());
  let envelope = output.json();
  assert_eq!(envelope["ok"], true, "{envelope}");
  assert_eq!(envelope["result"]["status"], "ok", "{envelope}");
}
