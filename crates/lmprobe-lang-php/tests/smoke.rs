//! Smoke tests for `lmprobe-lang-php` against the in-crate
//! PHP fixtures (`fixtures/*.php`).
//!
//! Asserts the trait impls produce the patterns PHP needs for the
//! binary's deep + smoke tiers. Per-crate gate — the binary's
//! cross-language regression matrix lives at
//! `crates/lmprobe/tests/{smoke,deep}.rs`.

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, ImportGrammar, InheritanceGrammar, KindTaxonomy, PortableKind,
  TraceTemplate,
};
use lmprobe_lang_php::{
  PhpCallGrammar, PhpDecorationGrammar, PhpExtractor, PhpImportGrammar, PhpInheritanceGrammar,
  PhpKindTaxonomy, PhpTopLevelKinds, PhpTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = PhpExtractor;
  let _ = PhpCallGrammar;
  let _ = PhpImportGrammar;
  let _ = PhpInheritanceGrammar;
  let _ = PhpDecorationGrammar;
  let _ = PhpTraceTemplate;
  let _ = PhpKindTaxonomy;
  let _ = PhpTopLevelKinds;
}

#[test]
fn taxonomy_function_definition() {
  assert_eq!(
    PhpKindTaxonomy.kind_for(PortableKind::Function),
    &["function_definition"]
  );
}

#[test]
fn taxonomy_method_declaration() {
  assert_eq!(
    PhpKindTaxonomy.kind_for(PortableKind::Method),
    &["method_declaration"]
  );
}

#[test]
fn taxonomy_class_declaration() {
  assert_eq!(
    PhpKindTaxonomy.kind_for(PortableKind::Class),
    &["class_declaration"]
  );
}

#[test]
fn taxonomy_interface_and_trait() {
  assert_eq!(
    PhpKindTaxonomy.kind_for(PortableKind::Interface),
    &["interface_declaration"]
  );
  assert_eq!(
    PhpKindTaxonomy.kind_for(PortableKind::Trait),
    &["trait_declaration"]
  );
}

#[test]
fn taxonomy_enum_declaration() {
  assert_eq!(
    PhpKindTaxonomy.kind_for(PortableKind::Enum),
    &["enum_declaration"]
  );
}

#[test]
fn taxonomy_const_declaration() {
  assert_eq!(
    PhpKindTaxonomy.kind_for(PortableKind::Const),
    &["const_declaration"]
  );
}

#[test]
fn taxonomy_import_namespace_use() {
  assert_eq!(
    PhpKindTaxonomy.kind_for(PortableKind::Import),
    &["namespace_use_declaration"]
  );
}

#[test]
fn taxonomy_empty_kinds() {
  assert!(PhpKindTaxonomy.kind_for(PortableKind::Struct).is_empty());
  assert!(PhpKindTaxonomy.kind_for(PortableKind::TypeAlias).is_empty());
  assert!(PhpKindTaxonomy.kind_for(PortableKind::Macro).is_empty());
  assert!(PhpKindTaxonomy.kind_for(PortableKind::Route).is_empty());
}

#[test]
fn import_grammar_use_patterns() {
  // Phase 6C round-2 (TC-103) — 3 patterns: exact + bare-root
  // prefix continuation + group. Round-1's 2-pattern set added
  // back the `\$$$` continuation per PM doctrine for prefix-match.
  let p = PhpImportGrammar.import_patterns("App\\Models\\User");
  assert_eq!(p.len(), 3, "round-2: plain + prefix continuation + group");
  assert!(p.iter().any(|s| s == "use App\\Models\\User;"));
  assert!(p.iter().any(|s| s == "use App\\Models\\User\\$$$;"));
  assert!(p.iter().any(|s| s == "use App\\Models\\User\\{$$$};"));
}

#[test]
fn import_grammar_single_segment_name() {
  let p = PhpImportGrammar.import_patterns("Foo");
  assert!(p.iter().any(|s| s == "use Foo;"));
  assert!(p.iter().any(|s| s == "use Foo\\{$$$};"));
}

#[test]
fn inheritance_supertype_extends_and_implements() {
  let p = PhpInheritanceGrammar.supertype_decl_patterns("Controller");
  assert!(p
    .iter()
    .any(|s| s == "class Controller extends $SUPER { $$$ }"));
  assert!(p
    .iter()
    .any(|s| s == "class Controller implements $$$IFACES { $$$ }"));
  assert!(p
    .iter()
    .any(|s| s == "class Controller extends $SUPER implements $$$ { $$$ }"));
}

#[test]
fn inheritance_subtype_extends_and_implements() {
  let p = PhpInheritanceGrammar.subtype_patterns("BaseModel");
  assert!(p.iter().any(|s| s == "class $C extends BaseModel { $$$ }"));
  assert!(p
    .iter()
    .any(|s| s == "class $C implements BaseModel { $$$ }"));
}

#[test]
fn inheritance_override_empty() {
  // PHP has no override keyword in the tree-sitter grammar — empty
  // Vec → the sensor reports NOT_IMPLEMENTED for this verb.
  assert!(PhpInheritanceGrammar.override_patterns("foo").is_empty());
}

#[test]
fn decoration_php8_attributes() {
  let p = PhpDecorationGrammar.decoration_patterns("Route");
  assert!(p.iter().any(|s| s == "#[Route]"));
  assert!(p.iter().any(|s| s == "#[Route($$$)]"));
  assert_eq!(p.len(), 2);
}

#[test]
fn call_grammar_empty() {
  // PHP call grammar not wired — produces empty vecs.
  assert!(PhpCallGrammar.call_patterns("foo").is_empty());
}

#[test]
fn trace_template_none() {
  assert!(PhpTraceTemplate.template_for("VAR").is_none());
}

#[test]
fn fixtures_present() {
  let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
  for f in &["controller.php", "service.php"] {
    assert!(dir.join(f).exists(), "missing fixture: {}", f);
  }
}
