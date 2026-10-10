//! Phase 3 AST false-positive context classification.
//!
//! This is deliberately a conservative positional classifier over existing
//! Layer-1 findings. It is not data flow and it never creates findings.

use crate::{AstContext, SecretFinding, SuppressedSecretFinding};
use ast_grep_core::tree_sitter::StrDoc;
use ast_grep_core::Node;
use ast_grep_language::{LanguageExt, SupportLang};
use std::ops::Range;
use std::path::Path;

type AstNode<'a> = Node<'a, StrDoc<SupportLang>>;

trait AstContextExt {
  fn is_downgraded(&self) -> bool;
  fn is_suppressible(&self) -> bool;
}

impl AstContextExt for AstContext {
  fn is_downgraded(&self) -> bool {
    *self != Self::CodeString
  }

  fn is_suppressible(&self) -> bool {
    matches!(
      *self,
      Self::Comment | Self::ImportPath | Self::ConstEnumIdentifier | Self::TestOrGeneratedPath
    )
  }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub enum AstFpCutMode {
  Off,
  #[default]
  Annotate,
  Suppress,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AstFpCutOutcome {
  pub findings: Vec<SecretFinding>,
  pub suppressed: Vec<SuppressedSecretFinding>,
  pub ast_files_parsed: usize,
}

#[derive(Debug, Clone, Copy)]
struct LangContextTable {
  lang: SupportLang,
  comment_kinds: &'static [&'static str],
  string_kinds: &'static [&'static str],
  import_ancestor_kinds: &'static [&'static str],
  const_enum_ancestor_kinds: &'static [&'static str],
  identifier_kinds: &'static [&'static str],
}

const PYTHON_TABLE: LangContextTable = LangContextTable {
  lang: SupportLang::Python,
  comment_kinds: &["comment"],
  string_kinds: &["string"],
  import_ancestor_kinds: &["import_statement", "import_from_statement"],
  const_enum_ancestor_kinds: &["assignment", "class_definition"],
  identifier_kinds: &["identifier"],
};

const JAVASCRIPT_TABLE: LangContextTable = LangContextTable {
  lang: SupportLang::JavaScript,
  comment_kinds: &["comment"],
  string_kinds: &["string", "template_string"],
  import_ancestor_kinds: &["import_statement"],
  const_enum_ancestor_kinds: &[
    "lexical_declaration",
    "variable_declaration",
    "enum_declaration",
    "type_alias_declaration",
    "interface_declaration",
  ],
  identifier_kinds: &["identifier", "property_identifier", "type_identifier"],
};

const TYPESCRIPT_TABLE: LangContextTable = LangContextTable {
  lang: SupportLang::TypeScript,
  ..JAVASCRIPT_TABLE
};

const TSX_TABLE: LangContextTable = LangContextTable {
  lang: SupportLang::Tsx,
  ..JAVASCRIPT_TABLE
};

const GO_TABLE: LangContextTable = LangContextTable {
  lang: SupportLang::Go,
  comment_kinds: &["comment"],
  string_kinds: &["interpreted_string_literal", "raw_string_literal"],
  import_ancestor_kinds: &["import_declaration", "import_spec", "import_spec_list"],
  const_enum_ancestor_kinds: &[
    "const_declaration",
    "const_spec",
    "type_declaration",
    "type_spec",
  ],
  identifier_kinds: &["identifier", "field_identifier", "type_identifier"],
};

const JAVA_TABLE: LangContextTable = LangContextTable {
  lang: SupportLang::Java,
  comment_kinds: &["line_comment", "block_comment"],
  string_kinds: &["string_literal", "text_block"],
  import_ancestor_kinds: &["import_declaration", "scoped_identifier"],
  const_enum_ancestor_kinds: &[
    "enum_declaration",
    "enum_constant",
    "field_declaration",
    "constant_declaration",
  ],
  identifier_kinds: &["identifier", "enum_constant"],
};

