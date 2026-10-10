use crate::support::scan;

#[test]
fn gitleaks_detect_location_test_get_location() {
  // Port of detect/location_test.go TestGetLocation.
  let config = r#"
title = "location"

[[rules]]
id = "location-token"
description = "location"
regex = '''XYZ|WXYZ'''
keywords = []
"#;
  let content = format!("{}XYZ{}\nWXYZ{}", "a".repeat(35), "a", "b".repeat(12));
  let findings = scan(config, "location.txt", &content);
  assert_eq!(findings.len(), 2);

  let first = findings
    .iter()
    .find(|finding| finding.match_text.expose_raw() == "XYZ")
    .expect("first-line finding");
  assert_eq!(first.start_line, 1);
  assert_eq!(first.start_column, 36);
  assert_eq!(first.end_line, 1);
  assert_eq!(first.end_column, 38);

  let second = findings
    .iter()
    .find(|finding| finding.match_text.expose_raw() == "WXYZ")
    .expect("second-line finding");
  assert_eq!(second.start_line, 2);
  assert_eq!(second.start_column, 1);
  assert_eq!(second.end_line, 2);
  assert_eq!(second.end_column, 4);
}
