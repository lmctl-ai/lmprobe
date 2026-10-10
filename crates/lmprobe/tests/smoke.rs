//! Binary-level smoke harness — guards the §0.3 module split.
//!
//! For every case below we run the `lmprobe` binary, capture stdout +
//! exit code, and either (a) write goldens (`UPDATE_GOLDENS=1`) or (b)
//! diff against goldens.
//!
//! Set `LMPROBE_BIN=/path/to/binary` to test a specific binary instead
//! of the freshly compiled dev build (e.g. the pinned baseline).
//!
//! Fixtures live at `$LMPROBE_QA_CORPUS/<lang>/<repo>` — cases
//! whose fixture is missing are skipped (not failed) so the harness is
//! tolerant of fresh checkouts. Running with `UPDATE_GOLDENS=1` requires
//! the fixtures to be present.

use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Root of the external QA corpus (`<lang>/<repo>` clones), taken from
/// `LMPROBE_QA_CORPUS`. When unset, every corpus-backed case is skipped.
fn fixture_base() -> PathBuf {
  std::env::var_os("LMPROBE_QA_CORPUS")
    .map(PathBuf::from)
    .unwrap_or_else(|| PathBuf::from("/nonexistent/lmprobe-qa-corpus"))
}

#[derive(Clone, Copy)]
enum Mode {
  /// Validate stdout + exit code.
  Exact,
  /// Validate exit code + multiset of stdout lines (sorted compare).
  /// For sensors whose worker-thread ordering is non-deterministic
  /// (`grep`, `ref`, `find`).
  ExactSortedLines,
  /// Validate exit code only — stdout is unstable (panic backtrace, etc.).
  ExitOnly,
}

struct Case {
  name: &'static str,
  /// Fixture path relative to `fixture_base()`. None = run from CARGO_MANIFEST_DIR.
  fixture: Option<&'static str>,
  args: &'static [&'static str],
  mode: Mode,
}

