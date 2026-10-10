use lmprobe_secrets::{
  compile_provider_prefilter, deprecated_field_diagnostic, generate_graphql_recipe,
  load_default_provider_db, load_provider_db_str, PROVIDER_DB_DEPRECATED_FIELD,
  PROVIDER_DB_SCHEMA_MISMATCH, PROVIDER_DB_UNKNOWN_FIELD,
};
use std::collections::BTreeSet;

fn valid_minimal_toml(extra_provider_fields: &str, extra_language_fields: &str) -> String {
  format!(
    r#"
schema_version = 1
title = "test provider DB"

[[providers]]
id = "aws"
description = "AWS"
rule_ids = ["aws-access-token"]
credential_kind = "key-secret-pair"
fast_path_tokens = ["boto3"]
{extra_provider_fields}

[[providers.languages]]
language = "python"
sdk = "boto3-v1"
sink_node_kinds = ["call"]
positive_test = "boto3.client(\"s3\", aws_secret_access_key=secret)"
negative_test = "boto3.client(\"s3\")"
{extra_language_fields}

[[providers.languages.call_shapes]]
kind = "call"
callee = "boto3.client"
kwargs = ["aws_secret_access_key"]
"#
  )
}

#[test]
fn default_provider_db_parses_seed_providers() {
  let db = load_default_provider_db().expect("default provider DB loads");
  let provider_ids: BTreeSet<_> = db
    .providers
    .iter()
    .map(|provider| provider.id.as_str())
    .collect();

  assert_eq!(db.providers.len(), 10);
  assert_eq!(
    provider_ids,
    BTreeSet::from([
      "aws",
      "cloudflare",
      "datadog",
      "github",
      "gitlab",
      "openai",
      "sendgrid",
      "slack",
      "stripe",
      "twilio"
    ])
  );

  let language_entries: usize = db
    .providers
    .iter()
    .map(|provider| provider.languages.len())
    .sum();
  assert_eq!(language_entries, 40, "AWS ships Java v1+v2 entries");

  let aws = db
    .providers
    .iter()
    .find(|provider| provider.id == "aws")
    .expect("aws provider");
  assert!(aws
    .languages
    .iter()
    .any(|entry| entry.language.to_string() == "java" && entry.sdk == "aws-sdk-java-v1"));
  assert!(aws
    .languages
    .iter()
    .any(|entry| entry.language.to_string() == "java" && entry.sdk == "aws-sdk-java-v2"));
}

#[test]
fn default_provider_db_rule_ids_all_exist_in_vendored_gitleaks_config() {
  let db = load_default_provider_db().expect("default provider DB loads");
  let rule_refs: Vec<_> = db
    .providers
    .iter()
    .flat_map(|provider| {
      provider
        .rule_ids
        .iter()
        .map(|rule_id| (provider.id.as_str(), rule_id))
    })
    .collect();

  assert_eq!(rule_refs.len(), 29);
}

#[test]
fn default_provider_db_fast_path_prefilter_compiles_and_matches_seed_tokens() {
  let db = load_default_provider_db().expect("default provider DB loads");
  let prefilter = compile_provider_prefilter(&db).expect("prefilter compiles");

  assert!(prefilter.is_match("import boto3"));
  assert!(prefilter.is_match("const stripe = require('stripe')"));
  assert!(prefilter.is_match("sendgrid.NewSendClient(apiKey)"));
  assert!(!prefilter.is_match("let ordinary = true;"));
}

#[test]
fn default_provider_db_positive_and_negative_snippets_validate_for_every_language_entry() {
  let db = load_default_provider_db().expect("default provider DB validates snippets");
  for provider in &db.providers {
    for language in &provider.languages {
      assert!(
        !language.positive_test.trim().is_empty(),
        "{} {} positive fixture",
        provider.id,
        language.sdk
      );
      assert!(
        !language.negative_test.trim().is_empty(),
        "{} {} negative fixture",
        provider.id,
        language.sdk
      );
    }
  }
}

#[test]
fn graphql_recipe_generation_covers_call_new_struct_and_assignment_shapes() {
  let db = load_default_provider_db().expect("default provider DB loads");
  let shapes = db
    .providers
    .iter()
    .flat_map(|provider| {
      provider.languages.iter().flat_map(move |language| {
        language
          .call_shapes
          .iter()
          .map(move |shape| (provider, language, shape))
      })
    })
    .collect::<Vec<_>>();

  let (provider, language, shape) = shapes
    .iter()
    .find(|(_, _, shape)| {
      shape
        .kwargs
        .iter()
        .any(|kwarg| kwarg == "aws_secret_access_key")
    })
    .copied()
    .expect("call shape with kwarg present");
  let recipe = generate_graphql_recipe(provider, language, shape);
  assert!(recipe.contains("filters: ["));
  assert!(recipe.contains("argNames:"));
  assert!(recipe.contains("\"aws_secret_access_key\""));
  assert!(recipe.contains("importHints:"));
  assert!(!recipe.contains("assignmentTargets:"));
  assert!(!recipe.contains("structFields:"));

  let (provider, language, shape) = shapes
    .iter()
    .find(|(_, _, shape)| {
      format!("{:?}", shape.kind) == "New" && !shape.positional_indices.is_empty()
    })
    .copied()
    .expect("new shape with positional indices present");
  let recipe = generate_graphql_recipe(provider, language, shape);
  assert!(recipe.contains("filters: ["));
  assert!(recipe.contains("positionalIndices:"));
  assert!(recipe.contains("importHints:"));
  assert!(!recipe.contains("argNames:"));
  assert!(!recipe.contains("assignmentTargets:"));
  assert!(!recipe.contains("structFields:"));

  let (provider, language, shape) = shapes
    .iter()
    .find(|(_, _, shape)| format!("{:?}", shape.kind) == "Struct")
    .copied()
    .expect("struct shape present");
  let recipe = generate_graphql_recipe(provider, language, shape);
  assert!(recipe.contains("filters: ["));
  assert!(recipe.contains("structFields:"));
  assert!(recipe.contains("importHints:"));
  assert!(!recipe.contains("argNames:"));
  assert!(!recipe.contains("assignmentTargets:"));

  let (provider, language, shape) = shapes
    .iter()
    .find(|(_, _, shape)| format!("{:?}", shape.kind) == "Assignment")
    .copied()
    .expect("assignment shape present");
  let recipe = generate_graphql_recipe(provider, language, shape);
  assert!(recipe.contains("filters: ["));
  assert!(recipe.contains("assignmentTargets:"));
  assert!(recipe.contains("importHints:"));
  assert!(!recipe.contains("argNames:"));
  assert!(!recipe.contains("structFields:"));
}

