//! Phase 4E §4E-C.4 — surface-equality test (per arbitration H).
//!
//! Two layers:
//! - **Layer 1**: for each (verb, args) tuple, run the same logical
//!   query through the CLI verb and through the GraphQL `query`
//!   subcommand and assert the inner shape (post-envelope-strip,
//!   post-array-sort) is byte-equal. Catches per-channel drift
//!   between CLI markdown→json renderers and the GraphQL
//!   SimpleObject derive.
//! - **Layer 2**: for D7 mutation refusals (DIRTY_TREE,
//!   WORKSPACE_READ_ONLY) confirm the diagnostic code surfaces
//!   identically — exit 2 + stderr DIRTY_TREE on the CLI side,
//!   `errors[].extensions.code = DIRTY_TREE` on the GraphQL side.
//!
//! Goldens-style sorting: the JSON envelope ships `hits[]` /
//! `projects[]` / `paths[]` in walker-thread-arrival order which
//! is non-deterministic. We sort each array by a canonical key
//! before compare so the test isn't flaky.

use std::path::PathBuf;
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

fn workspace_root() -> PathBuf {
  // CARGO_MANIFEST_DIR = crates/lmprobe; walk up two for the
  // workspace root.
  let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  p.pop();
  p.pop();
  p
}

/// Run a CLI command from `cwd`, capturing stdout + exit + stderr.
fn run(args: &[&str], cwd: &PathBuf) -> (String, String, i32) {
  let out = Command::new(binary())
    .args(args)
    .current_dir(cwd)
    .output()
    .expect("spawn lmprobe");
  (
    String::from_utf8_lossy(&out.stdout).into_owned(),
    String::from_utf8_lossy(&out.stderr).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

/// Canonicalize a JSON value for cross-channel compare. Two known
/// drifts between CLI envelopes and GraphQL responses:
///
/// 1. **Field names**: CLI is snake_case (`file_path`); GraphQL is
///    camelCase (`filePath`) by async-graphql convention. We
///    canonicalize to snake_case.
/// 2. **Enum values**: CLI ships `exit_status: "matched"` etc;
///    GraphQL ships `exitStatus: "MATCHED"` (SCREAMING_SNAKE_CASE
///    by async-graphql Enum convention). We canonicalize enum-
///    valued fields to lowercase.
///
/// Plus: strip `schema` (varies cosmetically — irrelevant to payload
/// equivalence) and sort arrays so worker-thread arrival order
/// doesn't matter.
fn normalize(v: serde_json::Value) -> serde_json::Value {
  let v = canonicalize(v);
  if let serde_json::Value::Object(mut map) = v {
    // Strip channel-specific noise:
    // - `schema`: cosmetic version string.
    // - `warnings`: channel-specific by design — CLI surfaces
    //   `DEPRECATED_POSITIONAL_QUERY` for the positional-name
    //   mode; GraphQL uses the typed `query` arg and has no
    //   such concept. Per arbitration H "shape equivalence on
    //   the data payload" — warnings are tested separately
    //   in Layer 2.
    map.remove("schema");
    map.remove("warnings");
    for (_k, val) in map.iter_mut() {
      if let serde_json::Value::Array(arr) = val {
        arr.sort_by(|a, b| canonical_key(a).cmp(&canonical_key(b)));
      }
    }
    return serde_json::Value::Object(map);
  }
  v
}

/// Recursively map every object key from camelCase → snake_case
/// (idempotent on snake_case input) and lowercase any
/// `exit_status` / `change_type` enum-valued strings. Same target
/// shape across both channels.
///
/// Empty `[]` / `null` values are dropped from objects: serde's
/// `skip_serializing_if = "Vec::is_empty" / "Option::is_none"`
/// makes the CLI envelope omit those keys, while GraphQL's
/// selection-set contract returns them as explicit `[]` / `null`.
/// Treating absent ≡ empty puts both channels on the same footing
/// for the equivalence comparison.
fn canonicalize(v: serde_json::Value) -> serde_json::Value {
  match v {
    serde_json::Value::Object(map) => {
      let mut out = serde_json::Map::new();
      for (k, val) in map {
        let key = camel_to_snake(&k);
        let val = canonicalize(val);
        let val = if matches!(key.as_str(), "modified_at" | "changed_at") {
          match val
            .as_str()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
          {
            Some(time) => serde_json::Value::String(time.with_timezone(&chrono::Utc).to_rfc3339()),
            None => val,
          }
        } else {
          val
        };
        // Drop empty arrays + nulls — serde's
        // skip_serializing_if hides them on the CLI side.
        if matches!(&val, serde_json::Value::Array(a) if a.is_empty()) {
          continue;
        }
        if matches!(&val, serde_json::Value::Null) {
          continue;
        }
        let val = if matches!(key.as_str(), "exit_status" | "change_type") {
          if let serde_json::Value::String(s) = val {
            serde_json::Value::String(s.to_lowercase())
          } else {
            val
          }
        } else {
          val
        };
        out.insert(key, val);
      }
      serde_json::Value::Object(out)
    }
    serde_json::Value::Array(arr) => {
      serde_json::Value::Array(arr.into_iter().map(canonicalize).collect())
    }
    other => other,
  }
}

/// `filePath` → `file_path`. Idempotent on already-snake input.
fn camel_to_snake(s: &str) -> String {
  let mut out = String::with_capacity(s.len() + 4);
  for (i, c) in s.chars().enumerate() {
    if c.is_ascii_uppercase() {
      if i > 0 {
        out.push('_');
      }
      out.push(c.to_ascii_lowercase());
    } else {
      out.push(c);
    }
  }
  out
}

/// Build a canonical sort key from an item's stable fields. Runs
/// after `canonicalize`, so all keys are already snake_case.
fn canonical_key(v: &serde_json::Value) -> String {
  if let Some(p) = v.as_str() {
    // `find`'s paths array is `Vec<String>`.
    return p.to_string();
  }
  let s = |key: &str| {
    v.get(key)
      .and_then(|v| v.as_str())
      .unwrap_or("")
      .to_string()
  };
  let n = |key: &str| v.get(key).and_then(|v| v.as_u64()).unwrap_or(0);
  let path = s("file_path");
  let line = n("line_start");
  let line2 = n("line_number");
  let name = s("name");
  format!("{}|{:08}|{:08}|{}", path, line, line2, name)
}

/// Layer-1 case: same logical sensor expressed via CLI args and
/// via GraphQL document.
struct ParityCase {
  name: &'static str,
  cli_args: &'static [&'static str],
  /// GraphQL document. Selection set should cover the same
  /// fields the CLI envelope produces so the post-normalize
  /// compare is meaningful.
  gql: &'static str,
  /// Field name inside `data` for the GraphQL response. e.g.
  /// `discover` for `{ discover(path: ".") { ... } }`.
  gql_data_key: &'static str,
}

fn parity_cases() -> Vec<ParityCase> {
  vec![
    ParityCase {
      name: "discover_root",
      cli_args: &["--format", "json", "discover", "."],
      gql: r#"{ discover(path: ".") { projects { name projectType version path dependenciesCount children } warnings { code message hint lang } exitStatus } }"#,
      gql_data_key: "discover",
    },
    ParityCase {
      name: "find_main_rs",
      cli_args: &[
        "--format",
        "json",
        "find",
        r"main\.rs$",
        "crates/lmprobe/src",
      ],
      gql: r#"{ find(pattern: "main\\.rs$", path: "crates/lmprobe/src") { paths files { path absolutePath fileType hasSymlinkParent sizeBytes modifiedAt changedAt device inode } warnings { code message hint lang } exitStatus } }"#,
      gql_data_key: "find",
    },
    // `search` clap layout has two optionals: `[QUERY] [PATH]`.
    // `--name Pattern crates/core/src/matcher` would consume
    // `crates/core/src/matcher` as `QUERY` and default PATH
    // to `.`. Use positional query to avoid that asymmetry —
    // both channels then express the same "search for
    // Pattern under crates/core/src/matcher" intent.
    ParityCase {
      name: "search_pattern_in_matcher",
      cli_args: &[
        "--format",
        "json",
        "search",
        "Pattern",
        "crates/core/src/matcher",
      ],
      gql: r#"{ search(query: "Pattern", path: "crates/core/src/matcher") { hits { name kind lang filePath lineStart content } warnings { code message hint lang } exitStatus } }"#,
      gql_data_key: "search",
    },
    ParityCase {
      name: "def_pattern_in_matcher",
      cli_args: &[
        "--format",
        "json",
        "def",
        "Pattern",
        "crates/core/src/matcher",
      ],
      gql: r#"{ def(name: "Pattern", path: "crates/core/src/matcher") { hits { name kind lang filePath lineStart content } warnings { code message hint lang } exitStatus } }"#,
      gql_data_key: "def",
    },
    ParityCase {
      name: "callers_blame",
      cli_args: &["--format", "json", "callers", "blame", "crates/lmprobe/src"],
      gql: r#"{ callers(name: "blame", path: "crates/lmprobe/src") { hits { name kind lang filePath lineStart content enclosingDecl enclosingDeclKind } warnings { code message hint lang } exitStatus } }"#,
      gql_data_key: "callers",
    },
  ]
}

#[test]
fn surface_equality_layer_1_read_parity() {
  let root = workspace_root();
  let mut failures: Vec<String> = Vec::new();

  for case in parity_cases() {
    // CLI side
    let (cli_stdout, _cli_stderr, cli_exit) = run(case.cli_args, &root);
    let cli_json: serde_json::Value = match serde_json::from_str(&cli_stdout) {
      Ok(v) => v,
      Err(e) => {
        failures.push(format!(
          "{}: CLI stdout not JSON — {}\n--- stdout:\n{}",
          case.name, e, cli_stdout
        ));
        continue;
      }
    };

    // GraphQL side
    let (gql_stdout, _gql_stderr, gql_exit) = run(&["--format", "json", "query", case.gql], &root);
    let gql_envelope: serde_json::Value = match serde_json::from_str(&gql_stdout) {
      Ok(v) => v,
      Err(e) => {
        failures.push(format!(
          "{}: GraphQL stdout not JSON — {}\n--- stdout:\n{}",
          case.name, e, gql_stdout
        ));
        continue;
      }
    };

    // Pull `.data.<verb>` from the GraphQL envelope.
    let gql_inner = match gql_envelope
      .get("data")
      .and_then(|d| d.get(case.gql_data_key))
    {
      Some(v) => v.clone(),
      None => {
        failures.push(format!(
          "{}: GraphQL response missing data.{}\n--- envelope:\n{}",
          case.name,
          case.gql_data_key,
          serde_json::to_string_pretty(&gql_envelope).unwrap_or_default()
        ));
        continue;
      }
    };

    // CLI exit codes are the verb's exit status; GraphQL
    // bundles a `exitStatus` field. We don't assert exit-code
    // equivalence here (the GraphQL `query` exit reflects the
    // GraphQL request, not the inner verb); instead, compare
    // the inner `exit_status` / `exitStatus` field shape.
    let _ = (cli_exit, gql_exit);

    let cli_norm = normalize(cli_json);
    let gql_norm = normalize(gql_inner);

    if cli_norm != gql_norm {
      // Render a focused diff for the failure message.
      let cli_pretty = serde_json::to_string_pretty(&cli_norm).unwrap_or_default();
      let gql_pretty = serde_json::to_string_pretty(&gql_norm).unwrap_or_default();
      failures.push(format!(
        "{}: CLI ↔ GraphQL shapes differ\n--- CLI normalized:\n{}\n--- GraphQL normalized:\n{}",
        case.name, cli_pretty, gql_pretty
      ));
      continue;
    }
    eprintln!("surface_equality: {} ok", case.name);
  }

  if !failures.is_empty() {
    panic!(
      "surface-equality layer 1: {} failure(s):\n  - {}",
      failures.len(),
      failures.join("\n  - ")
    );
  }
}

// ──────────────────────── Layer 2: D7 diagnostics ────────────────

fn provision_dirty_repo() -> PathBuf {
  let tmp = std::env::temp_dir().join(format!(
    "lmprobe-surfeq-dirty-{}-{}",
    std::process::id(),
    // Add a fast-changing suffix so concurrent test runs
    // don't collide.
    std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|d| {
        // Unique within the process: parallel tests can read the same clock value.
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        d.as_nanos() * 1024 + N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as u128
      })
      .unwrap_or(0)
  ));
  let _ = std::fs::remove_dir_all(&tmp);
  std::fs::create_dir_all(&tmp).expect("create tmp dir");
  std::fs::write(tmp.join("main.rs"), b"fn main() { println!(\"hello\"); }\n")
    .expect("write main.rs");
  let git = |args: &[&str]| {
    Command::new("git")
      .args(args)
      .current_dir(&tmp)
      .output()
      .expect("spawn git")
  };
  git(&["init", "-q"]);
  git(&["add", "main.rs"]);
  git(&[
    "-c",
    "user.email=t@t",
    "-c",
    "user.name=t",
    "commit",
    "-qm",
    "init",
  ]);
  // Add an untracked file so `git status --porcelain` is non-empty.
  std::fs::write(tmp.join("untracked.txt"), b"scratch\n").expect("write untracked");
  tmp
}

