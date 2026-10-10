//! Phase 1 semantic slice: bounded Python symbol reachability.
//!
//! This module uses lmprobe's in-process `ast-grep-core` /
//! `ast-grep-language` stack directly. The higher-level engine sensors
//! are public, but they are directory/envelope-oriented and keep the
//! node-level call/assignment helpers private; this crate needs a
//! library analyzer over one source string with secret-safe findings.
//! No CLI shell-out is involved.

use aho_corasick::AhoCorasick;
use ast_grep_core::tree_sitter::StrDoc;
use ast_grep_core::Node;
use ast_grep_language::{LanguageExt, SupportLang};
use lmprobe_types::schema::{
  ActiveCredentialOrigin, ConfirmedCredentialFlowFinding, CredentialSourceEvidence, FlowStep,
  FlowStepKind, Secret, SemanticConfidence, SinkEvidence, SinkEvidenceOnlyFinding, SinkKind,
  SourceEvidence, SymbolReachabilityFinding,
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::{
  compile_provider_prefilter, compile_source_prefilter, load_default_provider_db,
  load_default_source_db, CallShapeKind, Provider, ProviderDb, ProviderLanguage,
  ProviderLanguageId, ReadShapeKind, SourceDb,
};

type PythonNode<'a> = Node<'a, StrDoc<SupportLang>>;

const DETECTOR_SYMBOL_REACHABILITY: &str = "symbol-reachability";
const DETECTOR_CONFIRMED_CREDENTIAL_FLOW: &str = "confirmed-credential-flow";
const DETECTOR_SINK_EVIDENCE: &str = "sink-evidence";
const DETECTOR_PROVIDER_SINK: &str = "provider-sink";
pub const SINK_CATALOG_VERSION: &str = "2026-05-p4-v1";

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize)]
pub struct SemanticScanResult {
  pub sink_catalog_version: String,
  pub symbol_reachability: Vec<SymbolReachabilityFinding>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub source_evidence: Vec<CredentialSourceEvidence>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub confirmed_flows: Vec<ConfirmedCredentialFlowFinding>,
  pub sink_evidence: Vec<SinkEvidenceOnlyFinding>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub warnings: Vec<SemanticWarning>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct SemanticWarning {
  pub code: String,
  pub message: String,
}

impl SemanticScanResult {
  pub fn new() -> Self {
    Self {
      sink_catalog_version: SINK_CATALOG_VERSION.into(),
      symbol_reachability: Vec::new(),
      source_evidence: Vec::new(),
      confirmed_flows: Vec::new(),
      sink_evidence: Vec::new(),
      warnings: Vec::new(),
    }
  }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub enum SinkEvidenceMode {
  #[default]
  Off,
  On,
}

/// Per-language binding/scoping contract for P4.
///
/// The current analyzers use this as an explicit language dispatch seam:
/// Python keeps whole-function local binding collection with global/nonlocal
/// directives; JS/TS model block scopes, `let`/`const` TDZ, and function
/// hoisted `var`; Go models lexical `{}` blocks for `:=` and `var`; Java
/// separates class fields from method/block locals. P5 should attach
/// cross-file symbol graph edges behind this strategy boundary instead of
/// reintroducing file-global literal state.
pub trait BindingStrategy {
  fn language(&self) -> &'static str;
  fn support_lang(&self) -> SupportLang;
  fn function_scope_kinds(&self) -> &'static [&'static str];
  fn block_scope_kinds(&self) -> &'static [&'static str];
  fn declaration_kinds(&self) -> &'static [&'static str];
  fn sink_node_kinds(&self) -> &'static [&'static str];
  fn import_kinds(&self) -> &'static [&'static str];
  fn export_kinds(&self) -> &'static [&'static str];
}

pub struct PythonBindingStrategy;
pub struct JavaScriptBindingStrategy;
pub struct TypeScriptBindingStrategy;
pub struct GoBindingStrategy;
pub struct JavaBindingStrategy;

impl BindingStrategy for PythonBindingStrategy {
  fn language(&self) -> &'static str {
    "python"
  }
  fn support_lang(&self) -> SupportLang {
    SupportLang::Python
  }
  fn function_scope_kinds(&self) -> &'static [&'static str] {
    &["function_definition"]
  }
  fn block_scope_kinds(&self) -> &'static [&'static str] {
    &["class_definition"]
  }
  fn declaration_kinds(&self) -> &'static [&'static str] {
    &["assignment"]
  }
  fn sink_node_kinds(&self) -> &'static [&'static str] {
    &["call", "assignment"]
  }
  fn import_kinds(&self) -> &'static [&'static str] {
    &["import_statement", "import_from_statement"]
  }
  fn export_kinds(&self) -> &'static [&'static str] {
    &[]
  }
}

impl BindingStrategy for JavaScriptBindingStrategy {
  fn language(&self) -> &'static str {
    "javascript"
  }
  fn support_lang(&self) -> SupportLang {
    SupportLang::JavaScript
  }
  fn function_scope_kinds(&self) -> &'static [&'static str] {
    &[
      "function_declaration",
      "function",
      "function_expression",
      "arrow_function",
    ]
  }
  fn block_scope_kinds(&self) -> &'static [&'static str] {
    &["statement_block", "class_body"]
  }
  fn declaration_kinds(&self) -> &'static [&'static str] {
    &[
      "lexical_declaration",
      "variable_declaration",
      "variable_declarator",
    ]
  }
  fn sink_node_kinds(&self) -> &'static [&'static str] {
    &["call_expression", "new_expression"]
  }
  fn import_kinds(&self) -> &'static [&'static str] {
    &["import_statement"]
  }
  fn export_kinds(&self) -> &'static [&'static str] {
    &["export_statement"]
  }
}

impl BindingStrategy for TypeScriptBindingStrategy {
  fn language(&self) -> &'static str {
    "typescript"
  }
  fn support_lang(&self) -> SupportLang {
    SupportLang::TypeScript
  }
  fn function_scope_kinds(&self) -> &'static [&'static str] {
    JavaScriptBindingStrategy.function_scope_kinds()
  }
  fn block_scope_kinds(&self) -> &'static [&'static str] {
    JavaScriptBindingStrategy.block_scope_kinds()
  }
  fn declaration_kinds(&self) -> &'static [&'static str] {
    JavaScriptBindingStrategy.declaration_kinds()
  }
  fn sink_node_kinds(&self) -> &'static [&'static str] {
    JavaScriptBindingStrategy.sink_node_kinds()
  }
  fn import_kinds(&self) -> &'static [&'static str] {
    JavaScriptBindingStrategy.import_kinds()
  }
  fn export_kinds(&self) -> &'static [&'static str] {
    JavaScriptBindingStrategy.export_kinds()
  }
}

impl BindingStrategy for GoBindingStrategy {
  fn language(&self) -> &'static str {
    "go"
  }
  fn support_lang(&self) -> SupportLang {
    SupportLang::Go
  }
  fn function_scope_kinds(&self) -> &'static [&'static str] {
    &["function_declaration", "method_declaration", "func_literal"]
  }
  fn block_scope_kinds(&self) -> &'static [&'static str] {
    &["block"]
  }
  fn declaration_kinds(&self) -> &'static [&'static str] {
    &["short_var_declaration", "var_declaration", "var_spec"]
  }
  fn sink_node_kinds(&self) -> &'static [&'static str] {
    &[
      "call_expression",
      "composite_literal",
      "keyed_element",
      "assignment_statement",
    ]
  }
  fn import_kinds(&self) -> &'static [&'static str] {
    &["import_declaration", "import_spec"]
  }
  fn export_kinds(&self) -> &'static [&'static str] {
    &[
      "function_declaration",
      "method_declaration",
      "type_declaration",
      "var_declaration",
      "const_declaration",
    ]
  }
}

impl BindingStrategy for JavaBindingStrategy {
  fn language(&self) -> &'static str {
    "java"
  }
  fn support_lang(&self) -> SupportLang {
    SupportLang::Java
  }
  fn function_scope_kinds(&self) -> &'static [&'static str] {
    &["method_declaration", "constructor_declaration"]
  }
  fn block_scope_kinds(&self) -> &'static [&'static str] {
    &["block", "class_body", "class_declaration"]
  }
  fn declaration_kinds(&self) -> &'static [&'static str] {
    &[
      "field_declaration",
      "local_variable_declaration",
      "variable_declarator",
      "assignment_expression",
    ]
  }
  fn sink_node_kinds(&self) -> &'static [&'static str] {
    &[
      "object_creation_expression",
      "method_invocation",
      "assignment_expression",
    ]
  }
  fn import_kinds(&self) -> &'static [&'static str] {
    &["import_declaration"]
  }
  fn export_kinds(&self) -> &'static [&'static str] {
    &[
      "class_declaration",
      "method_declaration",
      "field_declaration",
    ]
  }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PythonCallSink {
  pub method_name: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PythonAttributeSink {
  pub object_name: Option<String>,
  pub attribute_name: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PythonCredentialKwargSink {
  pub callee_text: Option<String>,
  pub keyword_name: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PythonSinkCatalog {
  pub call_sinks: Vec<PythonCallSink>,
  pub attribute_sinks: Vec<PythonAttributeSink>,
  pub credential_kwarg_sinks: Vec<PythonCredentialKwargSink>,
}

impl Default for PythonSinkCatalog {
  fn default() -> Self {
    Self {
      call_sinks: ["login", "connect", "authenticate"]
        .into_iter()
        .map(|method_name| PythonCallSink {
          method_name: method_name.to_string(),
        })
        .collect(),
      attribute_sinks: Vec::new(),
      credential_kwarg_sinks: Vec::new(),
    }
  }
}

#[derive(Debug)]
struct ProviderMatcher {
  db: ProviderDb,
  prefilter: AhoCorasick,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct ProviderFindingMeta {
  provider_id: String,
  provider_rule_ids: Vec<String>,
  credential_kind: String,
  sdk: String,
  provider_shape_id: String,
}

impl ProviderFindingMeta {
  fn from_shape(provider: &Provider, language: &ProviderLanguage, shape_index: usize) -> Self {
    Self {
      provider_id: provider.id.clone(),
      provider_rule_ids: provider.rule_ids.clone(),
      credential_kind: provider.credential_kind.clone(),
      sdk: language.sdk.clone(),
      provider_shape_id: format!(
        "{}/{}/{}/{}",
        provider.id, language.language, language.sdk, shape_index
      ),
    }
  }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct ProviderSinkCandidate {
  value_name: String,
  sink: SinkEvidence,
  meta: ProviderFindingMeta,
}

struct ProviderSinkEvidenceInput<'a> {
  path: &'a str,
  line: &'a str,
  line_number: usize,
  callee_text: &'a str,
  arg_position: Option<usize>,
  arg_name: Option<String>,
  sink_kind: SinkKind,
  value_text: String,
  meta: &'a ProviderFindingMeta,
}

static DEFAULT_PROVIDER_MATCHER: OnceLock<Option<ProviderMatcher>> = OnceLock::new();

fn default_provider_matcher() -> Option<&'static ProviderMatcher> {
  DEFAULT_PROVIDER_MATCHER
    .get_or_init(|| {
      let db = load_default_provider_db().ok()?;
      let prefilter = compile_provider_prefilter(&db).ok()?;
      Some(ProviderMatcher { db, prefilter })
    })
    .as_ref()
}

#[derive(Debug)]
struct SourceMatcher {
  db: SourceDb,
  prefilter: AhoCorasick,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct SourceReadMatch {
  evidence: CredentialSourceEvidence,
  kind: ReadShapeKind,
}

static DEFAULT_SOURCE_MATCHER: OnceLock<Option<SourceMatcher>> = OnceLock::new();

fn default_source_matcher() -> Option<&'static SourceMatcher> {
  DEFAULT_SOURCE_MATCHER
    .get_or_init(|| {
      let db = load_default_source_db().ok()?;
      let prefilter = compile_source_prefilter(&db).ok()?;
      Some(SourceMatcher { db, prefilter })
    })
    .as_ref()
}

impl SourceMatcher {
  fn source_read(
    &self,
    path: &str,
    content: &str,
    node: &PythonNode<'_>,
    variable: &str,
    requested_language: ProviderLanguageId,
  ) -> Option<SourceReadMatch> {
    for source in &self.db.sources {
      for language in &source.languages {
        if !crate::source_matchers::source_language_matches_provider(
          requested_language,
          language.language,
        ) {
          continue;
        }
        for shape in &language.read_shapes {
          if !import_hints_match(content, &shape.import_hints) {
            continue;
          }
          let Some(hit) = std::iter::once(node.clone())
            .chain(node.dfs().filter(|child| child.is_named()))
            .find_map(|candidate| {
              crate::source_matchers::source_read_hit_from_node(
                Path::new(path),
                &candidate,
                source,
                language,
                shape,
              )
            })
          else {
            continue;
          };
          return Some(SourceReadMatch {
            evidence: CredentialSourceEvidence {
              file_path: path.to_string(),
              line: hit.line_number,
              column: hit.column,
              source_id: hit.source_id,
              source_category: hit.source_category,
              provider_id: hit.provider_id,
              env_var_name: hit.env_var_name,
              key_name: hit.key_name,
              sdk: hit.sdk,
              variable: variable.to_string(),
            },
            kind: shape.kind,
          });
        }
      }
    }
    None
  }
}

impl ProviderMatcher {
  fn python_languages<'a>(
    &'a self,
  ) -> impl Iterator<Item = (&'a Provider, &'a ProviderLanguage)> + 'a {
    self.languages(ProviderLanguageId::Python)
  }

  fn languages<'a>(
    &'a self,
    language_id: ProviderLanguageId,
  ) -> impl Iterator<Item = (&'a Provider, &'a ProviderLanguage)> + 'a {
    self.db.providers.iter().flat_map(move |provider| {
      provider.languages.iter().filter_map(move |language| {
        (language.language == language_id).then_some((provider, language))
      })
    })
  }

  fn assignment_candidates(
    &self,
    path: &str,
    content: &str,
    target_text: &str,
    value_name: &str,
    line: usize,
    column: usize,
  ) -> Vec<ProviderSinkCandidate> {
    let mut candidates = Vec::new();
    for (provider, language) in self.python_languages() {
      for (shape_index, shape) in language.call_shapes.iter().enumerate() {
        if shape.kind != CallShapeKind::Assignment {
          continue;
        }
        if shape.assignment_target.as_deref() != Some(target_text) {
          continue;
        }
        if !import_hints_match(content, &shape.import_hints) {
          continue;
        }
        let Some((_, attribute_name)) = split_attribute_text(target_text) else {
          continue;
        };
        candidates.push(ProviderSinkCandidate {
          value_name: value_name.to_string(),
          sink: SinkEvidence {
            file_path: path.to_string(),
            line,
            column,
            callee_text: target_text.to_string(),
            arg_position: None,
            arg_name: Some(attribute_name.to_string()),
            sink_kind: SinkKind::AttributeAssignment,
          },
          meta: ProviderFindingMeta::from_shape(provider, language, shape_index),
        });
      }
    }
    candidates
  }

  fn call_candidates(
    &self,
    path: &str,
    content: &str,
    callee_text: &str,
    arguments: &PythonNode<'_>,
    line: usize,
    column: usize,
  ) -> Vec<ProviderSinkCandidate> {
    let args = top_level_argument_nodes(arguments);
    let mut candidates = Vec::new();
    for (provider, language) in self.python_languages() {
      for (shape_index, shape) in language.call_shapes.iter().enumerate() {
        if !matches!(shape.kind, CallShapeKind::Call | CallShapeKind::New) {
          continue;
        }
        if shape.callee.as_deref() != Some(callee_text) {
          continue;
        }
        if !import_hints_match(content, &shape.import_hints) {
          continue;
        }
        let meta = ProviderFindingMeta::from_shape(provider, language, shape_index);
        for (position, argument) in args.iter().enumerate() {
          if argument.kind() == "keyword_argument" {
            let Some(name) = argument.field("name").map(|node| node.text().to_string()) else {
              continue;
            };
            if !shape.kwargs.iter().any(|expected| expected == &name) {
              continue;
            }
            let Some(value) = argument.field("value") else {
              continue;
            };
            let Some(value_name) = simple_identifier_text(&value) else {
              continue;
            };
            candidates.push(ProviderSinkCandidate {
              value_name,
              sink: SinkEvidence {
                file_path: path.to_string(),
                line,
                column,
                callee_text: callee_text.to_string(),
                arg_position: Some(position),
                arg_name: Some(name),
                sink_kind: SinkKind::CredentialKwarg,
              },
              meta: meta.clone(),
            });
          } else if shape.positional_indices.contains(&position) {
            let Some(value_name) = simple_identifier_text(argument) else {
              continue;
            };
            candidates.push(ProviderSinkCandidate {
              value_name,
              sink: SinkEvidence {
                file_path: path.to_string(),
                line,
                column,
                callee_text: callee_text.to_string(),
                arg_position: Some(position),
                arg_name: None,
                sink_kind: SinkKind::MethodCall,
              },
              meta: meta.clone(),
            });
          }
        }
      }
    }
    candidates
  }

  fn line_sink_evidence(
    &self,
    path: &str,
    content: &str,
    line: &str,
    line_number: usize,
  ) -> Vec<SinkEvidenceOnlyFinding> {
    let trimmed = line.trim();
    let mut sinks = Vec::new();
    for (provider, language) in self.python_languages() {
      for (shape_index, shape) in language.call_shapes.iter().enumerate() {
        if !import_hints_match(content, &shape.import_hints) {
          continue;
        }
        let meta = ProviderFindingMeta::from_shape(provider, language, shape_index);
        match shape.kind {
          CallShapeKind::Assignment => {
            let Some(target) = shape.assignment_target.as_deref() else {
              continue;
            };
            if !trimmed.contains(target) {
              continue;
            }
            let Some(value_text) = value_after(trimmed, "=").and_then(identifier_like) else {
              continue;
            };
            let Some((_, attribute_name)) = split_attribute_text(target) else {
              continue;
            };
            sinks.push(provider_sink_evidence(ProviderSinkEvidenceInput {
              path,
              line,
              line_number,
              callee_text: target,
              arg_position: None,
              arg_name: Some(attribute_name.to_string()),
              sink_kind: SinkKind::AttributeAssignment,
              value_text,
              meta: &meta,
            }));
          }
          CallShapeKind::Call | CallShapeKind::New => {
            let Some(callee) = shape.callee.as_deref() else {
              continue;
            };
            let Some(args) = call_args_after(trimmed, callee) else {
              continue;
            };
            for keyword in &shape.kwargs {
              let marker = format!("{keyword}=");
              if let Some(value_text) = value_after(args, &marker).and_then(identifier_like) {
                sinks.push(provider_sink_evidence(ProviderSinkEvidenceInput {
                  path,
                  line,
                  line_number,
                  callee_text: callee,
                  arg_position: None,
                  arg_name: Some(keyword.clone()),
                  sink_kind: SinkKind::CredentialKwarg,
                  value_text,
                  meta: &meta,
                }));
              }
            }
            for position in &shape.positional_indices {
              if let Some(value_text) = nth_arg(args, *position).and_then(identifier_like) {
                sinks.push(provider_sink_evidence(ProviderSinkEvidenceInput {
                  path,
                  line,
                  line_number,
                  callee_text: callee,
                  arg_position: Some(*position),
                  arg_name: None,
                  sink_kind: SinkKind::MethodCall,
                  value_text,
                  meta: &meta,
                }));
              }
            }
          }
          CallShapeKind::Struct => {}
        }
      }
    }
    sinks
  }

  fn ast_sink_uses(
    &self,
    path: &str,
    content: &str,
    node: &PythonNode<'_>,
    lang: AstLang,
  ) -> Vec<SinkEvidenceOnlyFinding> {
    let Some(language_id) = provider_language_for_ast(lang) else {
      return Vec::new();
    };
    let text = node.text();
    let text = text.as_ref();
    let mut sinks = Vec::new();
    for (provider, language) in self.languages(language_id) {
      for (shape_index, shape) in language.call_shapes.iter().enumerate() {
        if !shape_kind_applies_to_ast(shape.kind, lang, node.kind().as_ref()) {
          continue;
        }
        if !import_hints_match(content, &shape.import_hints) {
          continue;
        }
        let Some(callee_text) = shape_callee_text(shape) else {
          continue;
        };
        if !shape_text_matches(text, callee_text, shape.kind) {
          continue;
        }
        let meta = ProviderFindingMeta::from_shape(provider, language, shape_index);
        for value in values_for_shape(text, shape) {
          sinks.push(provider_sink_evidence(ProviderSinkEvidenceInput {
            path,
            line: text,
            line_number: node.start_pos().line() + 1,
            callee_text,
            arg_position: value.position,
            arg_name: value.name,
            sink_kind: value.sink_kind,
            value_text: value.value_text,
            meta: &meta,
          }));
        }
      }
    }
    sinks
  }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct ShapeValue {
  value_text: String,
  position: Option<usize>,
  name: Option<String>,
  sink_kind: SinkKind,
}

