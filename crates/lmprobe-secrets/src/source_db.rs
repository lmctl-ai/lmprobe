use crate::provider_db::{load_default_provider_db, ProviderLanguageId};
use crate::SecretDiagnostic;
use aho_corasick::AhoCorasick;
use serde::Deserialize;
use std::collections::HashSet;
use std::fmt;

pub const DEFAULT_SOURCE_DB: &str = include_str!("../sources/default.toml");
pub const SUPPORTED_SOURCE_SCHEMA_VERSION: u32 = 1;

pub const SOURCE_DB_SCHEMA_MISMATCH: &str = "SOURCE_DB_SCHEMA_MISMATCH";
pub const SOURCE_DB_UNKNOWN_FIELD: &str = "SOURCE_DB_UNKNOWN_FIELD";
pub const SOURCE_DB_DEPRECATED_FIELD: &str = "SOURCE_DB_DEPRECATED_FIELD";
pub const SOURCE_DB_GENERIC_FAST_PATH_TOKEN: &str = "SOURCE_DB_GENERIC_FAST_PATH_TOKEN";

const BLOCKED_FAST_PATH_TOKENS: &[&str] = &[
  "os.environ",
  "process.env",
  "System.getenv",
  "os.Getenv",
  "os.environ.get",
  "os.getenv",
];