pub fn apply_ast_fp_cut(
  file_path: &str,
  content: &str,
  findings: Vec<SecretFinding>,
  mode: AstFpCutMode,
) -> AstFpCutOutcome {
  if findings.is_empty() {
    return AstFpCutOutcome::default();
  }
  if mode == AstFpCutMode::Off {
    return AstFpCutOutcome {
      findings,
      suppressed: Vec::new(),
      ast_files_parsed: 0,
    };
  }

  let path_context =
    is_test_or_generated_path(file_path).then_some(AstContext::TestOrGeneratedPath);
  let table = path_context
    .is_none()
    .then(|| table_for_path(file_path))
    .flatten();
  let ast_files_parsed = usize::from(table.is_some());

  let mut active = Vec::new();
  let mut suppressed = Vec::new();
  if let Some(table) = table {
    let grep = table.lang.ast_grep(content);
    let root = grep.root();
    for finding in findings {
      let context = classify_finding(content, &root, table, &finding);
      push_classified_finding(finding, context, mode, &mut active, &mut suppressed);
    }
  } else {
    for finding in findings {
      let context = path_context.unwrap_or(AstContext::CodeString);
      push_classified_finding(finding, context, mode, &mut active, &mut suppressed);
    }
  }

  AstFpCutOutcome {
    findings: active,
    suppressed,
    ast_files_parsed,
  }
}

fn push_classified_finding(
  mut finding: SecretFinding,
  context: AstContext,
  mode: AstFpCutMode,
  active: &mut Vec<SecretFinding>,
  suppressed: &mut Vec<SuppressedSecretFinding>,
) {
  finding.ast_context = Some(context);
  if context.is_downgraded() {
    finding.confidence = crate::scanner::CONFIDENCE_RULE_COMPATIBLE_LOW.into();
  }

  if mode == AstFpCutMode::Suppress && context.is_suppressible() {
    suppressed.push(SuppressedSecretFinding {
      finding,
      suppressed_reason: context,
    });
  } else {
    active.push(finding);
  }
}

fn classify_finding(
  content: &str,
  root: &AstNode<'_>,
  table: LangContextTable,
  finding: &SecretFinding,
) -> AstContext {
  let Some(range) = finding_secret_range(content, finding) else {
    return AstContext::CodeString;
  };
  let Some(node) = smallest_covering_node(root, range) else {
    return AstContext::CodeString;
  };
  classify_node(&node, table)
}

fn classify_node(node: &AstNode<'_>, table: LangContextTable) -> AstContext {
  let node_kind = node.kind();
  if table.contains(table.comment_kinds, node_kind.as_ref()) {
    if is_doc_comment(node.text().as_ref()) {
      return AstContext::StringDoc;
    }
    return AstContext::Comment;
  }

  let ancestors: Vec<_> = std::iter::once(node.clone())
    .chain(node.ancestors())
    .collect();
  if ancestors
    .iter()
    .any(|ancestor| table.contains(table.comment_kinds, ancestor.kind().as_ref()))
  {
    let comment = ancestors
      .iter()
      .find(|ancestor| table.contains(table.comment_kinds, ancestor.kind().as_ref()))
      .expect("comment ancestor");
    return if is_doc_comment(comment.text().as_ref()) {
      AstContext::StringDoc
    } else {
      AstContext::Comment
    };
  }

  if ancestors
    .iter()
    .any(|ancestor| table.contains(table.import_ancestor_kinds, ancestor.kind().as_ref()))
  {
    return AstContext::ImportPath;
  }

  if is_python_docstring(&ancestors, table) {
    return AstContext::StringDoc;
  }

  if table.contains(table.identifier_kinds, node_kind.as_ref())
    && ancestors
      .iter()
      .any(|ancestor| table.contains(table.const_enum_ancestor_kinds, ancestor.kind().as_ref()))
  {
    return AstContext::ConstEnumIdentifier;
  }

  AstContext::CodeString
}

impl LangContextTable {
  fn contains(self, values: &[&str], value: &str) -> bool {
    values.contains(&value)
  }
}

