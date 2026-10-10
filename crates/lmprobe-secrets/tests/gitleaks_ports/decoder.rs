use crate::support::{scan, simple_akia_config};

#[test]
fn gitleaks_codec_decoder_test_decode_base64_percent_hex_unicode() {
  // Port of detect/codec/decoder_test.go TestDecode through the scanner's
  // recursive decode path.
  let config = simple_akia_config("");
  let cases = [
    ("base64", "YXdzVG9rZW4gOj0gIkFLSUFMQUxFTUVMMzMyNDNPTElBIg=="),
    ("percent", "awsToken%20%3A%3D%20%22AKIALALEMEL33243OLIA%22"),
    (
      "hex",
      "617773546f6b656e203a3d2022414b49414c414c454d454c33333234334f4c494122",
    ),
    (
      "unicode",
      r#"awsToken := "\u0041\u004b\u0049\u0041LALEMEL33243OLIA""#,
    ),
  ];

  for (name, content) in cases {
    let findings = scan(&config, &format!("{name}.go"), content);
    assert_eq!(findings.len(), 1, "{name} should decode to one AWS hit");
    assert!(findings[0]
      .tags
      .iter()
      .any(|tag| tag == "decoded:recursive"));
  }
}

#[test]
fn gitleaks_codec_decoder_test_decode_invalid_base64_is_left_alone() {
  // Port of the invalid-base64 row in detect/codec/decoder_test.go TestDecode.
  let config = simple_akia_config("");
  let findings = scan(
    &config,
    "invalid.txt",
    "a3d3fa7c2bb99e469ba55e5834ce79ee4853a8a3",
  );
  assert!(findings.is_empty());
}
