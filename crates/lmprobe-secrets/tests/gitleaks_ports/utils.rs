use crate::support::{scan, simple_akia_config};
use lmprobe_secrets::shannon_entropy;

#[test]
fn gitleaks_detect_utils_entropy_gate_math() {
  // Port target for detect/utils_test.go entropy helper coverage requested by
  // the migration task.
  assert_eq!(shannon_entropy("aaaaaaaaaaaaaaaa"), 0.0);
  let entropy = shannon_entropy("AKIALALEMEL33243OLIA");
  assert!((entropy - 3.0841837).abs() < 0.0001, "{entropy}");
}

#[test]
fn gitleaks_detect_utils_fingerprint_format() {
  // Port target for detect/utils_test.go fingerprint helper coverage requested
  // by the migration task.
  let config = simple_akia_config("");
  let findings = scan(&config, "tmp.go", r#"awsToken := "AKIALALEMEL33243OLIA""#);
  assert_eq!(findings.len(), 1);
  assert_eq!(findings[0].fingerprint, "tmp.go:aws-access-token:1");
}

#[test]
#[ignore = "SCM link generation is not implemented by lmprobe secrets"]
fn gitleaks_detect_utils_test_create_scm_link() {
  // Port of detect/utils_test.go Test_createScmLink.
}
