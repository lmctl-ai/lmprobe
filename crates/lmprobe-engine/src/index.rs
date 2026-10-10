//! Shared indexing primitives.
//!
//! Holds the kind tables and AST helpers that several sensors
//! consume. As D5 traits land per-language extractors will move out
//! of here into `langs.rs`; for Phase 0 these are the same hard-coded
//! lists the pre-split binary used.

use lmprobe_lang_core::langs::SgLang;

/// Kinds the evolution walker considers — the §1.4 KindTaxonomy
/// covers everything else, but evolution still needs a small
/// hard-coded set because it walks per-commit blobs without going
/// through the per-language taxonomy machinery.
pub const EVOLUTION_KINDS: &[&str] = &[
  "function_item",
  "struct_item",
  "enum_item",
  "trait_item",
  "type_item",
  "class_declaration",
  "function_declaration",
  "method_declaration",
  "interface_declaration",
  "function_definition",
  "class_definition",
];

/// Render a node as a "skeleton" — declaration line(s) with the body
/// elided. Used by `--outline` and by `evolution` for signature
/// comparison across commits.
pub fn create_skeleton(
  m: &ast_grep_core::NodeMatch<ast_grep_core::tree_sitter::StrDoc<SgLang>>,
) -> String {
  let body_fields = ["body", "value", "content", "block"];
  for field in body_fields {
    if let Some(body) = m.field(field) {
      let full_text = m.text();
      let start = body.range().start - m.range().start;
      let end = body.range().end - m.range().start;
      let mut skeleton = full_text[..start].to_string();
      skeleton.push_str(" { ... }");
      skeleton.push_str(&full_text[end..]);
      return skeleton;
    }
  }
  let text = m.text();
  let lines: Vec<_> = text.lines().take(2).collect();
  if lines.len() < text.lines().count() {
    format!("{}\n  ...", lines.join("\n"))
  } else {
    text.to_string()
  }
}

/// Plain-text variant — same line-truncation behaviour as
/// `create_skeleton` but without an AST node. Used by §3.4 context
/// bundle's `file_outline` where we already have the matched text
/// but not the original NodeMatch.
pub fn create_skeleton_from_text(text: &str) -> String {
  let lines: Vec<_> = text.lines().take(2).collect();
  if lines.len() < text.lines().count() {
    format!("{}\n  ...", lines.join("\n"))
  } else {
    text.to_string()
  }
}
