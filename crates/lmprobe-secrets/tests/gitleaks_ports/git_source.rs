use crate::support::{run_git, run_lmprobe_json, GITLEAKS_CONFIG};
use std::fs;

#[test]
fn gitleaks_sources_git_test_history_finds_committed_and_removed_secret() {
  // Applicable port of sources/git_test.go git-log semantics: git history mode
  // scans historical additions, not just the final working tree.
  let temp = tempfile::tempdir().expect("tempdir");
  run_git(temp.path(), &["init"]);
  run_git(
    temp.path(),
    &["config", "user.email", "lmprobe@example.test"],
  );
  run_git(temp.path(), &["config", "user.name", "lmprobe"]);
  fs::write(
    temp.path().join("secret.go"),
    r#"awsToken := "AKIALALEMEL33243OLIA""#,
  )
  .expect("write secret");
  run_git(temp.path(), &["add", "secret.go"]);
  run_git(temp.path(), &["commit", "-m", "add secret"]);
  fs::write(temp.path().join("secret.go"), "clean := true\n").expect("remove secret");
  run_git(temp.path(), &["add", "secret.go"]);
  run_git(temp.path(), &["commit", "-m", "remove secret"]);

  let value = run_lmprobe_json(&[
    "secrets",
    "--source",
    "git",
    "--gitleaks-compat",
    "--config",
    GITLEAKS_CONFIG,
    temp.path().to_str().expect("utf8 temp"),
  ]);
  let findings = value["findings"].as_array().expect("findings");
  assert_eq!(findings.len(), 1);
  assert_eq!(findings[0]["source_mode"], "git");
}

#[test]
#[ignore = "go-gitdiff channel internals are not part of lmprobe"]
fn gitleaks_sources_git_test_git_log() {
  // Addressing sources/git_test.go commented TestGitLog.
}

#[test]
#[ignore = "git diff/staged source is tracked as v1.1 follow-up"]
fn gitleaks_sources_git_test_git_diff() {
  // Addressing sources/git_test.go commented TestGitDiff.
}