fn provider_language_for_ast(lang: AstLang) -> Option<ProviderLanguageId> {
  match lang {
    AstLang::JavaScript => Some(ProviderLanguageId::JavaScript),
    AstLang::Go => Some(ProviderLanguageId::Go),
    AstLang::Java => Some(ProviderLanguageId::Java),
  }
}

fn shape_kind_applies_to_ast(kind: CallShapeKind, lang: AstLang, node_kind: &str) -> bool {
  match kind {
    CallShapeKind::Call => matches!(
      node_kind,
      "call_expression" | "method_invocation" | "keyed_element"
    ),
    CallShapeKind::New => matches!(
      node_kind,
      "new_expression" | "object_creation_expression" | "call_expression"
    ),
    CallShapeKind::Struct => matches!(
      node_kind,
      "composite_literal" | "keyed_element" | "object" | "object_creation_expression"
    ),
    CallShapeKind::Assignment => match lang {
      AstLang::JavaScript => matches!(node_kind, "assignment_expression"),
      AstLang::Go => matches!(node_kind, "assignment_statement"),
      AstLang::Java => matches!(node_kind, "assignment_expression"),
    },
  }
}

fn shape_callee_text(shape: &crate::CallShape) -> Option<&str> {
  match shape.kind {
    CallShapeKind::Call | CallShapeKind::New => shape.callee.as_deref(),
    CallShapeKind::Struct => shape.struct_type.as_deref(),
    CallShapeKind::Assignment => shape.assignment_target.as_deref(),
  }
}

fn shape_text_matches(text: &str, callee_text: &str, kind: CallShapeKind) -> bool {
  if text.contains(callee_text) {
    return true;
  }
  kind == CallShapeKind::Call && text.contains(&format!("require(\"{callee_text}\")"))
    || text.contains(&format!("require('{callee_text}')"))
}

fn values_for_shape(text: &str, shape: &crate::CallShape) -> Vec<ShapeValue> {
  let mut values = Vec::new();
  match shape.kind {
    CallShapeKind::Call | CallShapeKind::New => {
      let args = shape
        .callee
        .as_deref()
        .and_then(|callee| call_args_for_shape(text, callee));
      if let Some(args) = args {
        for keyword in &shape.kwargs {
          let marker = format!("{keyword}:");
          let equals_marker = format!("{keyword}=");
          for candidate in [
            value_after(args, &marker).and_then(identifier_like),
            value_after(args, &equals_marker).and_then(identifier_like),
          ]
          .into_iter()
          .flatten()
          {
            values.push(ShapeValue {
              value_text: candidate,
              position: None,
              name: Some(keyword.clone()),
              sink_kind: SinkKind::CredentialKwarg,
            });
          }
        }
        for position in &shape.positional_indices {
          if let Some(value_text) = nth_arg(args, *position).and_then(identifier_like) {
            values.push(ShapeValue {
              value_text,
              position: Some(*position),
              name: None,
              sink_kind: SinkKind::MethodCall,
            });
          }
        }
      }
      append_object_key_values(text, &shape.object_keys, &mut values);
      let object_path_keys: Vec<_> = shape
        .object_paths
        .iter()
        .filter_map(|path| path.rsplit('.').next().map(str::to_string))
        .collect();
      append_object_key_values(text, &object_path_keys, &mut values);
    }
    CallShapeKind::Struct => append_object_key_values(text, &shape.struct_fields, &mut values),
    CallShapeKind::Assignment => {
      if let Some(value_text) = value_after(text, "=").and_then(identifier_like) {
        values.push(ShapeValue {
          value_text,
          position: None,
          name: shape.assignment_target.clone(),
          sink_kind: SinkKind::AttributeAssignment,
        });
      }
    }
  }
  values
}

fn append_object_key_values(text: &str, keys: &[String], values: &mut Vec<ShapeValue>) {
  for key in keys {
    let marker = format!("{key}:");
    if let Some(value_text) = value_after(text, &marker).and_then(identifier_like) {
      values.push(ShapeValue {
        value_text,
        position: None,
        name: Some(key.clone()),
        sink_kind: SinkKind::CredentialKwarg,
      });
    }
  }
}

fn call_args_for_shape<'a>(text: &'a str, callee: &str) -> Option<&'a str> {
  if let Some(args) = call_args_after(text, callee) {
    return Some(args);
  }
  for marker in [
    format!("require(\"{callee}\")"),
    format!("require('{callee}')"),
  ] {
    if let Some(args) = call_args_after(text, &marker) {
      return Some(args);
    }
  }
  None
}

fn import_hints_match(content: &str, import_hints: &[String]) -> bool {
  import_hints.is_empty()
    || import_hints
      .iter()
      .any(|hint| !hint.trim().is_empty() && content.contains(hint))
}