#[test]
fn surface_equality_layer_2_dirty_tree_diagnostic_equivalence() {
  let tmp = provision_dirty_repo();

  // CLI side: `lmprobe fix --apply -p ... -r ...` against a
  // dirty repo refuses with DIRTY_TREE on stderr + exit 2.
  let (cli_stdout, cli_stderr, cli_exit) = run(
    &[
      "--format",
      "json",
      "fix",
      "-p",
      "println!($A)",
      "-r",
      "eprintln!($A)",
      "--apply",
      ".",
    ],
    &tmp,
  );

  // GraphQL side: same intent expressed as a mutation.
  let gql = r#"mutation { fix(scope: { pattern: { p: "println!($A)", lang: RUST }, paths: ["."] }, pattern: "println!($A)", replacement: "eprintln!($A)", dryRun: false) { mode } }"#;
  let (gql_stdout, _gql_stderr, gql_exit) = run(&["--format", "json", "query", gql], &tmp);

  let _ = std::fs::remove_dir_all(&tmp);

  // CLI: diagnostic code shows up in either the JSON envelope's
  // `warnings[].code` or stderr text. Either source counts; the
  // contract is "the user-visible artefact says DIRTY_TREE".
  let cli_says_dirty = cli_stderr.contains("DIRTY_TREE")
    || cli_stdout.contains("\"code\": \"DIRTY_TREE\"")
    || cli_stdout.contains("\"DIRTY_TREE\"");

  // GraphQL: errors[0].code == "DIRTY_TREE" inside the
  // `lmprobe.query.v1` envelope.
  let gql_envelope: serde_json::Value =
    serde_json::from_str(&gql_stdout).expect("GraphQL stdout JSON");
  let gql_says_dirty = gql_envelope
    .get("errors")
    .and_then(|e| e.as_array())
    .and_then(|arr| arr.first())
    .and_then(|err| err.get("code"))
    .and_then(|c| c.as_str())
    == Some("DIRTY_TREE");

  assert!(
    cli_says_dirty,
    "CLI did not surface DIRTY_TREE\nstdout:\n{}\nstderr:\n{}\nexit: {}",
    cli_stdout, cli_stderr, cli_exit
  );
  assert!(
    gql_says_dirty,
    "GraphQL did not surface DIRTY_TREE in errors[].code\nstdout:\n{}",
    gql_stdout
  );
  // Phase 4E-D: exit-code equivalence per arbitration I. Both
  // surfaces exit 2 on refusal — the GraphQL `query` envelope's
  // `exit_status: "error"` now maps to process exit 2 (was
  // exit-0 in 4E-C, which violated arbitration I).
  assert_eq!(
    cli_exit, 2,
    "CLI should exit 2 on DIRTY_TREE; got {}",
    cli_exit
  );
  assert_eq!(
    gql_exit, 2,
    "GraphQL `query` should exit 2 on errors[]; got {}",
    gql_exit
  );
  eprintln!("surface_equality: DIRTY_TREE diagnostic equivalence ok");
}

