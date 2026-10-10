//! Phase 5B smoke test for `lmprobe-lang-solidity`.
//!
//! Until the Coder populates this with real assertions, the smoke
//! confirms the crate's public structs compile + are constructible.

use lmprobe_lang_solidity::{
  SolidityCallGrammar, SolidityDecorationGrammar, SolidityExtractor, SolidityImportGrammar,
  SolidityInheritanceGrammar, SolidityKindTaxonomy, SolidityTopLevelKinds, SolidityTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = SolidityExtractor;
  let _ = SolidityCallGrammar;
  let _ = SolidityImportGrammar;
  let _ = SolidityInheritanceGrammar;
  let _ = SolidityDecorationGrammar;
  let _ = SolidityTraceTemplate;
  let _ = SolidityKindTaxonomy;
  let _ = SolidityTopLevelKinds;
}
