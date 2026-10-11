//! `fix --apply` with a path other than the repository root: the dry-run reports files relative
//! to the cwd, while the worktree and the captured patch are rooted at the repository top level.
//! Before the fix, `fix --apply src` produced an empty patch ("No valid patches in input").

mod lmapi_support;

use lmapi_support::*;
use std::fs;

#[test]
fn fix_apply_works_for_the_root_a_subdirectory_and_a_file_from_any_cwd() {
  // (cwd relative to the repo, path argument)
  for (cwd, path) in [
    (".", "."),
    (".", "src"),
    (".", "./src"),
    (".", "src/lib.rs"),
    ("src", "."),
    ("src", "lib.rs"),
  ] {
    let r = fixture();
    let out = lm(
      &[
        "fix",
        "-p",
        "x.saturating_add(1)",
        "-r",
        "x + 1",
        path,
        "--apply",
        "--json",
      ],
      &r.path().join(cwd),
    );
    let doc = out.json();
    assert_eq!(out.code(), 0, "cwd {cwd}, path {path}: {doc}");
    assert_eq!(doc["status"], "ok", "cwd {cwd}, path {path}: {doc}");
    let lib = fs::read_to_string(r.path().join("src/lib.rs")).unwrap();
    assert!(
      lib.contains("    x + 1\n"),
      "cwd {cwd}, path {path}: not applied"
    );
    assert!(!lib.contains("saturating_add"), "cwd {cwd}, path {path}");
  }
}