fn provider_sink_evidence(input: ProviderSinkEvidenceInput<'_>) -> SinkEvidenceOnlyFinding {
  SinkEvidenceOnlyFinding {
    detector: DETECTOR_PROVIDER_SINK.into(),
    confidence: SemanticConfidence::ProviderSinkPattern,
    provider_id: Some(input.meta.provider_id.clone()),
    provider_rule_ids: input.meta.provider_rule_ids.clone(),
    credential_kind: Some(input.meta.credential_kind.clone()),
    sdk: Some(input.meta.sdk.clone()),
    provider_shape_id: Some(input.meta.provider_shape_id.clone()),
    sink: SinkEvidence {
      file_path: input.path.to_string(),
      line: input.line_number,
      column: input.line.find(input.callee_text).unwrap_or(0) + 1,
      callee_text: input.callee_text.to_string(),
      arg_position: input.arg_position,
      arg_name: input.arg_name,
      sink_kind: input.sink_kind,
    },
    value_text: input.value_text,
  }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct ActiveCredential {
  line: usize,
  column: usize,
  origin: ActiveCredentialOrigin,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum PythonScopeKind {
  Module,
  Function,
  Class,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct ScopeFrame {
  kind: PythonScopeKind,
  local_bindings: HashSet<String>,
  global_names: HashSet<String>,
  nonlocal_names: HashSet<String>,
  active_literals: HashMap<String, ActiveCredential>,
}

#[derive(Debug, Default)]
struct ScopedLiteralModel {
  frames: Vec<ScopeFrame>,
}

impl ScopedLiteralModel {
  fn push_scope(&mut self, kind: PythonScopeKind, node: &PythonNode<'_>) {
    let directives = collect_python_scope_directives(node);
    self.frames.push(ScopeFrame {
      kind,
      local_bindings: collect_scope_local_bindings(node, &directives),
      global_names: directives.global_names,
      nonlocal_names: directives.nonlocal_names,
      active_literals: HashMap::new(),
    });
  }

  fn pop_scope(&mut self) {
    self.frames.pop();
  }

  fn update_assignment(&mut self, node: &PythonNode<'_>, source: Option<CredentialSourceEvidence>) {
    let Some(left) = node.field("left") else {
      return;
    };
    let Some(var_name) = simple_identifier_text(&left) else {
      return;
    };
    let target_idx = self.assignment_target_frame(&var_name);
    let Some(frame) = self.frames.get_mut(target_idx) else {
      return;
    };
    let Some(right) = node.field("right") else {
      frame.active_literals.remove(&var_name);
      return;
    };

    if let Some(source) = source {
      frame.active_literals.insert(
        var_name,
        ActiveCredential {
          line: node.start_pos().line() + 1,
          column: node.start_pos().column(node) + 1,
          origin: ActiveCredentialOrigin::Source(source),
        },
      );
      return;
    }

    if right.kind() == "string" {
      if let Some(literal) = strip_simple_python_string_literal(right.text().as_ref()) {
        frame.active_literals.insert(
          var_name,
          ActiveCredential {
            line: node.start_pos().line() + 1,
            column: node.start_pos().column(node) + 1,
            origin: ActiveCredentialOrigin::Literal(Secret::new(literal)),
          },
        );
        return;
      }
    }

    frame.active_literals.remove(&var_name);
  }

  fn resolve(&self, var_name: &str) -> Option<&ActiveCredential> {
    let current_idx = self.frames.len().checked_sub(1)?;
    let current = &self.frames[current_idx];
    if current.global_names.contains(var_name) {
      return self.frames.first()?.active_literals.get(var_name);
    }
    if current.nonlocal_names.contains(var_name) {
      for frame in self.frames[..current_idx].iter().rev() {
        if frame.kind == PythonScopeKind::Function {
          return frame.active_literals.get(var_name);
        }
      }
      return None;
    }
    for (idx, frame) in self.frames.iter().enumerate().rev() {
      // Python class bodies are namespaces, but methods do not close over
      // bare class attributes. P5 can replace this local frame walk with a
      // cross-file symbol graph while preserving the same scoped lookup API.
      if idx != current_idx && frame.kind == PythonScopeKind::Class {
        continue;
      }
      if let Some(active) = frame.active_literals.get(var_name) {
        return Some(active);
      }
      if frame.local_bindings.contains(var_name) {
        return None;
      }
    }
    None
  }

  fn assignment_target_frame(&self, var_name: &str) -> usize {
    let Some(current_idx) = self.frames.len().checked_sub(1) else {
      return 0;
    };
    let current = &self.frames[current_idx];
    if current.global_names.contains(var_name) {
      return 0;
    }
    if current.nonlocal_names.contains(var_name) {
      for (idx, frame) in self.frames[..current_idx].iter().enumerate().rev() {
        if frame.kind == PythonScopeKind::Function {
          return idx;
        }
      }
    }
    current_idx
  }
}

/// Analyze one Python source file for bounded intra-file literal-to-sink flow.
///
/// This is deliberately not a taint engine. It tracks only simple
/// `name = "literal"` assignments within Python block scopes. Resolution
/// starts in the current module/function/class scope and walks outward for
/// closure reads only when the current scope does not bind the name. Class
/// scopes are not used as closure providers for nested methods because
/// Python does not resolve bare method names through class attributes.
pub fn analyze_python_source(
  path: impl AsRef<Path>,
  content: &str,
  catalog: &PythonSinkCatalog,
) -> Vec<SymbolReachabilityFinding> {
  analyze_python_semantic(path, content, catalog).symbol_reachability
}

fn analyze_python_semantic(
  path: impl AsRef<Path>,
  content: &str,
  catalog: &PythonSinkCatalog,
) -> SemanticScanResult {
  let path = path.as_ref().to_string_lossy().into_owned();
  let grep = SupportLang::Python.ast_grep(content);
  let root = grep.root();
  let mut model = ScopedLiteralModel::default();
  let mut result = SemanticScanResult::new();
  let provider_matcher =
    default_provider_matcher().filter(|matcher| matcher.prefilter.is_match(content));
  let source_matcher =
    default_source_matcher().filter(|matcher| matcher.prefilter.is_match(content));
  let context = PythonWalkContext {
    path: &path,
    content,
    catalog,
    provider_matcher,
    source_matcher,
  };

  walk_scope(
    &context,
    &root,
    PythonScopeKind::Module,
    &mut model,
    &mut result,
  );

  dedup_source_evidence(&mut result);
  result
}

#[derive(Clone, Copy)]
struct PythonWalkContext<'a> {
  path: &'a str,
  content: &'a str,
  catalog: &'a PythonSinkCatalog,
  provider_matcher: Option<&'a ProviderMatcher>,
  source_matcher: Option<&'a SourceMatcher>,
}

fn walk_scope(
  context: &PythonWalkContext<'_>,
  node: &PythonNode<'_>,
  kind: PythonScopeKind,
  model: &mut ScopedLiteralModel,
  result: &mut SemanticScanResult,
) {
  model.push_scope(kind, node);
  let body = node.field("body").unwrap_or_else(|| node.clone());
  for child in body.children().filter(|child| child.is_named()) {
    walk_node(context, &child, model, result);
  }
  model.pop_scope();
}

fn walk_node(
  context: &PythonWalkContext<'_>,
  node: &PythonNode<'_>,
  model: &mut ScopedLiteralModel,
  result: &mut SemanticScanResult,
) {
  match node.kind().as_ref() {
    "function_definition" => walk_scope(context, node, PythonScopeKind::Function, model, result),
    "class_definition" => {
      walk_scope(context, node, PythonScopeKind::Class, model, result);
    }
    "assignment" => {
      analyze_attribute_assignment_sink(
        context.path,
        context.content,
        node,
        context.catalog,
        context.provider_matcher,
        model,
        result,
      );
      let source = bind_source_assignment(context, node, result);
      model.update_assignment(node, source);
    }
    "call" => {
      record_source_read(context, node, None, result);
      analyze_call_sink(
        context.path,
        context.content,
        node,
        context.catalog,
        context.provider_matcher,
        model,
        result,
      );
      walk_child_nodes(context, node, model, result);
    }
    "subscript" => {
      record_source_read(context, node, None, result);
      walk_child_nodes(context, node, model, result);
    }
    _ => walk_child_nodes(context, node, model, result),
  }
}

fn walk_child_nodes(
  context: &PythonWalkContext<'_>,
  node: &PythonNode<'_>,
  model: &mut ScopedLiteralModel,
  result: &mut SemanticScanResult,
) {
  for child in node.children().filter(|child| child.is_named()) {
    walk_node(context, &child, model, result);
  }
}

fn bind_source_assignment(
  context: &PythonWalkContext<'_>,
  node: &PythonNode<'_>,
  result: &mut SemanticScanResult,
) -> Option<CredentialSourceEvidence> {
  let left = node.field("left")?;
  let var_name = simple_identifier_text(&left)?;
  let right = node.field("right")?;
  record_source_read(context, &right, Some(&var_name), result)
}

fn record_source_read(
  context: &PythonWalkContext<'_>,
  node: &PythonNode<'_>,
  variable: Option<&str>,
  result: &mut SemanticScanResult,
) -> Option<CredentialSourceEvidence> {
  let matcher = context.source_matcher?;
  let fallback_variable;
  let variable = if let Some(variable) = variable {
    variable
  } else {
    fallback_variable = node.text().to_string();
    &fallback_variable
  };
  let matched = matcher.source_read(
    context.path,
    context.content,
    node,
    variable,
    ProviderLanguageId::Python,
  )?;
  let evidence = matched.evidence;
  result.source_evidence.push(evidence.clone());
  (matched.kind != ReadShapeKind::DotenvLoad).then_some(evidence)
}

fn analyze_attribute_assignment_sink(
  path: &str,
  content: &str,
  node: &PythonNode<'_>,
  catalog: &PythonSinkCatalog,
  provider_matcher: Option<&ProviderMatcher>,
  model: &ScopedLiteralModel,
  result: &mut SemanticScanResult,
) {
  let Some(left) = node.field("left") else {
    return;
  };
  let Some(right) = node.field("right") else {
    return;
  };
  let Some(var_name) = simple_identifier_text(&right) else {
    return;
  };
  let left_text = left.text();
  let Some((object_name, attribute_name)) = split_attribute_text(left_text.as_ref()) else {
    return;
  };
  if let Some(matcher) = provider_matcher {
    for candidate in matcher.assignment_candidates(
      path,
      content,
      left_text.as_ref(),
      &var_name,
      node.start_pos().line() + 1,
      node.start_pos().column(node) + 1,
    ) {
      emit_if_active_with_meta(
        path,
        &candidate.value_name,
        model,
        candidate.sink,
        result,
        Some(&candidate.meta),
      );
    }
  }
  if !catalog.attribute_sinks.iter().any(|sink| {
    sink.attribute_name == attribute_name
      && sink
        .object_name
        .as_deref()
        .is_none_or(|expected| expected == object_name)
  }) {
    return;
  }

  emit_if_active(
    path,
    &var_name,
    model,
    SinkEvidence {
      file_path: path.to_string(),
      line: node.start_pos().line() + 1,
      column: node.start_pos().column(node) + 1,
      callee_text: left_text.to_string(),
      arg_position: None,
      arg_name: Some(attribute_name.to_string()),
      sink_kind: SinkKind::AttributeAssignment,
    },
    result,
  );
}

fn analyze_call_sink(
  path: &str,
  content: &str,
  node: &PythonNode<'_>,
  catalog: &PythonSinkCatalog,
  provider_matcher: Option<&ProviderMatcher>,
  model: &ScopedLiteralModel,
  result: &mut SemanticScanResult,
) {
  let Some(function) = node.field("function") else {
    return;
  };
  let Some(arguments) = node.field("arguments") else {
    return;
  };
  let callee_text = function.text().to_string();
  let line = node.start_pos().line() + 1;
  let column = node.start_pos().column(node) + 1;

  if let Some(matcher) = provider_matcher {
    for candidate in matcher.call_candidates(path, content, &callee_text, &arguments, line, column)
    {
      emit_if_active_with_meta(
        path,
        &candidate.value_name,
        model,
        candidate.sink,
        result,
        Some(&candidate.meta),
      );
    }
  }

  if method_call_matches_catalog(&callee_text, catalog) {
    for (position, argument) in top_level_argument_nodes(&arguments).into_iter().enumerate() {
      let Some(var_name) = simple_identifier_text(&argument) else {
        continue;
      };
      emit_if_active(
        path,
        &var_name,
        model,
        SinkEvidence {
          file_path: path.to_string(),
          line,
          column,
          callee_text: callee_text.clone(),
          arg_position: Some(position),
          arg_name: None,
          sink_kind: SinkKind::MethodCall,
        },
        result,
      );
    }
  }

  for (position, argument) in top_level_argument_nodes(&arguments).into_iter().enumerate() {
    if argument.kind() != "keyword_argument" {
      continue;
    }
    let Some(name) = argument.field("name").map(|node| node.text().to_string()) else {
      continue;
    };
    if !catalog.credential_kwarg_sinks.iter().any(|sink| {
      sink.keyword_name == name
        && sink
          .callee_text
          .as_deref()
          .is_none_or(|expected| expected == callee_text)
    }) {
      continue;
    }
    let Some(value) = argument.field("value") else {
      continue;
    };
    let Some(var_name) = simple_identifier_text(&value) else {
      continue;
    };
    emit_if_active(
      path,
      &var_name,
      model,
      SinkEvidence {
        file_path: path.to_string(),
        line,
        column,
        callee_text: callee_text.clone(),
        arg_position: Some(position),
        arg_name: Some(name),
        sink_kind: SinkKind::CredentialKwarg,
      },
      result,
    );
  }
}

fn emit_if_active(
  path: &str,
  var_name: &str,
  model: &ScopedLiteralModel,
  sink: SinkEvidence,
  result: &mut SemanticScanResult,
) {
  emit_if_active_with_meta(path, var_name, model, sink, result, None);
}

fn emit_if_active_with_meta(
  path: &str,
  var_name: &str,
  model: &ScopedLiteralModel,
  sink: SinkEvidence,
  result: &mut SemanticScanResult,
  meta: Option<&ProviderFindingMeta>,
) {
  let Some(active) = model.resolve(var_name) else {
    return;
  };
  match &active.origin {
    ActiveCredentialOrigin::Literal(literal) => {
      result.symbol_reachability.push(SymbolReachabilityFinding {
        detector: DETECTOR_SYMBOL_REACHABILITY.into(),
        confidence: SemanticConfidence::BoundedSymbolFlow,
        provider_id: meta.map(|meta| meta.provider_id.clone()),
        provider_rule_ids: meta
          .map(|meta| meta.provider_rule_ids.clone())
          .unwrap_or_default(),
        credential_kind: meta.map(|meta| meta.credential_kind.clone()),
        sdk: meta.map(|meta| meta.sdk.clone()),
        provider_shape_id: meta.map(|meta| meta.provider_shape_id.clone()),
        source: SourceEvidence {
          file_path: path.to_string(),
          line: active.line,
          column: active.column,
          variable: var_name.to_string(),
          literal: literal.clone(),
        },
        sink,
        flow: Vec::new(),
      });
    }
    ActiveCredentialOrigin::Source(source) => {
      if meta.is_some_and(|meta| source.provider_id.as_deref() == Some(meta.provider_id.as_str())) {
        result
          .confirmed_flows
          .push(confirmed_flow_from_source(source, var_name, sink, meta));
      }
    }
  }
}

fn confirmed_flow_from_source(
  source: &CredentialSourceEvidence,
  var_name: &str,
  sink: SinkEvidence,
  meta: Option<&ProviderFindingMeta>,
) -> ConfirmedCredentialFlowFinding {
  let provider_id = source.provider_id.clone();
  ConfirmedCredentialFlowFinding {
    detector: DETECTOR_CONFIRMED_CREDENTIAL_FLOW.into(),
    confidence: SemanticConfidence::ConfirmedCredentialFlow,
    provider_id,
    source: source.clone(),
    sink: sink.clone(),
    intermediate_vars: vec![var_name.to_string()],
    flow: vec![
      FlowStep {
        kind: FlowStepKind::Source,
        file_path: source.file_path.clone(),
        line: source.line,
        column: source.column,
        symbol: source.variable.clone(),
      },
      FlowStep {
        kind: FlowStepKind::Definition,
        file_path: source.file_path.clone(),
        line: source.line,
        column: source.column,
        symbol: var_name.to_string(),
      },
      FlowStep {
        kind: FlowStepKind::Import,
        file_path: source.file_path.clone(),
        line: source.line,
        column: source.column,
        symbol: meta
          .map(|meta| meta.provider_id.clone())
          .unwrap_or_else(|| "local".into()),
      },
      FlowStep {
        kind: FlowStepKind::Sink,
        file_path: sink.file_path.clone(),
        line: sink.line,
        column: sink.column,
        symbol: var_name.to_string(),
      },
    ],
  }
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
struct PythonScopeDirectives {
  global_names: HashSet<String>,
  nonlocal_names: HashSet<String>,
}

fn collect_scope_local_bindings(
  node: &PythonNode<'_>,
  directives: &PythonScopeDirectives,
) -> HashSet<String> {
  let mut bindings = HashSet::new();
  if node.kind() == "function_definition" {
    if let Some(parameters) = node.field("parameters") {
      collect_identifier_descendants(&parameters, &mut bindings);
    }
  }
  let body = node.field("body").unwrap_or_else(|| node.clone());
  collect_bindings_in_current_scope(&body, &mut bindings);
  for name in directives
    .global_names
    .iter()
    .chain(&directives.nonlocal_names)
  {
    bindings.remove(name);
  }
  bindings
}

fn collect_python_scope_directives(node: &PythonNode<'_>) -> PythonScopeDirectives {
  let mut directives = PythonScopeDirectives::default();
  let body = node.field("body").unwrap_or_else(|| node.clone());
  collect_directives_in_current_scope(&body, &mut directives);
  directives
}

fn collect_directives_in_current_scope(
  node: &PythonNode<'_>,
  directives: &mut PythonScopeDirectives,
) {
  for child in node.children().filter(|child| child.is_named()) {
    match child.kind().as_ref() {
      "function_definition" | "class_definition" => continue,
      "global_statement" => collect_identifier_descendants(&child, &mut directives.global_names),
      "nonlocal_statement" => {
        collect_identifier_descendants(&child, &mut directives.nonlocal_names);
      }
      _ => collect_directives_in_current_scope(&child, directives),
    }
  }
}

fn collect_bindings_in_current_scope(node: &PythonNode<'_>, bindings: &mut HashSet<String>) {
  for child in node.children().filter(|child| child.is_named()) {
    match child.kind().as_ref() {
      "function_definition" | "class_definition" => {
        if let Some(name) = child
          .field("name")
          .and_then(|name| simple_identifier_text(&name))
        {
          bindings.insert(name);
        }
      }
      "assignment" => {
        if let Some(name) = child
          .field("left")
          .and_then(|left| simple_identifier_text(&left))
        {
          bindings.insert(name);
        }
        collect_bindings_in_current_scope(&child, bindings);
      }
      _ => collect_bindings_in_current_scope(&child, bindings),
    }
  }
}

fn collect_identifier_descendants(node: &PythonNode<'_>, bindings: &mut HashSet<String>) {
  for child in node.children().filter(|child| child.is_named()) {
    if let Some(name) = simple_identifier_text(&child) {
      bindings.insert(name);
    } else {
      collect_identifier_descendants(&child, bindings);
    }
  }
}

fn simple_identifier_text(node: &PythonNode<'_>) -> Option<String> {
  (node.kind() == "identifier").then(|| node.text().to_string())
}

fn split_attribute_text(text: &str) -> Option<(&str, &str)> {
  let (object_name, attribute_name) = text.rsplit_once('.')?;
  if object_name.is_empty() || attribute_name.is_empty() {
    return None;
  }
  Some((object_name, attribute_name))
}

fn method_call_matches_catalog(callee_text: &str, catalog: &PythonSinkCatalog) -> bool {
  let Some((_, method_name)) = split_attribute_text(callee_text) else {
    return false;
  };
  catalog
    .call_sinks
    .iter()
    .any(|sink| sink.method_name == method_name)
}

fn top_level_argument_nodes<'a>(arguments: &PythonNode<'a>) -> Vec<PythonNode<'a>> {
  arguments
    .children()
    .filter(|child| child.is_named())
    .collect()
}

fn strip_simple_python_string_literal(text: &str) -> Option<String> {
  let text = text.trim();
  let quote = text.chars().next()?;
  if quote != '\'' && quote != '"' {
    return None;
  }
  if !text.ends_with(quote) || text.len() < 2 {
    return None;
  }
  Some(text[quote.len_utf8()..text.len() - quote.len_utf8()].to_string())
}

pub fn analyze_source(
  path: impl AsRef<Path>,
  content: &str,
  sink_evidence: SinkEvidenceMode,
) -> SemanticScanResult {
  let path_ref = path.as_ref();
  let mut result = SemanticScanResult::new();
  match language_for_path(path_ref) {
    Some(SupportLang::Python) => {
      result = analyze_python_semantic(path_ref, content, &PythonSinkCatalog::default());
      if sink_evidence == SinkEvidenceMode::On {
        result.sink_evidence = analyze_python_sink_evidence(path_ref, content);
      }
    }
    Some(SupportLang::JavaScript) | Some(SupportLang::TypeScript) | Some(SupportLang::Tsx) => {
      result = analyze_ast_language(
        path_ref,
        content,
        AstLang::JavaScript,
        &JavaScriptBindingStrategy,
        sink_evidence,
      );
    }
    Some(SupportLang::Go) => {
      result = analyze_ast_language(
        path_ref,
        content,
        AstLang::Go,
        &GoBindingStrategy,
        sink_evidence,
      );
    }
    Some(SupportLang::Java) => {
      result = analyze_ast_language(
        path_ref,
        content,
        AstLang::Java,
        &JavaBindingStrategy,
        sink_evidence,
      );
    }
    _ => {}
  }
  dedup_sink_evidence(&mut result);
  dedup_source_evidence(&mut result);
  dedup_confirmed_flows(&mut result);
  result
}

pub fn content_has_sink_token(content: &str) -> bool {
  [
    "login",
    "connect",
    "authenticate",
    "Authorization",
    "Credentials",
    "NewStaticCredentialsProvider",
    "BasicAWSCredentials",
    "S3Client",
    "fromStatic",
    "Octokit",
    "stripe",
    "SecretAccessKey",
    "api_key",
    "aws_secret_access_key",
    "setRequestProperty",
    ".Set(",
  ]
  .iter()
  .any(|token| content.contains(token))
}

pub fn content_has_source_token(content: &str) -> bool {
  default_source_matcher().is_some_and(|matcher| matcher.prefilter.is_match(content))
}

pub fn content_has_module_graph_token(content: &str) -> bool {
  [
    "import", "export", "from ", "package ", "var ", "const ", "String ", "=",
  ]
  .iter()
  .any(|token| content.contains(token))
}

pub fn analyze_sources_with_module_graph(
  files: &[(PathBuf, String)],
  sink_evidence: SinkEvidenceMode,
) -> SemanticScanResult {
  let mut result = SemanticScanResult::new();
  let mut relevant: HashMap<String, ModuleInfo> = HashMap::new();
  let mut target_paths = HashSet::new();
  for (path, content) in files
    .iter()
    .filter(|(_, content)| content_has_sink_token(content))
  {
    let Some(info) = collect_module_info(path, content) else {
      continue;
    };
    for import in info.imports.values() {
      if let Some(target) = resolve_module_path(&import.module_spec, &info.path, files, info.lang) {
        target_paths.insert(target);
      }
    }
    relevant.insert(path.to_string_lossy().into_owned(), info);
  }
  for target_path in target_paths {
    let Some((path, content)) = files.iter().find(|(path, _)| *path == target_path) else {
      continue;
    };
    if let std::collections::hash_map::Entry::Vacant(entry) =
      relevant.entry(path.to_string_lossy().into_owned())
    {
      if let Some(info) = collect_module_info(path, content) {
        entry.insert(info);
      }
    }
  }

  for info in relevant.values().filter(|info| !info.sinks.is_empty()) {
    for sink in &info.sinks {
      let Some(import_ref) = imported_symbol_for_sink(info, &sink.value_text) else {
        if sink_evidence == SinkEvidenceMode::On {
          result.sink_evidence.push(sink.clone());
        }
        continue;
      };
      let Some(target_path) =
        resolve_module_path(&import_ref.module_spec, &info.path, files, info.lang)
      else {
        result.warnings.push(SemanticWarning {
          code: "CROSS_FILE_UNRESOLVED_IMPORT".into(),
          message: format!(
            "could not resolve import {} from {}",
            import_ref.module_spec,
            info.path.display()
          ),
        });
        continue;
      };
      let target_key = target_path.to_string_lossy().into_owned();
      let Some(target) = relevant.get(&target_key) else {
        continue;
      };
      if target.has_reexport(&import_ref.imported_name) {
        result.warnings.push(SemanticWarning {
          code: "CROSS_FILE_DEPTH_CAP".into(),
          message: format!(
            "skipped {} via {}: one-hop cross-file flow does not follow re-exports",
            import_ref.imported_name,
            target.path.display()
          ),
        });
        continue;
      }
      let Some(export) = target.exports.get(&import_ref.imported_name) else {
        continue;
      };
      match &export.definition.origin {
        ActiveCredentialOrigin::Literal(literal) => {
          result.symbol_reachability.push(SymbolReachabilityFinding {
            detector: DETECTOR_SYMBOL_REACHABILITY.into(),
            confidence: SemanticConfidence::BoundedSymbolFlow,
            provider_id: sink.provider_id.clone(),
            provider_rule_ids: sink.provider_rule_ids.clone(),
            credential_kind: sink.credential_kind.clone(),
            sdk: sink.sdk.clone(),
            provider_shape_id: sink.provider_shape_id.clone(),
            source: SourceEvidence {
              file_path: export.definition_file.clone(),
              line: export.definition.line,
              column: export.definition.column,
              variable: export.symbol.clone(),
              literal: literal.clone(),
            },
            sink: sink.sink.clone(),
            flow: cross_file_literal_flow(export, info, &import_ref, sink),
          });
        }
        ActiveCredentialOrigin::Source(source) => {
          result.source_evidence.push(source.clone());
          if source.provider_id == sink.provider_id {
            result.confirmed_flows.push(cross_file_confirmed_flow(
              source,
              export,
              info,
              &import_ref,
              sink,
            ));
          }
        }
      }
    }
  }
  dedup_symbol_reachability(&mut result);
  dedup_sink_evidence(&mut result);
  dedup_source_evidence(&mut result);
  dedup_confirmed_flows(&mut result);
  result
}

fn language_for_path(path: &Path) -> Option<SupportLang> {
  match path
    .extension()
    .and_then(|extension| extension.to_str())
    .map(|extension| extension.to_ascii_lowercase())
    .as_deref()
  {
    Some("py") => Some(SupportLang::Python),
    Some("js") | Some("mjs") | Some("cjs") | Some("jsx") => Some(SupportLang::JavaScript),
    Some("ts") | Some("mts") | Some("cts") => Some(SupportLang::TypeScript),
    Some("tsx") => Some(SupportLang::Tsx),
    Some("go") => Some(SupportLang::Go),
    Some("java") => Some(SupportLang::Java),
    _ => None,
  }
}

#[derive(Debug, Clone)]
struct ModuleInfo {
  path: PathBuf,
  lang: SupportLang,
  exports: HashMap<String, ModuleExport>,
  imports: HashMap<String, ModuleImport>,
  reexports: HashSet<String>,
  sinks: Vec<SinkEvidenceOnlyFinding>,
}

impl ModuleInfo {
  fn new(path: &Path, lang: SupportLang) -> Self {
    Self {
      path: path.to_path_buf(),
      lang,
      exports: HashMap::new(),
      imports: HashMap::new(),
      reexports: HashSet::new(),
      sinks: Vec::new(),
    }
  }

  fn has_reexport(&self, symbol: &str) -> bool {
    self.reexports.contains(symbol) || self.imports.contains_key(symbol)
  }
}

#[derive(Debug, Clone)]
struct ModuleExport {
  symbol: String,
  definition_file: String,
  export_file: String,
  definition: ActiveCredential,
  export_line: usize,
  export_column: usize,
}

#[derive(Debug, Clone)]
struct ModuleImport {
  local_name: String,
  imported_name: String,
  module_spec: String,
  line: usize,
  column: usize,
}

#[derive(Debug, Clone)]
struct ImportedSinkRef {
  local_name: String,
  imported_name: String,
  module_spec: String,
  line: usize,
  column: usize,
}

fn collect_module_info(path: &Path, content: &str) -> Option<ModuleInfo> {
  let lang = language_for_path(path)?;
  let grep = lang.ast_grep(content);
  let root = grep.root();
  let mut info = ModuleInfo::new(path, lang);
  let provider_matcher =
    default_provider_matcher().filter(|matcher| matcher.prefilter.is_match(content));
  let source_matcher =
    default_source_matcher().filter(|matcher| matcher.prefilter.is_match(content));
  for child in root.children().filter(|child| child.is_named()) {
    collect_module_node(
      &mut info,
      content,
      provider_matcher,
      source_matcher,
      &child,
      true,
    );
  }
  Some(info)
}

fn collect_module_node(
  info: &mut ModuleInfo,
  content: &str,
  provider_matcher: Option<&ProviderMatcher>,
  source_matcher: Option<&SourceMatcher>,
  node: &PythonNode<'_>,
  top_level: bool,
) {
  let kind = node.kind();
  collect_imports(info, node);
  collect_exports(info, content, source_matcher, node, top_level);
  if info.lang == SupportLang::Python && node.kind() == "call" {
    // P3 consistency invariant: module-graph sink enumeration uses the
    // same provider DB matcher as file-level scanning. The import
    // resolution, one-hop depth cap, and flow construction below are
    // intentionally unchanged.
    info.sinks.extend(parse_python_cross_file_sinks(
      &info.path.to_string_lossy(),
      node,
    ));
    if let Some(matcher) = provider_matcher {
      info.sinks.extend(matcher.line_sink_evidence(
        &info.path.to_string_lossy(),
        content,
        node.text().as_ref(),
        node.start_pos().line() + 1,
      ));
    }
  } else if let Some(ast_lang) = ast_lang_for_support(info.lang) {
    let strategy: &dyn BindingStrategy = match ast_lang {
      AstLang::JavaScript => &JavaScriptBindingStrategy,
      AstLang::Go => &GoBindingStrategy,
      AstLang::Java => &JavaBindingStrategy,
    };
    if strategy.sink_node_kinds().contains(&kind.as_ref()) {
      info.sinks.extend(parse_generic_sink_uses(
        &info.path.to_string_lossy(),
        node.text().as_ref(),
        node.start_pos().line() + 1,
        ast_lang,
      ));
      if let Some(matcher) = provider_matcher {
        info.sinks.extend(matcher.ast_sink_uses(
          &info.path.to_string_lossy(),
          content,
          node,
          ast_lang,
        ));
      }
    }
  }
  let child_top_level = top_level
    && !matches!(
      kind.as_ref(),
      "function_definition"
        | "class_definition"
        | "function_declaration"
        | "function"
        | "function_expression"
        | "arrow_function"
        | "method_declaration"
        | "constructor_declaration"
        | "block"
        | "statement_block"
    );
  for child in node.children().filter(|child| child.is_named()) {
    collect_module_node(
      info,
      content,
      provider_matcher,
      source_matcher,
      &child,
      child_top_level,
    );
  }
}

fn parse_python_cross_file_sinks(
  path: &str,
  node: &PythonNode<'_>,
) -> Vec<SinkEvidenceOnlyFinding> {
  let Some(function) = node.field("function") else {
    return Vec::new();
  };
  let Some(arguments) = node.field("arguments") else {
    return Vec::new();
  };
  let callee_text = function.text().to_string();
  if !["client.login", "client.connect", "client.authenticate"]
    .iter()
    .any(|expected| *expected == callee_text || callee_text.ends_with(".login"))
  {
    return Vec::new();
  }
  top_level_argument_nodes(&arguments)
    .into_iter()
    .enumerate()
    .filter_map(|(position, argument)| {
      let value_text = simple_identifier_text(&argument)?;
      Some(SinkEvidenceOnlyFinding {
        detector: DETECTOR_SINK_EVIDENCE.into(),
        confidence: SemanticConfidence::SyntaxEvidenceOnly,
        provider_id: None,
        provider_rule_ids: Vec::new(),
        credential_kind: None,
        sdk: None,
        provider_shape_id: None,
        sink: SinkEvidence {
          file_path: path.to_string(),
          line: node.start_pos().line() + 1,
          column: node.start_pos().column(node) + 1,
          callee_text: callee_text.clone(),
          arg_position: Some(position),
          arg_name: None,
          sink_kind: SinkKind::MethodCall,
        },
        value_text,
      })
    })
    .collect()
}

fn ast_lang_for_support(lang: SupportLang) -> Option<AstLang> {
  match lang {
    SupportLang::JavaScript | SupportLang::TypeScript | SupportLang::Tsx => {
      Some(AstLang::JavaScript)
    }
    SupportLang::Go => Some(AstLang::Go),
    SupportLang::Java => Some(AstLang::Java),
    _ => None,
  }
}

fn collect_imports(info: &mut ModuleInfo, node: &PythonNode<'_>) {
  match info.lang {
    SupportLang::Python if node.kind() == "import_from_statement" => {
      for import in parse_python_import_from(node) {
        info.imports.insert(import.local_name.clone(), import);
      }
    }
    SupportLang::JavaScript | SupportLang::TypeScript | SupportLang::Tsx
      if node.kind() == "import_statement" =>
    {
      for import in parse_js_import(node) {
        info.imports.insert(import.local_name.clone(), import);
      }
    }
    SupportLang::JavaScript | SupportLang::TypeScript | SupportLang::Tsx
      if node.kind() == "export_statement" =>
    {
      for import in parse_js_reexport(node) {
        info.reexports.insert(import.imported_name.clone());
        info.imports.insert(import.local_name.clone(), import);
      }
    }
    SupportLang::Go if node.kind() == "import_spec" => {
      if let Some(import) = parse_go_import(node) {
        info.imports.insert(import.local_name.clone(), import);
      }
    }
    SupportLang::Java if node.kind() == "import_declaration" => {
      if let Some(import) = parse_java_import(node) {
        info.imports.insert(import.local_name.clone(), import);
      }
    }
    _ => {}
  }
}

fn collect_exports(
  info: &mut ModuleInfo,
  content: &str,
  source_matcher: Option<&SourceMatcher>,
  node: &PythonNode<'_>,
  top_level: bool,
) {
  match info.lang {
    SupportLang::Python if top_level && node.kind() == "assignment" => {
      let Some(left) = node.field("left") else {
        return;
      };
      let Some(right) = node.field("right") else {
        return;
      };
      for name in binding_names_from_pattern(&left) {
        if let Some(source) = module_source_evidence(
          info,
          content,
          source_matcher,
          &right,
          &name,
          ProviderLanguageId::Python,
        ) {
          add_source_export(info, &name, source, node, &name);
        } else if let Some(literal) = literal_from_node(&right) {
          add_literal_export(info, &name, &literal, node, &name);
        }
      }
    }
    SupportLang::JavaScript | SupportLang::TypeScript | SupportLang::Tsx
      if node.kind() == "variable_declarator" && has_ancestor_kind(node, "export_statement") =>
    {
      for assignment in parse_js_assignments(node) {
        let language = if matches!(info.lang, SupportLang::TypeScript | SupportLang::Tsx) {
          ProviderLanguageId::TypeScript
        } else {
          ProviderLanguageId::JavaScript
        };
        if let Some(source) =
          module_source_assignment(info, content, source_matcher, node, &assignment, language)
        {
          add_source_export(info, &assignment.name, source, node, &assignment.name);
        } else if let Some(literal) = assignment.literal {
          add_literal_export(info, &assignment.name, &literal, node, &assignment.name);
        }
      }
    }
    SupportLang::Go if top_level && node.kind() == "var_spec" => {
      for assignment in parse_go_assignments(node) {
        if starts_uppercase(&assignment.name) {
          if let Some(source) = module_source_assignment(
            info,
            content,
            source_matcher,
            node,
            &assignment,
            ProviderLanguageId::Go,
          ) {
            add_source_export(info, &assignment.name, source, node, &assignment.name);
          } else if let Some(literal) = assignment.literal {
            add_literal_export(info, &assignment.name, &literal, node, &assignment.name);
          }
        }
      }
    }
    SupportLang::Java if node.kind() == "variable_declarator" && java_declarator_is_field(node) => {
      for assignment in parse_java_assignments(node) {
        let class = nearest_java_class_name(node).unwrap_or_else(|| {
          info
            .path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("Unknown")
            .to_string()
        });
        if let Some(source) = module_source_assignment(
          info,
          content,
          source_matcher,
          node,
          &assignment,
          ProviderLanguageId::Java,
        ) {
          add_source_export(
            info,
            &format!("{class}.{}", assignment.name),
            source.clone(),
            node,
            &assignment.name,
          );
          add_source_export(info, &assignment.name, source, node, &assignment.name);
        } else if let Some(literal) = assignment.literal {
          add_literal_export(
            info,
            &format!("{class}.{}", assignment.name),
            &literal,
            node,
            &assignment.name,
          );
          add_literal_export(info, &assignment.name, &literal, node, &assignment.name);
        }
      }
    }
    _ => {}
  }
}

fn module_source_assignment(
  info: &ModuleInfo,
  content: &str,
  source_matcher: Option<&SourceMatcher>,
  node: &PythonNode<'_>,
  assignment: &AstAssignment,
  language: ProviderLanguageId,
) -> Option<CredentialSourceEvidence> {
  if assignment.index != 0 {
    return None;
  }
  let source_node = ast_source_assignment_node(node, provider_language_to_ast(language)?)?;
  module_source_evidence(
    info,
    content,
    source_matcher,
    &source_node,
    &assignment.name,
    language,
  )
}

fn module_source_evidence(
  info: &ModuleInfo,
  content: &str,
  source_matcher: Option<&SourceMatcher>,
  node: &PythonNode<'_>,
  variable: &str,
  language: ProviderLanguageId,
) -> Option<CredentialSourceEvidence> {
  source_matcher?
    .source_read(
      &info.path.to_string_lossy(),
      content,
      node,
      variable,
      language,
    )
    .and_then(|matched| (matched.kind != ReadShapeKind::DotenvLoad).then_some(matched.evidence))
}

fn provider_language_to_ast(language: ProviderLanguageId) -> Option<AstLang> {
  match language {
    ProviderLanguageId::JavaScript | ProviderLanguageId::TypeScript => Some(AstLang::JavaScript),
    ProviderLanguageId::Go => Some(AstLang::Go),
    ProviderLanguageId::Java => Some(AstLang::Java),
    ProviderLanguageId::Python => None,
  }
}

fn add_literal_export(
  info: &mut ModuleInfo,
  export_name: &str,
  literal: &str,
  node: &PythonNode<'_>,
  source_symbol: &str,
) {
  info.exports.insert(
    export_name.to_string(),
    ModuleExport {
      symbol: source_symbol.to_string(),
      definition_file: info.path.to_string_lossy().into_owned(),
      export_file: info.path.to_string_lossy().into_owned(),
      definition: ActiveCredential {
        line: node.start_pos().line() + 1,
        column: node.start_pos().column(node) + 1,
        origin: ActiveCredentialOrigin::Literal(Secret::new(literal.to_string())),
      },
      export_line: node.start_pos().line() + 1,
      export_column: node.start_pos().column(node) + 1,
    },
  );
}

fn add_source_export(
  info: &mut ModuleInfo,
  export_name: &str,
  source: CredentialSourceEvidence,
  node: &PythonNode<'_>,
  source_symbol: &str,
) {
  info.exports.insert(
    export_name.to_string(),
    ModuleExport {
      symbol: source_symbol.to_string(),
      definition_file: source.file_path.clone(),
      export_file: info.path.to_string_lossy().into_owned(),
      definition: ActiveCredential {
        line: source.line,
        column: source.column,
        origin: ActiveCredentialOrigin::Source(source),
      },
      export_line: node.start_pos().line() + 1,
      export_column: node.start_pos().column(node) + 1,
    },
  );
}

fn imported_symbol_for_sink(info: &ModuleInfo, value_text: &str) -> Option<ImportedSinkRef> {
  let normalized = normalize_value_name(value_text);
  if let Some(import) = info.imports.get(&normalized) {
    return Some(ImportedSinkRef {
      local_name: import.local_name.clone(),
      imported_name: import.imported_name.clone(),
      module_spec: import.module_spec.clone(),
      line: import.line,
      column: import.column,
    });
  }
  if let Some((qualifier, member)) = normalized.split_once('.') {
    if let Some(import) = info.imports.get(qualifier) {
      let imported_name = if import.imported_name == "*" || import.imported_name == "default" {
        member.to_string()
      } else {
        format!("{}.{}", import.imported_name, member)
      };
      return Some(ImportedSinkRef {
        local_name: qualifier.to_string(),
        imported_name,
        module_spec: import.module_spec.clone(),
        line: import.line,
        column: import.column,
      });
    }
  }
  None
}

fn cross_file_literal_flow(
  export: &ModuleExport,
  info: &ModuleInfo,
  import_ref: &ImportedSinkRef,
  sink: &SinkEvidenceOnlyFinding,
) -> Vec<FlowStep> {
  vec![
    FlowStep {
      kind: FlowStepKind::Definition,
      file_path: export.definition_file.clone(),
      line: export.definition.line,
      column: export.definition.column,
      symbol: export.symbol.clone(),
    },
    FlowStep {
      kind: FlowStepKind::Export,
      file_path: export.export_file.clone(),
      line: export.export_line,
      column: export.export_column,
      symbol: export.symbol.clone(),
    },
    FlowStep {
      kind: FlowStepKind::Import,
      file_path: info.path.to_string_lossy().into_owned(),
      line: import_ref.line,
      column: import_ref.column,
      symbol: import_ref.local_name.clone(),
    },
    FlowStep {
      kind: FlowStepKind::Sink,
      file_path: sink.sink.file_path.clone(),
      line: sink.sink.line,
      column: sink.sink.column,
      symbol: normalize_value_name(&sink.value_text),
    },
  ]
}

fn cross_file_confirmed_flow(
  source: &CredentialSourceEvidence,
  export: &ModuleExport,
  info: &ModuleInfo,
  import_ref: &ImportedSinkRef,
  sink: &SinkEvidenceOnlyFinding,
) -> ConfirmedCredentialFlowFinding {
  ConfirmedCredentialFlowFinding {
    detector: DETECTOR_CONFIRMED_CREDENTIAL_FLOW.into(),
    confidence: SemanticConfidence::ConfirmedCredentialFlow,
    provider_id: source.provider_id.clone(),
    source: source.clone(),
    sink: sink.sink.clone(),
    intermediate_vars: vec![import_ref.local_name.clone()],
    flow: vec![
      FlowStep {
        kind: FlowStepKind::Source,
        file_path: source.file_path.clone(),
        line: source.line,
        column: source.column,
        symbol: source.variable.clone(),
      },
      FlowStep {
        kind: FlowStepKind::Export,
        file_path: export.export_file.clone(),
        line: export.export_line,
        column: export.export_column,
        symbol: export.symbol.clone(),
      },
      FlowStep {
        kind: FlowStepKind::Import,
        file_path: info.path.to_string_lossy().into_owned(),
        line: import_ref.line,
        column: import_ref.column,
        symbol: import_ref.local_name.clone(),
      },
      FlowStep {
        kind: FlowStepKind::Sink,
        file_path: sink.sink.file_path.clone(),
        line: sink.sink.line,
        column: sink.sink.column,
        symbol: normalize_value_name(&sink.value_text),
      },
    ],
  }
}

fn resolve_module_path(
  spec: &str,
  importer: &Path,
  files: &[(PathBuf, String)],
  lang: SupportLang,
) -> Option<PathBuf> {
  let base = importer.parent().unwrap_or_else(|| Path::new(""));
  let candidates = module_path_candidates(spec, base, lang);
  for candidate in candidates {
    if let Some(found) = files
      .iter()
      .map(|(path, _)| path)
      .find(|path| **path == candidate)
    {
      return Some(found.clone());
    }
    if candidate.is_dir() {
      if let Some(found) = files
        .iter()
        .map(|(path, _)| path)
        .find(|path| path.parent() == Some(candidate.as_path()))
      {
        return Some(found.clone());
      }
    }
  }
  let normalized = spec.trim_start_matches("./").replace('.', "/");
  files
    .iter()
    .map(|(path, _)| path)
    .find(|path| {
      path
        .to_string_lossy()
        .replace('\\', "/")
        .ends_with(&format!("{normalized}.java"))
        || path
          .to_string_lossy()
          .replace('\\', "/")
          .ends_with(&format!("{normalized}.py"))
    })
    .cloned()
}

fn module_path_candidates(spec: &str, base: &Path, lang: SupportLang) -> Vec<PathBuf> {
  let raw = spec.trim_matches('"').trim_matches('\'');
  let stem = if raw.starts_with('.') {
    base.join(raw)
  } else {
    base.join(raw.replace('.', "/"))
  };
  match lang {
    SupportLang::Python => vec![stem.with_extension("py"), stem.join("__init__.py")],
    SupportLang::JavaScript | SupportLang::TypeScript | SupportLang::Tsx => vec![
      stem.clone(),
      stem.with_extension("js"),
      stem.with_extension("ts"),
      stem.with_extension("tsx"),
      stem.join("index.js"),
      stem.join("index.ts"),
    ],
    SupportLang::Go => vec![stem.clone(), stem.join("main.go"), stem.join("secrets.go")],
    SupportLang::Java => vec![stem.with_extension("java")],
    _ => Vec::new(),
  }
}

fn analyze_python_sink_evidence(path: &Path, content: &str) -> Vec<SinkEvidenceOnlyFinding> {
  let path = path.to_string_lossy().into_owned();
  let provider_matcher =
    default_provider_matcher().filter(|matcher| matcher.prefilter.is_match(content));
  let mut findings = Vec::new();
  for (idx, line) in content.lines().enumerate() {
    findings.extend(
      provider_matcher
        .map(|matcher| matcher.line_sink_evidence(&path, content, line, idx + 1))
        .unwrap_or_default(),
    );
    if let Some(finding) = (|| {
      let trimmed = line.trim();
      let (callee, value) = {
        let args = call_args_after(trimmed, "client.login")?;
        ("client.login", first_arg(args)?)
      };
      identifier_like(value).map(|value_text| SinkEvidenceOnlyFinding {
        detector: DETECTOR_SINK_EVIDENCE.into(),
        confidence: SemanticConfidence::SyntaxEvidenceOnly,
        provider_id: None,
        provider_rule_ids: Vec::new(),
        credential_kind: None,
        sdk: None,
        provider_shape_id: None,
        value_text,
        sink: SinkEvidence {
          file_path: path.clone(),
          line: idx + 1,
          column: line.find(callee).unwrap_or(0) + 1,
          callee_text: callee.to_string(),
          arg_position: Some(0),
          arg_name: None,
          sink_kind: SinkKind::MethodCall,
        },
      })
    })() {
      findings.push(finding);
    }
  }
  findings
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum AstLang {
  JavaScript,
  Go,
  Java,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct AstScope {
  kind: AstScopeKind,
  literals: HashMap<String, ActiveCredential>,
  declared: HashSet<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum AstScopeKind {
  File,
  Function,
  Block,
  Class,
}

fn analyze_ast_language(
  path: &Path,
  content: &str,
  lang: AstLang,
  strategy: &dyn BindingStrategy,
  sink_evidence: SinkEvidenceMode,
) -> SemanticScanResult {
  let mut result = SemanticScanResult::new();
  let path = path.to_string_lossy().into_owned();
  let grep = strategy.support_lang().ast_grep(content);
  let root = grep.root();
  let provider_matcher =
    default_provider_matcher().filter(|matcher| matcher.prefilter.is_match(content));
  let source_matcher =
    default_source_matcher().filter(|matcher| matcher.prefilter.is_match(content));
  let context = AstWalkContext {
    path: &path,
    content,
    lang,
    strategy,
    provider_matcher,
    source_matcher,
    sink_evidence,
  };
  let mut scopes = vec![AstScope {
    kind: AstScopeKind::File,
    literals: HashMap::new(),
    declared: HashSet::new(),
  }];

  for child in root.children().filter(|child| child.is_named()) {
    walk_ast_language_node(&context, &child, &mut scopes, &mut result);
  }
  dedup_symbol_reachability(&mut result);
  result
}

#[derive(Clone, Copy)]
struct AstWalkContext<'a> {
  path: &'a str,
  content: &'a str,
  lang: AstLang,
  strategy: &'a dyn BindingStrategy,
  provider_matcher: Option<&'a ProviderMatcher>,
  source_matcher: Option<&'a SourceMatcher>,
  sink_evidence: SinkEvidenceMode,
}

fn walk_ast_language_node(
  context: &AstWalkContext<'_>,
  node: &PythonNode<'_>,
  scopes: &mut Vec<AstScope>,
  result: &mut SemanticScanResult,
) {
  let kind = node.kind();
  let is_function = context
    .strategy
    .function_scope_kinds()
    .contains(&kind.as_ref());
  let is_block = context
    .strategy
    .block_scope_kinds()
    .contains(&kind.as_ref());
  if is_function || is_block {
    scopes.push(AstScope {
      kind: if is_function {
        AstScopeKind::Function
      } else if kind.contains("class") {
        AstScopeKind::Class
      } else {
        AstScopeKind::Block
      },
      literals: HashMap::new(),
      declared: HashSet::new(),
    });
  }

  if context
    .strategy
    .declaration_kinds()
    .contains(&kind.as_ref())
  {
    for assignment in ast_assignments(node, context.lang) {
      let target =
        assignment_target_scope(scopes, context.lang, assignment.hoisted, assignment.field);
      if let Some(scope) = scopes.get_mut(target) {
        scope.declared.insert(assignment.name.clone());
        if let Some(source) = ast_source_assignment(context, node, &assignment, result) {
          scope.literals.insert(
            assignment.name,
            ActiveCredential {
              line: source.line,
              column: source.column,
              origin: ActiveCredentialOrigin::Source(source),
            },
          );
        } else if let Some(literal) = assignment.literal {
          scope.literals.insert(
            assignment.name,
            ActiveCredential {
              line: node.start_pos().line() + 1,
              column: node.start_pos().column(node) + 1,
              origin: ActiveCredentialOrigin::Literal(Secret::new(literal)),
            },
          );
        } else {
          scope.literals.remove(&assignment.name);
        }
      }
    }
  }

  record_ast_side_effect_source_read(context, node, result);

  if context.strategy.sink_node_kinds().contains(&kind.as_ref()) {
    let mut sinks = parse_generic_sink_uses(
      context.path,
      node.text().as_ref(),
      node.start_pos().line() + 1,
      context.lang,
    );
    if let Some(matcher) = context.provider_matcher {
      sinks.extend(matcher.ast_sink_uses(context.path, context.content, node, context.lang));
    }
    for sink in sinks {
      let value_name = normalize_value_name(&sink.value_text);
      if let Some(active) = resolve_ast_credential(scopes, &value_name) {
        emit_ast_active(context.path, &value_name, active, sink, result);
      } else if context.sink_evidence == SinkEvidenceMode::On {
        result.sink_evidence.push(sink);
      }
    }
  }

  for child in node.children().filter(|child| child.is_named()) {
    walk_ast_language_node(context, &child, scopes, result);
  }

  if is_function || is_block {
    scopes.pop();
  }
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct AstAssignment {
  name: String,
  literal: Option<String>,
  hoisted: bool,
  field: bool,
  index: usize,
}

fn assignment_target_scope(
  scopes: &[AstScope],
  lang: AstLang,
  hoisted: bool,
  field: bool,
) -> usize {
  if hoisted {
    return scopes
      .iter()
      .enumerate()
      .rev()
      .find(|(_, scope)| matches!(scope.kind, AstScopeKind::Function | AstScopeKind::File))
      .map(|(idx, _)| idx)
      .unwrap_or(0);
  }
  if lang == AstLang::Java && field {
    return scopes
      .iter()
      .enumerate()
      .rev()
      .find(|(_, scope)| matches!(scope.kind, AstScopeKind::Class | AstScopeKind::File))
      .map(|(idx, _)| idx)
      .unwrap_or(0);
  }
  scopes.len().saturating_sub(1)
}

fn resolve_ast_credential<'a>(scopes: &'a [AstScope], name: &str) -> Option<&'a ActiveCredential> {
  for scope in scopes.iter().rev() {
    if let Some(literal) = scope.literals.get(name) {
      return Some(literal);
    }
    if scope.declared.contains(name) {
      return None;
    }
  }
  None
}

fn emit_ast_active(
  path: &str,
  var_name: &str,
  active: &ActiveCredential,
  sink: SinkEvidenceOnlyFinding,
  result: &mut SemanticScanResult,
) {
  match &active.origin {
    ActiveCredentialOrigin::Literal(literal) => {
      result.symbol_reachability.push(SymbolReachabilityFinding {
        detector: DETECTOR_SYMBOL_REACHABILITY.into(),
        confidence: SemanticConfidence::BoundedSymbolFlow,
        provider_id: sink.provider_id.clone(),
        provider_rule_ids: sink.provider_rule_ids.clone(),
        credential_kind: sink.credential_kind.clone(),
        sdk: sink.sdk.clone(),
        provider_shape_id: sink.provider_shape_id.clone(),
        source: SourceEvidence {
          file_path: path.to_string(),
          line: active.line,
          column: active.column,
          variable: var_name.to_string(),
          literal: literal.clone(),
        },
        sink: sink.sink,
        flow: Vec::new(),
      });
    }
    ActiveCredentialOrigin::Source(source) => {
      if source.provider_id == sink.provider_id {
        result.confirmed_flows.push(confirmed_flow_from_source(
          source, var_name, sink.sink, None,
        ));
      }
    }
  }
}

fn ast_source_assignment(
  context: &AstWalkContext<'_>,
  node: &PythonNode<'_>,
  assignment: &AstAssignment,
  result: &mut SemanticScanResult,
) -> Option<CredentialSourceEvidence> {
  if assignment.index != 0 {
    return None;
  }
  let matcher = context.source_matcher?;
  let source_node = ast_source_assignment_node(node, context.lang)?;
  let language = provider_language_for_ast(context.lang)?;
  let matched = matcher.source_read(
    context.path,
    context.content,
    &source_node,
    &assignment.name,
    language,
  )?;
  if matched.kind == ReadShapeKind::DotenvLoad {
    return None;
  }
  let evidence = matched.evidence;
  result.source_evidence.push(evidence.clone());
  Some(evidence)
}

fn record_ast_side_effect_source_read(
  context: &AstWalkContext<'_>,
  node: &PythonNode<'_>,
  result: &mut SemanticScanResult,
) -> Option<()> {
  if !matches!(
    node.kind().as_ref(),
    "call_expression" | "method_invocation"
  ) {
    return None;
  }
  let matcher = context.source_matcher?;
  let language = provider_language_for_ast(context.lang)?;
  let matched = matcher.source_read(
    context.path,
    context.content,
    node,
    node.text().as_ref(),
    language,
  )?;
  if matched.kind == ReadShapeKind::DotenvLoad {
    result.source_evidence.push(matched.evidence);
  }
  Some(())
}

fn ast_source_assignment_node<'a>(node: &PythonNode<'a>, lang: AstLang) -> Option<PythonNode<'a>> {
  match (lang, node.kind().as_ref()) {
    (AstLang::JavaScript, "variable_declarator") => node.field("value"),
    (AstLang::Go, "short_var_declaration") => node.field("right"),
    (AstLang::Go, "var_spec") => node
      .field("value")
      .or_else(|| node.field_children("value").next()),
    (AstLang::Java, "variable_declarator") => node.field("value"),
    (AstLang::Java, "assignment_expression") => node.field("right"),
    _ => None,
  }
}

