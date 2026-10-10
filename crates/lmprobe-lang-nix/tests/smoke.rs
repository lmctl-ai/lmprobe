//! Phase 5B smoke test for `lmprobe-lang-nix`.
//!
//! Until the Coder populates this with real assertions, the smoke
//! confirms the crate's public structs compile + are constructible.

use lmprobe_lang_nix::{
  NixCallGrammar, NixDecorationGrammar, NixExtractor, NixImportGrammar, NixInheritanceGrammar,
  NixKindTaxonomy, NixTopLevelKinds, NixTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = NixExtractor;
  let _ = NixCallGrammar;
  let _ = NixImportGrammar;
  let _ = NixInheritanceGrammar;
  let _ = NixDecorationGrammar;
  let _ = NixTraceTemplate;
  let _ = NixKindTaxonomy;
  let _ = NixTopLevelKinds;
}
