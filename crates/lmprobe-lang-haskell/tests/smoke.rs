//! Phase 5B smoke test for `lmprobe-lang-haskell`.
//!
//! Until the Coder populates this with real assertions, the smoke
//! confirms the crate's public structs compile + are constructible.

use lmprobe_lang_haskell::{
  HaskellCallGrammar, HaskellDecorationGrammar, HaskellExtractor, HaskellImportGrammar,
  HaskellInheritanceGrammar, HaskellKindTaxonomy, HaskellTopLevelKinds, HaskellTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = HaskellExtractor;
  let _ = HaskellCallGrammar;
  let _ = HaskellImportGrammar;
  let _ = HaskellInheritanceGrammar;
  let _ = HaskellDecorationGrammar;
  let _ = HaskellTraceTemplate;
  let _ = HaskellKindTaxonomy;
  let _ = HaskellTopLevelKinds;
}