fn ast_assignments(node: &PythonNode<'_>, lang: AstLang) -> Vec<AstAssignment> {
  match lang {
    AstLang::JavaScript => parse_js_assignments(node),
    AstLang::Go => parse_go_assignments(node),
    AstLang::Java => parse_java_assignments(node),
  }
}

fn parse_js_assignments(node: &PythonNode<'_>) -> Vec<AstAssignment> {
  if node.kind() != "variable_declarator" {
    return Vec::new();
  }
  let Some(name) = node.field("name") else {
    return Vec::new();
  };
  let literal = node
    .field("value")
    .and_then(|value| literal_from_node(&value));
  let hoisted = js_declarator_is_var(node);
  binding_names_from_pattern(&name)
    .into_iter()
    .map(|name| AstAssignment {
      name,
      literal: literal.clone(),
      hoisted,
      field: false,
      index: 0,
    })
    .collect()
}

fn parse_go_assignments(node: &PythonNode<'_>) -> Vec<AstAssignment> {
  match node.kind().as_ref() {
    "short_var_declaration" => {
      let names = node
        .field("left")
        .map(|left| binding_names_from_pattern(&left))
        .unwrap_or_default();
      let values = node
        .field("right")
        .map(|right| literal_values_from_node(&right))
        .unwrap_or_default();
      pair_names_and_literals(names, values, false, false)
    }
    "var_spec" => {
      let names: Vec<_> = node
        .field_children("name")
        .flat_map(|name| binding_names_from_pattern(&name))
        .collect();
      let values: Vec<_> = node
        .field_children("value")
        .flat_map(|value| literal_values_from_node(&value))
        .collect();
      pair_names_and_literals(names, values, false, false)
    }
    _ => Vec::new(),
  }
}

