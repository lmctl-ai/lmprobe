//! Legacy invariance: lmapi mode must not change any legacy invocation. The goldens under
//! `tests/goldens/legacy_invariance/` were recorded from the binary built at the commit before
//! the lmapi integration (lmprobe 0.43.0 without lmapi mode); the current binary must reproduce them
//! byte for byte (exit code, stdout and stderr, with the fixture path, cache path and build
//! commit normalized).
//!
//! Re-record from a given binary (only when a legacy change is intended):
//! `LMPROBE_RECORD_LEGACY_BIN=/path/to/lmprobe cargo test -p lmprobe --test legacy_invariance`

mod lmapi_support;

use lmapi_support::*;
use std::fs;
use std::path::{Path, PathBuf};

/// (golden name, argv). Includes argv that looks lmapi-ish but must stay legacy.
const CASES: &[(&str, &[&str])] = &[
  // help / version / no args
  ("help_long", &["--help"]),
  ("help_short", &["-h"]),
  ("help_grep", &["grep", "--help"]),
  ("help_git_status", &["git", "status", "--help"]),
  ("version", &["--version"]),
  ("no_args", &[]),
  // search family
  ("grep_markdown", &["grep", "hello", "."]),
  ("grep_json", &["grep", "hello", ".", "--json"]),
  (
    "grep_ndjson",
    &["--format", "ndjson", "grep", "helper", "src"],
  ),
  ("grep_context", &["grep", "-A", "1", "authorize", "app.py"]),
  ("grep_no_match", &["grep", "zzz_absent_zzz", "."]),
  (
    "grep_no_match_json",
    &["grep", "zzz_absent_zzz", ".", "--json"],
  ),
  (
    "grep_partial_too_large",
    &[
      "grep",
      "hello",
      "notes.txt",
      "--max-filesize",
      "8",
      "--json",
    ],
  ),
  ("grep_bad_regex", &["grep", "(", ".", "--json"]),
  ("grep_missing_path", &["grep", "x", "missing-dir", "--json"]),
  ("def", &["def", "helper", "."]),
  ("def_json", &["def", "helper", ".", "--json"]),
  ("def_missing_arg", &["def"]),
  ("ref", &["ref", "helper", "."]),
  (
    "search_name",
    &["search", "--name", "helper", "src", "--json"],
  ),
  ("find", &["find", "\\.py$", "."]),
  ("find_json", &["find", "\\.py$", ".", "--json"]),
  ("list_kinds", &["list-kinds", "rust"]),
  ("discover", &["discover", "."]),
  // graph family
  ("callers", &["callers", "helper", "src"]),
  ("callees_json", &["callees", "run", "src", "--json"]),
  ("imports", &["imports", "os", "."]),
  ("decorations", &["decorations", "require_auth", "."]),
  ("supertypes", &["supertypes", "Square", "src", "--json"]),
  ("context_json", &["context", "User", ".", "--json"]),
  // git family
  ("git_status_markdown", &["git", "status"]),
  ("git_status_json", &["git", "status", "--json"]),
  (
    "git_log_json",
    &["git", "log", "--max-count", "5", "--json"],
  ),
  (
    "git_show_json",
    &[
      "git",
      "--repo",
      ".",
      "show",
      "HEAD",
      "--file",
      "src/lib.rs",
      "--json",
    ],
  ),
  ("git_rev_parse", &["git", "rev-parse", "HEAD~1", "--json"]),
  ("git_ls_files", &["git", "ls-files", "--json"]),
  (
    "git_bad_repo",
    &["git", "status", "--repo", "/nonexistent/repo", "--json"],
  ),
  // history family
  ("history_churn", &["history", "churn", ".", "--json"]),
  ("history_todos", &["history", "todos", "."]),
  (
    "history_commit",
    &["history", "commit", "--pattern", "helper", "--json"],
  ),
  (
    "ast_diff",
    &[
      "ast-diff",
      "src/lib.rs",
      "--from",
      "HEAD~1",
      "--to",
      "HEAD",
      "--json",
    ],
  ),
  (
    "diff_context",
    &["diff-context", "--from", "HEAD~1", "--to", "HEAD"],
  ),
  // previews (no writes)
  (
    "edit_preview",
    &["edit", "notes.txt", "-p", "hello", "-r", "bye"],
  ),
  (
    "fix_preview_json",
    &[
      "fix",
      "-p",
      "x.saturating_add(1)",
      "-r",
      "x + 1",
      ".",
      "--json",
    ],
  ),
  // scanners and catalogs
  (
    "secrets_config",
    &["secrets", ".", "--config", "rules.toml", "--json"],
  ),
  (
    "secrets_recipes_list",
    &["secrets-recipes", "list", "--provider", "aws", "--json"],
  ),
  (
    "secrets_sources_show",
    &["secrets-sources", "show", "aws-secrets-manager/python"],
  ),
  (
    "guard_sites",
    &[
      "guard-sites",
      ".",
      "--sensitive",
      "delete_user",
      "--guard",
      "authorize",
    ],
  ),
  (
    "openapi_facts",
    &["openapi-facts", "openapi.yaml", "--json"],
  ),
  // GraphQL and program JSON
  (
    "query",
    &[
      "query",
      "{ grep(pattern: \"hello\", path: \".\") { exitStatus } }",
    ],
  ),
  ("query_error", &["query", "{ nope }", "--json"]),
  ("infile", &["--infile", "q.graphql"]),
  // lmapi-looking argv that must stay legacy
  ("lmapi_name_as_def_arg", &["def", "git_status", "."]),
  ("lmapi_name_as_grep_pattern", &["grep", "git_status", "."]),
  ("path_named_like_an_op", &["find", "marker", "git_status"]),
  (
    "schema_after_double_dash",
    &["grep", "--json", "--", "--schema"],
  ),
  ("unknown_subcommand", &["no-such-command"]),
  (
    "format_json_list_kinds",
    &["--format", "json", "list-kinds", "python"],
  ),
];

