//! Phase 5B smoke test for `lmprobe-lang-bash`.
//!
//! Until the Coder populates this with real assertions, the smoke
//! confirms the crate's public structs compile + are constructible.

use lmprobe_lang_bash::{
  BashCallGrammar, BashDecorationGrammar, BashExtractor, BashImportGrammar, BashInheritanceGrammar,
  BashKindTaxonomy, BashTopLevelKinds, BashTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = BashExtractor;
  let _ = BashCallGrammar;
  let _ = BashImportGrammar;
  let _ = BashInheritanceGrammar;
  let _ = BashDecorationGrammar;
  let _ = BashTraceTemplate;
  let _ = BashKindTaxonomy;
  let _ = BashTopLevelKinds;
}
