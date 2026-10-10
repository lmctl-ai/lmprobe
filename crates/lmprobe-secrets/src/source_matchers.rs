use ast_grep_core::tree_sitter::StrDoc;
use ast_grep_core::Node;
use ast_grep_language::{LanguageExt, SupportLang};
use lmprobe_types::schema::{SourceReadFilterInput, SourceReadHit, SourceReadKind};
use std::path::Path;

use crate::{
  ProviderLanguageId, ReadShape, ReadShapeKind, Source, SourceCategory, SourceDb, SourceLanguage,
};

type SourceNode<'a> = Node<'a, StrDoc<SupportLang>>;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SourceNodeMatch {
  pub read_text: String,
  pub env_var_name: Option<String>,
  pub key_name: Option<String>,
  pub access_path: Option<String>,
}

pub fn source_reads_in_content(
  path: &Path,
  content: &str,
  db: &SourceDb,
  requested_language: ProviderLanguageId,
  filters: &[SourceReadFilterInput],
) -> Vec<SourceReadHit> {
  let Some(lang) = support_lang_for_provider_language(requested_language) else {
    return Vec::new();
  };
  let grep = lang.ast_grep(content);
  let root = grep.root();
  let mut hits = Vec::new();
  for source in &db.sources {
    for language_entry in &source.languages {
      if !source_language_matches_provider(requested_language, language_entry.language) {
        continue;
      }
      for shape in &language_entry.read_shapes {
        let import_evidence = source_import_evidence(&root, &shape.import_hints, filters);
        if !shape.import_hints.is_empty() && import_evidence.is_empty() {
          continue;
        }
        for node in root.dfs().filter(|node| node.is_named()) {
          let Some(mut hit) = source_read_hit_from_node(path, &node, source, language_entry, shape)
          else {
            continue;
          };
          hit.import_evidence = import_evidence.clone();
          if filters.is_empty()
            || filters
              .iter()
              .any(|filter| source_read_filter_matches(&hit, shape.kind, filter))
          {
            hits.push(hit);
          }
        }
      }
    }
  }
  hits
}

pub fn source_read_hit_from_node(
  path: &Path,
  node: &SourceNode<'_>,
  source: &Source,
  language_entry: &SourceLanguage,
  shape: &ReadShape,
) -> Option<SourceReadHit> {
  let matched = source_node_match(node, source, shape)?;
  Some(new_source_read_hit(
    path,
    node,
    source,
    language_entry,
    matched,
  ))
}

pub fn source_node_match(
  node: &SourceNode<'_>,
  source: &Source,
  shape: &ReadShape,
) -> Option<SourceNodeMatch> {
  match shape.kind {
    ReadShapeKind::EnvRead => env_read_match(node, source, shape),
    ReadShapeKind::KvGet => kv_get_match(node, shape),
    ReadShapeKind::SdkCall | ReadShapeKind::DotenvLoad => callee_shape_match(node, &shape.callees),
    ReadShapeKind::ConfigLookup => config_lookup_match(node, shape),
  }
}

fn new_source_read_hit(
  path: &Path,
  node: &SourceNode<'_>,
  source: &Source,
  language_entry: &SourceLanguage,
  matched: SourceNodeMatch,
) -> SourceReadHit {
  SourceReadHit {
    file_path: path.to_string_lossy().into_owned(),
    line_number: node.start_pos().line() + 1,
    column: node.start_pos().column(node) + 1,
    source_id: source.id.clone(),
    source_category: source_category_name(source.category).to_string(),
    provider_id: source.provider_id.clone(),
    sdk: language_entry.sdk.clone(),
    read_text: matched.read_text,
    key_name: matched.key_name,
    env_var_name: matched.env_var_name,
    access_path: matched.access_path,
    import_evidence: Vec::new(),
  }
}

