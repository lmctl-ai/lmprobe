//! Smoke tests for `lmprobe-lang-xml` against the in-crate
//! XML fixtures (`fixtures/*.xml`).
//!
//! Asserts the trait impls produce the patterns XML needs for the
//! binary's deep + smoke tiers. Per-crate gate — the binary's
//! cross-language regression matrix lives at
//! `crates/lmprobe/tests/{smoke,deep}.rs`.

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, ImportGrammar, KindTaxonomy, PortableKind, TopLevelKinds,
  TraceTemplate,
};
use lmprobe_lang_xml::{
  XmlCallGrammar, XmlDecorationGrammar, XmlExtractor, XmlImportGrammar, XmlInheritanceGrammar,
  XmlKindTaxonomy, XmlTopLevelKinds, XmlTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = XmlExtractor;
  let _ = XmlCallGrammar;
  let _ = XmlImportGrammar;
  let _ = XmlInheritanceGrammar;
  let _ = XmlDecorationGrammar;
  let _ = XmlTraceTemplate;
  let _ = XmlKindTaxonomy;
  let _ = XmlTopLevelKinds;
}

#[test]
fn taxonomy_class_is_element() {
  let kinds = XmlKindTaxonomy.kind_for(PortableKind::Class);
  assert_eq!(kinds, &["element"]);
}

#[test]
fn taxonomy_const_is_attribute() {
  // `Attribute` (capital A — tree-sitter-xml convention) is the
  // leaf named-entry declaration.
  let kinds = XmlKindTaxonomy.kind_for(PortableKind::Const);
  assert_eq!(kinds, &["Attribute"]);
}

#[test]
fn taxonomy_unmodelled_kinds_empty() {
  for p in [
    PortableKind::Function,
    PortableKind::Method,
    PortableKind::Struct,
    PortableKind::Interface,
    PortableKind::Trait,
    PortableKind::Enum,
    PortableKind::TypeAlias,
    PortableKind::Macro,
    PortableKind::Import,
    PortableKind::Route,
  ] {
    assert!(
      XmlKindTaxonomy.kind_for(p).is_empty(),
      "expected {:?} to be empty for XML",
      p
    );
  }
}

#[test]
fn top_level_kinds_element_and_attribute() {
  let k = XmlTopLevelKinds.child_kinds();
  assert_eq!(k, &["element", "Attribute"]);
}

#[test]
fn call_grammar_empty_data_tier() {
  assert!(XmlCallGrammar.call_patterns("foo").is_empty());
}

#[test]
fn import_grammar_empty_data_tier() {
  assert!(XmlImportGrammar.import_patterns("foo").is_empty());
}

#[test]
fn decoration_grammar_empty_data_tier() {
  assert!(XmlDecorationGrammar.decoration_patterns("foo").is_empty());
}

#[test]
fn trace_template_none() {
  assert!(XmlTraceTemplate.template_for("x").is_none());
}

#[test]
fn fixtures_present() {
  let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
  for f in &["sample.xml", "empty.xml"] {
    assert!(dir.join(f).exists(), "missing fixture: {}", f);
  }
}