fn parse_java_assignments(node: &PythonNode<'_>) -> Vec<AstAssignment> {
  match node.kind().as_ref() {
    "variable_declarator" => {
      let Some(name) = node.field("name") else {
        return Vec::new();
      };
      let literal = node
        .field("value")
        .and_then(|value| literal_from_node(&value));
      binding_names_from_pattern(&name)
        .into_iter()
        .map(|name| AstAssignment {
          name: normalize_value_name(&name),
          literal: literal.clone(),
          hoisted: false,
          field: java_declarator_is_field(node),
          index: 0,
        })
        .collect()
    }
    "assignment_expression" => {
      let Some(left) = node.field("left") else {
        return Vec::new();
      };
      let Some(right) = node.field("right") else {
        return Vec::new();
      };
      binding_names_from_pattern(&left)
        .into_iter()
        .enumerate()
        .map(|(index, name)| AstAssignment {
          name: normalize_value_name(&name),
          literal: literal_from_node(&right),
          hoisted: false,
          field: is_field_access(&left),
          index,
        })
        .collect()
    }
    _ => Vec::new(),
  }
}

fn js_declarator_is_var(node: &PythonNode<'_>) -> bool {
  node
    .parent()
    .map(|parent| {
      parent
        .field_children("kind")
        .any(|kind| kind.text().as_ref() == "var")
        || parent
          .children()
          .any(|child| child.kind() == "var" || child.text().as_ref() == "var")
    })
    .unwrap_or(false)
}

