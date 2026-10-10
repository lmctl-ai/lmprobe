use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

struct Fixture {
  root: PathBuf,
}

impl Drop for Fixture {
  fn drop(&mut self) {
    let _ = fs::remove_dir_all(&self.root);
  }
}

fn fixture() -> Fixture {
  let suffix = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_nanos())
    .unwrap_or(0);
  let root = std::env::temp_dir().join(format!(
    "lmprobe-config-keys-{}-{suffix}",
    std::process::id()
  ));
  let _ = fs::remove_dir_all(&root);
  fs::create_dir_all(root.join("src")).expect("create fixture dirs");
  fs::write(
    root.join("app.json"),
    r#"{
  "server": {
    "host": "localhost",
    "port": 8080
  },
  "featureFlag": true
}
"#,
  )
  .expect("write json");
  fs::write(
    root.join("pyproject.toml"),
    r#"[database]
url = "postgres://localhost"
pool_size = 5

[tool.pytest.ini_options]
addopts = "-q"
"#,
  )
  .expect("write toml");
  fs::write(
    root.join("settings.yaml"),
    r#"service:
  timeout: 30
  nested:
    enabled: true
"#,
  )
  .expect("write yaml");
  fs::write(root.join("bad.json"), r#"{ "broken": "#).expect("write malformed json");
  fs::write(
    root.join("src/main.py"),
    r#"def load_config(config):
    feature = config["featureFlag"]
    timeout = config.get("timeout")
    addopts = config.get("addopts", "")
    return feature, timeout, addopts
"#,
  )
  .expect("write python");
  Fixture { root }
}

fn run_raw(root: &Path, args: &[&str]) -> (String, String, i32) {
  let out = Command::new(binary())
    .args(args)
    .current_dir(root)
    .output()
    .expect("spawn lmprobe");
  (
    String::from_utf8_lossy(&out.stdout).into_owned(),
    String::from_utf8_lossy(&out.stderr).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

fn run_json(root: &Path, args: &[&str]) -> (serde_json::Value, String, i32) {
  let (stdout, stderr, exit) = run_raw(root, args);
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    stderr,
    exit,
  )
}

fn path_ends_with(path: &str, suffix: &str) -> bool {
  path.replace('\\', "/").ends_with(suffix)
}

#[test]
fn config_keys_extracts_json_toml_and_yaml_facts() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "config-keys",
      "app.json",
      "pyproject.toml",
      "settings.yaml",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  assert_eq!(envelope["schema"], "lmprobe.config_keys.v1");
  let keys = envelope["keys"].as_array().expect("keys");
  assert!(
    keys.iter().any(|key| key["file_path"]
      .as_str()
      .is_some_and(|p| p.ends_with("app.json"))
      && key["lang"] == "json"
      && key["key_path"] == "server.host"
      && key["key"] == "host"
      && key["line_number"].as_u64().unwrap_or(0) > 0
      && key["value_kind"] == "string"
      && key["value_summary"] == "localhost"),
    "json host key missing:\n{envelope:#}"
  );
  assert!(
    keys
      .iter()
      .any(|key| key["key_path"] == "database.pool_size" && key["value_kind"] == "number"),
    "toml pool_size key missing:\n{envelope:#}"
  );
  assert!(
    keys
      .iter()
      .any(|key| key["key_path"] == "service.timeout" && key["lang"] == "yaml"),
    "yaml timeout key missing:\n{envelope:#}"
  );
}

#[test]
fn config_keys_grep_code_attaches_requested_anchor_evidence() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "config-keys",
      ".",
      "--grep-code",
      "--around",
      "1",
      "--parents",
      "--role",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  let timeout = envelope["keys"]
    .as_array()
    .expect("keys")
    .iter()
    .find(|key| key["key_path"] == "service.timeout")
    .expect("timeout key");
  let anchors = timeout["code_anchors"].as_array().expect("code anchors");
  assert!(
    anchors.iter().any(|anchor| anchor["file_path"]
      .as_str()
      .is_some_and(|path| path_ends_with(path, "src/main.py"))
      && anchor["line_content"]
        .as_str()
        .is_some_and(|line| line.contains("timeout"))
      && anchor["parents"]
        .as_array()
        .is_some_and(|parents| !parents.is_empty())
      && anchor["source_context"]
        .as_array()
        .is_some_and(|lines| !lines.is_empty())
      && anchor.get("role").is_some()),
    "requested code anchor expansions missing:\n{timeout:#}"
  );
  assert!(
    !envelope.to_string().contains("dead")
      && !envelope.to_string().contains("unused")
      && !envelope.to_string().contains("missing")
      && !envelope.to_string().contains("drift")
      && !envelope.to_string().contains("ignored"),
    "runtime output should stay evidence-only:\n{envelope:#}"
  );
}

