//! Smoke tests for `lmprobe-lang-json` against the in-crate
//! JSON fixtures (`fixtures/*.json`).
//!
//! Asserts the trait impls produce the patterns JSON needs for the
//! binary's deep + smoke tiers. Per-crate gate — the binary's
//! cross-language regression matrix lives at
//! `crates/lmprobe/tests/{smoke,deep}.rs`.

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, ImportGrammar, KindTaxonomy, PortableKind, TopLevelKinds,
  TraceTemplate,
};
use lmprobe_lang_json::{
  JsonCallGrammar, JsonDecorationGrammar, JsonExtractor, JsonImportGrammar, JsonInheritanceGrammar,
  JsonKindTaxonomy, JsonTopLevelKinds, JsonTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = JsonExtractor;
  let _ = JsonCallGrammar;
  let _ = JsonImportGrammar;
  let _ = JsonInheritanceGrammar;
  let _ = JsonDecorationGrammar;
  let _ = JsonTraceTemplate;
  let _ = JsonKindTaxonomy;
  let _ = JsonTopLevelKinds;
}

#[test]
fn taxonomy_class_includes_containers() {
  // Class covers `object` and `array` — the two container shapes.
  let kinds = JsonKindTaxonomy.kind_for(PortableKind::Class);
  assert!(kinds.contains(&"object"));
  assert!(kinds.contains(&"array"));
}

#[test]
fn taxonomy_const_is_pair() {
  // `pair` is JSON's leaf key/value declaration — the `Const`
  // mapping users hit via `--kind const`.
  let kinds = JsonKindTaxonomy.kind_for(PortableKind::Const);
  assert_eq!(kinds, &["pair"]);
}

#[test]
fn taxonomy_unmodelled_kinds_empty() {
  // Data-tier: code-shaped portable kinds map to nothing.
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
      JsonKindTaxonomy.kind_for(p).is_empty(),
      "expected {:?} to be empty for JSON",
      p
    );
  }
}

#[test]
fn top_level_kinds_pair() {
  // `pair` is the direct-child entry inside an object — what
  // `--has-child` gates against for JSON.
  let k = JsonTopLevelKinds.child_kinds();
  assert_eq!(k, &["pair"]);
}

#[test]
fn call_grammar_empty_data_tier() {
  assert!(JsonCallGrammar.call_patterns("foo").is_empty());
}

#[test]
fn import_grammar_empty_data_tier() {
  assert!(JsonImportGrammar.import_patterns("foo").is_empty());
}

#[test]
fn decoration_grammar_empty_data_tier() {
  assert!(JsonDecorationGrammar.decoration_patterns("foo").is_empty());
}

#[test]
fn trace_template_none() {
  assert!(JsonTraceTemplate.template_for("x").is_none());
}

#[test]
fn fixtures_present() {
  let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
  for f in &["sample.json"] {
    assert!(dir.join(f).exists(), "missing fixture: {}", f);
  }
}
