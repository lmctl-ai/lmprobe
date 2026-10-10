use crate::support::{scan, simple_akia_config};
use lmprobe_secrets::load_gitleaks_config_str;

#[test]
fn gitleaks_config_allowlist_test_regex_allowed() {
  // Port of gitleaks config/allowlist_test.go TestRegexAllowed.
  let config = simple_akia_config(
    r#"
[allowlist]
regexes = ['''AKIALALEMEL33243OLIA''']
"#,
  );

  let findings = scan(&config, "tmp.go", r#"awsToken := "AKIALALEMEL33243OLIA""#);
  assert!(findings.is_empty());
}

#[test]
fn gitleaks_config_allowlist_test_path_allowed() {
  // Port of gitleaks config/allowlist_test.go TestPathAllowed.
  let config = simple_akia_config(
    r#"
[allowlist]
paths = ['''allowed/path''']
"#,
  );

  let findings = scan(
    &config,
    "allowed/path/tmp.go",
    r#"awsToken := "AKIALALEMEL33243OLIA""#,
  );
  assert!(findings.is_empty());
}

#[test]
#[ignore = "commit allowlists: tracked as v1.1 follow-up"]
fn gitleaks_config_allowlist_test_commit_allowed() {
  // Port of gitleaks config/allowlist_test.go TestCommitAllowed.
  // lmprobe's public Layer-1 file API has no commit-bearing fragment yet; git
  // source commit allowlists are tracked in the v1.1 gap list.
  let config = r#"
title = "commit allowlist"

[allowlist]
commits = ["commitA"]

[[rules]]
id = "aws-access-token"
description = "AWS access token"
regex = '''AKIA[A-Z0-9]{16}'''
keywords = ["akia"]
"#;
  let parsed = load_gitleaks_config_str(config).expect("parse config");
  assert_eq!(parsed.allowlist.unwrap().commits, ["commita"]);
}

#[test]
fn gitleaks_config_allowlist_test_validate() {
  // Port of gitleaks config/allowlist_test.go TestValidate.
  let config = r#"
title = "invalid allowlist"

[allowlist]
"#;
  assert!(load_gitleaks_config_str(config).is_err());

  let deduped = load_gitleaks_config_str(
    r#"
title = "dedup"

[[allowlists]]
commits = ["commitA", "commitB", "commitA"]
stopwords = ["stopwordA", "stopwordB", "stopwordA"]

[[allowlists]]
commits = ["commita", "commitb"]
stopwords = ["stopworda", "stopwordb"]

[[rules]]
id = "demo"
description = "demo"
regex = '''demo'''
"#,
  )
  .expect("deduped config");
  assert_eq!(deduped.allowlists.len(), 1);
  assert_eq!(deduped.allowlists[0].commits, ["commita", "commitb"]);
  assert_eq!(deduped.allowlists[0].stopwords, ["stopworda", "stopwordb"]);
}