fn is_python_docstring(ancestors: &[AstNode<'_>], table: LangContextTable) -> bool {
  if table.lang != SupportLang::Python {
    return false;
  }
  if ancestors.is_empty() {
    return false;
  }
  if !ancestors
    .iter()
    .any(|ancestor| table.contains(table.string_kinds, ancestor.kind().as_ref()))
  {
    return false;
  }
  if ancestors
    .iter()
    .any(|ancestor| ancestor.kind() == "assignment")
  {
    return false;
  }
  ancestors
    .iter()
    .any(|ancestor| ancestor.kind() == "expression_statement")
}

fn is_doc_comment(text: &str) -> bool {
  let trimmed = text.trim_start();
  trimmed.starts_with("/**") || trimmed.starts_with("///") || trimmed.starts_with("//!")
}

fn smallest_covering_node<'a>(root: &AstNode<'a>, range: Range<usize>) -> Option<AstNode<'a>> {
  root
    .dfs()
    .filter(|node| {
      let node_range = node.range();
      node_range.start <= range.start && range.end <= node_range.end
    })
    .min_by_key(|node| {
      let node_range = node.range();
      node_range.end.saturating_sub(node_range.start)
    })
}

fn finding_secret_range(content: &str, finding: &SecretFinding) -> Option<Range<usize>> {
  let line_start = byte_offset_for_line(content, finding.start_line)?;
  let search = &content[line_start..];
  let secret = finding.redacted_secret.expose_raw();
  if !secret.is_empty() {
    if let Some(offset) = search.find(secret) {
      let start = line_start + offset;
      return Some(start..start + secret.len());
    }
  }
  let match_text = finding.match_text.expose_raw();
  if !match_text.is_empty() {
    if let Some(offset) = search.find(match_text) {
      let start = line_start + offset;
      return Some(start..start + match_text.len());
    }
  }
  None
}

fn byte_offset_for_line(content: &str, one_based_line: usize) -> Option<usize> {
  if one_based_line == 0 {
    return None;
  }
  if one_based_line == 1 {
    return Some(0);
  }
  let mut current_line = 1;
  for (idx, byte) in content.bytes().enumerate() {
    if byte == b'\n' {
      current_line += 1;
      if current_line == one_based_line {
        return Some(idx + 1);
      }
    }
  }
  None
}

fn table_for_path(path: &str) -> Option<LangContextTable> {
  match Path::new(path)
    .extension()
    .and_then(|extension| extension.to_str())
    .map(|extension| extension.to_ascii_lowercase())
    .as_deref()
  {
    Some("py") | Some("pyw") => Some(PYTHON_TABLE),
    Some("js") | Some("mjs") | Some("cjs") | Some("jsx") => Some(JAVASCRIPT_TABLE),
    Some("ts") | Some("mts") | Some("cts") => Some(TYPESCRIPT_TABLE),
    Some("tsx") => Some(TSX_TABLE),
    Some("go") => Some(GO_TABLE),
    Some("java") => Some(JAVA_TABLE),
    _ => None,
  }
}