#[test]
fn config_keys_key_filter_targets_one_full_key_path_before_grep_code() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "config-keys",
      "pyproject.toml",
      "--key",
      "tool.pytest.ini_options.addopts",
      "--grep-code",
      "--around",
      "1",
      "--parents",
      "--role",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  let keys = envelope["keys"].as_array().expect("keys");
  assert_eq!(
    keys.len(),
    1,
    "only the targeted key should remain:\n{envelope:#}"
  );
  let addopts = &keys[0];
  assert_eq!(addopts["key_path"], "tool.pytest.ini_options.addopts");
  assert_eq!(addopts["key"], "addopts");
  assert!(
    addopts["code_anchors"]
      .as_array()
      .is_some_and(|anchors| anchors.iter().any(|anchor| anchor["line_content"]
        .as_str()
        .is_some_and(|line| line.contains("addopts"))
        && anchor["source_context"]
          .as_array()
          .is_some_and(|lines| !lines.is_empty())
        && anchor["parents"]
          .as_array()
          .is_some_and(|parents| !parents.is_empty())
        && anchor.get("role").is_some())),
    "targeted grep-code evidence should attach only addopts anchors:\n{addopts:#}"
  );
  assert!(
    envelope["warnings"]
      .as_array()
      .is_some_and(|warnings| warnings.is_empty()),
    "single-key grep-code should not hit broad key limits:\n{envelope:#}"
  );
}

#[test]
fn config_keys_reports_malformed_config_as_non_fatal_warning() {
  let fix = fixture();
  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &["--format", "json", "config-keys", "app.json", "bad.json"],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  assert!(envelope["keys"]
    .as_array()
    .is_some_and(|keys| !keys.is_empty()));
  assert!(
    envelope["warnings"]
      .as_array()
      .is_some_and(|warnings| warnings
        .iter()
        .any(|warning| warning["code"] == "CONFIG_PARSE_FAILED")),
    "parse warning missing:\n{envelope:#}"
  );
}

#[test]
fn config_keys_line_numbers_use_full_key_path_when_possible() {
  let fix = fixture();
  fs::write(
    fix.root.join("dupes.json"),
    r#"{
  "database": {
    "url": "postgres://localhost"
  },
  "server": {
    "url": "http://localhost"
  }
}
"#,
  )
  .expect("write duplicate-key json");
  fs::write(
    fix.root.join("tables.toml"),
    r#"[server]
url = "http://localhost"

[database]
url = "postgres://localhost"
"#,
  )
  .expect("write toml tables");

  let (envelope, stderr, exit) = run_json(
    &fix.root,
    &[
      "--format",
      "json",
      "config-keys",
      "dupes.json",
      "tables.toml",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  let keys = envelope["keys"].as_array().expect("keys");
  assert!(
    keys.iter().any(|key| key["file_path"]
      .as_str()
      .is_some_and(|path| path.ends_with("dupes.json"))
      && key["key_path"] == "server.url"
      && key["line_number"] == 6),
    "duplicate JSON leaf key should resolve to full-path line:\n{envelope:#}"
  );
  assert!(
    keys.iter().any(|key| key["file_path"]
      .as_str()
      .is_some_and(|path| path.ends_with("tables.toml"))
      && key["key_path"] == "server"
      && key["line_number"] == 1),
    "TOML table record should resolve to table header line:\n{envelope:#}"
  );
  assert!(
    keys.iter().any(|key| key["file_path"]
      .as_str()
      .is_some_and(|path| path.ends_with("tables.toml"))
      && key["key_path"] == "database.url"
      && key["line_number"] == 5),
    "TOML duplicate leaf key should resolve under the active table:\n{envelope:#}"
  );
}

#[test]
fn config_keys_ndjson_itemizes_keys() {
  let fix = fixture();
  let (stdout, stderr, exit) = run_raw(
    &fix.root,
    &["--format", "ndjson", "config-keys", "app.json"],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nstdout:\n{stdout}");
  let lines = stdout.lines().collect::<Vec<_>>();
  assert!(lines.len() > 2, "stdout:\n{stdout}");
  let item: serde_json::Value = serde_json::from_str(lines[0]).expect("key item");
  assert!(item.get("key_path").is_some(), "item:\n{item:#}");
  let summary: serde_json::Value = serde_json::from_str(lines.last().unwrap()).expect("summary");
  assert_eq!(summary["kind"], "summary");
  assert!(summary.get("keys").is_none());
}

#[test]
fn config_keys_graphql_matches_cli_surface() {
  let fix = fixture();
  let query = r#"
    {
      configKeys(path: ".", key: "service.timeout", grepCode: true, around: 1, parents: true, role: true) {
        keys {
          filePath
          lang
          keyPath
          key
          lineNumber
          valueKind
          valueSummary
          codeAnchors {
            filePath
            lineNumber
            lineContent
            role
            parents { kind name lineStart lineEnd }
            sourceContext { lineNumber lineContent }
          }
        }
        warnings { code message hint }
        exitStatus
      }
    }
  "#;
  let (envelope, stderr, exit) = run_json(&fix.root, &["--format", "json", "query", query]);
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  let keys = envelope
    .pointer("/data/configKeys/keys")
    .and_then(|value| value.as_array())
    .expect("graphql keys");
  assert_eq!(
    keys.len(),
    1,
    "graphql key filter should keep only the requested key:\n{envelope:#}"
  );
  assert!(
    keys.iter().any(|key| key["keyPath"] == "service.timeout"
      && key["codeAnchors"]
        .as_array()
        .is_some_and(|anchors| !anchors.is_empty())),
    "graphql configKeys should expose key facts and anchors:\n{envelope:#}"
  );
}