fn cases() -> Vec<Case> {
  use Mode::*;
  vec![
        // --- help ---
        Case { name: "help",                       fixture: None,                          args: &["--help"],                                       mode: Exact },
        Case { name: "grep_help",                  fixture: None,                          args: &["grep", "--help"],                               mode: Exact },
        Case { name: "find_help",                  fixture: None,                          args: &["find", "--help"],                               mode: Exact },

        // --- discover ---
        Case { name: "discover_rust_fd",           fixture: Some("rust/fd"),               args: &["discover", "."],                                mode: Exact },
        Case { name: "discover_python_markupsafe", fixture: Some("python/markupsafe"),     args: &["discover", "."],                                mode: Exact },
        Case { name: "discover_go_cobra",          fixture: Some("go/cobra"),              args: &["discover", "."],                                mode: Exact },
        Case { name: "discover_java_junit5",       fixture: Some("java/junit5-samples"),   args: &["discover", "."],                                mode: Exact },
        // §2.1 Phase 2B — `--depth 1 --tree` shows top-level project
        // only, no nested children. Pinned so depth pruning stays
        // observable in CI.
        Case { name: "discover_depth1_rust_fd",    fixture: Some("rust/fd"),               args: &["discover", "--tree", "--depth", "1", "."],      mode: Exact },

        // --- find (regex on path) ---
        Case { name: "find_mainrs_rust_fd",        fixture: Some("rust/fd"),               args: &["find", "main\\.rs$", ".", "--display", "name"],                      mode: ExactSortedLines },
        Case { name: "find_readme_python",         fixture: Some("python/markupsafe"),     args: &["find", "README", ".", "--display", "name"],                          mode: ExactSortedLines },
        Case { name: "find_gomod_go",              fixture: Some("go/cobra"),              args: &["find", "go\\.mod$", ".", "--display", "name"],                       mode: ExactSortedLines },
        Case { name: "find_multi_path_fixtures",   fixture: None,                          args: &["find", "(init\\.lua|flake\\.nix)$", "--path", "tests/fixtures/lua", "--path", "tests/fixtures/nix", "--display", "name"], mode: ExactSortedLines },

        // --- grep (regex on contents) ---
        Case { name: "grep_fnmain_rust_fd",        fixture: Some("rust/fd"),               args: &["grep", "^fn main", "."],                        mode: ExactSortedLines },
        Case { name: "grep_defescape_python",      fixture: Some("python/markupsafe"),     args: &["grep", "^def escape", "."],                     mode: ExactSortedLines },
        Case { name: "grep_packagemain_go",        fixture: Some("go/cobra"),              args: &["grep", "^package main", "."],                   mode: ExactSortedLines },
        Case { name: "grep_multi_path_fixtures",   fixture: None,                          args: &["grep", "^(return M|  description = )", "--path", "tests/fixtures/lua", "--path", "tests/fixtures/nix"], mode: ExactSortedLines },

        // --- search ---
        Case { name: "search_walk_rust_fd",        fixture: Some("rust/fd"),               args: &["search", "WalkParallel", "."],                  mode: Exact },
        Case { name: "search_escape_python",       fixture: Some("python/markupsafe"),     args: &["search", "escape_silent", "."],                 mode: Exact },
        Case { name: "search_execute_go",          fixture: Some("go/cobra"),              args: &["search", "ExecuteContextC", "."],               mode: Exact },

        // --- def ---
        Case { name: "def_walkparallel_rust_fd",   fixture: Some("rust/fd"),               args: &["def", "WalkParallel", "."],                     mode: Exact },
        Case { name: "def_escape_python",          fixture: Some("python/markupsafe"),     args: &["def", "escape_silent", "."],                    mode: Exact },
        Case { name: "def_execute_go",             fixture: Some("go/cobra"),              args: &["def", "ExecuteContext", "."],                   mode: Exact },
        // §0.6 — `def Execute` on cobra must return ONLY `Execute`,
        // not `ExecuteContext` / `ExecuteContextC`. Closes QA §2.7.
        Case { name: "def_execute_anchored_go",    fixture: Some("go/cobra"),              args: &["def", "Execute", "."],                          mode: Exact },

        // --- ref ---
        Case { name: "ref_walkparallel_rust_fd",   fixture: Some("rust/fd"),               args: &["ref", "WalkParallel", "."],                     mode: ExactSortedLines },
        Case { name: "ref_escape_python",          fixture: Some("python/markupsafe"),     args: &["ref", "escape_silent", "."],                    mode: ExactSortedLines },
        Case { name: "ref_executecontext_go",      fixture: Some("go/cobra"),              args: &["ref", "ExecuteContext", "."],                   mode: ExactSortedLines },

        // --- callers (happy paths) ---
        Case { name: "callers_walkparallel_rust",  fixture: Some("rust/fd"),               args: &["callers", "WalkParallel", "."],                 mode: Exact },
        Case { name: "callers_escape_python",      fixture: Some("python/markupsafe"),     args: &["callers", "escape_silent", "."],                mode: Exact },
        Case { name: "callers_executecontext_go",  fixture: Some("go/cobra"),              args: &["callers", "ExecuteContext", "."],               mode: Exact },

        // --- context exit-status semantics ---
        // A non-function symbol with a located definition still
        // yields useful anchored payloads, so `context` must report
        // matched even when callers/callees are empty.
        Case { name: "context_user_fixture",       fixture: None,                          args: &["--format", "json", "context", "--recent-history-limit", "1", "User", "tests/fixtures/rust"], mode: Exact },
        // Opt-in one-hop import-aware mode resolves a symbol imported
        // by the requested file, then returns the target definition.
        Case { name: "context_import_aware_fixture", fixture: None,                        args: &["--format", "json", "context", "--import-aware", "ImportedThing", "tests/fixtures/import_aware/page.ts"], mode: Exact },
        // Missing symbols are genuine no-hit bundles: preserve the
        // SYMBOL_NOT_FOUND warning, but exit as no_match.
        Case { name: "context_missing_fixture",    fixture: None,                          args: &["--format", "json", "context", "ZzZNotASymbolZzZ", "tests/fixtures/rust"], mode: Exact },

        // --- imports ---
        Case { name: "imports_clap_rust_fd",       fixture: Some("rust/fd"),               args: &["imports", "clap", "."],                         mode: Exact },
        Case { name: "imports_os_python",          fixture: Some("python/markupsafe"),     args: &["imports", "re", "."],                           mode: Exact },
        Case { name: "imports_fmt_go",             fixture: Some("go/cobra"),              args: &["imports", "fmt", "."],                          mode: Exact },

        // --- decorations ---
        Case { name: "decorations_derive_rust_fd", fixture: Some("rust/fd"),               args: &["decorations", "derive", "."],                   mode: Exact },
        Case { name: "decorations_test_rust_fd",   fixture: Some("rust/fd"),               args: &["decorations", "test", "."],                     mode: Exact },
        Case { name: "decorations_override_java",  fixture: Some("java/junit5-samples"),   args: &["decorations", "Override", "."],                 mode: Exact },

        // --- formerly-panicking cases (QA §2.1, §2.2, §2.14) ---
        // §0.4 fixes them; outputs are now stable so we capture stdout
        // (mode: Exact) instead of just the exit code.
        Case { name: "panic_callers_xml_dom4j",    fixture: Some("xml/dom4j/xml"),         args: &["callers", "servlet", "."],                      mode: Exact },
        Case { name: "panic_imports_quoted_go",    fixture: Some("go/cobra"),              args: &["imports", "\"fmt\"", "."],                      mode: Exact },
        // §2.14 covered separately by `pipe_brokenpipe`.

        // --- §0.10 evolution post-XDG ---
        // `--no-cache` keeps the test stateless (no XDG writes between
        // runs). Output is git-history-derived so it's stable per
        // fixture commit.
        Case { name: "evolution_execute_cobra_no_cache", fixture: Some("go/cobra"),        args: &["--no-cache", "evolution", "Execute", "."],      mode: Exact },

        // --- §3.1 Phase 3A history family ---
        // `history evolution` aliases `evolution`; pin both still
        // produce the same milestone tail.
        Case { name: "history_evolution_alias_cobra",   fixture: Some("go/cobra"),        args: &["--no-cache", "history", "evolution", "Execute"], mode: Exact },

        // --- §0.7 / Phase 1 final exit-criteria pin ---
        // `lmprobe <any> nonexistent .` must exit 1 (no_match), not
        // 0 (silent success) and not 2 (error). Pinned here so the
        // contract is part of every smoke run.
        Case { name: "exit_one_def_nonexistent",   fixture: Some("rust/fd"),               args: &["def", "ZzZNotASymbolZzZ", "."],                 mode: Exact },
        Case { name: "exit_one_search_nonexistent",fixture: Some("rust/fd"),               args: &["search", "--name", "ZzZNotASymbolZzZ", "."],   mode: Exact },

        // --- Phase 4E `query` subcommand smokes ---
        // The harness pins `--format markdown` (line 155); per
        // arbitration K, `query` falls back to JSON for markdown,
        // so these goldens are the JSON envelope. fixture: None
        // means cwd = CARGO_MANIFEST_DIR (`crates/lmprobe/`), so
        // `tests/fixtures/lua` and `tests/fixtures/nix` are stable
        // in-tree paths the discover sensor can hit.
        Case { name: "query_help",                  fixture: None,                         args: &["query", "--help"],                                                                                                                                  mode: Exact },
        Case { name: "query_discover_fixture",      fixture: None,                         args: &["query", "{ discover(path: \"tests/fixtures/lua\") { projects { name } warnings { code } exitStatus } }"],                                            mode: Exact },
        Case { name: "query_listkinds_rust",        fixture: None,                         args: &["query", "{ listKinds(lang: RUST) { rows { portable kinds } exitStatus } }"],                                                                            mode: Exact },
        // Phase 5C regression coverage — the 5 cases below pin
        // registry-routed dispatch for non-Rust languages. Without
        // these, a CLI bypass that falls through to lang-core's
        // catch-all would return empty kinds for 28 of 29 languages
        // and only `query_listkinds_rust` would catch it (Rust
        // retained lang-core arms via Phase 5A.3 predating ch.18).
        // Spread covers code-tier (java, python, kotlin) and
        // data-tier (yaml, json) categories.
        Case { name: "query_listkinds_java",        fixture: None,                         args: &["query", "{ listKinds(lang: JAVA) { rows { portable kinds } exitStatus } }"],                                                                            mode: Exact },
        Case { name: "query_listkinds_python",      fixture: None,                         args: &["query", "{ listKinds(lang: PYTHON) { rows { portable kinds } exitStatus } }"],                                                                          mode: Exact },
        Case { name: "query_listkinds_kotlin",      fixture: None,                         args: &["query", "{ listKinds(lang: KOTLIN) { rows { portable kinds } exitStatus } }"],                                                                          mode: Exact },
        Case { name: "query_listkinds_yaml",        fixture: None,                         args: &["query", "{ listKinds(lang: YAML) { rows { portable kinds } exitStatus } }"],                                                                            mode: Exact },
        Case { name: "query_listkinds_json",        fixture: None,                         args: &["query", "{ listKinds(lang: JSON) { rows { portable kinds } exitStatus } }"],                                                                            mode: Exact },
        Case { name: "query_alias_two_discovers",   fixture: None,                         args: &["query", "{ a: discover(path: \"tests/fixtures/lua\") { projects { name } } b: discover(path: \"tests/fixtures/nix\") { projects { name } } }"],     mode: Exact },
        Case { name: "query_unknown_field_error",   fixture: None,                         args: &["query", "{ nope }"],                                                                                                                                mode: Exact },

        // --- Phase 4E-B composition (ComplexObject) smokes ---
        // Hit `tests/fixtures/composition/lib.rs` (`compute_total`
        // with two named callers). Pins the def → callers → enclosing
        // chain in one round-trip — the §5.1 AI-leverage win.
        Case { name: "query_compose_def_callers",   fixture: None,                         args: &["query", "{ def(name: \"compute_total\", path: \"tests/fixtures/composition\") { hits { name filePath callers(limit: 5, path: \"tests/fixtures/composition\") { hits { name filePath enclosing { name kind } } } } } }"], mode: Exact },
        // Pins def → recentHistory: selecting only `changeType`
        // keeps the golden stable. The fixture commit is the only
        // history entry for `compute_total`, so `INTRODUCED` is
        // deterministic until the fixture itself is amended.
        Case { name: "query_compose_recent_history", fixture: None,                        args: &["query", "{ def(name: \"compute_total\", path: \"tests/fixtures/composition\") { hits { name recentHistory(limit: 2) { entries { changeType } exitStatus } } } }"], mode: Exact },
    ]
}