fn env_read_match(
  node: &SourceNode<'_>,
  source: &Source,
  shape: &ReadShape,
) -> Option<SourceNodeMatch> {
  let keys = if shape.keys.is_empty() {
    &source.env_var_names
  } else {
    &shape.keys
  };
  if let Some((callee, arguments)) = call_parts(node) {
    let access_path = exact_path_match(&callee, &shape.access_paths)?;
    let env_var_name =
      first_string_argument(&arguments).and_then(|name| exact_match(&name, keys))?;
    return Some(SourceNodeMatch {
      read_text: node.text().to_string(),
      env_var_name: Some(env_var_name),
      key_name: None,
      access_path: Some(access_path),
    });
  }

  match node.kind().as_ref() {
    "subscript" | "subscript_expression" => {
      let object = node
        .field("value")
        .or_else(|| node.field("object"))
        .or_else(|| node.field("argument"))?;
      let access_path = exact_path_match(&node_text_compact(&object), &shape.access_paths)?;
      let key = subscript_key_text(node).and_then(|name| exact_match(&name, keys))?;
      Some(SourceNodeMatch {
        read_text: node.text().to_string(),
        env_var_name: Some(key),
        key_name: None,
        access_path: Some(access_path),
      })
    }
    "member_expression" => {
      let object = node.field("object")?;
      let access_path = exact_path_match(&node_text_compact(&object), &shape.access_paths)?;
      let property = node
        .field("property")
        .or_else(|| node.field("name"))
        .map(|property| strip_literal(&property.text()))?;
      let key = exact_match(&property, keys)?;
      Some(SourceNodeMatch {
        read_text: node.text().to_string(),
        env_var_name: Some(key),
        key_name: None,
        access_path: Some(access_path),
      })
    }
    _ => None,
  }
}

fn kv_get_match(node: &SourceNode<'_>, shape: &ReadShape) -> Option<SourceNodeMatch> {
  let (callee, arguments) = call_parts(node)?;
  let access_path = receiver_qualified_path_match(&callee, &shape.access_paths)?;
  let key_name = keyword_or_literal_key(&arguments, &shape.key_names).or_else(|| {
    first_string_argument(&arguments).and_then(|arg| exact_path_match(&arg, &shape.key_names))
  })?;
  Some(SourceNodeMatch {
    read_text: node.text().to_string(),
    env_var_name: None,
    key_name: Some(key_name),
    access_path: Some(access_path),
  })
}

fn callee_shape_match(node: &SourceNode<'_>, callees: &[String]) -> Option<SourceNodeMatch> {
  let (callee, _) = call_parts(node)?;
  let access_path = receiver_qualified_path_match(&callee, callees)?;
  Some(SourceNodeMatch {
    read_text: node.text().to_string(),
    env_var_name: None,
    key_name: None,
    access_path: Some(access_path),
  })
}

fn config_lookup_match(node: &SourceNode<'_>, shape: &ReadShape) -> Option<SourceNodeMatch> {
  match node.kind().as_ref() {
    "subscript" | "subscript_expression" => {
      let object = node
        .field("value")
        .or_else(|| node.field("object"))
        .or_else(|| node.field("argument"))?;
      let access_path = exact_path_match(&node_text_compact(&object), &shape.object_paths)?;
      let key =
        subscript_key_text(node).and_then(|name| exact_path_match(&name, &shape.key_names))?;
      Some(SourceNodeMatch {
        read_text: node.text().to_string(),
        env_var_name: None,
        key_name: Some(key),
        access_path: Some(access_path),
      })
    }
    "member_expression" => {
      let object = node.field("object")?;
      let access_path = exact_path_match(&node_text_compact(&object), &shape.object_paths)?;
      let property = node
        .field("property")
        .or_else(|| node.field("name"))
        .map(|property| strip_literal(&property.text()))?;
      let key = exact_match(&property, &shape.key_names)?;
      Some(SourceNodeMatch {
        read_text: node.text().to_string(),
        env_var_name: None,
        key_name: Some(key),
        access_path: Some(access_path),
      })
    }
    _ => None,
  }
}

fn call_parts<'a>(node: &SourceNode<'a>) -> Option<(String, SourceNode<'a>)> {
  if !matches!(
    node.kind().as_ref(),
    "call" | "call_expression" | "method_invocation" | "new_expression"
  ) {
    return None;
  }
  let arguments = node.field("arguments").or_else(|| {
    node
      .children()
      .find(|child| child.is_named() && child.kind().as_ref().contains("argument"))
  })?;
  let callee = if let Some(function) = node.field("function") {
    node_text_compact(&function)
  } else if let (Some(object), Some(name)) = (node.field("object"), node.field("name")) {
    format!(
      "{}.{}",
      node_text_compact(&object),
      node_text_compact(&name)
    )
  } else if let Some(name) = node.field("name") {
    node_text_compact(&name)
  } else {
    let raw = node.text();
    raw
      .rsplit_once('(')
      .map(|(callee, _)| callee)
      .map(compact_text)
      .filter(|text| !text.is_empty())?
  };
  Some((callee, arguments))
}

