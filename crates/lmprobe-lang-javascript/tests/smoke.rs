//! Smoke tests for `lmprobe-lang-javascript` against the in-crate
//! JS fixture (`fixtures/app.js`).
//!
//! Asserts the trait impls produce reasonable patterns for a few
//! representative shapes. The binary's deep + smoke tiers are the
//! cross-language regression matrix; this is the per-crate gate
//! the upcoming xtask check-lang lint references.

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, ImportGrammar, InheritanceGrammar, KindTaxonomy, PortableKind,
  TraceTemplate,
};
use lmprobe_lang_javascript::{
  JavaScriptCallGrammar, JavaScriptDecorationGrammar, JavaScriptExtractor, JavaScriptImportGrammar,
  JavaScriptInheritanceGrammar, JavaScriptKindTaxonomy, JavaScriptTopLevelKinds,
  JavaScriptTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = JavaScriptExtractor;
  let _ = JavaScriptCallGrammar;
  let _ = JavaScriptImportGrammar;
  let _ = JavaScriptInheritanceGrammar;
  let _ = JavaScriptDecorationGrammar;
  let _ = JavaScriptTraceTemplate;
  let _ = JavaScriptKindTaxonomy;
  let _ = JavaScriptTopLevelKinds;
}

#[test]
fn taxonomy_function_kinds() {
  let kinds = JavaScriptKindTaxonomy.kind_for(PortableKind::Function);
  assert_eq!(
    kinds,
    &[
      "function_declaration",
      "function_expression",
      "arrow_function"
    ]
  );
}

#[test]
fn taxonomy_method_kind() {
  let kinds = JavaScriptKindTaxonomy.kind_for(PortableKind::Method);
  assert_eq!(kinds, &["method_definition"]);
}

#[test]
fn taxonomy_class_kind() {
  let kinds = JavaScriptKindTaxonomy.kind_for(PortableKind::Class);
  assert_eq!(kinds, &["class_declaration"]);
}

#[test]
fn taxonomy_no_interface_for_js() {
  let kinds = JavaScriptKindTaxonomy.kind_for(PortableKind::Interface);
  assert!(kinds.is_empty(), "JS has no Interface kind");
}

#[test]
fn taxonomy_no_enum_for_js() {
  let kinds = JavaScriptKindTaxonomy.kind_for(PortableKind::Enum);
  assert!(kinds.is_empty(), "JS has no Enum kind");
}

#[test]
fn taxonomy_const_kinds() {
  let kinds = JavaScriptKindTaxonomy.kind_for(PortableKind::Const);
  assert_eq!(kinds, &["lexical_declaration", "variable_declaration"]);
}

#[test]
fn taxonomy_import_kind() {
  let kinds = JavaScriptKindTaxonomy.kind_for(PortableKind::Import);
  assert_eq!(kinds, &["import_statement"]);
}

#[test]
fn import_grammar_patterns() {
  let p = JavaScriptImportGrammar.import_patterns("express");
  assert!(p
    .iter()
    .any(|s| s.contains("import") && s.contains("express")));
  assert!(p.iter().any(|s| s.contains("require")));
}

#[test]
fn export_grammar_patterns() {
  let p = JavaScriptImportGrammar.export_patterns("greet");
  assert!(p.iter().any(|s| s.contains("export function greet")));
  assert!(p.iter().any(|s| s.contains("export const greet")));
  assert!(p.iter().any(|s| s.contains("module.exports.greet")));
}

#[test]
fn decoration_grammar_at_patterns() {
  let p = JavaScriptDecorationGrammar.decoration_patterns("authorize");
  assert!(p.iter().any(|s| s == "@authorize"));
  assert!(p.iter().any(|s| s == "@authorize($$$)"));
}

#[test]
fn trace_template_console_error() {
  let t = JavaScriptTraceTemplate.template_for("x").expect("template");
  assert!(t.contains("console.error"));
  assert!(t.contains("[TRACE]"));
}

#[test]
fn inheritance_supertype_extends() {
  let p = JavaScriptInheritanceGrammar.supertype_decl_patterns("Base");
  assert!(p.iter().any(|s| s.contains("class Base extends")));
}

#[test]
fn inheritance_subtype_extends() {
  let p = JavaScriptInheritanceGrammar.subtype_patterns("Base");
  assert!(p.iter().any(|s| s.contains("extends Base")));
}

#[test]
fn call_grammar_standard_shapes() {
  let p = JavaScriptCallGrammar.call_patterns("greet");
  assert!(p.iter().any(|s| s == "greet($$$ARGS)"));
  assert!(p.iter().any(|s| s == "$OBJ.greet($$$ARGS)"));
}

#[test]
fn fixture_file_exists() {
  let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("fixtures")
    .join("app.js");
  assert!(
    path.exists(),
    "fixtures/app.js must exist for smoke harnesses"
  );
}