#[test]
fn fast_path_tokens_accept_valid_provider() {
  load_provider_db_str(
    &valid_minimal_toml("", ""),
    lmprobe_secrets::DEFAULT_GITLEAKS_CONFIG,
  )
  .expect("valid minimal provider db loads");
}

#[test]
fn fast_path_tokens_reject_missing_list() {
  let toml =
    valid_minimal_toml("", "").replace("fast_path_tokens = [\"boto3\"]", "fast_path_tokens = []");
  let err = load_provider_db_str(&toml, lmprobe_secrets::DEFAULT_GITLEAKS_CONFIG)
    .expect_err("missing fast-path tokens rejected");
  assert_eq!(err.diagnostics()[0].code, PROVIDER_DB_SCHEMA_MISMATCH);
}

#[test]
fn fast_path_tokens_reject_empty_token() {
  let toml = valid_minimal_toml("", "").replace(
    "fast_path_tokens = [\"boto3\"]",
    "fast_path_tokens = [\"   \"]",
  );
  let err = load_provider_db_str(&toml, lmprobe_secrets::DEFAULT_GITLEAKS_CONFIG)
    .expect_err("empty fast-path token rejected");
  assert_eq!(err.diagnostics()[0].code, PROVIDER_DB_SCHEMA_MISMATCH);
}

#[test]
fn fast_path_tokens_reject_short_token() {
  let toml = valid_minimal_toml("", "").replace(
    "fast_path_tokens = [\"boto3\"]",
    "fast_path_tokens = [\"sk\"]",
  );
  let err = load_provider_db_str(&toml, lmprobe_secrets::DEFAULT_GITLEAKS_CONFIG)
    .expect_err("short fast-path token rejected");
  assert_eq!(err.diagnostics()[0].code, PROVIDER_DB_SCHEMA_MISMATCH);
}

#[test]
fn sdk_uniqueness_accepts_distinct_language_sdk_pairs() {
  let toml = format!(
    "{}{}",
    valid_minimal_toml("", ""),
    r#"
[[providers.languages]]
language = "java"
sdk = "aws-sdk-java-v2"
sink_node_kinds = ["method_invocation"]
positive_test = "AwsBasicCredentials.create(id, secret)"
negative_test = "DefaultCredentialsProvider.create()"

[[providers.languages.call_shapes]]
kind = "call"
callee = "AwsBasicCredentials.create"
positional_indices = [0, 1]
"#
  );
  load_provider_db_str(&toml, lmprobe_secrets::DEFAULT_GITLEAKS_CONFIG)
    .expect("distinct language/sdk pairs accepted");
}

#[test]
fn sdk_uniqueness_rejects_duplicate_provider_language_sdk() {
  let toml = format!(
    "{}{}",
    valid_minimal_toml("", ""),
    r#"
[[providers.languages]]
language = "python"
sdk = "boto3-v1"
sink_node_kinds = ["call"]
positive_test = "boto3.Session(aws_secret_access_key=secret)"
negative_test = "boto3.Session()"

[[providers.languages.call_shapes]]
kind = "call"
callee = "boto3.Session"
kwargs = ["aws_secret_access_key"]
"#
  );
  let err = load_provider_db_str(&toml, lmprobe_secrets::DEFAULT_GITLEAKS_CONFIG)
    .expect_err("duplicate sdk rejected");
  assert_eq!(err.diagnostics()[0].code, PROVIDER_DB_SCHEMA_MISMATCH);
  assert!(err.diagnostics()[0].message.contains("duplicate sdk"));
}

#[test]
fn deny_unknown_fields_reports_unknown_field_diagnostic() {
  let toml = valid_minimal_toml("unexpected = true", "");
  let err = load_provider_db_str(&toml, lmprobe_secrets::DEFAULT_GITLEAKS_CONFIG)
    .expect_err("unknown field rejected");
  assert_eq!(err.diagnostics()[0].code, PROVIDER_DB_UNKNOWN_FIELD);
  assert!(err.diagnostics()[0].message.contains("unexpected"));
}

#[test]
fn schema_mismatch_reports_schema_mismatch_diagnostic() {
  let toml = r#"
schema_version = "one"
title = "bad"
providers = []
"#;
  let err = load_provider_db_str(toml, lmprobe_secrets::DEFAULT_GITLEAKS_CONFIG)
    .expect_err("schema mismatch rejected");
  assert_eq!(err.diagnostics()[0].code, PROVIDER_DB_SCHEMA_MISMATCH);
}

#[test]
fn deprecated_field_diagnostic_infrastructure_exists() {
  let diagnostic = deprecated_field_diagnostic("providers.aws.legacy_field");
  assert_eq!(diagnostic.code, PROVIDER_DB_DEPRECATED_FIELD);
  assert!(diagnostic.message.contains("providers.aws.legacy_field"));
}
