use lmprobe_secrets::{
  compile_source_prefilter, generate_source_graphql_recipe, load_default_source_db,
  load_source_db_str, ReadShapeKind, SOURCE_DB_GENERIC_FAST_PATH_TOKEN, SOURCE_DB_SCHEMA_MISMATCH,
  SOURCE_DB_UNKNOWN_FIELD,
};
use std::collections::BTreeSet;

fn valid_minimal_toml(extra_source_fields: &str, extra_language_fields: &str) -> String {
  format!(
    r#"
schema_version = 1
title = "test source DB"

[[sources]]
id = "env-aws"
description = "AWS env source"
category = "env"
provider_id = "aws"
credential_kind = "key-secret-pair"
env_var_names = ["AWS_SECRET_ACCESS_KEY"]
fast_path_tokens = ["AWS_SECRET_ACCESS_KEY"]
{extra_source_fields}

[[sources.languages]]
language = "python"
sdk = "stdlib-env"
source_node_kinds = ["call"]
positive_test = "os.getenv(\"AWS_SECRET_ACCESS_KEY\")"
negative_test = "os.getenv(\"AWS_REGION\")"
{extra_language_fields}

[[sources.languages.read_shapes]]
kind = "env_read"
access_paths = ["os.getenv"]
keys = ["AWS_SECRET_ACCESS_KEY"]
"#
  )
}

#[test]
fn default_source_db_parses_seed_sources() {
  let db = load_default_source_db().expect("default source DB loads");
  let ids: BTreeSet<_> = db.sources.iter().map(|source| source.id.as_str()).collect();
  assert_eq!(db.sources.len(), 10);
  assert_eq!(
    ids,
    BTreeSet::from([
      "azure-key-vault",
      "aws-secrets-manager",
      "dotenv-loader",
      "env-aws",
      "env-github",
      "env-openai",
      "env-stripe",
      "gcp-secret-manager",
      "hashicorp-vault-kv",
      "kubernetes-secret",
    ])
  );

  for source in &db.sources {
    assert!(
      !source.languages.is_empty(),
      "{} should ship language entries",
      source.id
    );
  }
}

#[test]
fn default_source_db_provider_ids_all_exist_in_provider_db() {
  let db = load_default_source_db().expect("default source DB loads");
  let refs: Vec<_> = db
    .sources
    .iter()
    .filter_map(|source| {
      source
        .provider_id
        .as_ref()
        .map(|provider| (&source.id, provider))
    })
    .collect();
  assert_eq!(refs.len(), 4);
  assert!(refs.iter().any(|(_, provider)| provider.as_str() == "aws"));
  assert!(refs
    .iter()
    .any(|(_, provider)| provider.as_str() == "stripe"));
  assert!(refs
    .iter()
    .any(|(_, provider)| provider.as_str() == "github"));
  assert!(refs
    .iter()
    .any(|(_, provider)| provider.as_str() == "openai"));
}

#[test]
fn default_source_db_positive_and_negative_snippets_validate_for_every_language_entry() {
  let db = load_default_source_db().expect("default source DB validates snippets");
  for source in &db.sources {
    for language in &source.languages {
      assert!(
        !language.positive_test.trim().is_empty(),
        "{} {} positive fixture",
        source.id,
        language.sdk
      );
      assert!(
        !language.negative_test.trim().is_empty(),
        "{} {} negative fixture",
        source.id,
        language.sdk
      );
    }
  }
}

#[test]
fn fast_path_tokens_accept_valid_source() {
  load_source_db_str(&valid_minimal_toml("", "")).expect("valid source db loads");
}

#[test]
fn fast_path_tokens_reject_empty_token() {
  let toml = valid_minimal_toml("", "").replace(
    r#"fast_path_tokens = ["AWS_SECRET_ACCESS_KEY"]"#,
    r#"fast_path_tokens = [""]"#,
  );
  let err = load_source_db_str(&toml).expect_err("empty token should fail");
  assert_eq!(err.diagnostics()[0].code, SOURCE_DB_SCHEMA_MISMATCH);
}

#[test]
fn fast_path_tokens_reject_short_token() {
  let toml = valid_minimal_toml("", "").replace(
    r#"fast_path_tokens = ["AWS_SECRET_ACCESS_KEY"]"#,
    r#"fast_path_tokens = ["AK"]"#,
  );
  let err = load_source_db_str(&toml).expect_err("short token should fail");
  assert_eq!(err.diagnostics()[0].code, SOURCE_DB_SCHEMA_MISMATCH);
}

#[test]
fn fast_path_tokens_reject_generic_env_apis() {
  for blocked in [
    "os.environ",
    "process.env",
    "System.getenv",
    "os.Getenv",
    "os.environ.get",
    "os.getenv",
  ] {
    let toml = valid_minimal_toml("", "").replace(
      r#"fast_path_tokens = ["AWS_SECRET_ACCESS_KEY"]"#,
      &format!(r#"fast_path_tokens = ["{blocked}"]"#),
    );
    let err = load_source_db_str(&toml).expect_err("generic env token should fail");
    assert!(
      err
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code == SOURCE_DB_GENERIC_FAST_PATH_TOKEN),
      "{blocked}: {err:?}"
    );
  }
}

