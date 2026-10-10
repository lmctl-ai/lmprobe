//! Smoke tests for `lmprobe-lang-c` against the in-crate
//! C fixtures (`fixtures/*.c`).
//!
//! Asserts the trait impls produce the patterns C needs for the
//! binary's deep + smoke tiers. Per-crate gate — the binary's
//! cross-language regression matrix lives at
//! `crates/lmprobe/tests/{smoke,deep}.rs`.

use lmprobe_lang_c::{
  CCallGrammar, CDecorationGrammar, CExtractor, CImportGrammar, CInheritanceGrammar, CKindTaxonomy,
  CTopLevelKinds, CTraceTemplate,
};
use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, ImportGrammar, InheritanceGrammar, KindTaxonomy, PortableKind,
  TraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = CExtractor;
  let _ = CCallGrammar;
  let _ = CImportGrammar;
  let _ = CInheritanceGrammar;
  let _ = CDecorationGrammar;
  let _ = CTraceTemplate;
  let _ = CKindTaxonomy;
  let _ = CTopLevelKinds;
}

#[test]
fn taxonomy_function_definition() {
  assert_eq!(
    CKindTaxonomy.kind_for(PortableKind::Function),
    &["function_definition"]
  );
}

#[test]
fn taxonomy_no_method_class_interface_trait() {
  // C has no methods, classes, interfaces, or traits.
  assert!(CKindTaxonomy.kind_for(PortableKind::Method).is_empty());
  assert!(CKindTaxonomy.kind_for(PortableKind::Class).is_empty());
  assert!(CKindTaxonomy.kind_for(PortableKind::Interface).is_empty());
  assert!(CKindTaxonomy.kind_for(PortableKind::Trait).is_empty());
  assert!(CKindTaxonomy.kind_for(PortableKind::Route).is_empty());
}

#[test]
fn taxonomy_struct_enum_distinct() {
  assert_eq!(
    CKindTaxonomy.kind_for(PortableKind::Struct),
    &["struct_specifier"]
  );
  assert_eq!(
    CKindTaxonomy.kind_for(PortableKind::Enum),
    &["enum_specifier"]
  );
}

#[test]
fn taxonomy_typealias_is_type_definition() {
  // C `typedef` only — no C++11 `using`.
  assert_eq!(
    CKindTaxonomy.kind_for(PortableKind::TypeAlias),
    &["type_definition"]
  );
}

#[test]
fn taxonomy_macro_covers_object_and_function_like() {
  assert_eq!(
    CKindTaxonomy.kind_for(PortableKind::Macro),
    &["preproc_def", "preproc_function_def"]
  );
}

#[test]
fn taxonomy_import_is_preproc_include() {
  assert_eq!(
    CKindTaxonomy.kind_for(PortableKind::Import),
    &["preproc_include"]
  );
}

#[test]
fn taxonomy_const_is_declaration() {
  assert_eq!(
    CKindTaxonomy.kind_for(PortableKind::Const),
    &["declaration"]
  );
}

#[test]
fn import_grammar_emits_both_include_shapes() {
  // Phase 6C #1: `#include <hdr>` (system) + `#include "hdr"`
  // (local). Non-empty result is what suppresses the engine's
  // NOT_IMPLEMENTED diagnostic for C.
  let p = CImportGrammar.import_patterns("stdio.h");
  assert!(
    p.iter().any(|s| s == "#include <stdio.h>"),
    "patterns: {:?}",
    p
  );
  assert!(
    p.iter().any(|s| s == "#include \"stdio.h\""),
    "patterns: {:?}",
    p
  );
}

#[test]
fn import_grammar_handles_path_with_slashes() {
  // Header paths can include subdirs (`foo/bar.h`); pass-through
  // unchanged into both shapes.
  let p = CImportGrammar.import_patterns("foo/bar.h");
  assert!(p.iter().any(|s| s == "#include <foo/bar.h>"));
  assert!(p.iter().any(|s| s == "#include \"foo/bar.h\""));
}

#[test]
fn inheritance_grammar_empty() {
  assert!(CInheritanceGrammar
    .supertype_decl_patterns("Node")
    .is_empty());
  assert!(CInheritanceGrammar.subtype_patterns("Node").is_empty());
  assert!(CInheritanceGrammar.override_patterns("push").is_empty());
}

#[test]
fn decoration_grammar_empty() {
  assert!(CDecorationGrammar.decoration_patterns("unused").is_empty());
}

#[test]
fn call_grammar_standard_shapes() {
  let p = CCallGrammar.call_patterns("malloc");
  assert!(p.iter().any(|s| s == "malloc($$$ARGS)"));
  assert!(p.iter().any(|s| s == "$OBJ.malloc($$$ARGS)"));
}

#[test]
fn call_grammar_callee_decl_not_empty() {
  let p = CCallGrammar.callee_decl_patterns("area");
  assert!(!p.is_empty());
}

#[test]
fn trace_template_returns_none() {
  assert!(CTraceTemplate.template_for("VAR").is_none());
}

#[test]
fn fixtures_present() {
  let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
  for f in &["math.c", "linked_list.c"] {
    assert!(dir.join(f).exists(), "missing fixture: {}", f);
  }
}
