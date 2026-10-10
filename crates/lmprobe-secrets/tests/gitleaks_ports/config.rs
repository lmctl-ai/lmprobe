use crate::support::{scan, simple_akia_config};
use lmprobe_secrets::load_gitleaks_config_str;

#[test]
fn gitleaks_config_test_translate_loads_rules_and_keywords() {
  // Port of gitleaks config/config_test.go TestTranslate.
  let config = load_gitleaks_config_str(lmprobe_secrets::DEFAULT_GITLEAKS_CONFIG)
    .expect("load vendored default");
  assert_eq!(config.rules.len(), 222);
  assert!(config.keywords.contains("akia"));
  assert!(config.rules.iter().any(|rule| rule.id == "generic-api-key"));
}

#[test]
fn gitleaks_config_test_translate_allowlists() {
  // Port of gitleaks config/config_test.go TestTranslateAllowlists.
  let config = r#"
title = "allowlists"

[[allowlists]]
paths = ['''fixtures''']

[[rules]]
id = "generic-api-key"
description = "generic"
regex = '''(?i)(api_key)\s*=\s*["']([a-z0-9]{20,})["']'''
secretGroup = 2
keywords = ["api_key"]

  [[rules.allowlists]]
  regexes = ['''known_test_value''']
  regexTarget = "secret"
"#;
  let parsed = load_gitleaks_config_str(config).expect("parse allowlists");
  assert_eq!(parsed.allowlists.len(), 1);
  assert_eq!(parsed.rules[0].allowlists.len(), 1);

  let global_allowed = scan(
    config,
    "fixtures/app.py",
    r#"api_key = "abcdefghijklmnopqrstuv""#,
  );
  assert!(global_allowed.is_empty());

  let rule_allowed = scan(config, "app.py", r#"api_key = "known_test_value_12345""#);
  assert!(rule_allowed.is_empty());
}

#[test]
fn gitleaks_config_test_extended_rule_keywords_are_downcase() {
  // Port of gitleaks config/config_test.go TestExtendedRuleKeywordsAreDowncase.
  let config = simple_akia_config("");
  let parsed = load_gitleaks_config_str(&config).expect("parse config");
  let aws = parsed
    .rules
    .iter()
    .find(|rule| rule.id == "aws-access-token")
    .expect("aws rule");
  assert_eq!(aws.keywords, ["akia"]);
}

#[test]
#[ignore = "config extend merge API: tracked as v1.1 follow-up"]
fn gitleaks_config_test_translate_extend() {
  // Port of gitleaks config/config_test.go TestTranslateExtend.
  // The CLI resolves [extend], but lmprobe-secrets does not yet expose a
  // library-level config extension resolver that can be table-tested against
  // gitleaks' config/testdata/config/valid/extend fixtures.
  let config = r#"
title = "extension"
[extend]
useDefault = true
"#;
  let parsed = load_gitleaks_config_str(config).expect("parse extend schema");
  assert!(parsed.extend.use_default);
}