fn is_test_or_generated_path(path: &str) -> bool {
  let lower = path.replace('\\', "/").to_ascii_lowercase();
  let parts: Vec<_> = lower.split('/').collect();
  parts.iter().any(|part| {
    matches!(
      *part,
      "test" | "tests" | "testdata" | "fixtures" | "__tests__" | "generated"
    ) || part.contains(".min.")
      || part.ends_with(".generated")
      || part.ends_with("_generated")
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{FileScanInput, Layer1Engine};

  const RULE_CONFIG: &str = r#"
title = "ast-fp-cut-test"
[[rules]]
id = "aws-access-token"
description = "aws"
regex = '''AKIA[0-9A-Z]{16}'''
"#;

  fn scan(path: &str, content: &str, mode: AstFpCutMode) -> AstFpCutOutcome {
    let engine = Layer1Engine::from_config_str(RULE_CONFIG).expect("engine");
    let findings = engine.scan_file(FileScanInput {
      file_path: path,
      content,
      source_mode: "test",
    });
    apply_ast_fp_cut(path, content, findings, mode)
  }

  fn assert_context(path: &str, content: &str, context: AstContext, active: bool) {
    let annotated = scan(path, content, AstFpCutMode::Annotate);
    assert_eq!(annotated.suppressed.len(), 0, "{path} annotate suppressed");
    assert_eq!(annotated.findings.len(), 1, "{path} annotate active");
    assert_eq!(annotated.findings[0].ast_context, Some(context));
    if context == AstContext::CodeString {
      assert_eq!(annotated.findings[0].confidence, "rule-compatible");
    } else {
      assert_eq!(annotated.findings[0].confidence, "rule-compatible-low");
    }

    let suppressed = scan(path, content, AstFpCutMode::Suppress);
    if active {
      assert_eq!(suppressed.findings.len(), 1, "{path} suppress active");
      assert_eq!(suppressed.suppressed.len(), 0, "{path} suppress suppressed");
      assert_eq!(suppressed.findings[0].ast_context, Some(context));
    } else {
      assert_eq!(suppressed.findings.len(), 0, "{path} suppress active");
      assert_eq!(suppressed.suppressed.len(), 1, "{path} suppress suppressed");
      assert_eq!(suppressed.suppressed[0].suppressed_reason, context);
    }
  }

  #[test]
  fn python_context_matrix_is_conservative() {
    assert_context(
      "app.py",
      "aws_secret_access_key = \"AKIAZ3M7Q6YB2N4P5R6S\"\n",
      AstContext::CodeString,
      true,
    );
    assert_context(
      "app.py",
      "# AKIAZ3M7Q6YB2N4P5R6S\n",
      AstContext::Comment,
      false,
    );
    assert_context(
      "app.py",
      "import AKIAZ3M7Q6YB2N4P5R6S\n",
      AstContext::ImportPath,
      false,
    );
    assert_context(
      "app.py",
      "AKIAZ3M7Q6YB2N4P5R6S = 1\n",
      AstContext::ConstEnumIdentifier,
      false,
    );
    assert_context(
      "app.py",
      "\"\"\"AKIAZ3M7Q6YB2N4P5R6S\"\"\"\n",
      AstContext::StringDoc,
      true,
    );
    assert_context(
      "testdata/app.py",
      "aws_secret_access_key = \"AKIAZ3M7Q6YB2N4P5R6S\"\n",
      AstContext::TestOrGeneratedPath,
      false,
    );
  }

  #[test]
  fn javascript_context_matrix_is_conservative() {
    assert_context(
      "app.js",
      "const aws_secret_access_key = \"AKIAZ3M7Q6YB2N4P5R6S\";\n",
      AstContext::CodeString,
      true,
    );
    assert_context(
      "app.js",
      "// AKIAZ3M7Q6YB2N4P5R6S\n",
      AstContext::Comment,
      false,
    );
    assert_context(
      "app.js",
      "import x from \"AKIAZ3M7Q6YB2N4P5R6S\";\n",
      AstContext::ImportPath,
      false,
    );
    assert_context(
      "app.js",
      "const AKIAZ3M7Q6YB2N4P5R6S = 1;\n",
      AstContext::ConstEnumIdentifier,
      false,
    );
    assert_context(
      "app.js",
      "/** AKIAZ3M7Q6YB2N4P5R6S */\nfunction f() {}\n",
      AstContext::StringDoc,
      true,
    );
    assert_context(
      "__tests__/app.js",
      "const aws_secret_access_key = \"AKIAZ3M7Q6YB2N4P5R6S\";\n",
      AstContext::TestOrGeneratedPath,
      false,
    );
  }

  #[test]
  fn typescript_context_matrix_is_conservative() {
    assert_context(
      "app.ts",
      "const aws_secret_access_key = \"AKIAZ3M7Q6YB2N4P5R6S\";\n",
      AstContext::CodeString,
      true,
    );
    assert_context(
      "app.ts",
      "// AKIAZ3M7Q6YB2N4P5R6S\n",
      AstContext::Comment,
      false,
    );
    assert_context(
      "app.ts",
      "import x from \"AKIAZ3M7Q6YB2N4P5R6S\";\n",
      AstContext::ImportPath,
      false,
    );
    assert_context(
      "app.ts",
      "enum K { AKIAZ3M7Q6YB2N4P5R6S }\n",
      AstContext::ConstEnumIdentifier,
      false,
    );
    assert_context(
      "app.ts",
      "/** AKIAZ3M7Q6YB2N4P5R6S */\nfunction f() {}\n",
      AstContext::StringDoc,
      true,
    );
    assert_context(
      "fixtures/app.ts",
      "const aws_secret_access_key = \"AKIAZ3M7Q6YB2N4P5R6S\";\n",
      AstContext::TestOrGeneratedPath,
      false,
    );
  }

  #[test]
  fn go_context_matrix_is_conservative() {
    assert_context(
      "app.go",
      "package main\nfunc f() { aws_secret_access_key := \"AKIAZ3M7Q6YB2N4P5R6S\" }\n",
      AstContext::CodeString,
      true,
    );
    assert_context(
      "app.go",
      "package main\n// AKIAZ3M7Q6YB2N4P5R6S\n",
      AstContext::Comment,
      false,
    );
    assert_context(
      "app.go",
      "package main\nimport \"AKIAZ3M7Q6YB2N4P5R6S\"\n",
      AstContext::ImportPath,
      false,
    );
    assert_context(
      "app.go",
      "package main\nconst AKIAZ3M7Q6YB2N4P5R6S = 1\n",
      AstContext::ConstEnumIdentifier,
      false,
    );
    assert_context(
      "app.go",
      "package main\n/** AKIAZ3M7Q6YB2N4P5R6S */\nfunc f() {}\n",
      AstContext::StringDoc,
      true,
    );
    assert_context(
      "generated/app.go",
      "package main\nfunc f() { aws_secret_access_key := \"AKIAZ3M7Q6YB2N4P5R6S\" }\n",
      AstContext::TestOrGeneratedPath,
      false,
    );
  }

  #[test]
  fn java_context_matrix_is_conservative() {
    assert_context(
      "App.java",
      "class App { void f() { String aws_secret_access_key = \"AKIAZ3M7Q6YB2N4P5R6S\"; } }\n",
      AstContext::CodeString,
      true,
    );
    assert_context(
      "App.java",
      "class App { // AKIAZ3M7Q6YB2N4P5R6S\n}\n",
      AstContext::Comment,
      false,
    );
    assert_context(
      "App.java",
      "import AKIAZ3M7Q6YB2N4P5R6S.Foo;\nclass App {}\n",
      AstContext::ImportPath,
      false,
    );
    assert_context(
      "App.java",
      "enum K { AKIAZ3M7Q6YB2N4P5R6S }\n",
      AstContext::ConstEnumIdentifier,
      false,
    );
    assert_context(
      "App.java",
      "/** AKIAZ3M7Q6YB2N4P5R6S */\nclass App {}\n",
      AstContext::StringDoc,
      true,
    );
    assert_context(
      "test/App.java",
      "class App { void f() { String aws_secret_access_key = \"AKIAZ3M7Q6YB2N4P5R6S\"; } }\n",
      AstContext::TestOrGeneratedPath,
      false,
    );
  }

  #[test]
  fn off_mode_does_not_parse_or_annotate() {
    let outcome = scan("app.py", "# AKIAZ3M7Q6YB2N4P5R6S\n", AstFpCutMode::Off);
    assert_eq!(outcome.ast_files_parsed, 0);
    assert_eq!(outcome.findings.len(), 1);
    assert_eq!(outcome.findings[0].ast_context, None);
  }

  #[test]
  fn no_layer1_hits_do_not_parse() {
    let outcome = apply_ast_fp_cut("app.py", "x = 1\n", Vec::new(), AstFpCutMode::Annotate);
    assert_eq!(outcome.ast_files_parsed, 0);
  }

  #[test]
  fn suppressed_findings_serialize_redacted() {
    let outcome = scan("app.py", "# AKIAZ3M7Q6YB2N4P5R6S\n", AstFpCutMode::Suppress);
    let json = serde_json::to_string(&outcome.suppressed).expect("serialize suppressed");

    assert!(!json.contains("AKIAZ3M7Q6YB2N4P5R6S"));
    assert!(json.contains("REDACTED"));
    assert!(json.contains("suppressed_reason"));
  }
}
