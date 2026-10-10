use crate::support::{
  run_git, run_lmprobe_json, scan, simple_akia_config, simple_akia_config_with_rule_extra,
  GITLEAKS_CONFIG,
};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;

#[test]
fn gitleaks_detect_test_detect_basic_rule_and_rule_allowlist() {
  // Port of detect/detect_test.go TestDetect and TestDetectRuleAllowlist.
  let config = simple_akia_config("");
  let findings = scan(&config, "tmp.go", r#"awsToken := "AKIALALEMEL33243OLIA""#);
  assert_eq!(findings.len(), 1);
  assert_eq!(findings[0].rule_id, "aws-access-token");
  assert_eq!(findings[0].start_line, 1);

  let allowlisted = simple_akia_config_with_rule_extra(
    r#"
[[rules.allowlists]]
regexes = ['''AKIALALEMEL33243OLIA''']
regexTarget = "secret"
"#,
  );
  assert!(scan(
    &allowlisted,
    "tmp.go",
    r#"awsToken := "AKIALALEMEL33243OLIA""#
  )
  .is_empty());
}

#[test]
fn gitleaks_detect_test_from_files() {
  // Port of detect/detect_test.go TestFromFiles.
  let temp = tempfile::tempdir().expect("tempdir");
  let file = temp.path().join("secret.go");
  fs::write(&file, r#"awsToken := "AKIALALEMEL33243OLIA""#).expect("write file");

  let value = run_lmprobe_json(&[
    "secrets",
    "--gitleaks-compat",
    "--config",
    GITLEAKS_CONFIG,
    temp.path().to_str().expect("utf8 temp"),
  ]);
  let findings = value["findings"].as_array().expect("findings");
  assert_eq!(findings.len(), 1);
  assert_eq!(findings[0]["rule_id"], "aws-access-token");
}

#[test]
fn gitleaks_detect_test_from_git() {
  // Port of detect/detect_test.go TestFromGit.
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
#[cfg(unix)]
fn gitleaks_detect_test_detect_with_symlinks() {
  // Port of detect/detect_test.go TestDetectWithSymlinks.
  let temp = tempfile::tempdir().expect("tempdir");
  let target = temp.path().join("secret.go");
  let link = temp.path().join("linked.go");
  fs::write(&target, r#"awsToken := "AKIALALEMEL33243OLIA""#).expect("write target");
  symlink(&target, &link).expect("create symlink");

  let default_value = run_lmprobe_json(&[
    "secrets",
    "--gitleaks-compat",
    "--config",
    GITLEAKS_CONFIG,
    temp.path().to_str().expect("utf8 temp"),
  ]);
  assert_eq!(default_value["findings"].as_array().unwrap().len(), 1);

  let followed = run_lmprobe_json(&[
    "secrets",
    "--gitleaks-compat",
    "--follow-symlinks",
    "--config",
    GITLEAKS_CONFIG,
    temp.path().to_str().expect("utf8 temp"),
  ]);
  assert_eq!(followed["findings"].as_array().unwrap().len(), 2);
}

#[test]
#[ignore = "staged git source: tracked as v1.1 follow-up"]
fn gitleaks_detect_test_from_git_staged() {
  // Port of detect/detect_test.go TestFromGitStaged.
}

#[test]
#[ignore = "archive extraction source: tracked as v1.1 follow-up"]
fn gitleaks_detect_test_detect_with_archives() {
  // Port of detect/detect_test.go TestDetectWithArchives.
}

#[test]
#[ignore = "gitleaksignore path normalization corners: tracked as v1.1 follow-up"]
fn gitleaks_detect_test_normalize_gitleaksignore_paths() {
  // Port of detect/detect_test.go TestNormalizeGitleaksIgnorePaths.
}

#[test]
#[ignore = "windows separator path matching: tracked as v1.1 follow-up"]
fn gitleaks_detect_test_windows_file_separator_rule_path() {
  // Port of detect/detect_test.go TestWindowsFileSeparator_RulePath.
}

#[test]
#[ignore = "windows separator allowlist matching: tracked as v1.1 follow-up"]
fn gitleaks_detect_test_windows_file_separator_rule_allowlist_paths() {
  // Port of detect/detect_test.go TestWindowsFileSeparator_RuleAllowlistPaths.
}
