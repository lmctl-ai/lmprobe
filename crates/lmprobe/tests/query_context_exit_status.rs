use std::path::PathBuf;
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

fn workspace_root() -> PathBuf {
  let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  p.pop();
  p.pop();
  p
}

fn normalize_path(path: &str) -> String {
  path.replace('\\', "/")
}

#[test]
fn query_context_uses_resolver_status_for_partial_useful_bundle() {
  let root = workspace_root();
  let out = Command::new(binary())
        .args([
            "--format",
            "json",
            "query",
            r#"{ context(symbol: "User", path: "crates/lmprobe/tests/fixtures/rust") { callers { name } callees { name } } }"#,
        ])
        .current_dir(&root)
        .output()
        .expect("spawn lmprobe query");

  let stdout = String::from_utf8_lossy(&out.stdout);
  let envelope: serde_json::Value =
    serde_json::from_str(&stdout).expect("query stdout should be JSON");

  assert_eq!(
    out.status.code(),
    Some(0),
    "query should exit 0 when context found a useful bundle\nstdout:\n{}",
    stdout,
  );
  assert_eq!(
    envelope.get("exit_status").and_then(|v| v.as_str()),
    Some("matched"),
    "top-level query exit_status should follow the resolver envelope\nstdout:\n{}",
    stdout,
  );
  assert_eq!(
    envelope
      .pointer("/data/context/callers")
      .and_then(|v| v.as_array())
      .map(|v| v.len()),
    Some(0),
    "regression fixture expects callers to stay empty\nstdout:\n{}",
    stdout,
  );
  assert_eq!(
    envelope
      .pointer("/data/context/callees")
      .and_then(|v| v.as_array())
      .map(|v| v.len()),
    Some(0),
    "regression fixture expects callees to stay empty\nstdout:\n{}",
    stdout,
  );
}

#[test]
fn query_context_import_aware_follows_one_relative_reexport() {
  let root = workspace_root();
  let out = Command::new(binary())
        .args([
            "--format",
            "json",
            "query",
            r#"{ context(symbol: "ReExportedThing", path: "crates/lmprobe/tests/fixtures/import_aware/barrel.ts", importAware: true) { definition { name filePath } warnings { code } exitStatus } }"#,
        ])
        .current_dir(&root)
        .output()
        .expect("spawn lmprobe query");

  let stdout = String::from_utf8_lossy(&out.stdout);
  let envelope: serde_json::Value =
    serde_json::from_str(&stdout).expect("query stdout should be JSON");

  assert_eq!(
    out.status.code(),
    Some(0),
    "query should exit 0 when import-aware context resolves a definition\nstdout:\n{}",
    stdout,
  );
  assert_eq!(
    envelope.get("exit_status").and_then(|v| v.as_str()),
    Some("matched"),
    "top-level query exit_status should follow the import-aware resolver\nstdout:\n{}",
    stdout,
  );
  assert_eq!(
    envelope
      .pointer("/data/context/definition/name")
      .and_then(|v| v.as_str()),
    Some("ReExportedThing"),
    "definition should come from the one-hop target\nstdout:\n{}",
    stdout,
  );
  assert!(
    envelope
      .pointer("/data/context/definition/filePath")
      .and_then(|v| v.as_str())
      .is_some_and(|path| normalize_path(path).ends_with("fixtures/import_aware/inner.ts")),
    "definition should resolve to inner.ts\nstdout:\n{}",
    stdout,
  );
  assert!(
    envelope
      .pointer("/data/context/warnings")
      .and_then(|v| v.as_array())
      .is_some_and(|warnings| warnings
        .iter()
        .any(|w| { w.get("code").and_then(|v| v.as_str()) == Some("IMPORT_AWARE_CONTEXT") })),
    "resolver should disclose that import-aware fallback was used\nstdout:\n{}",
    stdout,
  );
}

#[test]
fn query_context_import_aware_is_default_off() {
  let root = workspace_root();
  let out = Command::new(binary())
        .args([
            "--format",
            "json",
            "query",
            r#"{ context(symbol: "ReExportedThing", path: "crates/lmprobe/tests/fixtures/import_aware/barrel.ts") { definition { name filePath } warnings { code } exitStatus } }"#,
        ])
        .current_dir(&root)
        .output()
        .expect("spawn lmprobe query");

  let stdout = String::from_utf8_lossy(&out.stdout);
  let envelope: serde_json::Value =
    serde_json::from_str(&stdout).expect("query stdout should be JSON");

  assert_eq!(
    out.status.code(),
    Some(1),
    "query should exit 1 when import-aware context is not requested\nstdout:\n{}",
    stdout,
  );
  assert_eq!(
    envelope.get("exit_status").and_then(|v| v.as_str()),
    Some("no_match"),
    "top-level query exit_status should remain default-off no_match\nstdout:\n{}",
    stdout,
  );
  assert!(
    envelope
      .pointer("/data/context/warnings")
      .and_then(|v| v.as_array())
      .is_some_and(|warnings| warnings
        .iter()
        .any(|w| { w.get("code").and_then(|v| v.as_str()) == Some("SYMBOL_NOT_FOUND") })),
    "default-off path should not resolve through imports\nstdout:\n{}",
    stdout,
  );
}
