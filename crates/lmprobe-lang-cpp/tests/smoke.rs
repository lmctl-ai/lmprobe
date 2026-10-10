//! Smoke tests for `lmprobe-lang-cpp` against the in-crate
//! C++ fixtures (`fixtures/*.{hpp,cpp}`).
//!
//! Asserts the trait impls produce the patterns C++ needs for the
//! binary's deep + smoke tiers. Per-crate gate — the binary's
//! cross-language regression matrix lives at
//! `crates/lmprobe/tests/{smoke,deep}.rs`.

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, ImportGrammar, InheritanceGrammar, KindTaxonomy, PortableKind,
  TraceTemplate,
};
use lmprobe_lang_cpp::{
  CppCallGrammar, CppDecorationGrammar, CppExtractor, CppImportGrammar, CppInheritanceGrammar,
  CppKindTaxonomy, CppTopLevelKinds, CppTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = CppExtractor;
  let _ = CppCallGrammar;
  let _ = CppImportGrammar;
  let _ = CppInheritanceGrammar;
  let _ = CppDecorationGrammar;
  let _ = CppTraceTemplate;
  let _ = CppKindTaxonomy;
  let _ = CppTopLevelKinds;
}

#[test]
fn taxonomy_function_and_method_share_function_definition() {
  // C++'s tree-sitter grammar uses a single `function_definition`
  // kind for both free functions and class member functions; the
  // distinction is structural, not at the kind level.
  assert_eq!(
    CppKindTaxonomy.kind_for(PortableKind::Function),
    &["function_definition"]
  );
  assert_eq!(
    CppKindTaxonomy.kind_for(PortableKind::Method),
    &["function_definition"]
  );
}

#[test]
fn taxonomy_class_struct_enum_distinct() {
  assert_eq!(
    CppKindTaxonomy.kind_for(PortableKind::Class),
    &["class_specifier"]
  );
  assert_eq!(
    CppKindTaxonomy.kind_for(PortableKind::Struct),
    &["struct_specifier"]
  );
  assert_eq!(
    CppKindTaxonomy.kind_for(PortableKind::Enum),
    &["enum_specifier"]
  );
}

#[test]
fn taxonomy_typealias_covers_using_and_typedef() {
  // C++11 `using Foo = Bar;` (alias_declaration) plus classic
  // `typedef Bar Foo;` (type_definition).
  assert_eq!(
    CppKindTaxonomy.kind_for(PortableKind::TypeAlias),
    &["alias_declaration", "type_definition"]
  );
}

#[test]
fn taxonomy_macro_covers_object_and_function_like() {
  assert_eq!(
    CppKindTaxonomy.kind_for(PortableKind::Macro),
    &["preproc_def", "preproc_function_def"]
  );
}

#[test]
fn taxonomy_import_is_preproc_include() {
  assert_eq!(
    CppKindTaxonomy.kind_for(PortableKind::Import),
    &["preproc_include"]
  );
}

#[test]
fn taxonomy_no_interface_trait_route() {
  assert!(CppKindTaxonomy.kind_for(PortableKind::Interface).is_empty());
  assert!(CppKindTaxonomy.kind_for(PortableKind::Trait).is_empty());
  assert!(CppKindTaxonomy.kind_for(PortableKind::Route).is_empty());
}

#[test]
fn import_grammar_emits_both_include_shapes() {
  // Phase 6C #1: `#include <hdr>` (system) + `#include "hdr"`
  // (local). Non-empty result is what suppresses the engine's
  // NOT_IMPLEMENTED diagnostic for C++.
  let p = CppImportGrammar.import_patterns("iostream");
  assert!(
    p.iter().any(|s| s == "#include <iostream>"),
    "patterns: {:?}",
    p
  );
  assert!(
    p.iter().any(|s| s == "#include \"iostream\""),
    "patterns: {:?}",
    p
  );
}

#[test]
fn import_grammar_handles_path_with_slashes() {
  // Header paths can include subdirs (`foo/bar.hpp`); pass-through
  // unchanged into both shapes.
  let p = CppImportGrammar.import_patterns("myproj/foo.hpp");
  assert!(p.iter().any(|s| s == "#include <myproj/foo.hpp>"));
  assert!(p.iter().any(|s| s == "#include \"myproj/foo.hpp\""));
}

#[test]
fn inheritance_supertype_class_and_struct_public() {
  let p = CppInheritanceGrammar.supertype_decl_patterns("Circle");
  assert!(p
    .iter()
    .any(|s| s == "class Circle : public $SUPER { $$$ }"));
  assert!(p
    .iter()
    .any(|s| s == "class Circle : public $SUPER, $$$ { $$$ }"));
  assert!(p
    .iter()
    .any(|s| s == "struct Circle : public $SUPER { $$$ }"));
}

#[test]
fn inheritance_subtype_class_and_struct_public() {
  let p = CppInheritanceGrammar.subtype_patterns("Shape");
  assert!(p.iter().any(|s| s == "class $C : public Shape { $$$ }"));
  assert!(p
    .iter()
    .any(|s| s == "class $C : public Shape, $$$ { $$$ }"));
  assert!(p.iter().any(|s| s == "struct $S : public Shape { $$$ }"));
}

#[test]
fn inheritance_override_with_and_without_virtual() {
  let p = CppInheritanceGrammar.override_patterns("area");
  assert!(p.iter().any(|s| s == "$RET area($$$) override { $$$ }"));
  assert!(p
    .iter()
    .any(|s| s == "virtual $RET area($$$) override { $$$ }"));
}

#[test]
fn decoration_grammar_empty_for_cpp() {
  // C++11 `[[attr]]` syntax isn't modelled; preserve empty shape.
  assert!(CppDecorationGrammar
    .decoration_patterns("nodiscard")
    .is_empty());
}

#[test]
fn call_grammar_standard_shapes() {
  // C++ uses StandardCallGrammar — same shapes as Java/Rust/Go.
  let p = CppCallGrammar.call_patterns("stbi_load");
  assert!(p.iter().any(|s| s == "stbi_load($$$ARGS)"));
  assert!(p.iter().any(|s| s == "$OBJ.stbi_load($$$ARGS)"));
}

#[test]
fn trace_template_returns_none() {
  // C++ has no built-in trace template — caller must pass
  // `--template` explicitly.
  assert!(CppTraceTemplate.template_for("VAR").is_none());
}

#[test]
fn fixtures_present() {
  let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
  for f in &["Shape.hpp", "Shape.cpp", "templates.hpp"] {
    assert!(dir.join(f).exists(), "missing fixture: {}", f);
  }
}
