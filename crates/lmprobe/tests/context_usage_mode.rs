use std::path::PathBuf;
use std::process::Command;

fn binary() -> PathBuf {
  if let Ok(p) = std::env::var("LMPROBE_BIN") {
    return PathBuf::from(p);
  }
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

fn workspace_root() -> PathBuf {
  let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  p.pop();
  p.pop();
  p
}

fn fixture_path() -> &'static str {
  "crates/lmprobe/tests/fixtures/java/LogbackLoggingSystem.java"
}

fn run(args: &[&str]) -> (String, String, i32) {
  let out = Command::new(binary())
    .args(args)
    .current_dir(workspace_root())
    .output()
    .expect("spawn lmprobe context");
  (
    String::from_utf8_lossy(&out.stdout).into_owned(),
    String::from_utf8_lossy(&out.stderr).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

fn run_json(args: &[&str]) -> (serde_json::Value, String, i32) {
  let (stdout, stderr, exit) = run(args);
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    stderr,
    exit,
  )
}

fn warning<'a>(envelope: &'a serde_json::Value, code: &str) -> Option<&'a serde_json::Value> {
  envelope
    .get("warnings")
    .and_then(|v| v.as_array())
    .and_then(|warnings| {
      warnings
        .iter()
        .find(|warning| warning.get("code").and_then(|v| v.as_str()) == Some(code))
    })
}

#[test]
fn context_default_matches_explicit_def_for_usage_only_symbol() {
  let (default_env, default_stderr, default_exit) = run_json(&[
    "--format",
    "json",
    "context",
    "--recent-history-limit",
    "1",
    "uninstall",
    fixture_path(),
  ]);
  let (def_env, def_stderr, def_exit) = run_json(&[
    "--format",
    "json",
    "context",
    "--def",
    "--recent-history-limit",
    "1",
    "uninstall",
    fixture_path(),
  ]);

  assert_eq!(
    default_exit, 1,
    "default def-mode should not fall back to usage:\n{default_env:#}\nstderr:\n{default_stderr}"
  );
  assert_eq!(def_exit, default_exit, "stderr:\n{def_stderr}");
  assert_eq!(
    default_env, def_env,
    "default context should be byte-shape equivalent to explicit --def"
  );
  assert!(default_env
    .get("definition")
    .is_some_and(|definition| definition.is_null()));
  assert!(default_env.get("usages").is_none());
  let warning = warning(&default_env, "SYMBOL_NOT_FOUND")
    .unwrap_or_else(|| panic!("expected SYMBOL_NOT_FOUND:\n{default_env:#}"));
  assert!(
    warning
      .get("hint")
      .and_then(|v| v.as_str())
      .is_some_and(|hint| hint.contains("context --usage uninstall") && hint.contains("--around 8")),
    "def-mode missing-symbol diagnostic should suggest usage mode:\n{warning:#}"
  );
}

#[test]
fn context_usage_mode_returns_usage_anchor_with_expansions() {
  let (envelope, stderr, exit) = run_json(&[
    "--format",
    "json",
    "context",
    "--usage",
    "--around",
    "1",
    "--parents",
    "--body",
    "uninstall",
    fixture_path(),
  ]);

  assert_eq!(
    exit, 0,
    "usage mode should match the static call:\n{envelope:#}\nstderr:\n{stderr}"
  );
  assert!(envelope
    .get("definition")
    .is_some_and(|definition| definition.is_null()));
  let usage = envelope
    .get("usages")
    .and_then(|v| v.as_array())
    .and_then(|usages| usages.first())
    .unwrap_or_else(|| panic!("expected usage hit:\n{envelope:#}"));
  assert_eq!(usage.get("line_number").and_then(|v| v.as_u64()), Some(8));
  assert!(
    usage
      .get("line_content")
      .and_then(|v| v.as_str())
      .is_some_and(|line| line.contains("SLF4JBridgeHandler.uninstall();")),
    "usage hit should point at the call line:\n{usage:#}"
  );
  assert_eq!(
    usage
      .pointer("/before_context/0/line_number")
      .and_then(|v| v.as_u64()),
    Some(7)
  );
  assert!(
    usage
      .get("parents")
      .and_then(|v| v.as_array())
      .is_some_and(|parents| parents
        .iter()
        .any(|parent| { parent.get("name").and_then(|v| v.as_str()) == Some("cleanUp") })),
    "parents should include the containing method:\n{usage:#}"
  );
  assert!(
    usage
      .get("parents")
      .and_then(|v| v.as_array())
      .is_some_and(|parents| parents.iter().all(|parent| {
        parent.get("kind").and_then(|v| v.as_str()) != Some("method_invocation")
      })),
    "parents should not include the usage/call node itself:\n{usage:#}"
  );
  assert!(
    usage
      .pointer("/body/content")
      .and_then(|v| v.as_str())
      .is_some_and(|body| body.contains("SLF4JBridgeHandler.uninstall();")),
    "body expansion should include the usage call when requested:\n{usage:#}"
  );
  assert_ne!(
    usage.pointer("/body/kind").and_then(|v| v.as_str()),
    Some("method_invocation"),
    "body should describe the enclosing structural block, not the usage node itself:\n{usage:#}"
  );
}

#[test]
fn context_usage_markdown_renders_attached_expansions() {
  let (stdout, stderr, exit) = run(&[
    "--format",
    "markdown",
    "context",
    "--usage",
    "--around",
    "1",
    "--parents",
    "--body",
    "uninstall",
    fixture_path(),
  ]);

  assert_eq!(
    exit, 0,
    "usage markdown should render:\n{stdout}\nstderr:\n{stderr}"
  );
  assert!(
    stdout.contains("SLF4JBridgeHandler.isInstalled()"),
    "markdown should include before/after source context:\n{stdout}"
  );
  assert!(
    stdout.contains("parents:") && stdout.contains("cleanUp"),
    "markdown should include attached parent chain:\n{stdout}"
  );
  assert!(
    stdout.contains("body:") && stdout.contains("SLF4JBridgeHandler.uninstall();"),
    "markdown should include attached body expansion:\n{stdout}"
  );
}
