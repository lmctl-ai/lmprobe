//! Phase 5B smoke test for `lmprobe-lang-dart`.
//!
//! Until the Coder populates this with real assertions, the smoke
//! confirms the crate's public structs compile + are constructible.

use lmprobe_lang_dart::{
  DartCallGrammar, DartDecorationGrammar, DartExtractor, DartImportGrammar, DartInheritanceGrammar,
  DartKindTaxonomy, DartTopLevelKinds, DartTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = DartExtractor;
  let _ = DartCallGrammar;
  let _ = DartImportGrammar;
  let _ = DartInheritanceGrammar;
  let _ = DartDecorationGrammar;
  let _ = DartTraceTemplate;
  let _ = DartKindTaxonomy;
  let _ = DartTopLevelKinds;
}
