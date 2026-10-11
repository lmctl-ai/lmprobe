//! lmapi mode runs each operation as a child process with a private outfile in the temp dir.
//! With a private TMPDIR per invocation: no file is left behind on any path, parallel runs do not
//! collide, and an unusable temp dir or a huge result is a structured outcome.
//! (Signals, silent children and stdout floods are unit-tested with stubs in
//! `src/lmapi/selfexec_tests.rs`.)

mod lmapi_support;

use lmapi_support::*;
use serde_json::Value;
use std::fs;
use std::path::Path;

fn in_tmp<'a>(tmp: &Path) -> Opts<'a> {
  Opts {
    env: vec![("TMPDIR", tmp.display().to_string())],
    ..Opts::default()
  }
}

fn leftovers(tmp: &Path) -> Vec<String> {
  fs::read_dir(tmp)
    .unwrap()
    .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
    .collect()
}

#[test]
fn temp_files_are_removed_on_every_path() {
  let r = fixture();
  let tmp = tempfile::tempdir().unwrap();
  let cases: Vec<(Vec<&str>, i32)> = vec![
    (vec!["git_status"], 0),                                 // ok
    (vec!["grep_text", "zzz_absent_zzz", "--path", "."], 0), // no_match
    (vec!["git_status", "--repo", "/nonexistent"], 4),       // legacy error
    (vec!["def_symbol"], 2),                                 // validation, no child
    (
      vec![
        "edit_text",
        "--pattern",
        "a",
        "--replacement",
        "b",
        "notes.txt",
      ],
      9,
    ), // policy
    (
      vec![
        "edit_text",
        "--pattern",
        "a",
        "--replacement",
        "b",
        "notes.txt",
        "--dryrun",
      ],
      0,
    ),
    (
      vec!["grep_text", "--input-json", r#"{"pattern": "a\u0000b"}"#],
      2,
    ), // spawn refused
    (vec!["query_graphql", "{ nope }"], 1), // GraphQL error
    (
      vec![
        "fix_ast",
        "--pattern",
        "x.saturating_add(1)",
        "--replacement",
        "x + 1",
        "src",
        "--confirm",
      ],
      0,
    ),
  ];
  for (args, expected) in cases {
    let out = run(&args, r.path(), in_tmp(tmp.path()));
    assert_eq!(out.code(), expected, "{args:?}: {}", out.text());
    serde_json::from_slice::<Value>(&out.stdout).expect("one JSON document");
    assert_eq!(
      leftovers(tmp.path()),
      Vec::<String>::new(),
      "after {args:?}"
    );
  }
}

#[test]
fn eight_parallel_invocations_do_not_collide() {
  let r = fixture();
  let tmp = tempfile::tempdir().unwrap();
  // Distinct expected answers so a swapped outfile would be noticed.
  let needles = [
    "hello",
    "helper",
    "Square",
    "authorize",
    "token_key",
    "openapi",
    "fixture",
    "zzz_absent",
  ];
  let results: Vec<(String, Value)> = std::thread::scope(|scope| {
    let handles: Vec<_> = needles
      .iter()
      .map(|needle| {
        let (root, tmp) = (r.path(), tmp.path());
        scope.spawn(move || {
          let out = run(&["grep_text", needle, "--path", "."], root, in_tmp(tmp));
          assert_eq!(out.code(), 0, "{needle}: {}", out.text());
          (needle.to_string(), out.json())
        })
      })
      .collect();
    handles.into_iter().map(|h| h.join().unwrap()).collect()
  });
  for (needle, doc) in results {
    let text = doc.to_string();
    if needle == "zzz_absent" {
      assert_eq!(doc["status"], "no_match");
    } else {
      assert_eq!(doc["status"], "ok", "{needle}");
      assert!(text.contains(&needle), "{needle} not in its own result");
    }
  }
  assert_eq!(leftovers(tmp.path()), Vec::<String>::new());
}

#[test]
fn an_unusable_temp_dir_is_a_structured_internal_error() {
  let r = fixture();
  let out = run(
    &["git_status"],
    r.path(),
    in_tmp(Path::new("/nonexistent/tmp")),
  );
  let doc = out.json();
  assert_eq!(
    (out.code(), doc["error"]["code"].as_str()),
    (1, Some("INTERNAL_ERROR")),
    "{doc}"
  );
  assert!(
    doc["error"]["data"]["tool_code"]
      .as_str()
      .is_some_and(|c| c.starts_with("OUTFILE_")),
    "{doc}"
  );
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o555)).unwrap();
    let out = run(&["git_status"], r.path(), in_tmp(tmp.path()));
    let writable = fs::write(tmp.path().join("probe"), "x").is_ok(); // root ignores modes
    fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o755)).unwrap();
    if !writable {
      let doc = out.json();
      assert_eq!(doc["error"]["code"], "INTERNAL_ERROR", "{doc}");
    }
  }
}

