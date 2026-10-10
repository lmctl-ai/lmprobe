//! Phase 5B smoke test for `lmprobe-lang-css`.
//!
//! Until the Coder populates this with real assertions, the smoke
//! confirms the crate's public structs compile + are constructible.

use lmprobe_lang_css::{
  CssCallGrammar, CssDecorationGrammar, CssExtractor, CssImportGrammar, CssInheritanceGrammar,
  CssKindTaxonomy, CssTopLevelKinds, CssTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = CssExtractor;
  let _ = CssCallGrammar;
  let _ = CssImportGrammar;
  let _ = CssInheritanceGrammar;
  let _ = CssDecorationGrammar;
  let _ = CssTraceTemplate;
  let _ = CssKindTaxonomy;
  let _ = CssTopLevelKinds;
}
