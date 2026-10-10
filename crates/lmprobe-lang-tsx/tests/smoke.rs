//! Smoke tests for `lmprobe-lang-tsx` against the in-crate
//! TSX fixture (`fixtures/app.tsx`).
//!
//! Asserts the trait impls produce reasonable patterns for a few
//! representative shapes. The binary's deep + smoke tiers are the
//! cross-language regression matrix; this is the per-crate gate
//! the upcoming xtask check-lang lint references.

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, ImportGrammar, InheritanceGrammar, KindTaxonomy, PortableKind,
  TraceTemplate,
};
use lmprobe_lang_tsx::{
  TsxCallGrammar, TsxDecorationGrammar, TsxExtractor, TsxImportGrammar, TsxInheritanceGrammar,
  TsxKindTaxonomy, TsxTopLevelKinds, TsxTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = TsxExtractor;
  let _ = TsxCallGrammar;
  let _ = TsxImportGrammar;
  let _ = TsxInheritanceGrammar;
  let _ = TsxDecorationGrammar;
  let _ = TsxTraceTemplate;
  let _ = TsxKindTaxonomy;
  let _ = TsxTopLevelKinds;
}

#[test]
fn taxonomy_function_kinds() {
  let kinds = TsxKindTaxonomy.kind_for(PortableKind::Function);
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
  let kinds = TsxKindTaxonomy.kind_for(PortableKind::Method);
  assert_eq!(kinds, &["method_definition"]);
}

#[test]
fn taxonomy_class_kind() {
  let kinds = TsxKindTaxonomy.kind_for(PortableKind::Class);
  assert_eq!(kinds, &["class_declaration"]);
}

#[test]
fn taxonomy_interface_kind() {
  let kinds = TsxKindTaxonomy.kind_for(PortableKind::Interface);
  assert_eq!(kinds, &["interface_declaration"]);
}

#[test]
fn taxonomy_enum_kind() {
  let kinds = TsxKindTaxonomy.kind_for(PortableKind::Enum);
  assert_eq!(kinds, &["enum_declaration"]);
}

#[test]
fn taxonomy_type_alias_kind() {
  let kinds = TsxKindTaxonomy.kind_for(PortableKind::TypeAlias);
  assert_eq!(kinds, &["type_alias_declaration"]);
}

#[test]
fn taxonomy_const_kinds() {
  let kinds = TsxKindTaxonomy.kind_for(PortableKind::Const);
  assert_eq!(kinds, &["lexical_declaration", "variable_declaration"]);
}

#[test]
fn taxonomy_import_kind() {
  let kinds = TsxKindTaxonomy.kind_for(PortableKind::Import);
  assert_eq!(kinds, &["import_statement"]);
}

#[test]
fn import_grammar_patterns() {
  let p = TsxImportGrammar.import_patterns("react");
  assert!(p
    .iter()
    .any(|s| s.contains("import") && s.contains("react")));
  assert!(p.iter().any(|s| s.contains("require")));
}

#[test]
fn export_grammar_patterns() {
  let p = TsxImportGrammar.export_patterns("greet");
  assert!(p.iter().any(|s| s.contains("export function greet")));
  assert!(p.iter().any(|s| s.contains("export const greet")));
  assert!(p.iter().any(|s| s.contains("module.exports.greet")));
}

#[test]
fn decoration_grammar_at_patterns() {
  let p = TsxDecorationGrammar.decoration_patterns("authorize");
  assert!(p.iter().any(|s| s == "@authorize"));
  assert!(p.iter().any(|s| s == "@authorize($$$)"));
}

#[test]
fn trace_template_console_error() {
  let t = TsxTraceTemplate.template_for("x").expect("template");
  assert!(t.contains("console.error"));
  assert!(t.contains("[TRACE]"));
}

#[test]
fn inheritance_supertype_extends() {
  let p = TsxInheritanceGrammar.supertype_decl_patterns("Base");
  assert!(p.iter().any(|s| s.contains("class Base extends")));
}

#[test]
fn inheritance_subtype_extends() {
  let p = TsxInheritanceGrammar.subtype_patterns("Base");
  assert!(p.iter().any(|s| s.contains("extends Base")));
}

#[test]
fn call_grammar_standard_shapes() {
  let p = TsxCallGrammar.call_patterns("greet");
  assert!(p.iter().any(|s| s == "greet($$$ARGS)"));
  assert!(p.iter().any(|s| s == "$OBJ.greet($$$ARGS)"));
}

#[test]
fn fixture_file_exists() {
  let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("fixtures")
    .join("app.tsx");
  assert!(
    path.exists(),
    "fixtures/app.tsx must exist for smoke harnesses"
  );
}
