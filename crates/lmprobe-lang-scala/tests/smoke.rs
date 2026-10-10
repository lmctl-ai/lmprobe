//! Smoke tests for `lmprobe-lang-scala` against the in-crate
//! Scala fixtures (`fixtures/*.scala`).
//!
//! Asserts the trait impls produce the patterns Scala needs for the
//! binary's deep + smoke tiers. Per-crate gate — the binary's
//! cross-language regression matrix lives at
//! `crates/lmprobe/tests/{smoke,deep}.rs`.

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, ImportGrammar, InheritanceGrammar, KindTaxonomy, PortableKind,
  TraceTemplate,
};
use lmprobe_lang_scala::{
  ScalaCallGrammar, ScalaDecorationGrammar, ScalaExtractor, ScalaImportGrammar,
  ScalaInheritanceGrammar, ScalaKindTaxonomy, ScalaTopLevelKinds, ScalaTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = ScalaExtractor;
  let _ = ScalaCallGrammar;
  let _ = ScalaImportGrammar;
  let _ = ScalaInheritanceGrammar;
  let _ = ScalaDecorationGrammar;
  let _ = ScalaTraceTemplate;
  let _ = ScalaKindTaxonomy;
  let _ = ScalaTopLevelKinds;
}

#[test]
fn taxonomy_function_and_method_both_function_definition() {
  // Scala folds `def` declarations into a single `function_definition`
  // kind regardless of top-level vs class-method context.
  assert_eq!(
    ScalaKindTaxonomy.kind_for(PortableKind::Function),
    &["function_definition"]
  );
  assert_eq!(
    ScalaKindTaxonomy.kind_for(PortableKind::Method),
    &["function_definition"]
  );
}

#[test]
fn taxonomy_class_trait_interface_enum_distinct() {
  assert_eq!(
    ScalaKindTaxonomy.kind_for(PortableKind::Class),
    &["class_definition"]
  );
  // Scala has no Java-style `interface` keyword — `trait` doubles
  // for both portable kinds.
  assert_eq!(
    ScalaKindTaxonomy.kind_for(PortableKind::Trait),
    &["trait_definition"]
  );
  assert_eq!(
    ScalaKindTaxonomy.kind_for(PortableKind::Interface),
    &["trait_definition"]
  );
  assert_eq!(
    ScalaKindTaxonomy.kind_for(PortableKind::Enum),
    &["enum_definition"]
  );
}

#[test]
fn taxonomy_no_struct_macro_route() {
  assert!(ScalaKindTaxonomy.kind_for(PortableKind::Struct).is_empty());
  assert!(ScalaKindTaxonomy.kind_for(PortableKind::Macro).is_empty());
  assert!(ScalaKindTaxonomy.kind_for(PortableKind::Route).is_empty());
}

#[test]
fn taxonomy_typealias_type_definition() {
  assert_eq!(
    ScalaKindTaxonomy.kind_for(PortableKind::TypeAlias),
    &["type_definition"]
  );
}

#[test]
fn taxonomy_const_val_and_var() {
  assert_eq!(
    ScalaKindTaxonomy.kind_for(PortableKind::Const),
    &["val_definition", "var_definition"]
  );
}

#[test]
fn taxonomy_import_declaration() {
  assert_eq!(
    ScalaKindTaxonomy.kind_for(PortableKind::Import),
    &["import_declaration"]
  );
}

#[test]
fn import_grammar_single_pattern_covers_all_forms() {
  // Phase 6C #8 — one pattern suffices. tree-sitter-scala flattens
  // every import shape (wildcard / group / alias) as sibling
  // children of `import_declaration`; ast-grep lenient-matches the
  // bare `import {name}` against all of them. Per-form coverage is
  // enforced end-to-end by tests/imports.rs.
  let p = ScalaImportGrammar.import_patterns("scala.collection.mutable");
  assert_eq!(p.len(), 1, "exactly 1 import pattern (no dedupe gap)");
  assert_eq!(p[0], "import scala.collection.mutable");
}

