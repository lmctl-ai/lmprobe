//! Phase 5B smoke test for `lmprobe-lang-{{lang}}`.
//!
//! Until the Coder populates this with real assertions, the smoke
//! confirms the crate's public structs compile + are constructible.

use lmprobe_lang_{{lang}}::{
    {{Lang}}CallGrammar, {{Lang}}DecorationGrammar, {{Lang}}Extractor,
    {{Lang}}ImportGrammar, {{Lang}}InheritanceGrammar, {{Lang}}KindTaxonomy,
    {{Lang}}TopLevelKinds, {{Lang}}TraceTemplate,
};

#[test]
fn structs_construct() {
    let _ = {{Lang}}Extractor;
    let _ = {{Lang}}CallGrammar;
    let _ = {{Lang}}ImportGrammar;
    let _ = {{Lang}}InheritanceGrammar;
    let _ = {{Lang}}DecorationGrammar;
    let _ = {{Lang}}TraceTemplate;
    let _ = {{Lang}}KindTaxonomy;
    let _ = {{Lang}}TopLevelKinds;
}