fn binary() -> PathBuf {
  if let Ok(p) = std::env::var("LMPROBE_BIN") {
    return PathBuf::from(p);
  }
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

fn goldens_dir() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/goldens")
}

fn sort_lines(s: &str) -> String {
  let mut lines: Vec<&str> = s.lines().collect();
  lines.sort();
  let mut out = lines.join("\n");
  if s.ends_with('\n') {
    out.push('\n');
  }
  out
}

fn run_case(case: &Case) -> (String, i32) {
  let mut cmd = Command::new(binary());
  // §1.6 — pin format to markdown so goldens stay stable when run
  // under `cargo test` (where stdout is piped → ndjson by default).
  // Cases that exercise other formats supply --format explicitly in
  // their args.
  if !case.args.iter().any(|arg| *arg == "--format") {
    cmd.args(["--format", "markdown"]);
  }
  cmd.args(case.args);
  if let Some(fix) = case.fixture {
    cmd.current_dir(fixture_base().join(fix));
  } else {
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"));
  }
  let out = cmd.output().expect("spawn lmprobe");
  let mut stdout = String::from_utf8_lossy(&out.stdout).into_owned();
  // Clap pads some blank lines in long help. Lock the help text rather than
  // formatter-only trailing whitespace; operational output remains byte-exact.
  if case.args.contains(&"--help") {
    let trailing_newline = stdout.ends_with('\n');
    stdout = stdout
      .lines()
      .map(str::trim_end)
      .collect::<Vec<_>>()
      .join("\n");
    if trailing_newline {
      stdout.push('\n');
    }
  }
  let exit = out.status.code().unwrap_or(-1);
  (stdout, exit)
}