fn java_declarator_is_field(node: &PythonNode<'_>) -> bool {
  node
    .ancestors()
    .any(|ancestor| ancestor.kind() == "field_declaration")
}

fn is_field_access(node: &PythonNode<'_>) -> bool {
  node.text().as_ref().starts_with("this.")
}

fn pair_names_and_literals(
  names: Vec<String>,
  values: Vec<Option<String>>,
  hoisted: bool,
  field: bool,
) -> Vec<AstAssignment> {
  names
    .into_iter()
    .enumerate()
    .map(|(idx, name)| AstAssignment {
      name,
      literal: values.get(idx).cloned().flatten(),
      hoisted,
      field,
      index: idx,
    })
    .collect()
}

fn binding_names_from_pattern(node: &PythonNode<'_>) -> Vec<String> {
  let kind = node.kind();
  if matches!(
    kind.as_ref(),
    "identifier"
      | "property_identifier"
      | "shorthand_property_identifier_pattern"
      | "field_identifier"
  ) {
    let text = normalize_value_name(node.text().as_ref());
    return is_binding_identifier(&text)
      .then_some(text)
      .into_iter()
      .collect();
  }
  node
    .children()
    .filter(|child| child.is_named())
    .flat_map(|child| binding_names_from_pattern(&child))
    .collect()
}

fn literal_values_from_node(node: &PythonNode<'_>) -> Vec<Option<String>> {
  let direct = literal_from_node(node);
  let values: Vec<_> = node
    .children()
    .filter(|child| child.is_named())
    .filter_map(|child| literal_from_node(&child).map(Some))
    .collect();
  if values.is_empty() {
    vec![direct]
  } else {
    values
  }
}

fn literal_from_node(node: &PythonNode<'_>) -> Option<String> {
  strip_quoted(node.text().as_ref())
}

fn is_binding_identifier(text: &str) -> bool {
  let mut chars = text.chars();
  let Some(first) = chars.next() else {
    return false;
  };
  (first.is_ascii_alphabetic() || first == '_' || first == '$')
    && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
}

fn parse_python_import_from(node: &PythonNode<'_>) -> Vec<ModuleImport> {
  let text = node.text();
  let text = text.as_ref();
  let Some(rest) = text.strip_prefix("from ") else {
    return Vec::new();
  };
  let Some((module_spec, names)) = rest.split_once(" import ") else {
    return Vec::new();
  };
  names
    .split(',')
    .filter_map(|name| {
      let name = name.trim();
      let (imported, local) = split_alias(name);
      is_binding_identifier(&local).then(|| ModuleImport {
        local_name: local,
        imported_name: imported,
        module_spec: module_spec.trim().to_string(),
        line: node.start_pos().line() + 1,
        column: node.start_pos().column(node) + 1,
      })
    })
    .collect()
}

fn parse_js_import(node: &PythonNode<'_>) -> Vec<ModuleImport> {
  let Some(module_spec) = js_module_spec(node) else {
    return Vec::new();
  };
  let text = node.text();
  let text = text.as_ref();
  let mut imports = Vec::new();
  if let Some((_, named)) = text.split_once('{') {
    if let Some((named, _)) = named.split_once('}') {
      for part in named.split(',') {
        let (imported, local) = split_alias(part.trim());
        if is_binding_identifier(&local) {
          imports.push(ModuleImport {
            local_name: local,
            imported_name: imported,
            module_spec: module_spec.clone(),
            line: node.start_pos().line() + 1,
            column: node.start_pos().column(node) + 1,
          });
        }
      }
    }
  } else if let Some(after_import) = text.strip_prefix("import ") {
    let local = after_import
      .split_whitespace()
      .next()
      .unwrap_or_default()
      .trim_end_matches(',');
    if is_binding_identifier(local) {
      imports.push(ModuleImport {
        local_name: local.to_string(),
        imported_name: "default".into(),
        module_spec,
        line: node.start_pos().line() + 1,
        column: node.start_pos().column(node) + 1,
      });
    }
  }
  imports
}

fn parse_js_reexport(node: &PythonNode<'_>) -> Vec<ModuleImport> {
  let Some(module_spec) = js_module_spec(node) else {
    return Vec::new();
  };
  let text = node.text();
  let text = text.as_ref();
  let Some((_, named)) = text.split_once('{') else {
    return Vec::new();
  };
  let Some((named, _)) = named.split_once('}') else {
    return Vec::new();
  };
  named
    .split(',')
    .filter_map(|part| {
      let (imported, local) = split_alias(part.trim());
      is_binding_identifier(&local).then(|| ModuleImport {
        local_name: local,
        imported_name: imported,
        module_spec: module_spec.clone(),
        line: node.start_pos().line() + 1,
        column: node.start_pos().column(node) + 1,
      })
    })
    .collect()
}

fn js_module_spec(node: &PythonNode<'_>) -> Option<String> {
  node
    .field("source")
    .or_else(|| {
      node
        .children()
        .find(|child| matches!(child.kind().as_ref(), "string" | "string_fragment"))
    })
    .and_then(|source| strip_quoted(source.text().as_ref()))
}

fn parse_go_import(node: &PythonNode<'_>) -> Option<ModuleImport> {
  let path_node = node.field("path").or_else(|| {
    node.children().find(|child| {
      matches!(
        child.kind().as_ref(),
        "interpreted_string_literal" | "raw_string_literal"
      )
    })
  })?;
  let module_spec = strip_quoted(path_node.text().as_ref())?;
  let local = node
    .field("name")
    .map(|name| name.text().to_string())
    .unwrap_or_else(|| {
      module_spec
        .rsplit('/')
        .next()
        .unwrap_or(&module_spec)
        .trim_matches('.')
        .to_string()
    });
  Some(ModuleImport {
    local_name: local,
    imported_name: "*".into(),
    module_spec,
    line: node.start_pos().line() + 1,
    column: node.start_pos().column(node) + 1,
  })
}

