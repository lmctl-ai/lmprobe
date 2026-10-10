use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
  PathBuf::from(env!("CARGO_BIN_EXE_lmprobe"))
}

struct Fixture {
  repo: PathBuf,
  xdg: PathBuf,
}

impl Drop for Fixture {
  fn drop(&mut self) {
    let _ = fs::remove_dir_all(&self.repo);
    let _ = fs::remove_dir_all(&self.xdg);
  }
}

fn unique_dir(prefix: &str) -> PathBuf {
  let suffix = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_nanos())
    .unwrap_or(0);
  std::env::temp_dir().join(format!("lmprobe-{prefix}-{}-{suffix}", std::process::id()))
}

fn git(repo: &Path, args: &[&str]) {
  let out = Command::new("git")
    .args(args)
    .current_dir(repo)
    .output()
    .expect("spawn git");
  assert!(
    out.status.success(),
    "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
    args,
    String::from_utf8_lossy(&out.stdout),
    String::from_utf8_lossy(&out.stderr)
  );
}

fn commit(repo: &Path, message: &str) {
  git(repo, &["add", "."]);
  git(repo, &["commit", "-qm", message]);
}

fn fixture() -> Fixture {
  let repo = unique_dir("history-co-change-filters");
  let xdg = unique_dir("history-co-change-filters-xdg");
  let _ = fs::remove_dir_all(&repo);
  let _ = fs::remove_dir_all(&xdg);
  fs::create_dir_all(&repo).expect("mkdir repo");
  fs::create_dir_all(&xdg).expect("mkdir xdg");

  git(&repo, &["init", "-q"]);
  git(&repo, &["config", "user.email", "t@t"]);
  git(&repo, &["config", "user.name", "t"]);

  fs::write(repo.join("target.txt"), "target 1\n").expect("write target");
  fs::write(repo.join("a.txt"), "a 1\n").expect("write a");
  commit(&repo, "target and a");

  fs::write(repo.join("target.txt"), "target 1\ntarget 2\n").expect("write target");
  fs::write(repo.join("a.txt"), "a 1\na 2\n").expect("write a");
  commit(&repo, "target and a again");

  fs::write(repo.join("target.txt"), "target 1\ntarget 2\ntarget 3\n").expect("write target");
  fs::write(repo.join("b.txt"), "b 1\n").expect("write b");
  commit(&repo, "target and b");

  fs::write(
    repo.join("target.txt"),
    "target 1\ntarget 2\ntarget 3\ntarget 4\n",
  )
  .expect("write target");
  fs::write(repo.join("c.txt"), "c 1\n").expect("write c");
  commit(&repo, "target and c");

  Fixture { repo, xdg }
}

fn run_json(fix: &Fixture, args: &[&str]) -> (serde_json::Value, String, i32) {
  let out = Command::new(binary())
    .args(args)
    .env("XDG_CACHE_HOME", &fix.xdg)
    .current_dir(&fix.repo)
    .output()
    .expect("spawn lmprobe");
  let stdout = String::from_utf8_lossy(&out.stdout);
  (
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("stdout should be JSON:\n{stdout}")),
    String::from_utf8_lossy(&out.stderr).into_owned(),
    out.status.code().unwrap_or(-1),
  )
}

