//! Phase 5B smoke test for `lmprobe-lang-hcl`.
//!
//! Bespoke assertions verifying the trait impls ported from lang-core.

use lmprobe_lang_core::langs::{
  CallGrammar, DecorationGrammar, ImportGrammar, KindTaxonomy, PortableKind, TraceTemplate,
};
use lmprobe_lang_hcl::{
  HclCallGrammar, HclDecorationGrammar, HclExtractor, HclImportGrammar, HclInheritanceGrammar,
  HclKindTaxonomy, HclTopLevelKinds, HclTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = HclExtractor;
  let _ = HclCallGrammar;
  let _ = HclImportGrammar;
  let _ = HclInheritanceGrammar;
  let _ = HclDecorationGrammar;
  let _ = HclTraceTemplate;
  let _ = HclKindTaxonomy;
  let _ = HclTopLevelKinds;
}

#[test]
fn import_patterns_dual_anchor_round3() {
  // Phase 6C round-3: dual-anchor — block-name + source-path.
  let pats = HclImportGrammar.import_patterns("vpc");
  assert_eq!(pats.len(), 2);
  assert!(pats
    .iter()
    .any(|s| s == r#"module "vpc" { source = $SRC }"#));
  assert!(pats
    .iter()
    .any(|s| s == r#"module "$X" { source = "vpc" }"#));
}

#[test]
fn call_grammar_empty() {
  assert!(HclCallGrammar.call_patterns("foo").is_empty());
}

#[test]
fn decoration_grammar_empty() {
  assert!(HclDecorationGrammar.decoration_patterns("foo").is_empty());
}

#[test]
fn trace_template_none() {
  assert!(HclTraceTemplate.template_for("x").is_none());
}

#[test]
fn taxonomy_struct_is_block() {
  let kinds = HclKindTaxonomy.kind_for(PortableKind::Struct);
  assert_eq!(kinds, &["block"]);
}

#[test]
fn taxonomy_const_is_attribute() {
  let kinds = HclKindTaxonomy.kind_for(PortableKind::Const);
  assert_eq!(kinds, &["attribute"]);
}

#[test]
fn taxonomy_function_empty() {
  assert!(HclKindTaxonomy.kind_for(PortableKind::Function).is_empty());
}

#[test]
fn taxonomy_import_empty() {
  assert!(HclKindTaxonomy.kind_for(PortableKind::Import).is_empty());
}
