//! The lmapi policy matrix: access (read / write / dynamic read / dynamic write) x policy inputs
//! (`--readonly`, `--dryrun`, `--confirm`, `LMPROBE_READONLY`, `LMPROBE_DRYRUN`, a flag set to
//! false against an env floor) -> exact exit code, envelope and side effect.
//!
//! lmrust's order: read-only refuses writes first, then dry-run simulates them, then dangerous
//! operations need `--confirm`. Reads (and dynamic documents without a mutation) never consult
//! the policy.

mod lmapi_support;

use lmapi_support::*;
use serde_json::Value;
use std::fs;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
  /// `git_status` (read).
  Read,
  /// `edit_text` (write, dangerous): replaces "hello" in notes.txt.
  Write,
  /// `query_graphql` with a read document.
  DynRead,
  /// `query_graphql` with a mutation document (write, not dangerous).
  DynWrite,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Expect {
  /// Ran: a result document (exit 0).
  Ran,
  /// The dynamic mutation reached the legacy command, which refuses the fixture document.
  RanToLegacyError,
  /// lmrust's synthetic dry-run result (exit 0), nothing ran.
  DryRun,
  /// An lmapi envelope with this code and exit, nothing ran.
  Refused(&'static str, i32),
}
use Expect::*;

const MUTATION: &str =
  r#"mutation { fix(scope: {paths: ["src"]}, pattern: "x", replacement: "y") { matchesCount } }"#;
const QUERY: &str = r#"{ grep(pattern: "hello", path: ".") { exitStatus } }"#;

fn args(op: Op) -> Vec<String> {
  let v: Vec<&str> = match op {
    Op::Read => vec!["git_status"],
    Op::Write => vec![
      "edit_text",
      "--pattern",
      "hello",
      "--replacement",
      "bye",
      "notes.txt",
    ],
    Op::DynRead => vec!["query_graphql", QUERY],
    Op::DynWrite => vec!["query_graphql", MUTATION],
  };
  v.into_iter().map(String::from).collect()
}

struct Row {
  name: &'static str,
  flags: &'static [&'static str],
  env: &'static [(&'static str, &'static str)],
  /// Expected outcome for Read, Write, DynRead, DynWrite.
  expect: [Expect; 4],
}

const R6: Expect = Refused("READONLY_VIOLATION", 6);
const C9: Expect = Refused("MISSING_CONFIRMATION", 9);

fn rows() -> Vec<Row> {
  vec![
    Row {
      name: "nothing",
      flags: &[],
      env: &[],
      expect: [Ran, C9, Ran, RanToLegacyError],
    },
    Row {
      name: "--confirm",
      flags: &["--confirm"],
      env: &[],
      expect: [Ran, Ran, Ran, RanToLegacyError],
    },
    Row {
      name: "--confirm=false",
      flags: &["--confirm=false"],
      env: &[],
      expect: [Ran, C9, Ran, RanToLegacyError],
    },
    Row {
      name: "--readonly",
      flags: &["--readonly"],
      env: &[],
      expect: [Ran, R6, Ran, R6],
    },
    Row {
      name: "--read-only alias",
      flags: &["--read-only"],
      env: &[],
      expect: [Ran, R6, Ran, R6],
    },
    Row {
      name: "--readonly --confirm",
      flags: &["--readonly", "--confirm"],
      env: &[],
      expect: [Ran, R6, Ran, R6],
    },
    Row {
      name: "--dryrun",
      flags: &["--dryrun"],
      env: &[],
      expect: [Ran, DryRun, Ran, DryRun],
    },
    Row {
      name: "--dry-run alias",
      flags: &["--dry-run"],
      env: &[],
      expect: [Ran, DryRun, Ran, DryRun],
    },
    Row {
      name: "--dryrun --confirm",
      flags: &["--dryrun", "--confirm"],
      env: &[],
      expect: [Ran, DryRun, Ran, DryRun],
    },
    Row {
      name: "--dryrun --readonly",
      flags: &["--dryrun", "--readonly"],
      env: &[],
      expect: [Ran, R6, Ran, R6],
    },
    Row {
      name: "--readonly=false --confirm",
      flags: &["--readonly=false", "--confirm"],
      env: &[],
      expect: [Ran, Ran, Ran, RanToLegacyError],
    },
    Row {
      name: "LMPROBE_READONLY=1",
      flags: &["--confirm"],
      env: &[("LMPROBE_READONLY", "1")],
      expect: [Ran, R6, Ran, R6],
    },
    Row {
      name: "LMPROBE_READONLY=TRUE",
      flags: &["--confirm"],
      env: &[("LMPROBE_READONLY", "TRUE")],
      expect: [Ran, R6, Ran, R6],
    },
    Row {
      name: "LMPROBE_READONLY=0",
      flags: &["--confirm"],
      env: &[("LMPROBE_READONLY", "0")],
      expect: [Ran, Ran, Ran, RanToLegacyError],
    },
    Row {
      name: "LMPROBE_READONLY= (empty)",
      flags: &["--confirm"],
      env: &[("LMPROBE_READONLY", "")],
      expect: [Ran, Ran, Ran, RanToLegacyError],
    },
    Row {
      name: "env readonly beats --readonly=false",
      flags: &["--readonly=false", "--confirm"],
      env: &[("LMPROBE_READONLY", "1")],
      expect: [Ran, R6, Ran, R6],
    },
    Row {
      name: "env readonly beats --no-readonly",
      flags: &["--no-readonly", "--confirm"],
      env: &[("LMPROBE_READONLY", "yes")],
      expect: [Ran, R6, Ran, R6],
    },
    Row {
      name: "LMPROBE_DRYRUN=1",
      flags: &[],
      env: &[("LMPROBE_DRYRUN", "1")],
      expect: [Ran, DryRun, Ran, DryRun],
    },
    Row {
      name: "LMPROBE_DRYRUN=no",
      flags: &["--confirm"],
      env: &[("LMPROBE_DRYRUN", "no")],
      expect: [Ran, Ran, Ran, RanToLegacyError],
    },
    Row {
      name: "env dryrun beats --dryrun=false",
      flags: &["--dryrun=false", "--confirm"],
      env: &[("LMPROBE_DRYRUN", "1")],
      expect: [Ran, DryRun, Ran, DryRun],
    },
    Row {
      name: "env readonly beats env dryrun",
      flags: &[],
      env: &[("LMPROBE_READONLY", "1"), ("LMPROBE_DRYRUN", "1")],
      expect: [Ran, R6, Ran, R6],
    },
  ]
}