fn parse_java_import(node: &PythonNode<'_>) -> Option<ModuleImport> {
  let text = node.text();
  let spec = text
    .as_ref()
    .trim()
    .strip_prefix("import ")?
    .trim_end_matches(';')
    .trim()
    .trim_start_matches("static ")
    .to_string();
  let local = spec.rsplit('.').next()?.to_string();
  Some(ModuleImport {
    local_name: local,
    imported_name: spec.rsplit('.').next().unwrap_or(&spec).to_string(),
    module_spec: spec,
    line: node.start_pos().line() + 1,
    column: node.start_pos().column(node) + 1,
  })
}

fn split_alias(part: &str) -> (String, String) {
  if let Some((imported, local)) = part.split_once(" as ") {
    return (imported.trim().to_string(), local.trim().to_string());
  }
  (part.trim().to_string(), part.trim().to_string())
}

fn has_ancestor_kind(node: &PythonNode<'_>, kind: &str) -> bool {
  node.ancestors().any(|ancestor| ancestor.kind() == kind)
}

fn nearest_java_class_name(node: &PythonNode<'_>) -> Option<String> {
  node.ancestors().find_map(|ancestor| {
    (ancestor.kind() == "class_declaration")
      .then(|| ancestor.field("name").map(|name| name.text().to_string()))
      .flatten()
  })
}

fn starts_uppercase(name: &str) -> bool {
  name
    .chars()
    .next()
    .is_some_and(|ch| ch.is_ascii_uppercase())
}

fn parse_generic_sink_uses(
  path: &str,
  line: &str,
  line_number: usize,
  lang: AstLang,
) -> Vec<SinkEvidenceOnlyFinding> {
  let mut sinks = Vec::new();
  let trimmed = line.trim();
  let candidates: Vec<(&str, Option<usize>, Option<&str>, SinkKind)> = match lang {
    AstLang::JavaScript => vec![("client.login", Some(0), None, SinkKind::MethodCall)],
    AstLang::Go => vec![(
      ".Set(\"Authorization\"",
      Some(1),
      Some("Authorization"),
      SinkKind::MethodCall,
    )],
    AstLang::Java => vec![(
      "setRequestProperty(\"Authorization\"",
      Some(1),
      Some("Authorization"),
      SinkKind::MethodCall,
    )],
  };
  for (needle, position, arg_name, sink_kind) in candidates {
    if !trimmed.contains(needle) {
      continue;
    }
    let value = if let Some(position) = position {
      call_args_from_line(trimmed).and_then(|args| nth_arg(args, position))
    } else if let Some(name) = arg_name {
      value_after(trimmed, &format!("{name}:"))
    } else {
      None
    };
    let Some(value_text) = value.and_then(identifier_like) else {
      continue;
    };
    sinks.push(SinkEvidenceOnlyFinding {
      detector: DETECTOR_SINK_EVIDENCE.into(),
      confidence: SemanticConfidence::SyntaxEvidenceOnly,
      provider_id: None,
      provider_rule_ids: Vec::new(),
      credential_kind: None,
      sdk: None,
      provider_shape_id: None,
      value_text,
      sink: SinkEvidence {
        file_path: path.to_string(),
        line: line_number,
        column: line.find(needle).unwrap_or(0) + 1,
        callee_text: needle.trim_end_matches(':').to_string(),
        arg_position: position,
        arg_name: arg_name.map(str::to_string),
        sink_kind,
      },
    });
  }
  sinks
}

fn dedup_sink_evidence(result: &mut SemanticScanResult) {
  dedup_symbol_reachability(result);
  result.sink_evidence.retain(|sink| {
    !result.symbol_reachability.iter().any(|strong| {
      strong.sink.file_path == sink.sink.file_path
        && strong.sink.line == sink.sink.line
        && strong.sink.callee_text == sink.sink.callee_text
        && strong.sink.arg_position == sink.sink.arg_position
    })
  });
}

fn dedup_source_evidence(result: &mut SemanticScanResult) {
  let mut seen = HashSet::new();
  result.source_evidence.retain(|source| {
    seen.insert((
      source.file_path.clone(),
      source.line,
      source.column,
      source.source_id.clone(),
      source.variable.clone(),
    ))
  });
}

fn dedup_confirmed_flows(result: &mut SemanticScanResult) {
  let mut seen = HashSet::new();
  result.confirmed_flows.retain(|flow| {
    seen.insert((
      flow.sink.file_path.clone(),
      flow.sink.line,
      flow.sink.callee_text.clone(),
      flow.source.file_path.clone(),
      flow.source.line,
      flow.source.variable.clone(),
    ))
  });
}

fn dedup_symbol_reachability(result: &mut SemanticScanResult) {
  let mut seen = HashSet::new();
  result.symbol_reachability.retain(|finding| {
    seen.insert((
      finding.sink.file_path.clone(),
      finding.sink.line,
      finding.sink.callee_text.clone(),
      finding.sink.arg_position,
      finding.source.variable.clone(),
    ))
  });
}

fn strip_quoted(value: &str) -> Option<String> {
  let value = value.trim();
  let quote = value.chars().next()?;
  if quote != '"' && quote != '\'' && quote != '`' {
    return None;
  }
  let end = value.rfind(quote)?;
  (end > 0).then(|| value[quote.len_utf8()..end].to_string())
}

fn call_args_after<'a>(line: &'a str, callee: &str) -> Option<&'a str> {
  line
    .split_once(callee)?
    .1
    .split_once('(')?
    .1
    .rsplit_once(')')?
    .0
    .into()
}

fn call_args_from_line(line: &str) -> Option<&str> {
  line.split_once('(')?.1.rsplit_once(')')?.0.into()
}

fn first_arg(args: &str) -> Option<&str> {
  nth_arg(args, 0)
}

fn nth_arg(args: &str, index: usize) -> Option<&str> {
  args.split(',').nth(index).map(str::trim)
}

fn value_after<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
  Some(
    line
      .split_once(marker)?
      .1
      .split([',', ')', '}'])
      .next()?
      .trim()
      .trim_end_matches(';'),
  )
}

fn identifier_like(value: &str) -> Option<String> {
  let value = value.trim().trim_end_matches(';');
  if value.is_empty() || strip_quoted(value).is_some() || value.contains('(') {
    return None;
  }
  value
    .chars()
    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.')
    .then(|| normalize_value_name(value))
}