fn subscript_key_text(node: &SourceNode<'_>) -> Option<String> {
  node
    .field("subscript")
    .or_else(|| node.field("index"))
    .or_else(|| node.field("property"))
    .or_else(|| {
      node
        .children()
        .filter(|child| child.is_named())
        .find(|child| matches!(child.kind().as_ref(), "string" | "string_literal"))
    })
    .map(|key| strip_literal(&key.text()))
}

fn first_string_argument(arguments: &SourceNode<'_>) -> Option<String> {
  argument_nodes(arguments)
    .into_iter()
    .find_map(|argument| string_literal_from_node(&argument))
}

fn keyword_or_literal_key(arguments: &SourceNode<'_>, names: &[String]) -> Option<String> {
  for argument in argument_nodes(arguments) {
    if let Some(name) = argument.field("name").map(|name| node_text_compact(&name)) {
      if let Some(matched) = exact_match(&name, names) {
        return Some(matched);
      }
    }
    if let Some(key) = argument.field("key").map(|key| strip_literal(&key.text())) {
      if let Some(matched) = exact_match(&key, names) {
        return Some(matched);
      }
    }
    if let Some(literal) = string_literal_from_node(&argument) {
      if let Some(matched) = exact_path_match(&literal, names) {
        return Some(matched);
      }
    }
  }
  None
}

fn argument_nodes<'a>(arguments: &SourceNode<'a>) -> Vec<SourceNode<'a>> {
  arguments
    .children()
    .filter(|child| child.is_named())
    .collect()
}

fn string_literal_from_node(node: &SourceNode<'_>) -> Option<String> {
  match node.kind().as_ref() {
    "string" | "string_literal" | "interpreted_string_literal" | "raw_string_literal" => {
      Some(strip_literal(&node.text()))
    }
    _ => node
      .children()
      .filter(|child| child.is_named())
      .find_map(|child| string_literal_from_node(&child)),
  }
}

pub fn strip_literal(text: &str) -> String {
  text
    .trim()
    .trim_matches('"')
    .trim_matches('\'')
    .trim_matches('`')
    .to_string()
}

fn node_text_compact(node: &SourceNode<'_>) -> String {
  compact_text(&node.text())
}

