//! Smoke tests for `lmprobe-lang-kotlin` against the in-crate
//! Kotlin fixtures (`fixtures/*.kt`).
//!
//! Asserts the trait impls produce the patterns Kotlin needs for
//! the binary's deep + smoke tiers. Per-crate gate — the binary's
//! cross-language regression matrix lives at
//! `crates/lmprobe/tests/{smoke,deep}.rs`.

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, ImportGrammar, InheritanceGrammar, KindTaxonomy, PortableKind,
  TraceTemplate,
};
use lmprobe_lang_kotlin::{
  KotlinCallGrammar, KotlinDecorationGrammar, KotlinExtractor, KotlinImportGrammar,
  KotlinInheritanceGrammar, KotlinKindTaxonomy, KotlinTopLevelKinds, KotlinTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = KotlinExtractor;
  let _ = KotlinCallGrammar;
  let _ = KotlinImportGrammar;
  let _ = KotlinInheritanceGrammar;
  let _ = KotlinDecorationGrammar;
  let _ = KotlinTraceTemplate;
  let _ = KotlinKindTaxonomy;
  let _ = KotlinTopLevelKinds;
}

#[test]
fn taxonomy_function_kind() {
  let kinds = KotlinKindTaxonomy.kind_for(PortableKind::Function);
  assert_eq!(kinds, &["function_declaration"]);
}

#[test]
fn taxonomy_class_includes_object_declaration() {
  let kinds = KotlinKindTaxonomy.kind_for(PortableKind::Class);
  assert!(kinds.contains(&"class_declaration"));
  assert!(
    kinds.contains(&"object_declaration"),
    "Kotlin's `object` declarations register as Class"
  );
}

#[test]
fn taxonomy_interface_reuses_class_declaration() {
  // Kotlin's tree-sitter grammar emits `class_declaration` for
  // both classes and interfaces — disambiguation is structural.
  let kinds = KotlinKindTaxonomy.kind_for(PortableKind::Interface);
  assert_eq!(kinds, &["class_declaration"]);
}

#[test]
fn taxonomy_no_struct_kind() {
  let kinds = KotlinKindTaxonomy.kind_for(PortableKind::Struct);
  assert!(kinds.is_empty(), "Kotlin has no Struct kind");
}

#[test]
fn import_grammar_dot_glob() {
  let p = KotlinImportGrammar.import_patterns("kotlinx.coroutines");
  assert!(p.iter().any(|s| s == "import kotlinx.coroutines"));
  assert!(p.iter().any(|s| s == "import kotlinx.coroutines.$$$"));
}

#[test]
fn export_grammar_class_and_object() {
  let p = KotlinImportGrammar.export_patterns("Repository");
  assert!(p.iter().any(|s| s.starts_with("class Repository")));
  assert!(p.iter().any(|s| s.starts_with("interface Repository")));
  assert!(p.iter().any(|s| s.starts_with("object Repository")));
}

#[test]
fn decoration_grammar_use_site_targets() {
  // The bespoke piece — the four use-site-targeted forms must
  // ride alongside the bare + called shapes.
  let p = KotlinDecorationGrammar.decoration_patterns("Inject");
  assert!(p.iter().any(|s| s == "@Inject"));
  assert!(p.iter().any(|s| s == "@Inject($$$)"));
  assert!(p.iter().any(|s| s == "@field:Inject"));
  assert!(p.iter().any(|s| s == "@param:Inject"));
  assert!(p.iter().any(|s| s == "@get:Inject"));
  assert!(p.iter().any(|s| s == "@set:Inject"));
}

#[test]
fn inheritance_supertype_colon_form() {
  let p = KotlinInheritanceGrammar.supertype_decl_patterns("Repository");
  assert!(p.iter().any(|s| s.contains("class Repository : $SUPER")));
  assert!(p
    .iter()
    .any(|s| s.contains("interface Repository : $$$IFACES")));
}

#[test]
fn inheritance_override_fun_shape() {
  let p = KotlinInheritanceGrammar.override_patterns("byId");
  assert!(p.iter().any(|s| s == "override fun byId($$$): $RET = $$$"));
  assert!(p.iter().any(|s| s == "override fun byId($$$) { $$$ }"));
}

#[test]
fn call_grammar_empty_kotlin_unwired() {
  // Kotlin's call shape isn't yet modelled. Pre-extraction this
  // delegated to NoCallGrammar; post-extraction, KotlinCallGrammar
  // returns the same empty Vec so the sensor surfaces NOT_IMPLEMENTED.
  assert!(KotlinCallGrammar.call_patterns("foo").is_empty());
}

#[test]
fn trace_template_none() {
  assert!(KotlinTraceTemplate.template_for("x").is_none());
}

#[test]
fn fixtures_present() {
  let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
  for f in &["Repo.kt", "Annotations.kt", "Models.kt"] {
    assert!(dir.join(f).exists(), "missing fixture: {}", f);
  }
}