fn normalize_value_name(value: &str) -> String {
  value
    .trim()
    .trim_start_matches("this.")
    .trim_end_matches(';')
    .to_string()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn analyze(content: &str) -> Vec<SymbolReachabilityFinding> {
    analyze_python_source("case.py", content, &PythonSinkCatalog::default())
  }

  #[test]
  fn positive_a_links_literal_assignment_to_method_sink() {
    let findings = analyze("const1 = \"abc123\"\nclient.login(const1)\n");

    assert_eq!(findings.len(), 1);
    let finding = &findings[0];
    assert_eq!(finding.detector, "symbol-reachability");
    assert_eq!(finding.confidence, SemanticConfidence::BoundedSymbolFlow);
    assert_eq!(finding.source.line, 1);
    assert_eq!(finding.source.variable, "const1");
    assert_eq!(finding.source.literal.expose_raw(), "abc123");
    assert_eq!(finding.sink.line, 2);
    assert_eq!(finding.sink.callee_text, "client.login");
    assert_eq!(finding.sink.arg_position, Some(0));
    assert_eq!(finding.sink.arg_name, None);
    assert_eq!(finding.sink.sink_kind, SinkKind::MethodCall);
  }

  #[test]
  fn positive_b_links_literal_to_stripe_attribute_assignment() {
    let findings =
      analyze("import stripe\nkey = \"sk_live_AbC123example\"\nstripe.api_key = key\n");

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].source.line, 2);
    assert_eq!(findings[0].source.variable, "key");
    assert_eq!(findings[0].sink.line, 3);
    assert_eq!(findings[0].sink.callee_text, "stripe.api_key");
    assert_eq!(findings[0].sink.arg_name.as_deref(), Some("api_key"));
    assert_eq!(findings[0].sink.sink_kind, SinkKind::AttributeAssignment);
  }

  #[test]
  fn positive_c_links_literal_to_boto3_credential_kwarg() {
    let findings = analyze(
      "secret = \"wJalrXUtnFEMIexampleKEY\"\nboto3.client(\"s3\", aws_secret_access_key=secret)\n",
    );

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].source.line, 1);
    assert_eq!(findings[0].source.variable, "secret");
    assert_eq!(findings[0].sink.line, 2);
    assert_eq!(findings[0].sink.callee_text, "boto3.client");
    assert_eq!(
      findings[0].sink.arg_name.as_deref(),
      Some("aws_secret_access_key")
    );
    assert_eq!(findings[0].sink.arg_position, Some(1));
    assert_eq!(findings[0].sink.sink_kind, SinkKind::CredentialKwarg);
  }

  #[test]
  fn negative_d_reassignment_breaks_reachability() {
    let findings = analyze("k = \"topsecret\"\nk = sanitize()\nclient.login(k)\n");

    assert!(findings.is_empty());
  }

  #[test]
  fn negative_e_runtime_call_without_literal_has_no_source() {
    let findings = analyze("client.login(get_key())\n");

    assert!(findings.is_empty());
  }

  #[test]
  fn known_limit_v1_f_string_or_concat_argument_is_not_traced() {
    // KNOWN LIMIT v1: this bounded symbol flow does not evaluate expressions.
    let concat_findings = analyze("c = \"c\"\nclient.login(\"ab\" + c)\n");
    let f_string_findings = analyze("c = \"c\"\nclient.login(f\"ab{c}\")\n");

    assert!(concat_findings.is_empty());
    assert!(f_string_findings.is_empty());
  }

  #[test]
  fn known_limit_v1_container_index_flow_is_not_traced() {
    // KNOWN LIMIT v1: container writes and index reads are out of scope.
    let findings = analyze("d = {\"k\": \"sec\"}\nclient.login(d[\"k\"])\n");

    assert!(findings.is_empty());
  }

  #[test]
  fn known_limit_v1_reassignment_chain_depth_is_not_traced() {
    // KNOWN LIMIT v1: simple alias chains are out of scope for Phase 1.
    let findings = analyze("a = \"sec\"\nb = a\nclient.login(b)\n");

    assert!(findings.is_empty());
  }

  #[test]
  fn cross_scope_literal_does_not_leak_to_unrelated_function() {
    let findings = analyze(
      r#"
def danger():
    api = "S3cr3tV4lue_kJ8shdfLKJ23lkjsdf09sdfkjh23"

def use_it():
    client.login(api)
"#,
    );

    assert!(findings.is_empty());
  }

  #[test]
  fn per_scope_literals_bind_to_their_own_function() {
    let findings = analyze(
      r#"
def setup():
    key = "REAL_SECRET_AKIA111111111111"
    client.login(key)

def other():
    key = "OTHER_SECRET_AKIA222222222222"
    client.login(key)
"#,
    );

    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0].source.line, 3);
    assert_eq!(
      findings[0].source.literal.expose_raw(),
      "REAL_SECRET_AKIA111111111111"
    );
    assert_eq!(findings[0].sink.line, 4);
    assert_eq!(findings[1].source.line, 7);
    assert_eq!(
      findings[1].source.literal.expose_raw(),
      "OTHER_SECRET_AKIA222222222222"
    );
    assert_eq!(findings[1].sink.line, 8);
  }

  #[test]
  fn function_local_shadowing_wins_over_module_literal() {
    let findings = analyze(
      r#"
k = "MODULE_A"

def use_local():
    k = "FUNCTION_B"
    client.login(k)
"#,
    );

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].source.line, 5);
    assert_eq!(findings[0].source.literal.expose_raw(), "FUNCTION_B");
    assert_eq!(findings[0].sink.line, 6);
  }

  #[test]
  fn nested_scope_reads_outer_literal_when_not_rebound() {
    let findings = analyze(
      r#"
def outer():
    t = "OUTER_SECRET"
    def inner():
        client.login(t)
"#,
    );

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].source.line, 3);
    assert_eq!(findings[0].source.literal.expose_raw(), "OUTER_SECRET");
    assert_eq!(findings[0].sink.line, 5);
  }

  #[test]
  fn nested_scope_local_binding_blocks_outer_literal_even_before_assignment() {
    let findings = analyze(
      r#"
def outer():
    t = "OUTER_SECRET"
    def inner():
        client.login(t)
        t = sanitize()
"#,
    );

    assert!(findings.is_empty());
  }

  #[test]
  fn nested_scope_parameter_binding_blocks_outer_literal() {
    let findings = analyze(
      r#"
def outer():
    t = "OUTER_SECRET"
    def inner(t):
        client.login(t)
"#,
    );

    assert!(findings.is_empty());
  }

  #[test]
  fn serialized_finding_redacts_literal_by_default() {
    let findings = analyze("const1 = \"abc123\"\nclient.login(const1)\n");

    let debug = format!("{:?}", findings[0]);
    assert!(!debug.contains("abc123"));
    assert!(debug.contains("REDACTED"));

    let json = serde_json::to_string(&findings[0]).expect("serialize finding");
    assert!(!json.contains("abc123"));
    assert!(json.contains("REDACTED"));
  }

  #[test]
  fn python_global_and_nonlocal_bind_to_outer_scopes() {
    let global_findings = analyze(
      r#"
k = "MODULE_SECRET"
def f():
    global k
    client.login(k)
"#,
    );
    assert_eq!(global_findings.len(), 1);
    assert_eq!(global_findings[0].source.line, 2);

    let nonlocal_findings = analyze(
      r#"
def outer():
    k = "OUTER_SECRET"
    def inner():
        nonlocal k
        client.login(k)
"#,
    );
    assert_eq!(nonlocal_findings.len(), 1);
    assert_eq!(nonlocal_findings[0].source.line, 3);
  }

  #[test]
  fn javascript_literal_flow_scope_and_known_limits() {
    let result = analyze_source(
      "case.js",
      r#"
import { S3Client } from "@aws-sdk/client-s3";
const secret = "js_secret";
new S3Client({ credentials: { accessKeyId: id, secretAccessKey: secret } });
{ let scoped = "a"; }
{ client.login(scoped); }
"#,
      SinkEvidenceMode::Off,
    );
    assert_eq!(result.symbol_reachability.len(), 1);
    assert_eq!(result.symbol_reachability[0].source.variable, "secret");

    let tdz = analyze_source(
      "case.js",
      r#"
{
  client.login(k);
  let k = "later";
}
{
  var v = "hoisted_secret";
}
client.login(v);
{
  let sibling = "block_secret";
}
client.login(sibling);
"#,
      SinkEvidenceMode::Off,
    );
    assert_eq!(tdz.symbol_reachability.len(), 1);
    assert_eq!(tdz.symbol_reachability[0].source.variable, "v");

    // KNOWN LIMIT v1: concat/template/container/dynamic dispatch are not evaluated.
    let misses = analyze_source(
      "case.js",
      r#"
const c = "x";
client.login("a" + c);
client.login(`${c}`);
const d = {k: "sec"};
client.login(d.k);
obj["login"](c);
"#,
      SinkEvidenceMode::Off,
    );
    assert!(misses.symbol_reachability.is_empty());
  }

  #[test]
  fn javascript_ast_binding_extraction_handles_multi_init_and_destructuring() {
    let multi = analyze_source(
      "case.js",
      r#"
const a = "one_secret", b = "two_secret";
client.login(a);
client.login(b);
"#,
      SinkEvidenceMode::Off,
    );
    assert_eq!(multi.symbol_reachability.len(), 2);
    assert!(multi
      .symbol_reachability
      .iter()
      .any(|finding| finding.source.variable == "a"
        && finding.source.literal.expose_raw() == "one_secret"));
    assert!(multi
      .symbol_reachability
      .iter()
      .any(|finding| finding.source.variable == "b"
        && finding.source.literal.expose_raw() == "two_secret"));

    let destructuring = analyze_source(
      "case.js",
      r#"
const {a, b} = obj;
client.login(a);
client.login(b);
"#,
      SinkEvidenceMode::Off,
    );
    assert!(destructuring.symbol_reachability.is_empty());
  }

  #[test]
  fn typescript_literal_flow_and_cross_scope_negative() {
    let result = analyze_source(
      "case.ts",
      r#"
const tok = "ts_secret";
client.login(tok);
{ const tok2 = "inner"; }
{ client.login(tok2); }
"#,
      SinkEvidenceMode::Off,
    );
    assert_eq!(result.symbol_reachability.len(), 1);
    assert_eq!(result.symbol_reachability[0].source.variable, "tok");
  }

  #[test]
  fn go_literal_flow_scope_and_known_limits() {
    let result = analyze_source(
      "case.go",
      r#"
package main
func f() {
  secret := "go_secret"
  h.Set("Authorization", secret)
}
func g() {
  h.Set("Authorization", secret)
}
"#,
      SinkEvidenceMode::Off,
    );
    assert_eq!(result.symbol_reachability.len(), 1);
    assert_eq!(result.symbol_reachability[0].source.variable, "secret");

    let block = analyze_source(
      "case.go",
      r#"
package main
func f() {
  { k := "x" }
  h.Set("Authorization", k)
}
"#,
      SinkEvidenceMode::Off,
    );
    assert!(block.symbol_reachability.is_empty());

    let grouped = analyze_source(
      "case.go",
      r#"
package main
func f() {
  var (
    grouped = "group_secret"
  )
  h.Set("Authorization", grouped)
}
"#,
      SinkEvidenceMode::Off,
    );
    assert_eq!(grouped.symbol_reachability.len(), 1);

    // KNOWN LIMIT v1: concatenation, containers, and dynamic dispatch are not evaluated.
    let misses = analyze_source(
      "case.go",
      r#"
package main
func f() {
  k := "x"
  h.Set("Authorization", "Bearer " + k)
  m := map[string]string{"k": "sec"}
  h.Set("Authorization", m["k"])
}
"#,
      SinkEvidenceMode::Off,
    );
    assert!(misses.symbol_reachability.is_empty());
  }

  #[test]
  fn java_literal_flow_scope_and_known_limits() {
    let result = analyze_source(
      "Case.java",
      r#"
import com.amazonaws.auth.BasicAWSCredentials;
class Case {
  static final String FIELD = "field_secret";
  void f() {
    String secret = "java_secret";
    new BasicAWSCredentials(id, secret);
  }
  void g() {
    new BasicAWSCredentials(id, secret);
  }
}
"#,
      SinkEvidenceMode::Off,
    );
    assert_eq!(result.symbol_reachability.len(), 1);
    assert_eq!(result.symbol_reachability[0].source.variable, "secret");

    let fields = analyze_source(
      "Case.java",
      r#"
import com.amazonaws.auth.BasicAWSCredentials;
class Case {
  private final String fieldSecret = "field_secret";
  private String ctorSecret;
  Case() { this.ctorSecret = "ctor_secret"; }
  void f() {
    new BasicAWSCredentials(id, fieldSecret);
    new BasicAWSCredentials(id, this.ctorSecret);
    String fieldSecret = "local_secret";
    new BasicAWSCredentials(id, fieldSecret);
  }
}
"#,
      SinkEvidenceMode::Off,
    );
    assert_eq!(fields.symbol_reachability.len(), 3);
    assert_eq!(
      fields.symbol_reachability[2].source.literal.expose_raw(),
      "local_secret"
    );

    // KNOWN LIMIT v1: concat, containers, and reflection are not evaluated.
    let misses = analyze_source(
      "Case.java",
      r#"
class Case {
  void f() {
    String k = "x";
    request.setRequestProperty("Authorization", "Bearer " + k);
    map.put("k", "sec");
    request.setRequestProperty("Authorization", map.get("k"));
  }
}
"#,
      SinkEvidenceMode::Off,
    );
    assert!(misses.symbol_reachability.is_empty());
  }

  #[test]
  fn sink_evidence_is_default_off_and_deduped_when_literal_flow_exists() {
    let off = analyze_source(
      "case.js",
      "client.login(user_input);\n",
      SinkEvidenceMode::Off,
    );
    assert!(off.sink_evidence.is_empty());

    let on = analyze_source(
      "case.js",
      "client.login(user_input);\n",
      SinkEvidenceMode::On,
    );
    assert_eq!(on.sink_evidence.len(), 1);
    assert_eq!(
      on.sink_evidence[0].confidence,
      SemanticConfidence::SyntaxEvidenceOnly
    );

    let dedup = analyze_source(
      "case.js",
      "const key = \"secret\";\nclient.login(key);\n",
      SinkEvidenceMode::On,
    );
    assert_eq!(dedup.symbol_reachability.len(), 1);
    assert!(dedup.sink_evidence.is_empty());
  }

  #[test]
  fn ast_scope_not_corrupted_by_braces_in_strings_or_comments() {
    let result = analyze_source(
      "case.js",
      r#"
const noise = "}";
// }
{
  const key = "secret";
  client.login(key);
}
"#,
      SinkEvidenceMode::Off,
    );

    assert_eq!(result.symbol_reachability.len(), 1);
    assert_eq!(result.symbol_reachability[0].source.variable, "key");
  }

  fn file(path: &str, content: &str) -> (PathBuf, String) {
    (PathBuf::from(path), content.to_string())
  }

  fn assert_one_cross_file(files: Vec<(PathBuf, String)>, expected_secret: &str) {
    let result = analyze_sources_with_module_graph(&files, SinkEvidenceMode::Off);
    assert_eq!(result.symbol_reachability.len(), 1, "{expected_secret}");
    let finding = &result.symbol_reachability[0];
    assert_eq!(finding.source.literal.expose_raw(), expected_secret);
    assert_eq!(finding.flow.len(), 4);
    assert_eq!(finding.flow[0].kind, FlowStepKind::Definition);
    assert_eq!(finding.flow[1].kind, FlowStepKind::Export);
    assert_eq!(finding.flow[2].kind, FlowStepKind::Import);
    assert_eq!(finding.flow[3].kind, FlowStepKind::Sink);
  }

  #[test]
  fn cross_file_one_hop_positive_python_js_go_and_java() {
    assert_one_cross_file(
      vec![
        file("/repo/a.py", "SECRET = \"py_secret\"\n"),
        file("/repo/b.py", "from a import SECRET\nclient.login(SECRET)\n"),
      ],
      "py_secret",
    );

    assert_one_cross_file(
      vec![
        file("/repo/a.js", "export const SECRET = \"js_secret\";\n"),
        file(
          "/repo/b.js",
          "import { SECRET } from \"./a.js\";\nclient.login(SECRET);\n",
        ),
      ],
      "js_secret",
    );

    assert_one_cross_file(
      vec![
        file("/repo/a.ts", "export const SECRET = \"ts_secret\";\n"),
        file(
          "/repo/b.ts",
          "import { SECRET } from \"./a.ts\";\nclient.login(SECRET);\n",
        ),
      ],
      "ts_secret",
    );

    assert_one_cross_file(
      vec![
        file(
          "/repo/a/secrets.go",
          "package a\nvar Secret = \"go_secret\"\n",
        ),
        file(
          "/repo/main.go",
          "package main\nimport a \"./a\"\nfunc f(){ h.Set(\"Authorization\", a.Secret) }\n",
        ),
      ],
      "go_secret",
    );

    assert_one_cross_file(
      vec![
        file(
          "/repo/a/Secrets.java",
          "package a; public class Secrets { private final String SECRET = \"java_secret\"; }\n",
        ),
        file(
          "/repo/App.java",
          "import a.Secrets; import com.amazonaws.auth.BasicAWSCredentials; class App { void f(){ new BasicAWSCredentials(id, Secrets.SECRET); } }\n",
        ),
      ],
      "java_secret",
    );
  }

  #[test]
  fn cross_file_negatives_depth_cap_and_secret_safety() {
    for files in [
      vec![
        file("/repo/a.py", "SECRET = \"py_secret\"\n"),
        file("/repo/b.py", "from a import SECRET\nprint('unused')\n"),
      ],
      vec![
        file("/repo/a.js", "export const SECRET = \"js_secret\";\n"),
        file(
          "/repo/b.js",
          "import { SECRET } from \"./a.js\";\nconsole.log('unused');\n",
        ),
      ],
      vec![
        file(
          "/repo/a/secrets.go",
          "package a\nvar Secret = \"go_secret\"\n",
        ),
        file(
          "/repo/main.go",
          "package main\nimport a \"./a\"\nfunc f(){ println(\"unused\") }\n",
        ),
      ],
      vec![
        file(
          "/repo/a/Secrets.java",
          "package a; public class Secrets { private final String SECRET = \"java_secret\"; }\n",
        ),
        file(
          "/repo/App.java",
          "import a.Secrets; class App { void f(){ System.out.println(\"unused\"); } }\n",
        ),
      ],
    ] {
      let no_use = analyze_sources_with_module_graph(&files, SinkEvidenceMode::Off);
      assert!(no_use.symbol_reachability.is_empty());
    }

    let unrelated = analyze_sources_with_module_graph(
      &[
        file("/repo/a.py", "OTHER = \"wrong\"\n"),
        file("/repo/c.py", "SECRET = \"right\"\n"),
        file("/repo/b.py", "from a import SECRET\nclient.login(SECRET)\n"),
      ],
      SinkEvidenceMode::Off,
    );
    assert!(unrelated.symbol_reachability.is_empty());

    let unrelated_js = analyze_sources_with_module_graph(
      &[
        file("/repo/a.js", "export const b = \"wrong\";\n"),
        file("/repo/c.js", "export const a = \"right\";\n"),
        file(
          "/repo/b.js",
          "import { a } from \"./a.js\";\nclient.login(a);\n",
        ),
      ],
      SinkEvidenceMode::Off,
    );
    assert!(unrelated_js.symbol_reachability.is_empty());

    let unrelated_go = analyze_sources_with_module_graph(
      &[
        file("/repo/a/other.go", "package a\nvar Other = \"wrong\"\n"),
        file("/repo/c/secrets.go", "package c\nvar Secret = \"right\"\n"),
        file(
          "/repo/main.go",
          "package main\nimport a \"./a\"\nfunc f(){ h.Set(\"Authorization\", a.Secret) }\n",
        ),
      ],
      SinkEvidenceMode::Off,
    );
    assert!(unrelated_go.symbol_reachability.is_empty());

    let unrelated_java = analyze_sources_with_module_graph(
      &[
        file(
          "/repo/a/Secrets.java",
          "package a; public class Secrets { private final String OTHER = \"wrong\"; }\n",
        ),
        file(
          "/repo/c/Secrets.java",
          "package c; public class Secrets { private final String SECRET = \"right\"; }\n",
        ),
        file(
          "/repo/App.java",
          "import a.Secrets; class App { void f(){ new BasicAWSCredentials(id, Secrets.SECRET); } }\n",
        ),
      ],
      SinkEvidenceMode::Off,
    );
    assert!(unrelated_java.symbol_reachability.is_empty());

    let depth = analyze_sources_with_module_graph(
      &[
        file("/repo/a.py", "SECRET = \"py_secret\"\n"),
        file("/repo/b.py", "from a import SECRET\n"),
        file("/repo/c.py", "from b import SECRET\nclient.login(SECRET)\n"),
      ],
      SinkEvidenceMode::Off,
    );
    assert!(depth.symbol_reachability.is_empty());
    assert!(depth
      .warnings
      .iter()
      .any(|warning| warning.code == "CROSS_FILE_DEPTH_CAP"));

    let depth_js = analyze_sources_with_module_graph(
      &[
        file("/repo/a.js", "export const SECRET = \"js_secret\";\n"),
        file("/repo/b.js", "export { SECRET } from \"./a.js\";\n"),
        file(
          "/repo/c.js",
          "import { SECRET } from \"./b.js\";\nclient.login(SECRET);\n",
        ),
      ],
      SinkEvidenceMode::Off,
    );
    assert!(depth_js.symbol_reachability.is_empty());
    assert!(depth_js
      .warnings
      .iter()
      .any(|warning| warning.code == "CROSS_FILE_DEPTH_CAP"));

    let destructured_miss = analyze_sources_with_module_graph(
      &[
        file("/repo/x.js", "export const b = \"js_secret\";\n"),
        file(
          "/repo/y.js",
          "import { a } from \"./x.js\";\nclient.login(a);\n",
        ),
      ],
      SinkEvidenceMode::Off,
    );
    assert!(destructured_miss.symbol_reachability.is_empty());

    let result = analyze_sources_with_module_graph(
      &[
        file(
          "/repo/a.js",
          "export const SECRET = \"raw_cross_file_secret\";\n",
        ),
        file(
          "/repo/b.js",
          "import { SECRET } from \"./a.js\";\nclient.login(SECRET);\n",
        ),
      ],
      SinkEvidenceMode::Off,
    );
    let json = serde_json::to_string(&result).expect("serialize");
    assert!(!json.contains("raw_cross_file_secret"));
    assert!(json.contains("REDACTED"));
  }

  #[test]
  fn module_info_is_minimal_and_does_not_store_source_text() {
    let large = format!(
      "export const SECRET = \"small_secret\";\n// {}\n",
      "large-source-marker".repeat(10_000)
    );
    let info = collect_module_info(Path::new("/repo/a.js"), &large).expect("module info");
    assert!(std::mem::size_of::<ModuleInfo>() <= 256);
    assert_eq!(info.exports.len(), 1);
    assert!(!format!("{info:?}").contains("large-source-marker"));
  }

  #[test]
  fn module_graph_static_gating_skips_most_files() {
    let mut files = Vec::new();
    for idx in 0..5_000 {
      files.push(file(
        &format!("/repo/noise/{idx}.txt"),
        "plain prose with no module or sink tokens\n",
      ));
    }
    files.push(file("/repo/a.js", "export const SECRET = \"js_secret\";\n"));
    files.push(file(
      "/repo/b.js",
      "import { SECRET } from \"./a.js\";\nclient.login(SECRET);\n",
    ));

    let statically_skipped = files
      .iter()
      .filter(|(_, content)| {
        !content_has_sink_token(content) && !content_has_module_graph_token(content)
      })
      .count();
    let skipped_pct = (statically_skipped as f64 / files.len() as f64) * 100.0;
    assert!(skipped_pct > 90.0, "skipped_pct={skipped_pct}");

    let result = analyze_sources_with_module_graph(&files, SinkEvidenceMode::Off);
    assert_eq!(result.symbol_reachability.len(), 1);
  }
}
