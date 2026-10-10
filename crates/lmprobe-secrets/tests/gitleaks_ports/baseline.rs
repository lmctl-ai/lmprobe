use crate::support::{run_lmprobe_json, GITLEAKS_CONFIG};
use serde_json::json;
use std::fs;

#[test]
fn gitleaks_detect_baseline_test_ignore_issues_in_baseline() {
  // Port of detect/baseline_test.go TestIgnoreIssuesInBaseline.
  let temp = tempfile::tempdir().expect("tempdir");
  let secret = temp.path().join("secret.go");
  fs::write(&secret, r#"awsToken := "AKIALALEMEL33243OLIA""#).expect("write secret");
  let fingerprint = format!(
    "{}:aws-access-token:1",
    secret.to_string_lossy().replace('\\', "/")
  );
  let baseline = temp.path().join("baseline.json");
  fs::write(
    &baseline,
    serde_json::to_string(&json!([{ "Fingerprint": fingerprint }])).unwrap(),
  )
  .expect("write baseline");

  let value = run_lmprobe_json(&[
    "secrets",
    "--gitleaks-compat",
    "--config",
    GITLEAKS_CONFIG,
    "--baseline",
    baseline.to_str().expect("utf8 baseline"),
    temp.path().to_str().expect("utf8 temp"),
  ]);
  assert_eq!(value["findings"].as_array().unwrap().len(), 0);
}

#[test]
fn gitleaks_detect_baseline_test_file_load_baseline_json() {
  // Port of the JSON-success behavior behind detect/baseline_test.go
  // TestFileLoadBaseline.
  let temp = tempfile::tempdir().expect("tempdir");
  let secret = temp.path().join("secret.go");
  fs::write(&secret, r#"awsToken := "AKIALALEMEL33243OLIA""#).expect("write secret");
  let baseline = temp.path().join("baseline.json");
  fs::write(&baseline, "[]").expect("write baseline");

  let value = run_lmprobe_json(&[
    "secrets",
    "--gitleaks-compat",
    "--config",
    GITLEAKS_CONFIG,
    "--baseline",
    baseline.to_str().expect("utf8 baseline"),
    temp.path().to_str().expect("utf8 temp"),
  ]);
  assert_eq!(value["findings"].as_array().unwrap().len(), 1);
}

#[test]
#[ignore = "baseline CSV/SARIF loading: tracked as v1.1 follow-up"]
fn gitleaks_detect_baseline_test_file_load_baseline_csv_sarif_errors() {
  // Port of detect/baseline_test.go TestFileLoadBaseline CSV/SARIF rows.
  // lmprobe currently treats baseline as JSON only and silently ignores
  // malformed/non-JSON baseline files.
}

#[test]
#[ignore = "baseline equivalence beyond fingerprint: tracked as v1.1 follow-up"]
fn gitleaks_detect_baseline_test_is_new() {
  // Port of detect/baseline_test.go TestIsNew.
  // gitleaks compares full Finding metadata when applying a baseline. lmprobe
  // currently suppresses by collected fingerprints only.
}