#[test]
fn import_grammar_export_def_class_object_trait() {
  let p = ScalaImportGrammar.export_patterns("UserService");
  assert!(p.iter().any(|s| s == "def UserService($$$) = $$$"));
  assert!(p.iter().any(|s| s == "class UserService($$$) { $$$ }"));
  assert!(p.iter().any(|s| s == "object UserService { $$$ }"));
  assert!(p.iter().any(|s| s == "trait UserService { $$$ }"));
}

#[test]
fn inheritance_supertype_class_and_trait() {
  let p = ScalaInheritanceGrammar.supertype_decl_patterns("UserService");
  assert!(p
    .iter()
    .any(|s| s == "class UserService extends $SUPER { $$$ }"));
  assert!(p
    .iter()
    .any(|s| s == "class UserService($$$) extends $SUPER { $$$ }"));
  assert!(p
    .iter()
    .any(|s| s == "trait UserService extends $SUPER { $$$ }"));
}

#[test]
fn inheritance_subtype_includes_with_mixin() {
  let p = ScalaInheritanceGrammar.subtype_patterns("Repository");
  assert!(p.iter().any(|s| s == "class $C extends Repository { $$$ }"));
  // Scala's mixin syntax: `with` chains additional traits.
  assert!(p
    .iter()
    .any(|s| s == "class $C extends Repository with $$$ { $$$ }"));
  assert!(p.iter().any(|s| s == "trait $T extends Repository { $$$ }"));
}

#[test]
fn inheritance_override_def_form() {
  let p = ScalaInheritanceGrammar.override_patterns("speak");
  assert!(p.iter().any(|s| s == "override def speak($$$) = $$$"));
}

#[test]
fn call_grammar_emits_canonical_shapes() {
  // Phase 6D.1 — restores Scala callers (regression vs cycle-1).
  // Pin the four call-site shapes per locked doctrine
  // (durable-memory/04_sensors_graph.md ## callers): bare/no-new-ctor,
  // method, explicit-new, higher-order. Per-form coverage is
  // exercised end-to-end by tests/callers.rs.
  let p = ScalaCallGrammar.call_patterns("parse");
  assert_eq!(p.len(), 4, "exactly 4 call patterns");
  assert!(p.iter().any(|s| s == "parse($$$ARGS)"));
  assert!(p.iter().any(|s| s == "$OBJ.parse($$$ARGS)"));
  assert!(p.iter().any(|s| s == "new parse($$$ARGS)"));
  assert!(p.iter().any(|s| s == "$X.$M(parse)"));
}

#[test]
fn callee_decl_patterns_cover_all_def_shapes() {
  // Wide-net coverage: typed/untyped concrete, abstract, no-paren,
  // generic, curried, override + visibility modifiers.
  let p = ScalaCallGrammar.callee_decl_patterns("parse");
  assert_eq!(p.len(), 10, "10 def-shape patterns");
  assert!(p.iter().any(|s| s == "def parse($$$): $RET = $$$"));
  assert!(p.iter().any(|s| s == "def parse($$$): $RET"));
  assert!(p.iter().any(|s| s == "override def parse($$$): $RET = $$$"));
  assert!(p.iter().any(|s| s == "def parse[$$$]($$$): $RET = $$$"));
  assert!(p.iter().any(|s| s == "def parse($$$)($$$): $RET = $$$"));
}

#[test]
fn decoration_empty_no_decoration_grammar() {
  // Scala had no decoration grammar in lang-core; preserved here.
  assert!(ScalaDecorationGrammar
    .decoration_patterns("Override")
    .is_empty());
}

#[test]
fn trace_template_none() {
  // Scala had no trace template in lang-core — None signals
  // NO_TRACE_TEMPLATE so callers know to pass `--template`.
  assert!(ScalaTraceTemplate.template_for("VAR").is_none());
}

#[test]
fn fixtures_present() {
  let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
  for f in &["UserService.scala", "Models.scala"] {
    assert!(dir.join(f).exists(), "missing fixture: {}", f);
  }
}