/// §0.9 — `trace` must refuse when run outside a git repository
/// (Lead arbitration). We provision a tmp dir with one tiny .rs file
/// and expect exit 2 + the `--- Trace Results ---` empty render.
fn run_trace_nogit_refusal() -> (String, i32) {
  let tmp = std::env::temp_dir().join(format!("lmprobe-smoke-notgit-{}", std::process::id()));
  let _ = fs::remove_dir_all(&tmp);
  fs::create_dir_all(&tmp).expect("create tmp dir");
  fs::write(tmp.join("main.rs"), "fn main() {}\n").expect("write tmp file");
  // Confirm it's not under any git repo before running.
  let out = Command::new(binary())
    .args([
      "--format",
      "markdown",
      "trace",
      "fn main() { $$$ }",
      ".",
      "--exec",
      "true",
    ])
    .current_dir(&tmp)
    .output()
    .expect("spawn lmprobe trace");
  let _ = fs::remove_dir_all(&tmp);
  (
    String::from_utf8_lossy(&out.stdout).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

/// §0.9 (Reviewer must-fix #1) — `trace` must refuse a working tree
/// with **untracked** files, since `git status --porcelain` (the plan
/// contract) includes untracked by default. Fixture-builder: tmp git
/// repo with one committed file + one untracked file.
fn run_trace_refuses_untracked() -> (String, i32) {
  let tmp = std::env::temp_dir().join(format!("lmprobe-smoke-untracked-{}", std::process::id()));
  let _ = fs::remove_dir_all(&tmp);
  fs::create_dir_all(&tmp).expect("create tmp dir");
  fs::write(tmp.join("main.rs"), "fn main() { let x = 1; }\n").expect("write fixture");

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
  // Add the untracked file AFTER the commit.
  fs::write(tmp.join("untracked.txt"), "scratch\n").expect("write untracked");

  let out = Command::new(binary())
    .args([
      "--format",
      "markdown",
      "trace",
      "fn main() { $$$ }",
      ".",
      "--exec",
      "true",
    ])
    .current_dir(&tmp)
    .output()
    .expect("spawn lmprobe trace");
  let _ = fs::remove_dir_all(&tmp);
  (
    String::from_utf8_lossy(&out.stdout).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

/// §0.9 / §0.11 — `trace --force` positive case. Fixture-builder:
/// init a tmp git repo, commit one file, run trace with --force + a
/// benign `--exec true`, then verify the working tree is byte-
/// identical post-rollback. Returns (stdout, exit, file_unchanged).
fn run_trace_force_positive() -> (String, i32, bool) {
  let tmp = std::env::temp_dir().join(format!("lmprobe-smoke-trace-{}", std::process::id()));
  let _ = fs::remove_dir_all(&tmp);
  fs::create_dir_all(&tmp).expect("create tmp dir");

  let original = "fn main() { let x = 1; }\n";
  fs::write(tmp.join("main.rs"), original).expect("write fixture");

  // Init a git repo + commit so trace's safety guard is satisfied.
  let git = |args: &[&str]| {
    Command::new("git")
      .args(args)
      .current_dir(&tmp)
      .output()
      .expect("spawn git")
  };
  git(&["init", "-q"]);
  git(&[
    "-c",
    "user.email=t@t",
    "-c",
    "user.name=t",
    "config",
    "commit.gpgsign",
    "false",
  ]);
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

  // Use a custom template so the test doesn't depend on Rust's
  // default trace template producing valid syntax. Template is
  // a no-op comment so injection doesn't break compilation.
  let out = Command::new(binary())
    .args([
      "--format",
      "markdown",
      "trace",
      "fn main() { $$$BODY }",
      ".",
      "--exec",
      "true",
      "--template",
      "fn main() { /* TRACE */ $$$BODY }",
      "--force",
    ])
    .current_dir(&tmp)
    .output()
    .expect("spawn lmprobe trace");

  // Verify rollback restored the original byte-for-byte.
  let after = fs::read_to_string(tmp.join("main.rs")).unwrap_or_default();
  let unchanged = after == original;
  let _ = fs::remove_dir_all(&tmp);

  (
    String::from_utf8_lossy(&out.stdout).into_owned(),
    out.status.code().unwrap_or(-1),
    unchanged,
  )
}

/// QA §2.14 — `lmprobe <high-volume> | head` panics on broken pipe.
/// We simulate `head` by reading one byte of stdout, then dropping the
/// reader so subsequent writes from the child fail. Use a high-volume
/// command (`grep '.' .` matches every line) so the child is still
/// emitting output when we close the pipe.
fn run_pipe_brokenpipe() -> i32 {
  let fixture = fixture_base().join("rust/fd");
  if !fixture.exists() {
    return -2; // skip sentinel
  }
  let mut child = Command::new(binary())
    .args(["grep", ".", "."])
    .current_dir(&fixture)
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .spawn()
    .expect("spawn lmprobe grep");
  {
    let mut stdout = child.stdout.take().expect("stdout pipe");
    let mut buf = [0u8; 1];
    let _ = stdout.read(&mut buf);
    // drop closes the read end → next write fails with BrokenPipe
  }
  child
    .wait()
    .expect("wait lmprobe grep")
    .code()
    .unwrap_or(-1)
}

// The full golden smoke suite pins POSIX-rendered snapshots and external
// fixture paths. Windows gets broad integration coverage from the individual
// tests, plus this stable binary-level smoke check.
#[cfg(windows)]
#[test]
fn smoke() {
  let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let cases: &[&[&str]] = &[
    &["--version"],
    &["--help"],
    &["find", "--help"],
    &["grep", "--help"],
    &[
      "--format",
      "json",
      "find",
      "lib\\.rs$",
      "tests/fixtures/rust",
    ],
    &[
      "--format",
      "json",
      "grep",
      "pub struct User",
      "tests/fixtures/rust",
    ],
    &[
      "--format",
      "json",
      "query",
      "{ listKinds(lang: RUST) { rows { portable kinds } exitStatus } }",
    ],
  ];

  for args in cases {
    let out = Command::new(binary())
      .args(*args)
      .current_dir(&manifest_dir)
      .output()
      .unwrap_or_else(|err| panic!("spawn lmprobe {:?}: {err}", args));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
      out.status.success(),
      "lmprobe {:?} failed\nstdout:\n{}\nstderr:\n{}",
      args,
      stdout,
      stderr
    );
    assert!(
      !stdout.trim().is_empty(),
      "lmprobe {:?} produced empty stdout\nstderr:\n{}",
      args,
      stderr
    );
  }
}

#[cfg(not(windows))]
#[test]
fn smoke() {
  let update = std::env::var("UPDATE_GOLDENS").is_ok();
  let goldens = goldens_dir();
  fs::create_dir_all(&goldens).unwrap();

  let cases = cases();

  // §3B-PB2 — parallelise the main case loop. Each case spawns
  // its own lmprobe subprocess; the harness work is process I/O
  // + golden compare, so OS-level concurrency cuts wall-clock
  // ~6× on an 8-core box. Trace/pipe special cases stay
  // sequential below — they provision PID-named tmp dirs that
  // happen not to collide but the simpler harness wins.
  use std::sync::{Arc, Mutex};
  let failures_arc: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
  let skipped_arc: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
  let ran_count: Arc<Mutex<usize>> = Arc::new(Mutex::new(0usize));

  let n_threads = std::thread::available_parallelism()
    .map(|n| n.get())
    .unwrap_or(4)
    .min(cases.len().max(1));
  let queue: Arc<Mutex<Vec<&Case>>> = Arc::new(Mutex::new(cases.iter().collect()));

  std::thread::scope(|scope| {
    for _ in 0..n_threads {
      let queue = queue.clone();
      let failures_arc = failures_arc.clone();
      let skipped_arc = skipped_arc.clone();
      let ran_count = ran_count.clone();
      let goldens = goldens.clone();
      scope.spawn(move || loop {
        let case = match queue.lock().unwrap().pop() {
          Some(c) => c,
          None => break,
        };
        if let Some(fix) = case.fixture {
          let fix_path = fixture_base().join(fix);
          if !fix_path.exists() {
            skipped_arc.lock().unwrap().push(case.name);
            continue;
          }
        }
        *ran_count.lock().unwrap() += 1;

        let (stdout, exit) = run_case(case);
        let stdout_path = goldens.join(format!("{}.stdout", case.name));
        let exit_path = goldens.join(format!("{}.exit", case.name));

        if update {
          fs::write(&exit_path, format!("{}\n", exit)).unwrap();
          match case.mode {
            Mode::Exact => {
              fs::write(&stdout_path, &stdout).unwrap();
            }
            Mode::ExactSortedLines => {
              fs::write(&stdout_path, sort_lines(&stdout)).unwrap();
            }
            Mode::ExitOnly => {
              let _ = fs::remove_file(&stdout_path);
            }
          }
          continue;
        }

        let expected_exit: i32 = match fs::read_to_string(&exit_path) {
          Ok(s) => s
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("invalid exit golden for {}", case.name)),
          Err(_) => {
            failures_arc
              .lock()
              .unwrap()
              .push(format!("{}: missing exit golden", case.name));
            continue;
          }
        };
        if exit != expected_exit {
          failures_arc.lock().unwrap().push(format!(
            "{}: exit {} != golden {}",
            case.name, exit, expected_exit
          ));
        }
        match case.mode {
          Mode::Exact => {
            let expected_stdout = match fs::read_to_string(&stdout_path) {
              Ok(s) => s,
              Err(_) => {
                failures_arc
                  .lock()
                  .unwrap()
                  .push(format!("{}: missing stdout golden", case.name));
                continue;
              }
            };
            if stdout != expected_stdout {
              failures_arc
                .lock()
                .unwrap()
                .push(format!("{}: stdout differs from golden", case.name));
            }
          }
          Mode::ExactSortedLines => {
            let expected_stdout = match fs::read_to_string(&stdout_path) {
              Ok(s) => s,
              Err(_) => {
                failures_arc
                  .lock()
                  .unwrap()
                  .push(format!("{}: missing stdout golden", case.name));
                continue;
              }
            };
            let actual_sorted = sort_lines(&stdout);
            if actual_sorted != expected_stdout {
              failures_arc.lock().unwrap().push(format!(
                "{}: stdout (line-sorted) differs from golden",
                case.name
              ));
            }
          }
          Mode::ExitOnly => {}
        }
      });
    }
  });

  let mut failures = Arc::try_unwrap(failures_arc).unwrap().into_inner().unwrap();
  let mut skipped = Arc::try_unwrap(skipped_arc).unwrap().into_inner().unwrap();
  let mut ran = Arc::try_unwrap(ran_count).unwrap().into_inner().unwrap();

  // Pipe panic case (§2.14).
  let exit_path = goldens.join("pipe_brokenpipe.exit");
  let exit = run_pipe_brokenpipe();
  if exit == -2 {
    skipped.push("pipe_brokenpipe");
  } else if update {
    fs::write(&exit_path, format!("{}\n", exit)).unwrap();
    ran += 1;
  } else {
    ran += 1;
    let expected: i32 = fs::read_to_string(&exit_path)
      .ok()
      .and_then(|s| s.trim().parse().ok())
      .unwrap_or_else(|| panic!("missing pipe_brokenpipe exit golden"));
    if exit != expected {
      failures.push(format!(
        "pipe_brokenpipe: exit {} != golden {}",
        exit, expected
      ));
    }
  }

  // §0.9 — trace refuses non-git directory (Mode: Exact).
  let tname = "trace_refuses_nogit";
  let (stdout, exit) = run_trace_nogit_refusal();
  let stdout_path = goldens.join(format!("{}.stdout", tname));
  let exit_path = goldens.join(format!("{}.exit", tname));
  if update {
    fs::write(&exit_path, format!("{}\n", exit)).unwrap();
    fs::write(&stdout_path, &stdout).unwrap();
    ran += 1;
  } else {
    ran += 1;
    let expected_exit: i32 = fs::read_to_string(&exit_path)
      .ok()
      .and_then(|s| s.trim().parse().ok())
      .unwrap_or_else(|| panic!("missing {} exit golden", tname));
    let expected_stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
    if exit != expected_exit {
      failures.push(format!(
        "{}: exit {} != golden {}",
        tname, exit, expected_exit
      ));
    }
    if stdout != expected_stdout {
      failures.push(format!("{}: stdout differs from golden", tname));
    }
  }

  // §0.9 (Reviewer must-fix #1) — trace refuses untracked-only dirty.
  let tname = "trace_refuses_untracked";
  let (stdout, exit) = run_trace_refuses_untracked();
  let stdout_path = goldens.join(format!("{}.stdout", tname));
  let exit_path = goldens.join(format!("{}.exit", tname));
  if update {
    fs::write(&exit_path, format!("{}\n", exit)).unwrap();
    fs::write(&stdout_path, &stdout).unwrap();
    ran += 1;
  } else {
    ran += 1;
    let expected_exit: i32 = fs::read_to_string(&exit_path)
      .ok()
      .and_then(|s| s.trim().parse().ok())
      .unwrap_or_else(|| panic!("missing {} exit golden", tname));
    let expected_stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
    if exit != expected_exit {
      failures.push(format!(
        "{}: exit {} != golden {}",
        tname, exit, expected_exit
      ));
    }
    if stdout != expected_stdout {
      failures.push(format!("{}: stdout differs from golden", tname));
    }
  }

  // §0.9 / §0.11 — trace --force positive (deferred from 0B).
  let tname = "trace_force_positive";
  let (stdout, exit, unchanged) = run_trace_force_positive();
  if !unchanged {
    failures.push(format!(
      "{}: working tree mutated after rollback (file content changed)",
      tname
    ));
  }
  let stdout_path = goldens.join(format!("{}.stdout", tname));
  let exit_path = goldens.join(format!("{}.exit", tname));
  if update {
    fs::write(&exit_path, format!("{}\n", exit)).unwrap();
    fs::write(&stdout_path, &stdout).unwrap();
    ran += 1;
  } else {
    ran += 1;
    let expected_exit: i32 = fs::read_to_string(&exit_path)
      .ok()
      .and_then(|s| s.trim().parse().ok())
      .unwrap_or_else(|| panic!("missing {} exit golden", tname));
    let expected_stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
    if exit != expected_exit {
      failures.push(format!(
        "{}: exit {} != golden {}",
        tname, exit, expected_exit
      ));
    }
    if stdout != expected_stdout {
      failures.push(format!("{}: stdout differs from golden", tname));
    }
  }

  eprintln!(
    "smoke: ran {} cases ({} skipped: {:?})",
    ran,
    skipped.len(),
    skipped
  );
  if !failures.is_empty() {
    panic!(
      "smoke harness: {} failure(s):\n  - {}",
      failures.len(),
      failures.join("\n  - ")
    );
  }
}
