//! Phase 5B smoke test for `lmprobe-lang-typescript`.
//!
//! Until the Coder populates this with real assertions, the smoke
//! confirms the crate's public structs compile + are constructible.

use lmprobe_lang_typescript::{
  TypeScriptCallGrammar, TypeScriptDecorationGrammar, TypeScriptExtractor, TypeScriptImportGrammar,
  TypeScriptInheritanceGrammar, TypeScriptKindTaxonomy, TypeScriptTopLevelKinds,
  TypeScriptTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = TypeScriptExtractor;
  let _ = TypeScriptCallGrammar;
  let _ = TypeScriptImportGrammar;
  let _ = TypeScriptInheritanceGrammar;
  let _ = TypeScriptDecorationGrammar;
  let _ = TypeScriptTraceTemplate;
  let _ = TypeScriptKindTaxonomy;
  let _ = TypeScriptTopLevelKinds;
}