#[test]
fn deny_unknown_fields_reports_unknown_field_diagnostic() {
  let toml = valid_minimal_toml(r#"unexpected = "nope""#, "");
  let err = load_source_db_str(&toml).expect_err("unknown field should fail");
  assert_eq!(err.diagnostics()[0].code, SOURCE_DB_UNKNOWN_FIELD);
}

#[test]
fn schema_mismatch_reports_schema_mismatch_diagnostic() {
  let toml = valid_minimal_toml("", "").replacen("schema_version = 1", "schema_version = 99", 1);
  let err = load_source_db_str(&toml).expect_err("schema mismatch should fail");
  assert_eq!(err.diagnostics()[0].code, SOURCE_DB_SCHEMA_MISMATCH);
}

#[test]
fn sdk_uniqueness_rejects_duplicate_source_language_sdk() {
  let mut toml = valid_minimal_toml("", "");
  toml.push_str(
    r#"
[[sources.languages]]
language = "python"
sdk = "stdlib-env"
source_node_kinds = ["call"]
positive_test = "os.getenv(\"AWS_SECRET_ACCESS_KEY\")"
negative_test = "os.getenv(\"AWS_REGION\")"

[[sources.languages.read_shapes]]
kind = "env_read"
access_paths = ["os.getenv"]
keys = ["AWS_SECRET_ACCESS_KEY"]
"#,
  );
  let err = load_source_db_str(&toml).expect_err("duplicate sdk should fail");
  assert!(err
    .diagnostics()
    .iter()
    .any(|diagnostic| diagnostic.message.contains("duplicate sdk")));
}

#[test]
fn secret_name_regex_field_accepted_but_unused() {
  let db = load_source_db_str(&valid_minimal_toml(r#"secret_name_regex = "stripe""#, ""))
    .expect("secret_name_regex parses");
  assert_eq!(db.sources[0].secret_name_regex.as_deref(), Some("stripe"));
}

#[test]
fn graphql_source_recipe_generation_covers_all_five_read_shape_kinds() {
  let db = load_default_source_db().expect("default source DB loads");
  let shapes = db
    .sources
    .iter()
    .flat_map(|source| {
      source.languages.iter().flat_map(move |language| {
        language
          .read_shapes
          .iter()
          .map(move |shape| (source, language, shape))
      })
    })
    .collect::<Vec<_>>();

  let (source, language, shape) = shapes
    .iter()
    .find(|(_, _, shape)| shape.kind == ReadShapeKind::EnvRead)
    .copied()
    .expect("env shape");
  let recipe = generate_source_graphql_recipe(source, language.language, shape);
  assert!(recipe.contains("sourceReads("));
  assert!(recipe.contains("envVarNames:"));
  assert!(recipe.contains("accessPaths:"));
  assert!(recipe.contains("importHints:"));

  let (source, language, shape) = shapes
    .iter()
    .find(|(_, _, shape)| shape.kind == ReadShapeKind::KvGet)
    .copied()
    .expect("kv shape");
  let recipe = generate_source_graphql_recipe(source, language.language, shape);
  assert!(recipe.contains("keyNames:"));
  assert!(recipe.contains("accessPaths:"));
  assert!(recipe.contains("importHints:"));

  let (source, language, shape) = shapes
    .iter()
    .find(|(_, _, shape)| shape.kind == ReadShapeKind::SdkCall)
    .copied()
    .expect("sdk call shape");
  let recipe = generate_source_graphql_recipe(source, language.language, shape);
  assert!(recipe.contains("callees:"));
  assert!(recipe.contains("importHints:"));

  let (source, language, shape) = shapes
    .iter()
    .find(|(_, _, shape)| shape.kind == ReadShapeKind::DotenvLoad)
    .copied()
    .expect("dotenv shape");
  let recipe = generate_source_graphql_recipe(source, language.language, shape);
  assert!(recipe.contains("callees:"));
  assert!(recipe.contains("importHints:"));

  let config_source = db
    .sources
    .iter()
    .find(|source| source.id == "env-aws")
    .expect("source");
  let config_shape = lmprobe_secrets::ReadShape {
    kind: ReadShapeKind::ConfigLookup,
    access_paths: Vec::new(),
    keys: Vec::new(),
    key_names: vec!["aws_secret_access_key".into()],
    callees: Vec::new(),
    object_paths: vec!["config".into()],
    import_hints: vec!["config".into()],
  };
  let recipe = generate_source_graphql_recipe(config_source, language.language, &config_shape);
  assert!(recipe.contains("objectPaths:"));
  assert!(recipe.contains("keyNames:"));
  assert!(recipe.contains("importHints:"));
}

#[test]
fn compile_source_prefilter_matches_seed_tokens() {
  let db = load_default_source_db().expect("default source DB loads");
  let prefilter = compile_source_prefilter(&db).expect("prefilter compiles");
  assert!(prefilter.is_match("AWS_SECRET_ACCESS_KEY"));
  assert!(prefilter.is_match("STRIPE_API_KEY"));
  assert!(prefilter.is_match("GetSecretValueCommand"));
  assert!(prefilter.is_match("load_dotenv()"));
  assert!(!prefilter.is_match("process.env.AWS_REGION"));
}
