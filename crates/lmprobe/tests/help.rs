use std::process::Command;

fn run(args: &[&str]) -> std::process::Output {
  Command::new(env!("CARGO_BIN_EXE_lmprobe"))
    .args(args)
    .env("NO_COLOR", "1")
    .output()
    .expect("run lmprobe")
}

#[test]
fn bare_invocation_prints_help_without_touching_a_workspace() {
  let temp = tempfile::tempdir().unwrap();
  let out = Command::new(env!("CARGO_BIN_EXE_lmprobe"))
    .current_dir(temp.path())
    .env("XDG_CACHE_HOME", temp.path().join("cache"))
    .env("NO_COLOR", "1")
    .output()
    .unwrap();
  assert!(out.status.success());
  assert!(out.stderr.is_empty());
  let help = String::from_utf8(out.stdout).unwrap();
  assert!(help.contains("Usage: lmprobe"));
  assert!(help.contains("lmprobe search --name Parser ."));
  let short = run(&["-h"]);
  assert_eq!(help.trim(), String::from_utf8(short.stdout).unwrap().trim());
  assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn help_navigation_is_successful_and_required_arguments_stay_errors() {
  for args in [
    vec!["--help"],
    vec!["help", "search"],
    vec!["help", "history", "blame"],
    vec!["secrets-sources", "show", "--help"],
  ] {
    let out = run(&args);
    assert!(out.status.success(), "{args:?}");
    assert!(out.stderr.is_empty(), "{args:?}");
    assert!(String::from_utf8(out.stdout).unwrap().contains("Example"));
  }
  for args in [
    vec!["def"],
    vec!["trace", "foo($X)"],
    vec!["unknown-command"],
  ] {
    let out = run(&args);
    assert_eq!(out.status.code(), Some(2), "{args:?}");
    assert!(out.stdout.is_empty(), "{args:?}");
    assert!(!out.stderr.is_empty(), "{args:?}");
  }
}

#[test]
fn command_memory_links_resolve_to_local_files() {
  let directory =
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../durable-memory/commands");
  for entry in std::fs::read_dir(&directory).unwrap() {
    let path = entry.unwrap().path();
    if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
      continue;
    }
    let doc = std::fs::read_to_string(&path).unwrap();
    for link in doc.split("](").skip(1) {
      let target = link.split(')').next().unwrap();
      if target.starts_with("https://") || target.starts_with('#') {
        continue;
      }
      let file = target.split('#').next().unwrap();
      assert!(
        directory.join(file).is_file(),
        "{}: broken link {target}",
        path.display()
      );
    }
  }
}

#[test]
fn advertised_graphql_example_executes_successfully() {
  let help = String::from_utf8(run(&["query", "--help"]).stdout).unwrap();
  let query = help
    .lines()
    .find_map(|line| line.trim().strip_prefix("lmprobe query '"))
    .and_then(|example| example.strip_suffix('\''))
    .expect("inline query help example");
  let fixture = tempfile::tempdir().unwrap();
  std::fs::write(
    fixture.path().join("Cargo.toml"),
    "[package]\nname = \"help-example\"\nversion = \"0.1.0\"\n",
  )
  .unwrap();
  let out = Command::new(env!("CARGO_BIN_EXE_lmprobe"))
    .args(["--format", "json", "query", query])
    .current_dir(fixture.path())
    .env("XDG_CACHE_HOME", fixture.path().join("cache"))
    .output()
    .unwrap();
  assert!(
    out.status.success(),
    "{}",
    String::from_utf8_lossy(&out.stdout)
  );
  let data: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
  assert!(data["errors"].is_null() || data["errors"] == serde_json::json!([]));
  assert_eq!(
    data["data"]["discover"]["projects"][0]["name"],
    "help-example"
  );
}
