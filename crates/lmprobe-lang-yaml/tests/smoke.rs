//! Phase 5B smoke test for `lmprobe-lang-yaml`.
//!
//! Until the Coder populates this with real assertions, the smoke
//! confirms the crate's public structs compile + are constructible.

use lmprobe_lang_yaml::{
  YamlCallGrammar, YamlDecorationGrammar, YamlExtractor, YamlImportGrammar, YamlInheritanceGrammar,
  YamlKindTaxonomy, YamlTopLevelKinds, YamlTraceTemplate,
};

#[test]
fn structs_construct() {
  let _ = YamlExtractor;
  let _ = YamlCallGrammar;
  let _ = YamlImportGrammar;
  let _ = YamlInheritanceGrammar;
  let _ = YamlDecorationGrammar;
  let _ = YamlTraceTemplate;
  let _ = YamlKindTaxonomy;
  let _ = YamlTopLevelKinds;
}
