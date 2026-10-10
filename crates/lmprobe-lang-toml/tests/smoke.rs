//! Smoke tests for `lmprobe-lang-toml` against the in-crate
//! TOML fixtures (`fixtures/*.toml`).
//!
//! Asserts the trait impls produce the patterns TOML needs for the
//! binary's deep + smoke tiers. Per-crate gate — the binary's
//! cross-language regression matrix lives at
//! `crates/lmprobe/tests/{smoke,deep}.rs`.

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, ImportGrammar, KindTaxonomy, PortableKind, TopLevelKinds,
  TraceTemplate,
};
use lmprobe_lang_toml::{
  TomlCallGrammar, TomlDecorationGrammar, TomlExtractor, TomlImportGrammar, TomlInheritanceGrammar,
  TomlKindTaxonomy, TomlTopLevelKinds, TomlTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = TomlExtractor;
  let _ = TomlCallGrammar;
  let _ = TomlImportGrammar;
  let _ = TomlInheritanceGrammar;
  let _ = TomlDecorationGrammar;
  let _ = TomlTraceTemplate;
  let _ = TomlKindTaxonomy;
  let _ = TomlTopLevelKinds;
}

#[test]
fn taxonomy_class_includes_table_shapes() {
  // Class covers all three table-container nodes — `[section]`,
  // `[[array_of_tables]]`, and inline `{ … }`.
  let kinds = TomlKindTaxonomy.kind_for(PortableKind::Class);
  assert!(kinds.contains(&"table"));
  assert!(kinds.contains(&"table_array_element"));
  assert!(kinds.contains(&"inline_table"));
}

#[test]
fn taxonomy_const_is_pair() {
  // `pair` is TOML's leaf key/value declaration — the `Const`
  // mapping users hit via `--kind const`.
  let kinds = TomlKindTaxonomy.kind_for(PortableKind::Const);
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
      TomlKindTaxonomy.kind_for(p).is_empty(),
      "expected {:?} to be empty for TOML",
      p
    );
  }
}

#[test]
fn top_level_kinds_pair() {
  // `pair` is the direct-child entry inside a table — what
  // `--has-child` gates against for TOML.
  let k = TomlTopLevelKinds.child_kinds();
  assert_eq!(k, &["pair"]);
}

#[test]
fn call_grammar_empty_data_tier() {
  assert!(TomlCallGrammar.call_patterns("foo").is_empty());
}

#[test]
fn import_grammar_empty_data_tier() {
  assert!(TomlImportGrammar.import_patterns("foo").is_empty());
}

#[test]
fn decoration_grammar_empty_data_tier() {
  assert!(TomlDecorationGrammar.decoration_patterns("foo").is_empty());
}

#[test]
fn trace_template_none() {
  assert!(TomlTraceTemplate.template_for("x").is_none());
}

#[test]
fn fixtures_present() {
  let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
  for f in &["sample.toml", "inline.toml"] {
    assert!(dir.join(f).exists(), "missing fixture: {}", f);
  }
}