#[derive(Debug, Clone, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceDb {
  pub schema_version: u32,
  pub title: String,
  pub sources: Vec<Source>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Source {
  pub id: String,
  pub description: String,
  pub category: SourceCategory,
  #[serde(default)]
  pub provider_id: Option<String>,
  pub credential_kind: String,
  #[serde(default)]
  pub env_var_names: Vec<String>,
  #[serde(default)]
  pub secret_name_regex: Option<String>,
  #[serde(default)]
  pub source_urls: Vec<String>,
  pub fast_path_tokens: Vec<String>,
  pub languages: Vec<SourceLanguage>,
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum SourceCategory {
  Env,
  Vault,
  SecretManager,
  Dotenv,
  ConfigFile,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceLanguage {
  pub language: ProviderLanguageId,
  pub sdk: String,
  pub source_node_kinds: Vec<String>,
  pub positive_test: String,
  pub negative_test: String,
  pub read_shapes: Vec<ReadShape>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReadShape {
  pub kind: ReadShapeKind,
  #[serde(default)]
  pub access_paths: Vec<String>,
  #[serde(default)]
  pub keys: Vec<String>,
  #[serde(default)]
  pub key_names: Vec<String>,
  #[serde(default)]
  pub callees: Vec<String>,
  #[serde(default)]
  pub object_paths: Vec<String>,
  #[serde(default)]
  pub import_hints: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ReadShapeKind {
  EnvRead,
  KvGet,
  SdkCall,
  DotenvLoad,
  ConfigLookup,
}

#[derive(Debug, Clone)]
pub struct SourceDbError {
  diagnostics: Vec<SecretDiagnostic>,
}

impl SourceDbError {
  pub fn diagnostics(&self) -> &[SecretDiagnostic] {
    &self.diagnostics
  }

  fn single(diagnostic: SecretDiagnostic) -> Self {
    Self {
      diagnostics: vec![diagnostic],
    }
  }

  fn many(diagnostics: Vec<SecretDiagnostic>) -> Self {
    Self { diagnostics }
  }
}

impl fmt::Display for SourceDbError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if let Some(first) = self.diagnostics.first() {
      write!(f, "{}: {}", first.code, first.message)
    } else {
      f.write_str("source DB error")
    }
  }
}

impl std::error::Error for SourceDbError {}

pub fn load_default_source_db() -> Result<SourceDb, SourceDbError> {
  load_source_db_str(DEFAULT_SOURCE_DB)
}

pub fn load_source_db_str(source_toml: &str) -> Result<SourceDb, SourceDbError> {
  let db: SourceDb = toml::from_str(source_toml).map_err(classify_toml_error)?;
  if db.schema_version != SUPPORTED_SOURCE_SCHEMA_VERSION {
    return Err(SourceDbError::single(schema_mismatch(format!(
      "source DB schema_version {} is not supported by this binary; supported schema_version is {}",
      db.schema_version, SUPPORTED_SOURCE_SCHEMA_VERSION
    ))));
  }
  let provider_db = load_default_provider_db()
    .map_err(|err| SourceDbError::single(schema_mismatch(format!("provider DB: {err}"))))?;
  let provider_ids: HashSet<String> = provider_db
    .providers
    .iter()
    .map(|provider| provider.id.clone())
    .collect();
  validate_source_db(&db, &provider_ids)?;
  Ok(db)
}

pub fn validate_source_db(
  db: &SourceDb,
  provider_ids: &HashSet<String>,
) -> Result<(), SourceDbError> {
  let mut diagnostics = Vec::new();
  let mut source_ids = HashSet::new();

  if db.title.trim().is_empty() {
    diagnostics.push(schema_mismatch("source DB title must be non-empty"));
  }
  if db.sources.is_empty() {
    diagnostics.push(schema_mismatch(
      "source DB must contain at least one source",
    ));
  }

  for source in &db.sources {
    let source_path = format!("sources.{}", source.id);
    if source.id.trim().is_empty() {
      diagnostics.push(schema_mismatch("source id must be non-empty"));
    } else if !source_ids.insert(source.id.clone()) {
      diagnostics.push(schema_mismatch(format!(
        "duplicate source id `{}`",
        source.id
      )));
    }

    if let Some(provider_id) = source.provider_id.as_deref() {
      if provider_id.trim().is_empty() {
        diagnostics.push(schema_mismatch(format!(
          "{source_path}.provider_id must be non-empty"
        )));
      } else if !provider_ids.contains(provider_id) {
        diagnostics.push(schema_mismatch(format!(
          "{source_path}.provider_id references unknown provider `{provider_id}`"
        )));
      }
    }

    if matches!(source.category, SourceCategory::Env) && source.env_var_names.is_empty() {
      diagnostics.push(schema_mismatch(format!(
        "{source_path}.env_var_names must contain at least one environment variable"
      )));
    }

    validate_fast_path_tokens(&source_path, &source.fast_path_tokens, &mut diagnostics);

    let mut sdk_keys = HashSet::new();
    for language in &source.languages {
      let lang_path = format!(
        "{source_path}.languages.{}.{}",
        language.language, language.sdk
      );
      if language.sdk.trim().is_empty() {
        diagnostics.push(schema_mismatch(format!(
          "{lang_path}.sdk must be non-empty"
        )));
      }
      let sdk_key = (language.language, language.sdk.clone());
      if !language.sdk.trim().is_empty() && !sdk_keys.insert(sdk_key) {
        diagnostics.push(schema_mismatch(format!(
          "duplicate sdk `{}` for source `{}` language `{}`",
          language.sdk, source.id, language.language
        )));
      }
      if language.read_shapes.is_empty() {
        diagnostics.push(schema_mismatch(format!(
          "{lang_path}.read_shapes must contain at least one shape"
        )));
      }
      if language.positive_test.trim().is_empty() {
        diagnostics.push(schema_mismatch(format!(
          "{lang_path}.positive_test must be non-empty"
        )));
      }
      if language.negative_test.trim().is_empty() {
        diagnostics.push(schema_mismatch(format!(
          "{lang_path}.negative_test must be non-empty"
        )));
      }
      validate_read_shapes(&lang_path, &language.read_shapes, &mut diagnostics);
      validate_snippets(&lang_path, source, language, &mut diagnostics);
    }
  }

  if diagnostics.is_empty() {
    Ok(())
  } else {
    Err(SourceDbError::many(diagnostics))
  }
}

pub fn compile_source_prefilter(db: &SourceDb) -> Result<AhoCorasick, SourceDbError> {
  let tokens: Vec<&str> = db
    .sources
    .iter()
    .flat_map(|source| source.fast_path_tokens.iter().map(String::as_str))
    .collect();
  AhoCorasick::new(tokens).map_err(|err| {
    SourceDbError::single(schema_mismatch(format!(
      "failed to compile source fast-path prefilter: {err}"
    )))
  })
}

pub fn generate_source_graphql_recipe(
  source: &Source,
  language: ProviderLanguageId,
  shape: &ReadShape,
) -> String {
  let mut filters = Vec::new();
  match shape.kind {
    ReadShapeKind::EnvRead => {
      let env_names = if shape.keys.is_empty() {
        &source.env_var_names
      } else {
        &shape.keys
      };
      if !env_names.is_empty() {
        filters.push(format!("envVarNames: [{}]", gql_string_list(env_names)));
      }
      if !shape.access_paths.is_empty() {
        filters.push(format!(
          "accessPaths: [{}]",
          gql_string_list(&shape.access_paths)
        ));
      }
    }
    ReadShapeKind::KvGet => {
      if !shape.key_names.is_empty() {
        filters.push(format!("keyNames: [{}]", gql_string_list(&shape.key_names)));
      }
      if !shape.access_paths.is_empty() {
        filters.push(format!(
          "accessPaths: [{}]",
          gql_string_list(&shape.access_paths)
        ));
      }
    }
    ReadShapeKind::SdkCall | ReadShapeKind::DotenvLoad => {
      if !shape.callees.is_empty() {
        filters.push(format!("callees: [{}]", gql_string_list(&shape.callees)));
      }
    }
    ReadShapeKind::ConfigLookup => {
      if !shape.object_paths.is_empty() {
        filters.push(format!(
          "objectPaths: [{}]",
          gql_string_list(&shape.object_paths)
        ));
      }
      if !shape.key_names.is_empty() {
        filters.push(format!("keyNames: [{}]", gql_string_list(&shape.key_names)));
      }
    }
  }
  if !shape.import_hints.is_empty() {
    filters.push(format!(
      "importHints: [{}]",
      gql_string_list(&shape.import_hints)
    ));
  }
  let filter_arg = if filters.is_empty() {
    String::new()
  } else {
    format!(", filters: [{{ {} }}]", filters.join(", "))
  };
  format!(
    "query {source_id}_{language_id}_{kind}($path: String!) {{\n  sourceReads(path: $path, language: {language_arg}, redactArgValues: true{filter_arg}) {{\n    schema\n    argValuesRedacted\n    sourceReads {{\n      filePath\n      lineNumber\n      column\n      sourceId\n      sourceCategory\n      providerId\n      sdk\n      envVarName\n      keyName\n      accessPath\n      readText\n      importEvidence\n    }}\n    exitStatus\n  }}\n}}\n",
    source_id = graphql_ident(&source.id),
    language_id = language_graphql_ident(language),
    kind = shape.kind.graphql_ident(),
    language_arg = language_graphql_enum(language),
  )
}

pub fn deprecated_source_field_diagnostic(field_path: impl Into<String>) -> SecretDiagnostic {
  let field_path = field_path.into();
  SecretDiagnostic::new(
    SOURCE_DB_DEPRECATED_FIELD,
    format!("source DB field `{field_path}` is deprecated"),
  )
}

fn validate_fast_path_tokens(
  source_path: &str,
  tokens: &[String],
  diagnostics: &mut Vec<SecretDiagnostic>,
) {
  if tokens.is_empty() {
    diagnostics.push(schema_mismatch(format!(
      "{source_path}.fast_path_tokens must contain at least one token"
    )));
  }
  for token in tokens {
    let trimmed = token.trim();
    if trimmed.is_empty() {
      diagnostics.push(schema_mismatch(format!(
        "{source_path}.fast_path_tokens contains an empty token"
      )));
    } else if trimmed.chars().count() < 3 {
      diagnostics.push(schema_mismatch(format!(
        "{source_path}.fast_path_tokens token `{token}` must be at least 3 characters"
      )));
    } else if BLOCKED_FAST_PATH_TOKENS.contains(&trimmed) {
      diagnostics.push(generic_fast_path_token(format!(
        "{source_path}.fast_path_tokens token `{token}` is a generic env API; use curated env var names or SDK identifiers"
      )));
    }
  }
}

fn validate_read_shapes(
  lang_path: &str,
  shapes: &[ReadShape],
  diagnostics: &mut Vec<SecretDiagnostic>,
) {
  for (index, shape) in shapes.iter().enumerate() {
    let shape_path = format!("{lang_path}.read_shapes[{index}]");
    match shape.kind {
      ReadShapeKind::EnvRead => {
        if shape.access_paths.is_empty() {
          diagnostics.push(schema_mismatch(format!(
            "{shape_path}.access_paths must contain at least one env access path"
          )));
        }
      }
      ReadShapeKind::KvGet => {
        if shape.access_paths.is_empty() {
          diagnostics.push(schema_mismatch(format!(
            "{shape_path}.access_paths must contain at least one KV access path"
          )));
        }
        if shape.key_names.is_empty() {
          diagnostics.push(schema_mismatch(format!(
            "{shape_path}.key_names must contain at least one KV key/path marker"
          )));
        }
      }
      ReadShapeKind::SdkCall | ReadShapeKind::DotenvLoad => {
        if shape.callees.is_empty() {
          diagnostics.push(schema_mismatch(format!(
            "{shape_path}.callees must contain at least one callee"
          )));
        }
      }
      ReadShapeKind::ConfigLookup => {
        if shape.object_paths.is_empty() {
          diagnostics.push(schema_mismatch(format!(
            "{shape_path}.object_paths must contain at least one object path"
          )));
        }
        if shape.key_names.is_empty() {
          diagnostics.push(schema_mismatch(format!(
            "{shape_path}.key_names must contain at least one config key"
          )));
        }
      }
    }
  }
}

fn validate_snippets(
  lang_path: &str,
  source: &Source,
  language: &SourceLanguage,
  diagnostics: &mut Vec<SecretDiagnostic>,
) {
  let positive_matches = language
    .read_shapes
    .iter()
    .any(|shape| snippet_matches_shape(&language.positive_test, source, shape));
  if !positive_matches {
    diagnostics.push(schema_mismatch(format!(
      "{lang_path}.positive_test must match at least one read_shape"
    )));
  }
  let negative_matches = language
    .read_shapes
    .iter()
    .any(|shape| snippet_matches_shape(&language.negative_test, source, shape));
  if negative_matches {
    diagnostics.push(schema_mismatch(format!(
      "{lang_path}.negative_test must not match any read_shape"
    )));
  }
}

fn snippet_matches_shape(snippet: &str, source: &Source, shape: &ReadShape) -> bool {
  match shape.kind {
    ReadShapeKind::EnvRead => {
      let keys = if shape.keys.is_empty() {
        &source.env_var_names
      } else {
        &shape.keys
      };
      contains_any(snippet, &shape.access_paths) && contains_any(snippet, keys)
    }
    ReadShapeKind::KvGet => {
      contains_any(snippet, &shape.access_paths) && contains_any(snippet, &shape.key_names)
    }
    ReadShapeKind::SdkCall | ReadShapeKind::DotenvLoad => contains_any(snippet, &shape.callees),
    ReadShapeKind::ConfigLookup => {
      contains_any(snippet, &shape.object_paths) && contains_any(snippet, &shape.key_names)
    }
  }
}

fn contains_any(snippet: &str, needles: &[String]) -> bool {
  !needles.is_empty() && needles.iter().any(|needle| snippet.contains(needle))
}

fn classify_toml_error(err: toml::de::Error) -> SourceDbError {
  let msg = err.to_string();
  if msg.contains("unknown field") {
    SourceDbError::single(SecretDiagnostic::new(
      SOURCE_DB_UNKNOWN_FIELD,
      format!("source DB contains an unknown field: {msg}"),
    ))
  } else {
    SourceDbError::single(schema_mismatch(format!(
      "source DB TOML does not match schema: {msg}"
    )))
  }
}

fn schema_mismatch(message: impl Into<String>) -> SecretDiagnostic {
  SecretDiagnostic::new(SOURCE_DB_SCHEMA_MISMATCH, message)
}

fn generic_fast_path_token(message: impl Into<String>) -> SecretDiagnostic {
  SecretDiagnostic::new(SOURCE_DB_GENERIC_FAST_PATH_TOKEN, message)
}

fn gql_string(value: &str) -> String {
  format!("{value:?}")
}

fn gql_string_list(values: &[String]) -> String {
  values
    .iter()
    .map(|value| gql_string(value))
    .collect::<Vec<_>>()
    .join(", ")
}

fn graphql_ident(value: &str) -> String {
  value
    .chars()
    .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
    .collect()
}

fn language_graphql_ident(language: ProviderLanguageId) -> &'static str {
  match language {
    ProviderLanguageId::Python => "python",
    ProviderLanguageId::JavaScript => "javascript",
    ProviderLanguageId::TypeScript => "typescript",
    ProviderLanguageId::Go => "go",
    ProviderLanguageId::Java => "java",
  }
}

fn language_graphql_enum(language: ProviderLanguageId) -> &'static str {
  match language {
    ProviderLanguageId::Python => "PYTHON",
    ProviderLanguageId::JavaScript => "JAVASCRIPT",
    ProviderLanguageId::TypeScript => "TYPESCRIPT",
    ProviderLanguageId::Go => "GO",
    ProviderLanguageId::Java => "JAVA",
  }
}

impl ReadShapeKind {
  fn graphql_ident(self) -> &'static str {
    match self {
      Self::EnvRead => "env_read",
      Self::KvGet => "kv_get",
      Self::SdkCall => "sdk_call",
      Self::DotenvLoad => "dotenv_load",
      Self::ConfigLookup => "config_lookup",
    }
  }
}