/// `shared` serves every op except `Write` (the only one that can change the fixture).
fn check(row: &Row, op: Op, expect: Expect, shared: &Path) -> Result<(), String> {
  let fresh = (op == Op::Write).then(fixture);
  let root = fresh.as_ref().map_or(shared, |f| f.path());
  let mut argv = args(op);
  argv.extend(row.flags.iter().map(|s| s.to_string()));
  let opts = Opts {
    env: row.env.iter().map(|(k, v)| (*k, v.to_string())).collect(),
    ..Opts::default()
  };
  let out = run(&argv, root, opts);
  let doc: Value = serde_json::from_slice(&out.stdout)
    .map_err(|e| format!("stdout is not JSON ({e}): {}", out.text()))?;
  let notes = fs::read_to_string(root.join("notes.txt")).unwrap();
  let wrote = notes != "hello world\n";
  let code = out.code();
  let ok = match expect {
    Ran => {
      code == 0
        && doc["schema"] == "lmprobe.result.v1"
        && doc.get("dryRun").is_none()
        && (op != Op::Write || wrote)
    }
    RanToLegacyError => {
      code == 2
        && doc["error"]["code"] == "VALIDATION_ERROR"
        && doc["error"]["data"]["tool_code"] == "MISSING_SCOPE"
    }
    DryRun => {
      let command: Vec<String> = doc["command"]
        .as_array()
        .map(|a| {
          a.iter()
            .map(|v| v.as_str().unwrap_or("").to_string())
            .collect()
        })
        .unwrap_or_default();
      code == 0
        && doc["dryRun"] == true
        && doc["success"] == true
        && command.first().map(String::as_str) == Some("lmprobe")
        && !wrote
    }
    Refused(name, exit) => {
      code == exit
        && doc["error"]["code"] == name
        && doc["error"]["data"]["operation"].is_string()
        && !wrote
    }
  };
  if ok {
    Ok(())
  } else {
    Err(format!(
      "[{}] {op:?}: expected {expect:?}, got exit {code} wrote={wrote}: {doc}",
      row.name
    ))
  }
}

#[test]
fn policy_matrix() {
  let rows = rows();
  let cases: Vec<(&Row, Op, Expect)> = rows
    .iter()
    .flat_map(|row| {
      [Op::Read, Op::Write, Op::DynRead, Op::DynWrite]
        .into_iter()
        .zip(row.expect)
        .map(move |(op, e)| (row, op, e))
    })
    .collect();
  let failures = std::sync::Mutex::new(Vec::new());
  let shared_fixture = fixture();
  let shared = shared_fixture.path();
  std::thread::scope(|scope| {
    for chunk in cases.chunks(cases.len().div_ceil(8)) {
      let failures = &failures;
      scope.spawn(move || {
        for (row, op, expect) in chunk {
          if let Err(e) = check(row, *op, *expect, shared) {
            failures.lock().unwrap().push(e);
          }
        }
      });
    }
  });
  let failures = failures.into_inner().unwrap();
  assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn dry_run_reports_the_exact_legacy_command() {
  let fixture = fixture();
  let mut argv = args(Op::Write);
  argv.push("--dryrun".into());
  let out = run(&argv, fixture.path(), Opts::default());
  let doc = out.json();
  assert_eq!(doc["simulated"], "edit_text");
  assert_eq!(
    doc["command"],
    serde_json::json!([
      "lmprobe",
      "edit",
      "--pattern=hello",
      "--replacement=bye",
      "--apply",
      "notes.txt"
    ])
  );
}

#[test]
fn refusals_name_the_operation_and_command() {
  let fixture = fixture();
  let mut argv = args(Op::Write);
  argv.push("--readonly".into());
  let doc = run(&argv, fixture.path(), Opts::default()).json();
  assert_eq!(doc["error"]["data"]["operation"], "edit_text");
  assert_eq!(doc["error"]["data"]["command"][1], "edit");
  let doc = run(&args(Op::Write), fixture.path(), Opts::default()).json();
  assert_eq!(doc["error"]["code"], "MISSING_CONFIRMATION");
  assert_eq!(doc["error"]["data"]["operation"], "edit_text");
}