#[test]
fn a_large_result_comes_back_whole() {
  let r = fixture();
  let tmp = tempfile::tempdir().unwrap();
  let lines: String = (0..20_000).map(|i| format!("needle line {i}\n")).collect();
  fs::write(r.path().join("big.txt"), lines).unwrap();
  let out = run(
    &[
      "grep_text",
      "needle line",
      "--path",
      "big.txt",
      "--limit",
      "100000",
    ],
    r.path(),
    in_tmp(tmp.path()),
  );
  assert_eq!(out.code(), 0, "{}", out.err_text());
  let doc = out.json();
  let text = doc.to_string();
  assert!(
    text.contains("needle line 19999"),
    "result truncated ({} bytes)",
    text.len()
  );
  assert!(out.stdout.len() > 1 << 20, "{} bytes", out.stdout.len());
  assert_eq!(leftovers(tmp.path()), Vec::<String>::new());
}

/// Children of `pid` (Linux `/proc`).
#[cfg(target_os = "linux")]
fn children_of(pid: u32) -> Vec<u32> {
  fs::read_dir("/proc")
    .unwrap()
    .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
    .filter(|p| {
      fs::read_to_string(format!("/proc/{p}/stat"))
        .ok()
        // `pid (comm) state ppid ...`: the comm may contain spaces, so split after ')'.
        .and_then(|stat| stat.rsplit_once(')').map(|(_, rest)| rest.to_string()))
        .and_then(|rest| rest.split_whitespace().nth(1).map(str::to_string))
        .is_some_and(|ppid| ppid == pid.to_string())
    })
    .collect()
}

#[cfg(target_os = "linux")]
#[test]
fn killing_lmapi_mode_kills_its_legacy_child() {
  use std::process::{Command, Stdio};
  use std::time::{Duration, Instant};
  let r = fixture();
  // A 100 KiB literal pattern keeps the debug-build legacy grep busy for many seconds.
  let file = r.path().join("slow.json");
  let pattern = "b".repeat(100 * 1024);
  fs::write(&file, serde_json::json!({ "pattern": pattern }).to_string()).unwrap();
  let mut parent = Command::new(bin())
    .args(["grep_text", "--input-file"])
    .arg(&file)
    .current_dir(r.path())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .unwrap();
  let start = Instant::now();
  let child = loop {
    if let Some(c) = children_of(parent.id()).first() {
      break *c;
    }
    assert!(
      start.elapsed() < Duration::from_secs(10),
      "no legacy child appeared"
    );
    std::thread::sleep(Duration::from_millis(20));
  };
  parent.kill().unwrap();
  parent.wait().unwrap();
  let start = Instant::now();
  while Path::new(&format!("/proc/{child}")).exists() {
    let state = fs::read_to_string(format!("/proc/{child}/stat")).unwrap_or_default();
    if state.contains(") Z ") {
      break; // dead, waiting to be reaped by init
    }
    if start.elapsed() > Duration::from_secs(5) {
      let _ = Command::new("kill")
        .args(["-9", &child.to_string()])
        .status();
      panic!("the legacy child {child} outlived its killed parent");
    }
    std::thread::sleep(Duration::from_millis(20));
  }
}
