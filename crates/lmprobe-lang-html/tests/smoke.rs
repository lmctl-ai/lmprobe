//! Phase 5B smoke test for `lmprobe-lang-html`.
//!
//! Until the Coder populates this with real assertions, the smoke
//! confirms the crate's public structs compile + are constructible.

use lmprobe_lang_html::{
  HtmlCallGrammar, HtmlDecorationGrammar, HtmlExtractor, HtmlImportGrammar, HtmlInheritanceGrammar,
  HtmlKindTaxonomy, HtmlTopLevelKinds, HtmlTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = HtmlExtractor;
  let _ = HtmlCallGrammar;
  let _ = HtmlImportGrammar;
  let _ = HtmlInheritanceGrammar;
  let _ = HtmlDecorationGrammar;
  let _ = HtmlTraceTemplate;
  let _ = HtmlKindTaxonomy;
  let _ = HtmlTopLevelKinds;
}
