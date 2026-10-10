use crate::{load_gitleaks_config_str, SecretDiagnostic, DEFAULT_GITLEAKS_CONFIG};
use aho_corasick::AhoCorasick;
use serde::Deserialize;
use std::collections::HashSet;
use std::fmt;

pub const DEFAULT_PROVIDER_DB: &str = include_str!("../providers/default.toml");
pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;

pub const PROVIDER_DB_SCHEMA_MISMATCH: &str = "PROVIDER_DB_SCHEMA_MISMATCH";
pub const PROVIDER_DB_UNKNOWN_FIELD: &str = "PROVIDER_DB_UNKNOWN_FIELD";
pub const PROVIDER_DB_DEPRECATED_FIELD: &str = "PROVIDER_DB_DEPRECATED_FIELD";

#[derive(Debug, Clone, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProviderDb {
  pub schema_version: u32,
  pub title: String,
  pub providers: Vec<Provider>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Provider {
  pub id: String,
  pub description: String,
  pub rule_ids: Vec<String>,
  pub credential_kind: String,
  #[serde(default)]
  pub sdk_urls: Vec<String>,
  pub fast_path_tokens: Vec<String>,
  pub languages: Vec<ProviderLanguage>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProviderLanguage {
  pub language: ProviderLanguageId,
  pub sdk: String,
  pub sink_node_kinds: Vec<String>,
  pub positive_test: String,
  pub negative_test: String,
  pub call_shapes: Vec<CallShape>,
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ProviderLanguageId {
  Python,
  JavaScript,
  TypeScript,
  Go,
  Java,
}

impl fmt::Display for ProviderLanguageId {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Python => f.write_str("python"),
      Self::JavaScript => f.write_str("javascript"),
      Self::TypeScript => f.write_str("typescript"),
      Self::Go => f.write_str("go"),
      Self::Java => f.write_str("java"),
    }
  }
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CallShape {
  pub kind: CallShapeKind,
  #[serde(default)]
  pub callee: Option<String>,
  #[serde(default)]
  pub kwargs: Vec<String>,
  #[serde(default)]
  pub import_hints: Vec<String>,
  #[serde(default)]
  pub positional_indices: Vec<usize>,
  #[serde(default)]
  pub object_keys: Vec<String>,
  #[serde(default)]
  pub object_paths: Vec<String>,
  #[serde(default)]
  pub struct_type: Option<String>,
  #[serde(default)]
  pub struct_fields: Vec<String>,
  #[serde(default)]
  pub assignment_target: Option<String>,
  #[serde(default)]
  pub nested_callees: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum CallShapeKind {
  Call,
  New,
  Struct,
  Assignment,
}

#[derive(Debug, Clone)]
pub struct ProviderDbError {
  diagnostics: Vec<SecretDiagnostic>,
}

impl ProviderDbError {
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

impl fmt::Display for ProviderDbError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if let Some(first) = self.diagnostics.first() {
      write!(f, "{}: {}", first.code, first.message)
    } else {
      f.write_str("provider DB error")
    }
  }
}

impl std::error::Error for ProviderDbError {}

pub fn load_default_provider_db() -> Result<ProviderDb, ProviderDbError> {
  load_provider_db_str(DEFAULT_PROVIDER_DB, DEFAULT_GITLEAKS_CONFIG)
}

pub fn load_provider_db_str(
  provider_toml: &str,
  gitleaks_toml: &str,
) -> Result<ProviderDb, ProviderDbError> {
  let db: ProviderDb = toml::from_str(provider_toml).map_err(classify_toml_error)?;
  if db.schema_version != SUPPORTED_SCHEMA_VERSION {
    return Err(ProviderDbError::single(schema_mismatch(format!(
      "provider DB schema_version {} is not supported by this binary; supported schema_version is {}",
      db.schema_version, SUPPORTED_SCHEMA_VERSION
    ))));
  }

  let gitleaks = load_gitleaks_config_str(gitleaks_toml)
    .map_err(|err| ProviderDbError::single(schema_mismatch(format!("gitleaks config: {err}"))))?;
  let rule_ids: HashSet<String> = gitleaks.rules.into_iter().map(|rule| rule.id).collect();
  validate_provider_db(&db, &rule_ids)?;
  Ok(db)
}

pub fn validate_provider_db(
  db: &ProviderDb,
  gitleaks_rule_ids: &HashSet<String>,
) -> Result<(), ProviderDbError> {
  let mut diagnostics = Vec::new();
  let mut provider_ids = HashSet::new();

  if db.title.trim().is_empty() {
    diagnostics.push(schema_mismatch("provider DB title must be non-empty"));
  }
  if db.providers.is_empty() {
    diagnostics.push(schema_mismatch(
      "provider DB must contain at least one provider",
    ));
  }

  for provider in &db.providers {
    let provider_path = format!("providers.{}", provider.id);
    if provider.id.trim().is_empty() {
      diagnostics.push(schema_mismatch("provider id must be non-empty"));
    } else if !provider_ids.insert(provider.id.clone()) {
      diagnostics.push(schema_mismatch(format!(
        "duplicate provider id `{}`",
        provider.id
      )));
    }

    if provider.rule_ids.is_empty() {
      diagnostics.push(schema_mismatch(format!(
        "{provider_path}.rule_ids must contain at least one gitleaks rule id"
      )));
    }
    for rule_id in &provider.rule_ids {
      if !gitleaks_rule_ids.contains(rule_id) {
        diagnostics.push(schema_mismatch(format!(
          "{provider_path}.rule_ids references unknown gitleaks rule id `{rule_id}`"
        )));
      }
    }

    if provider.fast_path_tokens.is_empty() {
      diagnostics.push(schema_mismatch(format!(
        "{provider_path}.fast_path_tokens must contain at least one token"
      )));
    }
    for token in &provider.fast_path_tokens {
      let trimmed = token.trim();
      if trimmed.is_empty() {
        diagnostics.push(schema_mismatch(format!(
          "{provider_path}.fast_path_tokens contains an empty token"
        )));
      } else if trimmed.chars().count() < 3 {
        diagnostics.push(schema_mismatch(format!(
          "{provider_path}.fast_path_tokens token `{token}` must be at least 3 characters"
        )));
      }
    }

    let mut sdk_keys = HashSet::new();
    for language in &provider.languages {
      let lang_path = format!(
        "{provider_path}.languages.{}.{}",
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
          "duplicate sdk `{}` for provider `{}` language `{}`",
          language.sdk, provider.id, language.language
        )));
      }
      if language.call_shapes.is_empty() {
        diagnostics.push(schema_mismatch(format!(
          "{lang_path}.call_shapes must contain at least one shape"
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
      validate_call_shapes(&lang_path, &language.call_shapes, &mut diagnostics);
      validate_snippets(&lang_path, language, &mut diagnostics);
    }
  }

  if diagnostics.is_empty() {
    Ok(())
  } else {
    Err(ProviderDbError::many(diagnostics))
  }
}

pub fn compile_provider_prefilter(db: &ProviderDb) -> Result<AhoCorasick, ProviderDbError> {
  let tokens: Vec<&str> = db
    .providers
    .iter()
    .flat_map(|provider| provider.fast_path_tokens.iter().map(String::as_str))
    .collect();
  AhoCorasick::new(tokens).map_err(|err| {
    ProviderDbError::single(schema_mismatch(format!(
      "failed to compile provider fast-path prefilter: {err}"
    )))
  })
}

pub fn deprecated_field_diagnostic(field_path: impl Into<String>) -> SecretDiagnostic {
  let field_path = field_path.into();
  SecretDiagnostic::new(
    PROVIDER_DB_DEPRECATED_FIELD,
    format!("provider DB field `{field_path}` is deprecated"),
  )
}

pub fn generate_graphql_recipe(
  provider: &Provider,
  language: &ProviderLanguage,
  shape: &CallShape,
) -> String {
  let mut filters = Vec::new();
  match shape.kind {
    CallShapeKind::Call | CallShapeKind::New => {
      if !shape.kwargs.is_empty() {
        filters.push(format!("argNames: [{}]", gql_string_list(&shape.kwargs)));
      }
      if !shape.positional_indices.is_empty() {
        filters.push(format!(
          "positionalIndices: [{}]",
          shape
            .positional_indices
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(", ")
        ));
      }
      if !shape.object_keys.is_empty() {
        filters.push(format!(
          "objectKeys: [{}]",
          gql_string_list(&shape.object_keys)
        ));
      }
      if !shape.object_paths.is_empty() {
        filters.push(format!(
          "objectPaths: [{}]",
          gql_string_list(&shape.object_paths)
        ));
      }
    }
    CallShapeKind::Struct => {
      if !shape.struct_fields.is_empty() {
        filters.push(format!(
          "structFields: [{}]",
          gql_string_list(&shape.struct_fields)
        ));
      }
    }
    CallShapeKind::Assignment => {
      if let Some(target) = shape.assignment_target.as_deref() {
        filters.push(format!("assignmentTargets: [{}]", gql_string(target)));
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
  let callee = match shape.kind {
    CallShapeKind::Call | CallShapeKind::New => shape.callee.as_deref().unwrap_or_default(),
    CallShapeKind::Struct => shape.struct_type.as_deref().unwrap_or_default(),
    CallShapeKind::Assignment => shape.assignment_target.as_deref().unwrap_or_default(),
  };

  format!(
    "query {provider_id}_{language_id}_{sdk}_{kind}($path: String!) {{\n  callsites(path: $path, language: {language_arg}, callee: {callee}, redactArgValues: true{filter_arg}) {{\n    schema\n    argValuesRedacted\n    callsites {{\n      filePath\n      lineNumber\n      calleeText\n      positionalArgs\n      keywordArgs {{ name value }}\n      sinkKind\n      importEvidence\n      structuredArgs {{ name position text role }}\n      arguments {{ name position text role }}\n    }}\n    exitStatus\n  }}\n}}\n",
    provider_id = graphql_ident(&provider.id),
    language_id = language.language.graphql_ident(),
    sdk = graphql_ident(&language.sdk),
    kind = shape.kind.graphql_ident(),
    language_arg = language.language.graphql_enum(),
    callee = gql_string(callee),
  )
}

fn validate_call_shapes(
  lang_path: &str,
  shapes: &[CallShape],
  diagnostics: &mut Vec<SecretDiagnostic>,
) {
  for (index, shape) in shapes.iter().enumerate() {
    let shape_path = format!("{lang_path}.call_shapes[{index}]");
    match shape.kind {
      CallShapeKind::Call | CallShapeKind::New => {
        if shape.callee.as_deref().is_none_or(is_blank) {
          diagnostics.push(schema_mismatch(format!(
            "{shape_path}.callee must be non-empty"
          )));
        }
      }
      CallShapeKind::Struct => {
        if shape.struct_type.as_deref().is_none_or(is_blank) {
          diagnostics.push(schema_mismatch(format!(
            "{shape_path}.struct_type must be non-empty"
          )));
        }
        if shape.struct_fields.is_empty() {
          diagnostics.push(schema_mismatch(format!(
            "{shape_path}.struct_fields must contain at least one field"
          )));
        }
      }
      CallShapeKind::Assignment => {
        if shape.assignment_target.as_deref().is_none_or(is_blank) {
          diagnostics.push(schema_mismatch(format!(
            "{shape_path}.assignment_target must be non-empty"
          )));
        }
      }
    }
  }
}

fn validate_snippets(
  lang_path: &str,
  language: &ProviderLanguage,
  diagnostics: &mut Vec<SecretDiagnostic>,
) {
  let positive_matches = language
    .call_shapes
    .iter()
    .any(|shape| snippet_matches_shape(&language.positive_test, shape));
  if !positive_matches {
    diagnostics.push(schema_mismatch(format!(
      "{lang_path}.positive_test must match at least one call_shape"
    )));
  }

  let negative_matches = language
    .call_shapes
    .iter()
    .any(|shape| snippet_matches_shape(&language.negative_test, shape));
  if negative_matches {
    diagnostics.push(schema_mismatch(format!(
      "{lang_path}.negative_test must not match any call_shape"
    )));
  }
}

fn snippet_matches_shape(snippet: &str, shape: &CallShape) -> bool {
  match shape.kind {
    CallShapeKind::Call | CallShapeKind::New => {
      let Some(callee) = shape.callee.as_deref() else {
        return false;
      };
      if !snippet.contains(callee) {
        return false;
      }
      if !shape.kwargs.is_empty() && shape.kwargs.iter().any(|name| snippet.contains(name)) {
        return true;
      }
      if !shape.object_keys.is_empty() && shape.object_keys.iter().any(|key| snippet.contains(key))
      {
        return true;
      }
      if !shape.object_paths.is_empty()
        && shape.object_paths.iter().any(|path| {
          snippet.contains(path)
            || path
              .rsplit('.')
              .next()
              .is_some_and(|last| snippet.contains(last))
        })
      {
        return true;
      }
      if !shape.nested_callees.is_empty()
        && shape
          .nested_callees
          .iter()
          .any(|nested| snippet.contains(nested))
      {
        return true;
      }
      !shape.positional_indices.is_empty()
        && snippet
          .split_once(callee)
          .and_then(|(_, rest)| rest.split_once('('))
          .and_then(|(_, args)| args.split_once(')'))
          .is_some_and(|(args, _)| !args.trim().is_empty())
    }
    CallShapeKind::Struct => {
      shape
        .struct_type
        .as_deref()
        .is_some_and(|struct_type| snippet.contains(struct_type))
        && shape
          .struct_fields
          .iter()
          .any(|field| snippet.contains(field))
    }
    CallShapeKind::Assignment => shape
      .assignment_target
      .as_deref()
      .is_some_and(|target| snippet.contains(target) && snippet.contains('=')),
  }
}

impl ProviderLanguageId {
  fn graphql_ident(self) -> &'static str {
    match self {
      Self::Python => "python",
      Self::JavaScript => "javascript",
      Self::TypeScript => "typescript",
      Self::Go => "go",
      Self::Java => "java",
    }
  }

  fn graphql_enum(self) -> &'static str {
    match self {
      Self::Python => "PYTHON",
      Self::JavaScript => "JAVASCRIPT",
      Self::TypeScript => "TYPESCRIPT",
      Self::Go => "GO",
      Self::Java => "JAVA",
    }
  }
}

impl CallShapeKind {
  fn graphql_ident(self) -> &'static str {
    match self {
      Self::Call => "call",
      Self::New => "new",
      Self::Struct => "struct",
      Self::Assignment => "assignment",
    }
  }
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

fn is_blank(value: &str) -> bool {
  value.trim().is_empty()
}

fn classify_toml_error(err: toml::de::Error) -> ProviderDbError {
  let msg = err.to_string();
  if msg.contains("unknown field") {
    ProviderDbError::single(SecretDiagnostic::new(
      PROVIDER_DB_UNKNOWN_FIELD,
      format!("provider DB contains an unknown field: {msg}"),
    ))
  } else {
    ProviderDbError::single(schema_mismatch(format!(
      "provider DB TOML does not match schema: {msg}"
    )))
  }
}

fn schema_mismatch(message: impl Into<String>) -> SecretDiagnostic {
  SecretDiagnostic::new(PROVIDER_DB_SCHEMA_MISMATCH, message)
}