/// Cases whose stdout lines (one ndjson hit per line) come in the parallel walk's order; their
/// lines are compared as a sorted multiset, as `tests/smoke.rs` does.
const SORTED_LINES: &[&str] = &[
  "grep_markdown",
  "grep_json",
  "grep_no_match",
  "lmapi_name_as_grep_pattern",
  "schema_after_double_dash",
  "query",
  "infile",
];

fn golden_dir() -> PathBuf {
  Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/goldens/legacy_invariance")
}

/// The fixture: the shared lmapi fixture plus `q.graphql`, a `git_status/` directory and a
/// `--schema` line (all committed, so `git status` is clean and stable).
fn invariance_fixture() -> tempfile::TempDir {
  let r = fixture();
  let root = r.path();
  fs::write(
    root.join("q.graphql"),
    "{ grep(pattern: \"hello\", path: \".\") { exitStatus } }\n",
  )
  .unwrap();
  fs::create_dir_all(root.join("git_status")).unwrap();
  fs::write(root.join("git_status/marker.txt"), "git_status marker\n").unwrap();
  fs::write(root.join("flags.txt"), "--schema is just text here\n").unwrap();
  git(root, &["add", "."]);
  git(root, &["commit", "-qm", "invariance extras"]);
  r
}

fn normalize(text: &str, root: &Path, cache: &Path) -> String {
  let mut out = text.to_string();
  let mut roots = vec![root.display().to_string()];
  if let Ok(real) = root.canonicalize() {
    roots.push(real.display().to_string());
  }
  roots.sort_by_key(|r| std::cmp::Reverse(r.len()));
  for r in roots {
    out = out.replace(&r, "<ROOT>");
  }
  out = out.replace(&cache.display().to_string(), "<CACHE>");
  let commit = regex::Regex::new(r"\(commit [0-9a-f]{7,40}\)").unwrap();
  out = commit.replace_all(&out, "(commit <SHA>)").into_owned();
  // `find` reports file times and inode/device numbers of the freshly created fixture.
  let times = regex::Regex::new(r#""(changed_at|modified_at)": ?"[^"]*""#).unwrap();
  out = times.replace_all(&out, r#""$1":"<TIME>""#).into_owned();
  let numbers = regex::Regex::new(r#""(inode|device)": ?\d+"#).unwrap();
  numbers.replace_all(&out, r#""$1":<N>"#).into_owned()
}

fn sort_lines(text: &str) -> String {
  let mut lines: Vec<&str> = text.lines().collect();
  lines.sort_unstable();
  lines.join("\n")
}

fn record(bin: &Path, root: &Path, cache: &Path, args: &[&str], sorted: bool) -> String {
  let out = run_bin(
    bin,
    args,
    root,
    Opts {
      env: vec![
        ("XDG_CACHE_HOME", cache.display().to_string()),
        ("COLUMNS", "100".into()),
        ("TZ", "UTC".into()),
      ],
      remove_env: vec!["NO_COLOR"],
      ..Opts::default()
    },
  );
  format!(
    "exit: {:?}\n--- stdout ---\n{}\n--- stderr ---\n{}",
    out.code,
    if sorted {
      sort_lines(&normalize(&out.text(), root, cache))
    } else {
      normalize(&out.text(), root, cache)
    },
    if sorted {
      sort_lines(&normalize(&out.err_text(), root, cache))
    } else {
      normalize(&out.err_text(), root, cache)
    }
  )
}

#[test]
fn legacy_invocations_are_byte_identical_to_the_pre_lmapi_binary() {
  assert!(CASES.len() >= 40);
  let recording = std::env::var_os("LMPROBE_RECORD_LEGACY_BIN").map(PathBuf::from);
  let bin = recording.clone().unwrap_or_else(bin);
  let fixture = invariance_fixture();
  if recording.is_some() {
    fs::create_dir_all(golden_dir()).unwrap();
  }
  // Cases run in parallel, each with a private cache (history caches would otherwise couple them).
  let actuals: Vec<String> = std::thread::scope(|scope| {
    let handles: Vec<_> = CASES
      .iter()
      .map(|(name, args)| {
        let (bin, root) = (&bin, fixture.path());
        scope.spawn(move || {
          let cache = tempfile::tempdir().unwrap();
          record(bin, root, cache.path(), args, SORTED_LINES.contains(name))
        })
      })
      .collect();
    handles.into_iter().map(|h| h.join().unwrap()).collect()
  });
  let mut diffs = Vec::new();
  for ((name, args), actual) in CASES.iter().zip(actuals) {
    let file = golden_dir().join(format!("{name}.golden"));
    if recording.is_some() {
      fs::write(&file, &actual).unwrap();
      continue;
    }
    let expected =
      fs::read_to_string(&file).unwrap_or_else(|_| panic!("missing golden {}", file.display()));
    if expected != actual {
      diffs.push(format!(
        "{name} ({args:?}):\n--- golden\n{expected}\n--- now\n{actual}"
      ));
    }
  }
  assert!(diffs.is_empty(), "{}", diffs.join("\n\n"));
}

#[test]
fn the_goldens_cover_every_exit_code_class() {
  let mut seen = std::collections::BTreeSet::new();
  for (name, _) in CASES {
    let text = fs::read_to_string(golden_dir().join(format!("{name}.golden"))).unwrap();
    seen.insert(text.lines().next().unwrap().to_string());
  }
  for exit in [
    "exit: Some(0)",
    "exit: Some(1)",
    "exit: Some(2)",
    "exit: Some(3)",
  ] {
    assert!(seen.contains(exit), "no golden with {exit}: {seen:?}");
  }
}
