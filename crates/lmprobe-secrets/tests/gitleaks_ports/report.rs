use crate::support::{run_lmprobe_json, GITLEAKS_CONFIG};
use lmprobe_secrets::Secret;
use std::fs;

#[test]
fn gitleaks_report_json_test_write_json_structural_envelope() {
  // Structural-equivalence port of report/json_test.go TestWriteJSON for
  // lmprobe's envelope shape.
  let temp = tempfile::tempdir().expect("tempdir");
  fs::write(
    temp.path().join("secret.go"),
    r#"awsToken := "AKIALALEMEL33243OLIA""#,
  )
  .expect("write secret");
  let value = run_lmprobe_json(&[
    "secrets",
    "--gitleaks-compat",
    "--config",
    GITLEAKS_CONFIG,
    temp.path().to_str().expect("utf8 temp"),
  ]);
  assert_eq!(value["schema"], "lmprobe.secrets.v1");
  assert_eq!(value["findings"].as_array().unwrap().len(), 1);
  assert_eq!(value["findings"][0]["redacted_secret"], "REDACTED");
}

#[test]
fn gitleaks_report_finding_redaction_tests() {
  // Port of report/finding_test.go TestRedact, TestMask, and TestMaskSecret at
  // lmprobe's secret-safe type boundary.
  let secret = Secret::new("AKIALALEMEL33243OLIA");
  assert_eq!(secret.to_string(), "REDACTED");
  assert_eq!(format!("{secret:?}"), "REDACTED");
  assert_eq!(serde_json::to_string(&secret).unwrap(), r#""REDACTED""#);
}

#[test]
#[ignore = "SARIF structural report parity: tracked as v1.1 follow-up"]
fn gitleaks_report_sarif_test_write_sarif() {
  // Port marker for report/sarif_test.go TestWriteSarif.
}

#[test]
#[ignore = "stdout byte-for-byte report parity is not a lmprobe goal"]
fn gitleaks_report_report_test_write_stdout() {
  // Port marker for report/report_test.go TestWriteStdout.
}

#[test]
#[ignore = "JUnit report format is not implemented for lmprobe secrets"]
fn gitleaks_report_junit_test_write_junit() {
  // Port marker for report/junit_test.go TestWriteJunit.
}

#[test]
#[ignore = "template report output is not implemented for lmprobe secrets"]
fn gitleaks_report_template_test_write_template() {
  // Port marker for report/template_test.go TestWriteTemplate.
}

#[test]
#[ignore = "template report output is not implemented for lmprobe secrets"]
fn gitleaks_report_template_test_template_dangerous_functions() {
  // Port marker for report/template_test.go TestTemplateDangerousFunctions.
}

#[test]
#[ignore = "CSV structural report parity: tracked as v1.1 follow-up"]
fn gitleaks_report_csv_test_write_csv() {
  // Port marker for report/csv_test.go TestWriteCSV.
}
