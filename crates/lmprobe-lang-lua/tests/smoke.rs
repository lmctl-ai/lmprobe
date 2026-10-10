//! Phase 5B smoke test for `lmprobe-lang-lua`.
//!
//! Until the Coder populates this with real assertions, the smoke
//! confirms the crate's public structs compile + are constructible.

use lmprobe_lang_lua::{
  LuaCallGrammar, LuaDecorationGrammar, LuaExtractor, LuaImportGrammar, LuaInheritanceGrammar,
  LuaKindTaxonomy, LuaTopLevelKinds, LuaTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = LuaExtractor;
  let _ = LuaCallGrammar;
  let _ = LuaImportGrammar;
  let _ = LuaInheritanceGrammar;
  let _ = LuaDecorationGrammar;
  let _ = LuaTraceTemplate;
  let _ = LuaKindTaxonomy;
  let _ = LuaTopLevelKinds;
}