#[test]
fn history_co_change_filters_by_top_and_min_score_without_rescoring() {
  let fix = fixture();
  let (full, stderr, exit) = run_json(
    &fix,
    &[
      "--format",
      "json",
      "history",
      "co-change",
      "target.txt",
      ".",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nfull:\n{full:#}");
  let rows = full["rows"].as_array().expect("rows");
  assert_eq!(rows.len(), 3, "full:\n{full:#}");
  assert_eq!(rows[0]["path"], "a.txt");
  assert_eq!(rows[0]["co_occurrences"], 2);
  assert_eq!(rows[0]["score"], 0.5);

  let (top, stderr, exit) = run_json(
    &fix,
    &[
      "--format",
      "json",
      "history",
      "co-change",
      "target.txt",
      "--top",
      "1",
      ".",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\ntop:\n{top:#}");
  let top_rows = top["rows"].as_array().expect("top rows");
  assert_eq!(top_rows.len(), 1, "top:\n{top:#}");
  assert_eq!(top_rows[0]["path"], "a.txt");
  assert_eq!(top_rows[0]["score"], 0.5);

  let (min_score, stderr, exit) = run_json(
    &fix,
    &[
      "--format",
      "json",
      "history",
      "co-change",
      "target.txt",
      "--min-score",
      "0.3",
      ".",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nmin_score:\n{min_score:#}");
  let min_rows = min_score["rows"].as_array().expect("min rows");
  assert_eq!(min_rows.len(), 1, "min_score:\n{min_score:#}");
  assert_eq!(min_rows[0]["path"], "a.txt");
  assert_eq!(min_rows[0]["score"], 0.5);
}

#[test]
fn history_co_change_filters_by_exclude_glob_and_file_type_without_rescoring() {
  let fix = fixture();
  fs::create_dir_all(fix.repo.join(".github/workflows")).expect("mkdir github");
  fs::create_dir_all(fix.repo.join("src")).expect("mkdir src");
  fs::create_dir_all(fix.repo.join("docs")).expect("mkdir docs");
  fs::write(
    fix.repo.join("target.txt"),
    "target 1\ntarget 2\ntarget 3\ntarget 4\ntarget 5\n",
  )
  .expect("write target");
  fs::write(fix.repo.join(".github/workflows/ci.yml"), "name: ci\n").expect("write ci");
  fs::write(fix.repo.join("src/app.py"), "print('x')\n").expect("write py");
  fs::write(fix.repo.join("src/lib.rs"), "fn main() {}\n").expect("write rs");
  fs::write(fix.repo.join("docs/readme.md"), "# docs\n").expect("write docs");
  commit(&fix.repo, "target and chore-like files");

  let (full, stderr, exit) = run_json(
    &fix,
    &[
      "--format",
      "json",
      "history",
      "co-change",
      "target.txt",
      ".",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nfull:\n{full:#}");
  let full_rows = full["rows"].as_array().expect("full rows");
  assert!(
    full_rows
      .iter()
      .any(|row| row["path"] == ".github/workflows/ci.yml")
      && full_rows.iter().any(|row| row["path"] == "src/app.py"),
    "default output should remain unfiltered:\n{full:#}"
  );
  let py_score = full_rows
    .iter()
    .find(|row| row["path"] == "src/app.py")
    .and_then(|row| row["score"].as_f64())
    .expect("src/app.py score");

  let (excluded, stderr, exit) = run_json(
    &fix,
    &[
      "--format",
      "json",
      "history",
      "co-change",
      "target.txt",
      "--exclude-glob",
      ".github/**",
      ".",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\nexcluded:\n{excluded:#}");
  let excluded_rows = excluded["rows"].as_array().expect("excluded rows");
  assert!(
    excluded_rows
      .iter()
      .all(|row| row["path"] != ".github/workflows/ci.yml")
      && excluded_rows.iter().any(|row| row["path"] == "src/app.py"),
    "exclude glob should remove only matching output rows:\n{excluded:#}"
  );

  let (python_only, stderr, exit) = run_json(
    &fix,
    &[
      "--format",
      "json",
      "history",
      "co-change",
      "target.txt",
      "--type",
      "python",
      ".",
    ],
  );
  assert_eq!(exit, 0, "stderr:\n{stderr}\npython_only:\n{python_only:#}");
  let python_rows = python_only["rows"].as_array().expect("python rows");
  assert_eq!(python_rows.len(), 1, "python_only:\n{python_only:#}");
  assert_eq!(python_rows[0]["path"], "src/app.py");
  assert!(
    (python_rows[0]["score"].as_f64().expect("filtered score") - py_score).abs() < 0.00001,
    "type filter should not rescore rows:\nfull:\n{full:#}\npython:\n{python_only:#}"
  );

  let query = r#"
    {
      history {
        coChange(file: "target.txt", excludeGlobs: [".github/**"], fileType: "python", path: ".") {
          rows { path coOccurrences score }
          exitStatus
        }
      }
    }
  "#;
  let (envelope, stderr, exit) = run_json(&fix, &["--format", "json", "query", query]);
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  let gql_rows = envelope
    .pointer("/data/history/coChange/rows")
    .and_then(|value| value.as_array())
    .expect("graphql filtered rows");
  assert_eq!(gql_rows.len(), 1, "envelope:\n{envelope:#}");
  assert_eq!(gql_rows[0]["path"], "src/app.py");
  assert!(
    (gql_rows[0]["score"].as_f64().expect("graphql score") - py_score).abs() < 0.00001,
    "graphql filter should not rescore rows:\nfull:\n{full:#}\ngraphql:\n{envelope:#}"
  );
}

#[test]
fn history_co_change_graphql_exposes_filters() {
  let fix = fixture();
  let query = r#"
    {
      history {
        coChange(file: "target.txt", top: 2, minScore: 0.25, path: ".") {
          rows { path coOccurrences score }
          exitStatus
        }
      }
    }
  "#;
  let (envelope, stderr, exit) = run_json(&fix, &["--format", "json", "query", query]);
  assert_eq!(exit, 0, "stderr:\n{stderr}\nenvelope:\n{envelope:#}");
  let rows = envelope
    .pointer("/data/history/coChange/rows")
    .and_then(|value| value.as_array())
    .expect("graphql coChange rows");
  assert_eq!(rows.len(), 2, "envelope:\n{envelope:#}");
  assert_eq!(rows[0]["path"], "a.txt");
  assert_eq!(rows[0]["score"], 0.5);
}