fn compact_text(text: &str) -> String {
  text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn exact_match(actual: &str, candidates: &[String]) -> Option<String> {
  candidates
    .iter()
    .find(|candidate| *candidate == actual)
    .cloned()
}

fn exact_path_match(actual: &str, candidates: &[String]) -> Option<String> {
  let actual = compact_text(actual);
  candidates
    .iter()
    .find(|candidate| actual == compact_text(candidate))
    .cloned()
}

// Source DB access paths are matched by structural equality once the
// relevant AST field has been extracted. SDK/vault callees are the
// exception: they may be receiver-qualified (`client.get_secret_value`)
// while the DB stores the provider method (`get_secret_value`), so only
// a dot-boundary suffix is accepted. Env/config accessors use exact
// matching and never this helper.
fn receiver_qualified_path_match(actual: &str, candidates: &[String]) -> Option<String> {
  let actual = compact_text(actual);
  candidates.iter().find_map(|candidate| {
    let candidate_compact = compact_text(candidate);
    (actual == candidate_compact || actual.ends_with(&format!(".{candidate_compact}")))
      .then(|| candidate.clone())
  })
}

pub fn source_read_filter_matches(
  hit: &SourceReadHit,
  kind: ReadShapeKind,
  filter: &SourceReadFilterInput,
) -> bool {
  filter
    .source_ids
    .as_ref()
    .is_none_or(|ids| ids.iter().any(|id| id == &hit.source_id))
    && filter.provider_ids.as_ref().is_none_or(|ids| {
      hit
        .provider_id
        .as_ref()
        .is_some_and(|provider_id| ids.iter().any(|id| id == provider_id))
    })
    && filter.categories.as_ref().is_none_or(|categories| {
      categories
        .iter()
        .any(|category| *category == source_read_kind(kind))
    })
    && filter.env_var_names.as_ref().is_none_or(|names| {
      hit
        .env_var_name
        .as_ref()
        .is_some_and(|name| names.iter().any(|candidate| candidate == name))
    })
    && filter.key_names.as_ref().is_none_or(|names| {
      hit
        .key_name
        .as_ref()
        .is_some_and(|name| names.iter().any(|candidate| candidate == name))
    })
    && filter.access_paths.as_ref().is_none_or(|paths| {
      hit
        .access_path
        .as_ref()
        .is_some_and(|path| paths.iter().any(|candidate| candidate == path))
    })
    && filter.callees.as_ref().is_none_or(|callees| {
      callees.iter().any(|callee| {
        hit
          .access_path
          .as_ref()
          .is_some_and(|access_path| access_path == callee)
      })
    })
    && filter.object_paths.as_ref().is_none_or(|paths| {
      paths.iter().any(|path| {
        hit
          .access_path
          .as_ref()
          .is_some_and(|access_path| access_path == path)
      })
    })
    && filter.import_hints.as_ref().is_none_or(|hints| {
      hints
        .iter()
        .any(|hint| hit.import_evidence.iter().any(|evidence| evidence == hint))
    })
}

pub fn source_import_evidence(
  root: &SourceNode<'_>,
  shape_hints: &[String],
  filters: &[SourceReadFilterInput],
) -> Vec<String> {
  let mut hints = shape_hints.to_vec();
  for hint in filters
    .iter()
    .filter_map(|filter| filter.import_hints.as_ref())
    .flatten()
  {
    if !hints.contains(hint) {
      hints.push(hint.clone());
    }
  }
  hints
    .into_iter()
    .filter(|hint| {
      root.dfs().filter(|node| node.is_named()).any(|node| {
        node.kind().as_ref().contains("import") && import_hint_matches(&node.text(), hint)
      })
    })
    .collect()
}

fn import_hint_matches(import_text: &str, hint: &str) -> bool {
  import_text
    .split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '/' | '.' | '@')))
    .filter(|token| !token.is_empty())
    .any(|token| {
      token == hint || token.ends_with(&format!("/{hint}")) || token.ends_with(&format!(".{hint}"))
    })
}

pub fn source_read_kind(kind: ReadShapeKind) -> SourceReadKind {
  match kind {
    ReadShapeKind::EnvRead => SourceReadKind::EnvRead,
    ReadShapeKind::KvGet => SourceReadKind::KvGet,
    ReadShapeKind::SdkCall => SourceReadKind::SdkCall,
    ReadShapeKind::DotenvLoad => SourceReadKind::DotenvLoad,
    ReadShapeKind::ConfigLookup => SourceReadKind::ConfigLookup,
  }
}

pub fn source_category_name(category: SourceCategory) -> &'static str {
  match category {
    SourceCategory::Env => "env",
    SourceCategory::Vault => "vault",
    SourceCategory::SecretManager => "secret-manager",
    SourceCategory::Dotenv => "dotenv",
    SourceCategory::ConfigFile => "config-file",
  }
}

fn support_lang_for_provider_language(language: ProviderLanguageId) -> Option<SupportLang> {
  match language {
    ProviderLanguageId::Python => Some(SupportLang::Python),
    ProviderLanguageId::JavaScript => Some(SupportLang::JavaScript),
    ProviderLanguageId::TypeScript => Some(SupportLang::TypeScript),
    ProviderLanguageId::Go => Some(SupportLang::Go),
    ProviderLanguageId::Java => Some(SupportLang::Java),
  }
}

pub fn source_language_matches_provider(
  requested_language: ProviderLanguageId,
  provider_language: ProviderLanguageId,
) -> bool {
  matches!(
    (requested_language, provider_language),
    (ProviderLanguageId::Python, ProviderLanguageId::Python)
      | (
        ProviderLanguageId::JavaScript,
        ProviderLanguageId::JavaScript
      )
      | (
        ProviderLanguageId::TypeScript,
        ProviderLanguageId::TypeScript
      )
      | (
        ProviderLanguageId::TypeScript,
        ProviderLanguageId::JavaScript
      )
      | (ProviderLanguageId::Go, ProviderLanguageId::Go)
      | (ProviderLanguageId::Java, ProviderLanguageId::Java)
  )
}
