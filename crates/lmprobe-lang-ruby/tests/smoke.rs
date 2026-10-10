//! Smoke tests for `lmprobe-lang-ruby` against the in-crate Ruby
//! fixtures (`fixtures/*.rb`).
//!
//! Asserts the trait impls produce the patterns Ruby needs for the
//! binary's deep + smoke tiers. Per-crate gate — the binary's
//! cross-language regression matrix lives at
//! `crates/lmprobe/tests/{smoke,deep}.rs`.

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, ImportGrammar, InheritanceGrammar, KindTaxonomy, PortableKind,
  TraceTemplate,
};
use lmprobe_lang_ruby::{
  RubyCallGrammar, RubyDecorationGrammar, RubyExtractor, RubyImportGrammar, RubyInheritanceGrammar,
  RubyKindTaxonomy, RubyTopLevelKinds, RubyTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = RubyExtractor;
  let _ = RubyCallGrammar;
  let _ = RubyImportGrammar;
  let _ = RubyInheritanceGrammar;
  let _ = RubyDecorationGrammar;
  let _ = RubyTraceTemplate;
  let _ = RubyKindTaxonomy;
  let _ = RubyTopLevelKinds;
}

#[test]
fn taxonomy_function_is_method() {
  // Ruby's tree-sitter grammar uses `method` for both top-level
  // `def` and instance methods.
  assert_eq!(
    RubyKindTaxonomy.kind_for(PortableKind::Function),
    &["method"]
  );
}

#[test]
fn taxonomy_method_includes_singleton() {
  // `singleton_method` covers `def self.foo` / class-level methods.
  assert_eq!(
    RubyKindTaxonomy.kind_for(PortableKind::Method),
    &["method", "singleton_method"]
  );
}

#[test]
fn taxonomy_class() {
  assert_eq!(RubyKindTaxonomy.kind_for(PortableKind::Class), &["class"]);
}

#[test]
fn taxonomy_trait_is_module() {
  // Ruby modules serve the mixin/trait role.
  assert_eq!(RubyKindTaxonomy.kind_for(PortableKind::Trait), &["module"]);
}

#[test]
fn taxonomy_import_is_call() {
  // `require` / `require_relative` parse as `call` in tree-sitter.
  assert_eq!(RubyKindTaxonomy.kind_for(PortableKind::Import), &["call"]);
}

#[test]
fn taxonomy_no_struct_interface_enum_const_macro() {
  assert!(RubyKindTaxonomy.kind_for(PortableKind::Struct).is_empty());
  assert!(RubyKindTaxonomy
    .kind_for(PortableKind::Interface)
    .is_empty());
  assert!(RubyKindTaxonomy.kind_for(PortableKind::Enum).is_empty());
  assert!(RubyKindTaxonomy.kind_for(PortableKind::Const).is_empty());
  assert!(RubyKindTaxonomy.kind_for(PortableKind::Macro).is_empty());
}

#[test]
fn import_grammar_require_variants() {
  let p = RubyImportGrammar.import_patterns("json");
  assert_eq!(p.len(), 4);
  assert!(p.iter().any(|s| s == "require 'json'"));
  assert!(p.iter().any(|s| s == "require \"json\""));
  assert!(p.iter().any(|s| s == "require_relative 'json'"));
  assert!(p.iter().any(|s| s == "require_relative \"json\""));
}

#[test]
fn inheritance_supertype_pattern() {
  let p = RubyInheritanceGrammar.supertype_decl_patterns("Dog");
  assert_eq!(p.len(), 1);
  assert!(p[0].contains("class Dog < $SUPER"));
}

#[test]
fn inheritance_subtype_pattern() {
  let p = RubyInheritanceGrammar.subtype_patterns("Animal");
  assert_eq!(p.len(), 1);
  assert!(p[0].contains("class $C < Animal"));
}

#[test]
fn inheritance_no_override() {
  // Ruby has no `override` keyword — override_patterns defaults empty.
  assert!(RubyInheritanceGrammar.override_patterns("foo").is_empty());
}

#[test]
fn call_grammar_empty() {
  // Ruby's call shapes aren't modelled — preserved from NoCallGrammar.
  assert!(RubyCallGrammar.call_patterns("foo").is_empty());
}

#[test]
fn decoration_grammar_empty() {
  // Ruby has no annotation syntax.
  assert!(RubyDecorationGrammar.decoration_patterns("Foo").is_empty());
}

#[test]
fn trace_template_none() {
  // Ruby had no trace template in lang-core.
  assert!(RubyTraceTemplate.template_for("VAR").is_none());
}

#[test]
fn fixtures_present() {
  let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
  for f in &["greeter.rb", "animals.rb"] {
    assert!(dir.join(f).exists(), "missing fixture: {}", f);
  }
}