#[test]
fn surface_equality_layer_2_workspace_readonly_diagnostic_equivalence() {
  // Provision a temp workspace YAML pointing at the workspace
  // root so the --workspace flag latches.
  let tmp = std::env::temp_dir().join(format!(
    "lmprobe-surfeq-ws-{}-{}",
    std::process::id(),
    std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|d| {
        // Unique within the process: parallel tests can read the same clock value.
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        d.as_nanos() * 1024 + N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as u128
      })
      .unwrap_or(0)
  ));
  let _ = std::fs::remove_dir_all(&tmp);
  std::fs::create_dir_all(&tmp).expect("tmp dir");
  let ws_path = tmp.join("ws.yaml");
  let root = workspace_root();
  std::fs::write(
    &ws_path,
    format!("roots:\n  - path: {}\n    alias: ws-root\n", root.display()),
  )
  .expect("write workspace yaml");

  // CLI side: lmprobe --workspace ws.yaml fix --apply ... must
  // refuse with WORKSPACE_READ_ONLY on stderr + exit 2.
  let (cli_stdout, cli_stderr, cli_exit) = run(
    &[
      "--workspace",
      ws_path.to_str().unwrap(),
      "--format",
      "json",
      "fix",
      "-p",
      "println!($A)",
      "-r",
      "eprintln!($A)",
      "--apply",
      ".",
    ],
    &tmp,
  );

  // GraphQL side: --workspace flag stashes WorkspaceCtx in
  // schema data; mutation resolver refuses with
  // WORKSPACE_READ_ONLY before any D7 check.
  let gql = r#"mutation { fix(scope: { pattern: { p: "println!($A)", lang: RUST }, paths: ["."] }, pattern: "println!($A)", replacement: "eprintln!($A)", dryRun: false) { mode } }"#;
  let (gql_stdout, _gql_stderr, gql_exit) = run(
    &[
      "--workspace",
      ws_path.to_str().unwrap(),
      "--format",
      "json",
      "query",
      gql,
    ],
    &tmp,
  );

  let _ = std::fs::remove_dir_all(&tmp);

  let cli_says_ws = cli_stderr.contains("WORKSPACE_READ_ONLY");
  let gql_envelope: serde_json::Value =
    serde_json::from_str(&gql_stdout).expect("GraphQL stdout JSON");
  let gql_says_ws = gql_envelope
    .get("errors")
    .and_then(|e| e.as_array())
    .and_then(|arr| arr.first())
    .and_then(|err| err.get("code"))
    .and_then(|c| c.as_str())
    == Some("WORKSPACE_READ_ONLY");

  assert!(
    cli_says_ws,
    "CLI did not surface WORKSPACE_READ_ONLY\nstdout:\n{}\nstderr:\n{}\nexit: {}",
    cli_stdout, cli_stderr, cli_exit
  );
  assert!(
    gql_says_ws,
    "GraphQL did not surface WORKSPACE_READ_ONLY in errors[].code\nstdout:\n{}",
    gql_stdout
  );
  // Phase 4E-D: exit-code equivalence per arbitration I.
  assert_eq!(
    cli_exit, 2,
    "CLI should exit 2 on WORKSPACE_READ_ONLY; got {}",
    cli_exit
  );
  assert_eq!(
    gql_exit, 2,
    "GraphQL `query` should exit 2 on errors[]; got {}",
    gql_exit
  );
  eprintln!("surface_equality: WORKSPACE_READ_ONLY diagnostic equivalence ok");
}
