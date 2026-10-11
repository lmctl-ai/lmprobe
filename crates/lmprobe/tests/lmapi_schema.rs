//! The published document is a contract: byte-stable across runs, working directories and
//! environments (HOME, locale, time zone), in a canonical order, and within a size budget.

mod lmapi_support;

use lmapi_support::*;
use serde_json::Value;
use std::path::Path;

fn schema_bytes(cwd: &Path, env: Vec<(&str, String)>) -> Vec<u8> {
  let out = run(
    &["--schema"],
    cwd,
    Opts {
      env,
      ..Opts::default()
    },
  );
  assert_eq!(out.code(), 0, "{}", out.err_text());
  assert!(out.stderr.is_empty(), "--schema writes nothing to stderr");
  out.stdout
}

fn repo_root() -> &'static Path {
  Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

#[test]
fn schema_is_byte_identical_across_runs_cwd_and_environment() {
  let reference = schema_bytes(repo_root(), vec![]);
  let fixture = fixture();
  let home = tempfile::tempdir().unwrap();
  let variants: Vec<(&Path, Vec<(&str, String)>)> = vec![
    (repo_root(), vec![]),
    (fixture.path(), vec![]),
    (Path::new("/"), vec![]),
    (
      home.path(),
      vec![("HOME", home.path().display().to_string())],
    ),
    (
      repo_root(),
      vec![("LANG", "C".into()), ("LC_ALL", "C".into())],
    ),
    (
      repo_root(),
      vec![
        ("LANG", "de_DE.UTF-8".into()),
        ("LC_ALL", "de_DE.UTF-8".into()),
      ],
    ),
    (repo_root(), vec![("LC_ALL", "tr_TR.UTF-8".into())]),
    (repo_root(), vec![("TZ", "Pacific/Chatham".into())]),
    (
      repo_root(),
      vec![("TZ", "UTC".into()), ("COLUMNS", "20".into())],
    ),
    (
      repo_root(),
      vec![("NO_COLOR", "1".into()), ("TERM", "dumb".into())],
    ),
    (
      repo_root(),
      vec![
        ("LMPROBE_READONLY", "1".into()),
        ("LMPROBE_DRYRUN", "1".into()),
      ],
    ),
  ];
  std::thread::scope(|scope| {
    for (cwd, env) in variants {
      let reference = &reference;
      scope.spawn(move || {
        let label = format!("{} {env:?}", cwd.display());
        let bytes = schema_bytes(cwd, env);
        assert!(bytes == *reference, "--schema differs for {label}");
      });
    }
  });
}

#[test]
fn list_and_explain_are_stable_and_agree_with_the_schema() {
  let doc: Value = serde_json::from_slice(&schema_bytes(repo_root(), vec![])).unwrap();
  let list = lm(&["--list"], repo_root());
  assert_eq!(list.stdout, lm(&["--list"], Path::new("/")).stdout);
  let list = list.json();
  let ops = doc["operations"].as_array().unwrap();
  let listed: Vec<&Value> = list
    .as_array()
    .unwrap()
    .iter()
    .map(|o| &o["name"])
    .collect();
  let published: Vec<&Value> = ops.iter().map(|o| &o["name"]).collect();
  assert_eq!(listed, published, "--list order equals the document order");
  std::thread::scope(|scope| {
    for chunk in ops.chunks(ops.len().div_ceil(8)) {
      scope.spawn(move || {
        for op in chunk {
          let name = op["name"].as_str().unwrap();
          let explained = lm(&["--explain", name], repo_root());
          assert_eq!(explained.code(), 0);
          assert_eq!(
            &explained.json(),
            op,
            "--explain {name} equals its schema entry"
          );
        }
      });
    }
  });
}

#[test]
fn the_document_is_in_canonical_order() {
  let text = String::from_utf8(schema_bytes(repo_root(), vec![])).unwrap();
  let doc: Value = serde_json::from_str(&text).unwrap();
  // Re-serializing the parsed value reproduces the bytes: no duplicate keys, canonical order.
  let reparsed: lmrust::PublishedCliSchema = serde_json::from_value(doc.clone()).unwrap();
  let mut again = serde_json::to_string_pretty(&reparsed).unwrap();
  again.push('\n');
  assert!(
    again == text,
    "the document is not in lmrust's canonical field order"
  );
  let top: Vec<&str> = text
    .lines()
    .filter(|l| l.starts_with("  \"") && !l.starts_with("   "))
    .map(|l| l.trim().split('"').nth(1).unwrap())
    .collect();
  assert_eq!(top[..3], ["schema", "tool", "version"]);
  // Operation names are unique; params are unique within an op.
  let mut names: Vec<&str> = doc["operations"]
    .as_array()
    .unwrap()
    .iter()
    .map(|o| o["name"].as_str().unwrap())
    .collect();
  let count = names.len();
  names.sort_unstable();
  names.dedup();
  assert_eq!(names.len(), count);
  for op in doc["operations"].as_array().unwrap() {
    let mut params: Vec<&str> = op["params"]
      .as_array()
      .unwrap()
      .iter()
      .map(|p| p["name"].as_str().unwrap())
      .collect();
    let n = params.len();
    params.sort_unstable();
    params.dedup();
    assert_eq!(params.len(), n, "{}", op["name"]);
  }
}

#[test]
fn every_operation_has_description_access_and_returns() {
  let doc: Value = serde_json::from_slice(&schema_bytes(repo_root(), vec![])).unwrap();
  let ops = doc["operations"].as_array().unwrap();
  assert_eq!(ops.len(), 63);
  for op in ops {
    let name = &op["name"];
    assert!(
      op["description"]
        .as_str()
        .is_some_and(|d| d.trim().len() > 10),
      "{name}"
    );
    assert!(
      ["read", "write", "dynamic"].contains(&op["access"].as_str().unwrap()),
      "{name}"
    );
    assert_eq!(op["returns"], "object", "{name}");
  }
}

#[test]
fn the_document_and_channel_files_stay_within_budget() {
  const BUDGET: usize = 256 * 1024;
  let bytes = schema_bytes(repo_root(), vec![]);
  assert!(bytes.len() < BUDGET, "--schema is {} bytes", bytes.len());
  for name in ["cli.schema.dev.json", "cli.schema.latest.json"] {
    let len = std::fs::metadata(repo_root().join(name)).unwrap().len() as usize;
    assert!(len < BUDGET, "{name} is {len} bytes");
  }
}
